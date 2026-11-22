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
    planning: crate::export::Planning,
    project: String,
    members: Vec<(usize, Member)>,
}
#[derive(Default)]
pub(super) struct State {
    pub mode: Mode,
    camera: Camera,
    queue: crate::preview::Queue<PrintRequest>,
    rx: Option<Receiver<(u64, Result<PrintLayout, String>)>>,
    key: Vec<u8>,
    suspended: bool,
    last_plate_camera: Option<Camera>,
    pub layout: Option<PrintLayout>,
    renderer: Option<Renderer>,
    pub(super) error: Option<String>,
    selected: Option<usize>,
    filter: Option<usize>,
    active_plate: usize,
    scope: u8,
}
impl State {
    pub fn special(&self) -> bool {
        matches!(self.mode, Mode::Layers | Mode::Plates)
    }

    pub(super) fn release_inactive_plates(&mut self) {
        if self.mode == Mode::Plates {
            return;
        }
        if self.layout.take().is_some() || self.queue.has_pending() || self.rx.is_some() {
            self.renderer = None;
            self.queue.cancel();
            self.suspended = true;
        }
        // Drain late results and keep one worker at a time across view switches.
        let _ = self.take_worker_result();
    }

    fn take_worker_result(&mut self) -> Option<Result<PrintLayout, String>> {
        let result = match self.rx.as_ref()?.try_recv() {
            Ok((revision, result)) => self.queue.is_current(revision).then_some(result),
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => (!self.queue.has_pending())
                .then(|| Err("打印板任务意外结束 / Print layout worker stopped".into())),
        };
        self.rx = None;
        result
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.queue.cancel();
    }
}
fn build(request: PrintRequest, cancel: &crate::preview::Cancel) -> Result<PrintLayout, String> {
    let mut layout = PrintLayout::default();
    let mut geometry = crate::scene::MeshPool::default();
    let catalog = Catalog::scan(request.root.clone())?;
    let members = request.members.iter().map(|(i, member)| (*i, member));
    let jobs = print_list::checked_jobs(members, &catalog)?;
    for batch in print_list::batch_jobs(jobs, request.planning, &request.project) {
        let batch = batch.prepare(&request.root, &request.home, Some(cancel))?;
        layout.append(
            &batch.name,
            &batch.items,
            &batch.owners,
            batch.plan,
            &batch.config,
        );
        geometry.share(&mut layout.scene)?;
    }
    layout.finish()?;
    Ok(layout)
}
impl App {
    pub(super) fn open_print_list_plates(&mut self, checked_layers: bool) {
        self.preview_modes.scope = u8::from(checked_layers);
        self.preview_modes.mode = Mode::Plates;
    }

