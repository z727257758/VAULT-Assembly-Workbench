use super::*;
use crate::scene::layouts::{PrintLayout, PrintPart};
use glam::Vec3;

const LAYER_GAP: f32 = 80.;
const DEFAULT_PLATE_PITCH: f32 = 1.20;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Assembly,
    Exploded,
    Layers,
    Plates,
}
#[derive(Clone, Serialize)]
struct PrintRequest {
    root: PathBuf,
    home: PathBuf,
    members: Vec<(usize, Member)>,
}
#[derive(Default)]
pub(super) struct State {
    pub mode: Mode,
    camera: Camera,
    queue: crate::preview::Queue<PrintRequest>,
    rx: Option<Receiver<(u64, Result<PrintLayout, String>)>>,
    key: Vec<u8>,
    pub layout: Option<PrintLayout>,
    renderer: Option<Renderer>,
    pub(super) error: Option<String>,
    selected: Option<usize>,
    filter: Option<usize>,
    scope: u8,
}
impl State {
    pub fn special(&self) -> bool {
        matches!(self.mode, Mode::Layers | Mode::Plates)
    }
}
fn build(request: PrintRequest, cancel: &crate::preview::Cancel) -> Result<PrintLayout, String> {
    let mut layout = PrintLayout::default();
    let mut geometry = crate::scene::MeshPool::default();
    let catalog = Catalog::scan(request.root.clone())?;
    let mut copies = 0u64;
    for (index, member) in &request.members {
        crate::preview::check(Some(cancel))?;
        let p = &member.preset;
        let mut items = p.bom.clone();
        items.extend(
            catalog
                .exterior_components(
                    &p.selection,
                    &member.exterior,
                    p.assembly_kind == AssemblyKind::Sustainer,
                )?
                .into_iter()
                .map(|c| c.bom),
        );
        if !items.iter().any(|b| b.enabled) {
            continue;
        }
        copies += p
            .printing_bom()
            .iter()
            .chain(items.iter().skip(p.bom.len()))
            .filter(|b| b.enabled)
            .map(|b| u64::from(b.qty))
            .sum::<u64>();
        if copies > 1000 {
            return Err("平铺最多 1000 份 / Plate view supports up to 1000 copies".into());
        }
        for (job_index, job) in p.print_jobs(&member.name).into_iter().enumerate() {
            let mut job_items = job.items;
            if job_index == 0 {
                job_items.extend(items.iter().skip(p.bom.len()).cloned());
            }
            if !job_items.iter().any(|b| b.enabled) {
                continue;
            }
            let config = job.preset.print_config.clone().unwrap_or_default();
            let (inputs, plan) = crate::export::prepare_preview(
                &request.root,
                &job_items,
                &job.preset.label,
                &config,
                Some((&request.home, &job.preset.gridfinity, &job.preset.selection)),
            )
            .map_err(|e| format!("{}: {e}", job.name))?;
            crate::preview::check(Some(cancel))?;
            let start = layout.parts.len();
            layout.append(*index, &job.name, &inputs, plan, &config);
            for part in layout.parts[start..].iter_mut().flatten() {
                part.drawer = job.drawer;
            }
            geometry.share(&mut layout.scene)?;
        }
    }
    if layout.plates.is_empty() {
        return Err("打印清单没有勾选零件 / No checked parts in the print list".into());
    }
    layout.finish()?;
    Ok(layout)
}
impl App {
    fn print_request(&mut self) -> PrintRequest {
        self.commit_member();
        let mut members = if self.combination_view {
            self.members
                .iter()
                .enumerate()
                .filter(|(i, _)| match self.preview_modes.scope {
                    1 => self.member_selection.contains(i),
                    2 => *i == self.active_member,
                    _ => true,
                })
                .map(|(i, m)| (i, m.clone()))
                .collect()
        } else {
            vec![(
                self.active_member,
                Member {
                    name: self
                        .members
                        .get(self.active_member)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| self.scene.title.clone()),
                    preset: {
                        let mut p = self.member_snapshot();
                        if self.active_drawer.is_some() {
                            p.bom = crate::catalog::tower::drawer_items(&p.bom);
                        }
                        p
                    },
                    placement: Default::default(),
                    exterior: self
                        .members
                        .get(self.active_member)
                        .filter(|_| self.active_drawer.is_none())
                        .map(|m| m.exterior.clone())
                        .unwrap_or_default(),
                },
            )]
        };
        for (_, member) in &mut members {
            member.preset.apply_label_font(self.label_font);
        }
        PrintRequest {
            root: self.catalog.root.clone(),
            home: self.home.clone(),
            members,
        }
    }
    fn pump_print_layout(&mut self, ctx: &egui::Context) {
        let request = self.print_request();
        let key = serde_json::to_vec(&request).unwrap_or_default();
        let state = &mut self.preview_modes;
        if key != state.key {
            state.key = key;
            state
                .queue
                .schedule(request, Instant::now(), Duration::from_millis(300));
            state.layout = None;
            state.renderer = None;
            state.error = None;
            state.selected = None;
            state.filter = None;
        }
        if let Some((revision, result)) = state.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            state.rx = None;
            if state.queue.is_current(revision) {
                match result {
                    Ok(layout) => {
                        state.camera = Camera::default();
                        state.camera.yaw = 0.;
                        state.camera.pitch = DEFAULT_PLATE_PITCH;
                        state.camera.fit(&layout.scene, 0.);
                        let mut renderer = Renderer::new(self.renderer.state.clone());
                        renderer.upload(&layout.scene);
                        state.renderer = Some(renderer);
                        state.layout = Some(layout);
                    }
                    Err(e) => state.error = Some(e),
                }
            }
        }
        if state.rx.is_none()
            && let Some((revision, request)) = state.queue.take_ready(Instant::now())
        {
            let cancel = state.queue.cancellation();
            let (tx, rx) = mpsc::channel();
            state.rx = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((revision, build(request, &cancel)));
                ctx.request_repaint();
            });
        }
        if state.rx.is_some() || state.queue.has_pending() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    pub(super) fn preview_mode_fit(&mut self) {
        match self.preview_modes.mode {
            Mode::Layers => {
                let names: Vec<_> = self.members.iter().map(|m| m.name.clone()).collect();
                let layout = self.scene.layered(LAYER_GAP, &names, self.zh);
                self.preview_modes.camera.fit(&layout.scene, 0.);
            }
            Mode::Plates => {
                if let Some(layout) = &self.preview_modes.layout {
                    self.preview_modes.camera.fit(&layout.scene, 0.);
                }
            }
            _ if self.interior_editor_open() => {
                self.camera.fit(&self.interior_scene(), self.explosion);
            }
            _ => self.camera.fit(&self.scene, self.explosion),
        }
    }
    pub(super) fn preview_camera(&self) -> &Camera {
        if self.preview_modes.special() {
            &self.preview_modes.camera
        } else {
            &self.camera
        }
    }

    pub(super) fn preview_reset_view(&mut self) {
        let (yaw, pitch) = if self.preview_modes.mode == Mode::Plates {
            (0., DEFAULT_PLATE_PITCH)
        } else {
            let default = Camera::default();
            (default.yaw, default.pitch)
        };
        self.preview_set_view(yaw, pitch);
        self.preview_mode_fit();
    }
    pub(super) fn preview_set_view(&mut self, yaw: f32, pitch: f32) {
        let camera = if self.preview_modes.special() {
            &mut self.preview_modes.camera
        } else {
            &mut self.camera
        };
        camera.yaw = yaw;
        camera.pitch = pitch;
    }
    pub(super) fn preview_modes_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        let z = self.zh;
        if !self.scene.assembly || self.interior_editor_open() {
            self.preview_modes.mode = Mode::Assembly;
            return false;
        }
        // Existing per-member and smoke camera states remain the source for these two modes.
        if !self.preview_modes.special() {
            self.preview_modes.mode = if self.explosion == 0. {
                Mode::Assembly
            } else {
                Mode::Exploded
            };
        }
        let before = self.preview_modes.mode;
        let rect = ui.max_rect();
        egui::Area::new(egui::Id::new("stage-preview-modes"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(rect.center_bottom() - egui::vec2(0., 40.))
            .show(ctx, |ui| {
                ui.set_max_width((rect.width() - 48.).min(540.));
                appearance::floating_frame(ui).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (mode, label) in [
                            (Mode::Assembly, t(z, "整体", "Assembly")),
                            (Mode::Exploded, t(z, "爆炸", "Exploded")),
                            (Mode::Layers, t(z, "分层", "Layers")),
                            (Mode::Plates, t(z, "打印板", "Print plates")),
                        ] {
                            ui.selectable_value(&mut self.preview_modes.mode, mode, label);
                        }
                        if self.preview_modes.mode != Mode::Plates {
                            ui.separator();
                            if self.scene_key == "combination" && self.combination_view {
                                self.tower_preview_color_menu(ui);
                            }
                            if ui.button(t(z, "标牌…", "Label…")).clicked() {
                                self.open_preview_label(None, ctx);
                            }
                        }
                    });
                    if self.preview_modes.mode == Mode::Plates {
                        self.plate_toolbar(ui, ctx);
                    }
                });
            });
        if before != self.preview_modes.mode {
            self.dimensions = false;
            self.combination_move_source = None;
            match self.preview_modes.mode {
                Mode::Assembly => {
                    self.explosion = 0.;
                    self.camera.fit(&self.scene, 0.);
                }
                Mode::Exploded => {
                    self.explosion = 1.;
                    self.camera.fit(&self.scene, 1.);
                }
                Mode::Layers => {
                    self.preview_modes.camera = self.camera.clone();
                    self.preview_mode_fit();
                }
                Mode::Plates => {}
            }
        }
        match self.preview_modes.mode {
            Mode::Layers => {
                self.layers_view(ui, ctx);
                true
            }
            Mode::Plates => {
                self.plates_view(ui, ctx);
                true
            }
            _ => false,
        }
    }

    fn plate_toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        let scopes = [
            (0, t(z, "全部成员", "All members")),
            (1, t(z, "勾选成员", "Checked members")),
            (2, t(z, "当前成员", "Current member")),
        ];
        let mut edit = None;
        ui.horizontal_wrapped(|ui| {
            if let Some(layout) = &self.preview_modes.layout {
                let label = self.preview_modes.filter.map_or_else(
                    || t(z, "全部打印板", "All plates").to_owned(),
                    |i| format!("{} {}", t(z, "打印板", "Plate"), i + 1),
                );
                egui::ComboBox::from_id_salt("preview-plate-filter")
                    .selected_text(label)
                    .width(100.)
                    .height(300.)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.preview_modes.filter,
                            None,
                            t(z, "全部打印板", "All plates"),
                        );
                        for (i, plate) in layout.plates.iter().enumerate() {
                            let response = ui.selectable_value(
                                &mut self.preview_modes.filter,
                                Some(i),
                                format!("{} · {} / {}", i + 1, plate.name, plate.number + 1),
                            );
                            let slots = plate
                                .slots
                                .iter()
                                .map(u32::to_string)
                                .collect::<Vec<_>>()
                                .join(" + ");
                            response.on_hover_text(format!(
                                "{} {} · {} {slots}{}",
                                plate.copies,
                                t(z, "份", "copies"),
                                t(z, "耗材槽", "Slots"),
                                if plate.removable {
                                    t(z, " · 可拆标牌专用盘", " · Removable-label plate")
                                } else {
                                    ""
                                },
                            ));
                        }
                    });
                let selected_part = self
                    .preview_modes
                    .selected
                    .and_then(|i| layout.parts.get(i))
                    .and_then(Option::as_ref)
                    .filter(|part| {
                        self.preview_modes
                            .filter
                            .is_none_or(|plate| part.plate == plate)
                    });
                if let Some(part) = selected_part {
                    ui.small(format!("#{}", part.copy + 1))
                        .on_hover_text(format!(
                            "{} · {} {}",
                            part.path.rsplit('/').next().unwrap_or(""),
                            t(z, "打印板", "Plate"),
                            part.plate + 1
                        ));
                    if (crate::label::is_label(&part.path)
                        || part.path.starts_with(crate::gridfinity::PREFIX))
                        && ui.small_button(t(z, "编辑标牌…", "Edit label…")).clicked()
                    {
                        edit = Some(part.clone());
                    }
                }
            }
            ui.menu_button(t(z, "排盘选项", "Plate options"), |ui| {
                if self.combination_view {
                    for (scope, label) in scopes {
                        if ui
                            .selectable_value(&mut self.preview_modes.scope, scope, label)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                    ui.separator();
                }
                if ui.button(t(z, "打印参数…", "Print parameters…")).clicked() {
                    self.settings_page = Some(settings_panel::Page::Print);
                    ui.close();
                }
            });
        });
        if let Some(part) = edit {
            self.open_preview_label(Some(part), ctx);
        }
    }
    fn open_preview_label(&mut self, target: Option<PrintPart>, ctx: &egui::Context) {
        self.preview_modes.mode = Mode::Assembly;
        if let Some(info) = &target
            && self.combination_view
        {
            self.tower_edit_slot(info.member, info.drawer, ctx);
        } else if self.combination_view {
            self.focus_member(ctx);
        }
        if let Some(target) = target
            && target.path.starts_with(crate::gridfinity::PREFIX)
            && let Some(bin) = self
                .gridfinity
                .bins
                .iter()
                .find(|b| b.path() == target.path || b.label_path() == target.path)
        {
            self.gridfinity_selected = Some(bin.id);
            self.gridfinity_open = true;
        } else {
            self.label_open = true;
        }
    }
    fn layers_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        let names: Vec<_> = self.members.iter().map(|m| m.name.clone()).collect();
        let layout = self.scene.layered(LAYER_GAP, &names, z);
        let member_offsets = self.layer_reorder_offsets(ctx, LAYER_GAP, 0.);
        let reordering = self.pending_layer_reorder.is_some() || !member_offsets.is_empty();
        let size = ui.available_size().max(egui::vec2(10., 10.));
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        navigate(ctx, &response, rect, &mut self.preview_modes.camera);
        if !reordering && response.double_clicked() {
            self.preview_modes.camera.fit(&layout.scene, 0.);
        }
        if !reordering
            && response.clicked()
            && let Some(point) = response.interact_pointer_pos()
        {
            let (o, d) = self.preview_modes.camera.ray(point, rect);
            self.selected = layout.scene.pick(o, d, 0.).map(|h| h.part);
            if self.scene_key == "combination"
                && let Some(i) = self.selected
            {
                self.select_combination_part(i, ctx);
            }
        }
        let dpi = ctx.pixels_per_point();
        let id = self.renderer.render(
            0,
            [(size.x * dpi) as u32, (size.y * dpi) as u32],
            &layout.scene,
            &self.preview_modes.camera,
            ViewOptions {
                selected: self.selected,
                ghost: self.ghost,
                member_offsets,
                ..Default::default()
            },
        );
        paint_image(ui, id, rect);
        // Keep the legend outside the model so different-height neighbors do not overlap labels.
        let painter = ui.painter().with_clip_rect(rect);
        let mut y = rect.top() + 98.;
        for (label, _) in layout.labels {
            let galley = painter.layout(
                label,
                egui::FontId::proportional(12.),
                ui.visuals().hyperlink_color,
                (rect.width() * 0.4).clamp(120., 280.),
            );
            let pos = egui::pos2(rect.left() + 12., y);
            painter.rect_filled(
                egui::Rect::from_min_size(
                    pos - egui::vec2(4., 2.),
                    galley.size() + egui::vec2(8., 4.),
                ),
                4.,
                ui.visuals().window_fill,
            );
            y += galley.size().y + 8.;
            painter.galley(pos, galley, ui.visuals().hyperlink_color);
        }
        hint(ui, rect, z);
    }
    fn plates_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        self.pump_print_layout(ctx);
        if let Some(error) = &self.preview_modes.error {
            ui.centered_and_justified(|ui| {
                appearance::floating_frame(ui).show(ui, |ui| {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    if ui.button(t(z, "重新计算", "Retry")).clicked() {
                        self.preview_modes.key.clear();
                    }
                });
            });
            return;
        }
        let Some(mut layout) = self.preview_modes.layout.take() else {
            ui.centered_and_justified(|ui| {
                appearance::floating_frame(ui).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t(z, "正在准备打印板…", "Preparing print plates…"));
                    });
                });
            });
            return;
        };
        let printable_parts = layout.parts.iter().filter(|part| part.is_some()).count();
        let mut visibility_changed = false;
        for (index, part) in layout.scene.parts.iter_mut().enumerate() {
            let plate = layout.parts[index]
                .as_ref()
                .map_or_else(|| index - printable_parts, |part| part.plate);
            let visible = self
                .preview_modes
                .filter
                .is_none_or(|filter| filter == plate);
            visibility_changed |= visible != part.visible;
            part.visible = visible;
        }
        if visibility_changed {
            self.preview_modes.camera.fit(&layout.scene, 0.);
        }
        let size = ui.available_size().max(egui::vec2(10., 10.));
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        navigate(ctx, &response, rect, &mut self.preview_modes.camera);
        if response.double_clicked() {
            self.preview_modes.camera.fit(&layout.scene, 0.);
        }
        if response.clicked()
            && let Some(point) = response.interact_pointer_pos()
        {
            let (o, d) = self.preview_modes.camera.ray(point, rect);
            self.preview_modes.selected = layout
                .scene
                .pick(o, d, 0.)
                .map(|h| h.part)
                .filter(|&i| layout.parts[i].is_some());
        }
        let dpi = ctx.pixels_per_point();
        let id = self.preview_modes.renderer.as_mut().unwrap().render(
            0,
            [(size.x * dpi) as u32, (size.y * dpi) as u32],
            &layout.scene,
            &self.preview_modes.camera,
            ViewOptions::default(),
        );
        paint_image(ui, id, rect);
        paint_plates(
            ui,
            rect,
            &self.preview_modes.camera,
            &layout,
            self.preview_modes.filter,
            z,
        );
        if let Some(i) = self.preview_modes.selected
            && let Some(part) = layout.scene.parts.get(i).filter(|p| p.visible)
        {
            outline(ui, rect, &self.preview_modes.camera, part);
        }
        hint(ui, rect, z);
        self.preview_modes.layout = Some(layout);
    }
}
fn navigate(ctx: &egui::Context, response: &egui::Response, rect: egui::Rect, camera: &mut Camera) {
    if response.dragged_by(egui::PointerButton::Primary) {
        let d = ctx.input(|i| i.pointer.delta());
        camera.yaw -= d.x * 0.008;
        camera.pitch = (camera.pitch + d.y * 0.008).clamp(-1.55, 1.55);
    }
    if response.dragged_by(egui::PointerButton::Secondary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        let d = ctx.input(|i| i.pointer.delta());
        camera.pan[0] -= d.x / rect.width() * 2.;
        camera.pan[1] += d.y / rect.height() * 2.;
    }
    if response.hovered() {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        camera.zoom = (camera.zoom * (scroll * 0.002).exp()).clamp(0.05, 50.);
    }
}
fn paint_image(ui: &egui::Ui, id: egui::TextureId, rect: egui::Rect) {
    ui.painter().image(
        id,
        rect,
        egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
        Color32::WHITE,
    );
}
fn project(camera: &Camera, rect: egui::Rect, point: Vec3) -> Option<egui::Pos2> {
    let ndc = camera.matrix(rect.aspect_ratio()).project_point3(point);
    (0. ..=1.).contains(&ndc.z).then(|| {
        egui::pos2(
            rect.center().x + ndc.x * rect.width() * 0.5,
            rect.center().y - ndc.y * rect.height() * 0.5,
        )
    })
}
fn hint(ui: &egui::Ui, rect: egui::Rect, z: bool) {
    ui.painter().with_clip_rect(rect).text(
        rect.left_bottom() + egui::vec2(12., -12.),
        egui::Align2::LEFT_BOTTOM,
        t(
            z,
            "拖动旋转 · 右键平移 · 滚轮缩放 · 双击适配",
            "Drag orbit · Right drag pan · Scroll zoom · Double-click fit",
        ),
        egui::FontId::proportional(12.),
        Color32::GRAY,
    );
}
fn paint_plates(
    ui: &egui::Ui,
    rect: egui::Rect,
    camera: &Camera,
    layout: &PrintLayout,
    filter: Option<usize>,
    z: bool,
) {
    let painter = ui.painter().with_clip_rect(rect);
    for (i, plate) in layout
        .plates
        .iter()
        .enumerate()
        .filter(|(i, _)| filter.is_none_or(|f| *i == f))
    {
        let [ox, oy] = plate.origin;
        let point = |x, y| project(camera, rect, Vec3::new(ox + x, 0.05, -oy - y));
        // P1S's front-left keep-out, matching the bed used by the packer.
        let corners = [(0., 0.), (18., 0.), (18., 28.), (0., 28.)]
            .into_iter()
            .filter_map(|(x, y)| point(x, y))
            .collect::<Vec<_>>();
        if corners.len() == 4 {
            painter.add(egui::Shape::convex_polygon(
                corners,
                Color32::from_rgba_unmultiplied(180, 65, 60, 45),
                egui::Stroke::new(1., Color32::from_rgb(160, 75, 65)),
            ));
        }
        if let Some(p) = point(0., -12.) {
            let compact = filter.is_none()
                && point(256., -12.).is_some_and(|right| (right.x - p.x).abs() < 90.);
            let title = if filter.is_some() {
                format!(
                    "{} {} · {} / {} · {} {}{}",
                    t(z, "盘", "Plate"),
                    i + 1,
                    plate.name,
                    plate.number + 1,
                    plate.copies,
                    t(z, "份", "copies"),
                    if plate.removable {
                        t(z, " · 标牌", " · Labels")
                    } else {
                        ""
                    }
                )
            } else if compact {
                (i + 1).to_string()
            } else {
                format!(
                    "{} {} · {} {}",
                    t(z, "盘", "Plate"),
                    i + 1,
                    plate.copies,
                    t(z, "份", "copies")
                )
            };
            painter.text(
                p,
                egui::Align2::LEFT_TOP,
                title,
                egui::FontId::proportional(if compact { 9. } else { 12. }),
                Color32::from_rgb(175, 207, 222),
            );
            for (j, color) in plate.colors.iter().enumerate().filter(|_| !compact) {
                let c = Color32::from_rgb(
                    (color[0] * 255.) as u8,
                    (color[1] * 255.) as u8,
                    (color[2] * 255.) as u8,
                );
                painter.circle_filled(p + egui::vec2(5. + j as f32 * 14., 22.), 4., c);
            }
        }
    }
}
fn outline(ui: &egui::Ui, rect: egui::Rect, camera: &Camera, part: &crate::scene::Part) {
    let points: Vec<_> = (0..8)
        .map(|i| {
            project(
                camera,
                rect,
                part.pose.matrix(0.).transform_point3(Vec3::new(
                    if i & 1 == 0 {
                        part.mesh.min.x
                    } else {
                        part.mesh.max.x
                    },
                    if i & 2 == 0 {
                        part.mesh.min.y
                    } else {
                        part.mesh.max.y
                    },
                    if i & 4 == 0 {
                        part.mesh.min.z
                    } else {
                        part.mesh.max.z
                    },
                )),
            )
        })
        .collect();
    let painter = ui.painter().with_clip_rect(rect);
    for i in 0..8 {
        for bit in [1, 2, 4] {
            let j = i ^ bit;
            if i < j
                && let (Some(a), Some(b)) = (points[i], points[j])
            {
                painter.line_segment([a, b], egui::Stroke::new(1.5, Color32::LIGHT_BLUE));
            }
        }
    }
}

impl App {
    pub(super) fn preview_modes_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        let folder = self.home.join("output/preview-modes");
        let error = if self.smoke_start.elapsed() > Duration::from_secs(240) {
            Some(format!("Timeout: {}", self.status))
        } else if let Some(e) = &self.preview_modes.error {
            Some(e.clone())
        } else if !self.busy && !self.preview.has_pending() && self.failed_preview.is_some() {
            Some(self.status.clone())
        } else {
            None
        };
        if let Some(e) = error {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("error.txt"), e);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        if self.preview_modes.mode == Mode::Plates && self.preview_modes.layout.is_none() {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 {
            self.preview_mode_fit();
            if self.smoke_stage == 4 {
                self.preview_modes.camera.yaw = 0.;
                self.preview_modes.camera.pitch = std::f32::consts::FRAC_PI_2 - 0.0001;
            }
        }
        if self.smoke_frame >= 10 && !self.screenshot_requested {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        let Some(image) = image else {
            return;
        };
        let result = (|| -> Result<(), String> {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                folder.join(format!("mode-{}.png", self.smoke_stage)),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            if self.smoke_stage == 3 {
                let layout = self
                    .preview_modes
                    .layout
                    .as_ref()
                    .ok_or("Missing print layout")?;
                let expected: usize = self
                    .members
                    .iter()
                    .flat_map(|m| &m.preset.bom)
                    .filter(|b| b.enabled)
                    .map(|b| b.qty as usize)
                    .sum();
                if layout.plates.iter().map(|p| p.copies).sum::<usize>() != expected {
                    return Err("Print copies differ from checked member BOMs".into());
                }
                for member in 0..self.members.len() {
                    if !layout.parts.iter().flatten().any(|p| p.member == member) {
                        return Err("Missing member print geometry".into());
                    }
                }
            }
            if self.smoke_stage == 5 && (!self.label_open || self.label.text != "EDIT") {
                return Err("Label editor lost its member or text".into());
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::write(folder.join("error.txt"), e);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.screenshot_requested = false;
        self.smoke_frame = 0;
        match self.smoke_stage {
            0 => {
                self.preview_modes.mode = Mode::Exploded;
                self.explosion = 1.;
                self.camera.fit(&self.scene, 1.);
            }
            1 => {
                self.preview_modes.mode = Mode::Layers;
                self.preview_modes.camera = self.camera.clone();
            }
            2 => {
                self.preview_modes.mode = Mode::Plates;
            }
            3 => {
                let layout = self.preview_modes.layout.as_ref().unwrap();
                let selected = layout
                    .parts
                    .iter()
                    .position(|p| p.as_ref().is_some_and(|p| crate::label::is_label(&p.path)))
                    .unwrap();
                self.preview_modes.selected = Some(selected);
                self.preview_modes.filter = Some(layout.parts[selected].as_ref().unwrap().plate);
            }
            4 => {
                let layout = self.preview_modes.layout.as_ref().unwrap();
                let target = layout.parts[self.preview_modes.selected.unwrap()]
                    .clone()
                    .unwrap();
                self.open_preview_label(Some(target), ctx);
                self.label.text = "EDIT".into();
                self.assembly(ctx);
            }
            5 => {
                self.label_open = false;
                self.preview_modes.mode = Mode::Layers;
                self.preview_modes.camera = self.camera.clone();
            }
            6 => {
                self.preview_modes.mode = Mode::Plates;
            }
            _ => {
                let _ = std::fs::write(
                    folder.join("result.txt"),
                    format!(
                        "PASS: overall, exploded, vertical layers, all print plates, filtered top plate, label editor, single-member layers and refreshed print plates. GPU: {}",
                        self.renderer.state.adapter.get_info().name
                    ),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        self.smoke_stage += 1;
    }
}