    pub(super) fn current_print_member(&self) -> Member {
        let mut preset = self.member_snapshot();
        let owner = self.members.get(self.active_member);
        if self.active_drawer.is_some() {
            preset.bom = crate::catalog::tower::drawer_items(&preset.bom);
        }
        Member {
            name: owner
                .map(|m| m.name.clone())
                .unwrap_or_else(|| self.scene.title.clone()),
            preset,
            placement: Default::default(),
            exterior: owner
                .filter(|_| self.active_drawer.is_none())
                .map(|m| m.exterior.clone())
                .unwrap_or_default(),
        }
    }
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
            vec![(self.active_member, self.current_print_member())]
        };
        for (_, member) in &mut members {
            member
                .preset
                .apply_label_settings(self.project_label_style, self.label_font);
        }
        PrintRequest {
            root: self.catalog.root.clone(),
            home: self.home.clone(),
            planning: self.export_options.planning,
            project: self.tower.name.clone(),
            members,
        }
    }
    fn pump_print_layout(&mut self, ctx: &egui::Context) -> bool {
        let request = self.print_request();
        let key = serde_json::to_vec(&request).unwrap_or_default();
        let state = &mut self.preview_modes;
        let changed = key != state.key;
        if changed || state.suspended {
            state.key = key;
            state.suspended = false;
            state
                .queue
                .schedule(request, Instant::now(), Duration::from_millis(300));
            state.layout = None;
            state.renderer = None;
            state.error = None;
            if changed {
                state.last_plate_camera = None;
                state.selected = None;
                state.filter = None;
                state.active_plate = 0;
            }
        }
        let mut restored_camera = false;
        if let Some(result) = state.take_worker_result() {
            match result {
                Ok(layout) => {
                    if let Some(camera) = &state.last_plate_camera {
                        state.camera = camera.clone();
                        restored_camera = true;
                        state.filter = state.filter.filter(|&i| i < layout.plates.len());
                        state.active_plate = state
                            .active_plate
                            .min(layout.plates.len().saturating_sub(1));
                        state.selected = state
                            .selected
                            .filter(|&i| layout.parts.get(i).is_some_and(Option::is_some));
                    } else {
                        state.camera = Camera::default();
                        state.camera.yaw = 0.;
                        state.camera.pitch = DEFAULT_PLATE_PITCH;
                        state.camera.fit(&layout.scene, 0.);
                    }
                    let mut renderer = Renderer::new(self.renderer.state.clone());
                    renderer.upload(&layout.scene);
                    state.renderer = Some(renderer);
                    state.layout = Some(layout);
                }
                Err(e) => state.error = Some(e),
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
        restored_camera
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
            _ => self.camera.fit(&self.scene, self.explosion),
        }
    }
    pub(super) fn focus_preview_part(&mut self) {
        let Some(index) = self.selected else { return };
        if let Some(part) = self.scene.parts.get(index).filter(|p| p.visible) {
            self.camera.fit(
                &Scene {
                    parts: vec![part.clone()],
                    ..Default::default()
                },
                self.explosion,
            );
            self.camera.zoom = 0.85;
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
        if !self.scene.assembly {
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
                crate::ui_style::floating_frame(ui).show(ui, |ui| {
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
        self.print_planning_ui(ui);
        let z = self.zh;
        let scopes = [
            (0, t(z, "全部成员", "All members")),
            (1, t(z, "勾选层", "Checked layers")),
            (2, t(z, "当前成员", "Current member")),
        ];
        let mut edit = None;
        ui.horizontal_wrapped(|ui| {
            if let Some(layout) = &self.preview_modes.layout {
                let label = self.preview_modes.filter.map_or_else(
                    || t(z, "全部打印板", "All plates").to_owned(),
                    |i| format!("{} {:02}", t(z, "打印板", "Plate"), i + 1),
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
                                format!(
                                    "{:02} · {} / {} · {} {}",
                                    i + 1,
                                    plate.name,
                                    plate.number + 1,
                                    plate.copies,
                                    t(z, "份", "copies")
                                ),
                            );
                            response.on_hover_ui(|ui| {
                                ui.label(plate.printer.name());
                                ui.label(if plate.textured {
                                    t(z, "纹理 PEI 板", "Textured PEI Plate")
                                } else {
                                    t(z, "高温打印板", "High Temp Plate")
                                });
                                if plate.removable {
                                    ui.small(t(z, "可拆标牌专用盘", "Removable-label plate"));
                                }
                                ui.horizontal_wrapped(|ui| {
                                    ui.small(t(z, "耗材槽", "Slots"));
                                    for (&slot, &[r, g, b, _]) in
                                        plate.slots.iter().zip(&plate.colors)
                                    {
                                        let (dot, _) = ui.allocate_exact_size(
                                            egui::Vec2::splat(11.),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle(
                                            dot.center(),
                                            4.5,
                                            Color32::from_rgb(
                                                (r * 255.).round() as u8,
                                                (g * 255.).round() as u8,
                                                (b * 255.).round() as u8,
                                            ),
                                            ui.visuals().widgets.noninteractive.bg_stroke,
                                        );
                                        ui.small(slot.to_string());
                                    }
                                });
                            });
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
    pub(super) fn open_preview_label(&mut self, target: Option<PrintPart>, ctx: &egui::Context) {
        self.tab = 0;
        self.tower.print_list = false;
        self.preview_modes.mode = Mode::Assembly;
        if let Some(info) = &target
            && !self.members.is_empty()
        {
            // A print job for one already-selected subdrawer has no nested drawer index.
            let drawer = if !self.combination_view && info.member == self.active_member {
                info.drawer.or(self.active_drawer)
            } else {
                info.drawer
            };
            if inline_edit::target_preset(
                &self.members,
                crate::scene::Target {
                    member: info.member,
                    drawer,
                },
            )
            .is_none()
            {
                self.status = t(
                    self.zh,
                    "打印零件对应的抽屉已不存在",
                    "This print part's drawer is no longer in the project",
                )
                .into();
                return;
            }
            self.tower_edit_slot(info.member, drawer, ctx);
        } else if self.combination_view {
            self.focus_member(ctx);
        }
        if let Some(target) = target
            && target.path.starts_with(crate::gridfinity::PREFIX)
        {
            if let Some(bin) = self
                .gridfinity
                .bins
                .iter()
                .find(|b| b.path() == target.path || b.label_path() == target.path)
            {
                self.gridfinity_selected = Some(bin.id);
                self.open_bin_editor(ctx);
            } else {
                self.status = t(
                    self.zh,
                    "当前抽屉中找不到打印板上的盒子",
                    "This bin is no longer in the drawer",
                )
                .into();
            }
        } else {
            self.label_open = true;
        }
    }
    pub(super) fn paint_preview_selection(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        scene: &Scene,
        camera: &Camera,
        explosion: f32,
    ) {
        let matrix = camera.matrix(rect.aspect_ratio());
        if let Some(index) = self.selected {
            if let Some(part) = scene.parts.get(index).filter(|p| p.visible) {
                workspace::paint_selection_box(
                    ui,
                    rect,
                    matrix * part.pose.matrix(explosion),
                    part.mesh.min,
                    part.mesh.max,
                );
            }
            return;
        }
        if self.scene_key != "combination" {
            return;
        }
        let target = self.inline_edit.target.unwrap_or(crate::scene::Target {
            member: self.active_member,
            drawer: self.active_drawer,
        });
        let mut lo = glam::Vec3::splat(f32::INFINITY);
        let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
        for part in scene
            .parts
            .iter()
            .filter(|p| p.visible && target.contains(p))
        {
            let pose = part.pose.matrix(explosion);
            for point in workspace::box_corners(part.mesh.min, part.mesh.max) {
                let point = pose.transform_point3(point);
                lo = lo.min(point);
                hi = hi.max(point);
            }
        }
        if lo.is_finite() && hi.is_finite() {
            workspace::paint_selection_box(ui, rect, matrix, lo, hi);
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
        if !reordering {
            self.paint_preview_selection(ui, rect, &layout.scene, &self.preview_modes.camera, 0.);
        }
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
        let restored_camera = self.pump_print_layout(ctx);
        if let Some(error) = &self.preview_modes.error {
            ui.centered_and_justified(|ui| {
                crate::ui_style::floating_frame(ui).show(ui, |ui| {
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
                crate::ui_style::floating_frame(ui).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t(z, "正在准备打印板…", "Preparing print plates…"));
                    });
                });
            });
            return;
        };
        if let Some(plate) = self.preview_modes.filter {
            self.preview_modes.active_plate = plate;
        }
        let first_bed = layout.parts.len() - layout.plates.len();
        let plate_for = |index: usize| {
            layout.parts[index]
                .as_ref()
                .map_or_else(|| index - first_bed, |part| part.plate)
        };
        let mut visibility_changed = false;
        for (index, part) in layout.scene.parts.iter_mut().enumerate() {
            let plate = plate_for(index);
            let visible = self
                .preview_modes
                .filter
                .is_none_or(|filter| filter == plate);
            visibility_changed |= visible != part.visible;
            part.visible = visible;
        }
        if visibility_changed && !restored_camera {
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
            let hit = layout.scene.pick(o, d, 0.).map(|hit| hit.part);
            if let Some(index) = hit {
                self.preview_modes.active_plate = plate_for(index);
            }
            self.preview_modes.selected = hit.filter(|&i| layout.parts[i].is_some());
        }
        let dpi = ctx.pixels_per_point();
        let id = self.preview_modes.renderer.as_mut().unwrap().render(
            0,
            [(size.x * dpi) as u32, (size.y * dpi) as u32],
            &layout.scene,
            &self.preview_modes.camera,
            ViewOptions {
                print_beds: Some(crate::render::PrintBedView {
                    active: self.preview_modes.active_plate,
                    dark: ui.visuals().dark_mode,
                    plates: &layout.plates,
                }),
                ..ViewOptions::default()
            },
        );
        paint_image(ui, id, rect);
        paint_plate_titles(
            ui,
            rect,
            &self.preview_modes.camera,
            &layout,
            self.preview_modes.filter,
            z,
        );
        paint_plate_numbers(
            ui,
            rect,
            &self.preview_modes.camera,
            &layout,
            self.preview_modes.filter,
            self.preview_modes.active_plate,
        );
        if let Some(i) = self.preview_modes.selected
            && let Some(part) = layout.scene.parts.get(i).filter(|p| p.visible)
        {
            outline(ui, rect, &self.preview_modes.camera, part);
        }
        hint(ui, rect, z);
        self.preview_modes.last_plate_camera = Some(self.preview_modes.camera.clone());
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
        egui::TextStyle::Small.resolve(ui.style()),
        ui.visuals().weak_text_color(),
    );
}
fn paint_plate_titles(
    ui: &egui::Ui,
    rect: egui::Rect,
    camera: &Camera,
    layout: &PrintLayout,
    filter: Option<usize>,
    zh: bool,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let (toolbar, hud) = ui.ctx().memory(|memory| {
        (
            memory.area_rect(egui::Id::new("stage-preview-modes")),
            memory.area_rect(egui::Id::new("stage-project-hud")),
        )
    });
    let ink = ui.visuals().text_color();
    let direction = camera.direction();
    for (i, plate) in layout
        .plates
        .iter()
        .enumerate()
        .filter(|(i, _)| filter.is_none_or(|f| *i == f))
    {
        let [x, y] = plate.origin;
        // The physical rim extends beyond the printable rectangle by different
        // amounts on each model. Put the title beyond the edge facing the camera.
        let bed = &layout.bed(i).mesh;
        let (start, end) = if direction.x.abs() > direction.z.abs() {
            let side = if direction.x > 0. {
                bed.max.x
            } else {
                bed.min.x
            };
            ([side, 0.], [side, -plate.printer.bed_size()[1]])
        } else {
            let edge = if direction.z >= 0. {
                bed.max.z
            } else {
                bed.min.z
            };
            ([bed.min.x, edge], [bed.max.x, edge])
        };
        let edge = |[dx, dz]: [f32; 2]| project(camera, rect, Vec3::new(x + dx, 0., -y + dz));
        let (Some(left), Some(right)) = (edge(start), edge(end)) else {
            continue;
        };
        let number = format!("{} {:02}", t(zh, "盘", "Plate"), i + 1);
        let copies = format!("{} {}", plate.copies, t(zh, "份", "copies"));
        let mut full = format!(
            "{number} · {} / {} · {copies}{}",
            plate.name,
            plate.number + 1,
            if plate.removable {
                t(zh, " · 标牌", " · Labels")
            } else {
                ""
            }
        );
        if filter.is_some() {
            full.push_str(" · ");
            full.push_str(plate.printer.name());
        }
        let font = egui::FontId::proportional(if filter.is_some() { 13. } else { 11. });
        let width = (right.x - left.x).abs() - 6.;
        let mut title = painter.layout(full, font.clone(), ink, (rect.width() - 36.).max(1.));
        if filter.is_none() && title.size().x > width {
            title = painter.layout_no_wrap(format!("{number} · {copies}"), font.clone(), ink);
            if title.size().x > width {
                title = painter.layout_no_wrap(number, font, ink);
            }
        }
        let mut pos = egui::pos2(left.x.min(right.x) + 2., left.y.max(right.y) + 2.);
        let bounds = egui::Rect::from_min_size(pos, title.size());
        let fits = rect.contains_rect(bounds) && toolbar.is_none_or(|r| !r.intersects(bounds));
        if filter.is_some() {
            if !fits {
                pos = hud.map_or(rect.left_top(), |r| r.left_bottom()) + egui::vec2(0., 5.);
            }
        } else if !fits || title.size().x > width {
            continue;
        }
        painter.galley(pos, title, ink);
    }
}

fn paint_plate_numbers(
    ui: &egui::Ui,
    rect: egui::Rect,
    camera: &Camera,
    layout: &PrintLayout,
    filter: Option<usize>,
    active_plate: usize,
) {
    // The plate index occupies the gutter outside the machine's front-right corner.
    if camera.direction().y <= 0. {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let origin = camera.center;
    let (Some(mid), Some(right), Some(back)) = (
        project(camera, rect, origin),
        project(camera, rect, origin + Vec3::X * 16.),
        project(camera, rect, origin - Vec3::Z * 16.),
    ) else {
        return;
    };
    // Fit the two digits into a 16 mm square as the camera tilts and zooms.
    let x_axis = right - mid;
    let y_axis = back - mid;
    let height = x_axis.rot90().dot(y_axis).abs() / x_axis.length().max(0.001);
    let font_size = (x_axis.length().min(height) * 0.9).min(25.);
    if font_size < 6. {
        return;
    }
    let angle = x_axis.angle();
    // On an oblique view a tall part can project past its own bed. Reserve
    // its screen bounds before painting labels so the part stays unobstructed.
    let view = camera.matrix(rect.aspect_ratio());
    let blockers: Vec<_> = layout
        .scene
        .parts
        .iter()
        .zip(&layout.parts)
        .filter(|(part, info)| part.visible && info.is_some())
        .map(|(part, _)| {
            let clip_from_part = view * part.pose.matrix(0.);
            let center = clip_from_part.transform_point3((part.mesh.min + part.mesh.max) * 0.5);
            let half = (part.mesh.max - part.mesh.min) * 0.5;
            let extent = clip_from_part.x_axis.truncate().abs() * half.x
                + clip_from_part.y_axis.truncate().abs() * half.y
                + clip_from_part.z_axis.truncate().abs() * half.z;
            (center, extent)
        })
        .collect();
    let half_diagonal = 0.85 * font_size;
    let label_extent = glam::Vec2::new(
        2. * half_diagonal / rect.width(),
        2. * half_diagonal / rect.height(),
    );
    for (i, plate) in layout.plates.iter().enumerate() {
        if filter.is_some_and(|selected| selected != i) {
            continue;
        }
        let [ox, oy] = plate.origin;
        let center = Vec3::new(ox + layout.bed(i).mesh.max.x + 9., 0., -oy - 8.);
        let Some(p) = project(camera, rect, center) else {
            continue;
        };
        let anchor = view.transform_point3(center);
        if blockers.iter().any(|(c, e)| {
            c.z - e.z < anchor.z
                && (anchor - *c)
                    .truncate()
                    .abs()
                    .cmple(e.truncate() + label_extent)
                    .all()
        }) {
            continue;
        }
        let ink = if i == active_plate {
            ui.visuals().hyperlink_color
        } else {
            ui.visuals().weak_text_color()
        };
        let galley = painter.layout_no_wrap(
            format!("{:02}", i + 1),
            egui::FontId::monospace(font_size),
            ink,
        );
        let origin = p - galley.size() * 0.5;
        painter.add(
            egui::epaint::TextShape::new(origin, galley, ink)
                .with_angle_and_anchor(angle, egui::Align2::CENTER_CENTER),
        );
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
                painter.line_segment([a, b], egui::Stroke::new(1.5, ui.visuals().hyperlink_color));
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
            if matches!(self.smoke_stage, 4 | 10) {
                self.preview_modes.camera.yaw = 0.;
                self.preview_modes.camera.pitch = std::f32::consts::FRAC_PI_2 - 0.0001;
            } else if self.smoke_stage == 8 {
                self.preview_modes.camera.yaw = -0.32;
                self.preview_modes.camera.pitch = 0.85;
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
                if self.preview_modes.active_plate != 0 || self.preview_modes.filter.is_some() {
                    return Err("The overview must open with the first tray active".into());
                }
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
                    let config = self.members[member].preset.print_config.as_ref().unwrap();
                    let mut parts = layout
                        .parts
                        .iter()
                        .flatten()
                        .filter(|p| p.member == member)
                        .peekable();
                    if parts.peek().is_none()
                        || parts.any(|p| {
                            let plate = &layout.plates[p.plate];
                            plate.textured != config.textured || plate.printer != config.printer
                        })
                    {
                        return Err("Missing member geometry or incorrect plate surface".into());
                    }
                }
            }
            if self.smoke_stage == 9 {
                let layout = self
                    .preview_modes
                    .layout
                    .as_ref()
                    .ok_or("Missing mixed-printer overview")?;
                if !layout
                    .plates
                    .iter()
                    .any(|p| p.printer == crate::print_plate::Printer::A1Mini)
                    || !layout
                        .plates
                        .iter()
                        .any(|p| p.printer == crate::print_plate::Printer::P1s)
                {
                    return Err(
                        "Changing one member's printer did not rebuild the mixed overview".into(),
                    );
                }
            }
            if matches!(self.smoke_stage, 4 | 8 | 10) {
                let layout = self
                    .preview_modes
                    .layout
                    .as_ref()
                    .ok_or("Missing single-plate layout")?;
                let filter = self
                    .preview_modes
                    .filter
                    .ok_or("Missing single-plate filter")?;
                if self.preview_modes.active_plate != filter
                    || layout.plates[filter].textured != (self.smoke_stage == 4)
                {
                    return Err(format!(
                        "Stage {}: filter={filter}, active={}, textured={}",
                        self.smoke_stage,
                        self.preview_modes.active_plate,
                        layout.plates[filter].textured
                    ));
                }
                if self.smoke_stage == 10
                    && (layout.plates[filter].printer != crate::print_plate::Printer::A1Mini
                        || (layout.bed(filter).mesh.size.x - 184.).abs() > 0.1)
                {
                    return Err("The A1 mini preview retained the larger printer's bed".into());
                }
                let mut visible_beds = 0;
                let mut visible_parts = 0;
                for (part, metadata) in layout.scene.parts.iter().zip(&layout.parts) {
                    if let Some(metadata) = metadata {
                        if part.visible != (metadata.plate == filter) {
                            return Err("Single-plate filter lost printable geometry".into());
                        }
                        visible_parts += usize::from(part.visible);
                    } else if part.visible {
                        if part.pose.path != "@print-bed" || part.pose.instance as usize != filter {
                            return Err("Single-plate filter retained the wrong bed".into());
                        }
                        visible_beds += 1;
                    }
                }
                if visible_beds != 1 || visible_parts == 0 {
                    return Err("Single-plate view must show one tray with printable parts".into());
                }
            }
            if self.smoke_stage == 11 {
                let layout = self
                    .preview_modes
                    .layout
                    .as_ref()
                    .ok_or("Missing project layout")?;
                let mut owners = vec![None; layout.plates.len()];
                let shared = layout
                    .parts
                    .iter()
                    .flatten()
                    .any(|part| owners[part.plate].get_or_insert(part.member) != &part.member);
                if !shared {
                    return Err("Whole-project planning left every plate member-specific".into());
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
                self.selected = self.scene.parts.iter().position(|p| {
                    crate::scene::Target::of(p).member == 1 && p.pose.path.ends_with("Facade.stl")
                });
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
                self.export_options.plate.textured = false;
                self.preview_modes.mode = Mode::Plates;
            }
            7 => {
                let layout = self.preview_modes.layout.as_ref().unwrap();
                self.preview_modes.filter = layout
                    .plates
                    .iter()
                    .enumerate()
                    .filter(|(_, plate)| !plate.textured)
                    .max_by_key(|(_, p)| p.copies)
                    .map(|(i, _)| i);
            }
            8 => {
                self.export_options.plate.printer = crate::print_plate::Printer::A1Mini;
                for item in &mut self.bom {
                    item.enabled = crate::label::is_label(&item.path);
                }
                self.preview_modes.scope = 0;
                self.combination_view = true;
            }
            9 => {
                self.preview_modes.filter = self
                    .preview_modes
                    .layout
                    .as_ref()
                    .unwrap()
                    .plates
                    .iter()
                    .position(|plate| plate.printer == crate::print_plate::Printer::A1Mini);
            }
            10 => {
                self.commit_member();
                let config = crate::print_plate::Config::default();
                for member in &mut self.members {
                    member.preset.print_config = Some(config.clone());
                }
                self.export_options.plate = config;
                self.export_options.planning = crate::export::Planning::Combined;
                self.preview_modes.filter = None;
            }
            _ => {
                let _ = std::fs::write(
                    folder.join("result.txt"),
                    format!(
                        "PASS: overall, exploded, layers, print plates and both bed surfaces, labels, top and oblique views, mixed printers, filtered A1 mini plate and shared project plates. GPU: {}",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_print_plates_releases_geometry_and_discards_late_results() {
        let mut state = State::default();
        let now = Instant::now();
        state.queue.schedule(
            PrintRequest {
                root: PathBuf::new(),
                home: PathBuf::new(),
                planning: Default::default(),
                project: String::new(),
                members: vec![],
            },
            now,
            Duration::ZERO,
        );
        let (revision, _) = state.queue.take_ready(now).unwrap();
        let cancel = state.queue.cancellation();
        let (tx, rx) = mpsc::channel();
        state.mode = Mode::Plates;
        state.rx = Some(rx);
        state.layout = Some(PrintLayout::default());
        state.filter = Some(2);
        state.release_inactive_plates();
        assert!(state.layout.is_some());

        state.mode = Mode::Assembly;
        state.release_inactive_plates();
        assert!(state.layout.is_none() && state.suspended && state.rx.is_some());
        assert_eq!(state.filter, Some(2));
        assert!(crate::preview::check(Some(&cancel)).is_err());
        assert!(tx.send((revision, Ok(PrintLayout::default()))).is_ok());
        assert!(state.take_worker_result().is_none());
        assert!(state.rx.is_none());

        let (tx, rx) = mpsc::channel();
        state.rx = Some(rx);
        drop(tx);
        assert!(state.take_worker_result().unwrap().is_err());
        assert!(state.rx.is_none());
    }
}
