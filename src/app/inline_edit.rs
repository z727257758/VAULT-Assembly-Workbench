//! One assembly viewport with a compact editor bound to a member/drawer identity.
use super::*;
use crate::scene::Target;
use glam::Vec3;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saving_a_subdrawer_preserves_its_sibling_and_housing() {
        let preset: Preset = toml::from_str(
            "bom=[]\n[selection]\nwidth='M'\ndepth=5\nheight='2F'\nhidden_key=false",
        )
        .unwrap();
        let mut member = Member {
            name: "housing".into(),
            preset,
            placement: Default::default(),
            exterior: Default::default(),
        };
        let mut child = member.preset.clone();
        child.selection.height = "1F".into();
        child.label.text = "first".into();
        member.preset.drawers = vec![child.clone(), child];
        let mut members = vec![member.clone(), member];
        let target = Target {
            member: 0,
            drawer: Some(1),
        };
        let mut edit = target_preset(&members, target).unwrap().clone();
        edit.label.text = "edited".into();
        store_target(&mut members, target, edit.clone());
        assert_eq!(members[0].preset.drawers[1].label.text, "edited");
        assert_eq!(members[0].preset.drawers[0].label.text, "first");
        assert_eq!(members[0].preset.selection.height, "2F");
        assert_eq!(members[1].preset.drawers[1].label.text, "first");
        let before = toml::to_string(&members[0].preset).unwrap();
        store_target(
            &mut members,
            Target {
                member: 0,
                drawer: Some(3),
            },
            edit,
        );
        assert_eq!(before, toml::to_string(&members[0].preset).unwrap());
    }
    #[test]
    fn detail_camera_leaves_room_for_editor_at_both_window_sizes() {
        let scene = Scene {
            parts: vec![crate::scene::Part {
                mesh: std::sync::Arc::new(crate::mesh::Mesh {
                    vertices: vec![],
                    min: Vec3::ZERO,
                    max: Vec3::new(145., 52., 200.),
                    size: Vec3::new(145., 52., 200.),
                }),
                pose: crate::scene::Pose {
                    path: "drawer.stl".into(),
                    instance: 0,
                    body: None,
                    position: [30., 120., 0.],
                    rotation: [0.; 3],
                    travel: [0.; 3],
                    travel_frame: None,
                },
                color: [1.; 4],
                visible: true,
                source_size: Vec3::new(145., 52., 200.),
                source_bodies: 1,
            }],
            assembly: true,
            ..Default::default()
        };
        for size in [egui::vec2(690., 560.), egui::vec2(1080., 820.)] {
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let panel = (size.x * 0.43).clamp(270., 340.) + 60.;
            let mut camera = Camera::default();
            fit_detail(&mut camera, &scene, viewport, panel);
            let matrix = camera.matrix(viewport.aspect_ratio());
            let part = &scene.parts[0];
            for i in 0..8 {
                let point = Vec3::new(
                    if i & 1 == 0 { 0. } else { 145. },
                    if i & 2 == 0 { 0. } else { 52. },
                    if i & 4 == 0 { 0. } else { 200. },
                );
                let p = matrix.project_point3(part.pose.matrix(0.).transform_point3(point));
                let x = (p.x + 1.) * size.x * 0.5;
                assert!(
                    x >= panel && x <= size.x,
                    "Detail obscured by editor: {x} / {panel} / {}",
                    size.x
                );
                assert!(p.y.abs() <= 0.9 && (0. ..=1.).contains(&p.z));
            }
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    pub target: Option<Target>,
    pub(super) overview: Option<(Camera, bool)>,
    pub pending_focus: bool,
    path: Option<String>,
    viewport: Option<egui::Rect>,
}
pub(super) fn target_preset(members: &[Member], target: Target) -> Option<&Preset> {
    let preset = &members.get(target.member)?.preset;
    match target.drawer {
        Some(d) => preset.drawers.get(d),
        None => Some(preset),
    }
}
pub(super) fn store_target(members: &mut [Member], target: Target, preset: Preset) {
    let Some(member) = members.get_mut(target.member) else {
        return;
    };
    if let Some(drawer) = target.drawer {
        if let Some(child) = member.preset.drawers.get_mut(drawer) {
            *child = preset;
        }
    } else {
        member.preset = preset;
    }
}
fn fit_detail(camera: &mut Camera, scene: &Scene, viewport: egui::Rect, panel_width: f32) {
    camera.fit(scene, 0.);
    let matrix = camera.matrix(viewport.aspect_ratio());
    let mut projected = egui::Rect::NOTHING;
    // Only project mesh envelopes here; source vertices stay off the UI path.
    for part in scene.parts.iter().filter(|p| p.visible) {
        let transform = matrix * part.pose.matrix(0.);
        for i in 0..8 {
            let point = Vec3::new(
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
            );
            let p = transform.project_point3(point);
            projected.extend_with(egui::pos2(p.x, p.y));
        }
    }
    if !projected.is_finite() {
        return;
    }
    let usable = ((viewport.width() - panel_width - 48.) / viewport.width()).clamp(0.18, 1.);
    camera.zoom = (2. * usable / projected.width().max(0.01))
        .min(1.45 / projected.height().max(0.01))
        .clamp(0.05, 2.);
    camera.pan[0] = -panel_width / viewport.width();
}
impl App {
    pub(super) fn open_inline_target(
        &mut self,
        member: usize,
        drawer: Option<usize>,
        ctx: &egui::Context,
    ) {
        let target = Target { member, drawer };
        if target_preset(&self.members, target).is_none() {
            return;
        }
        if self.inline_edit.open && self.inline_edit.target == Some(target) {
            self.inline_edit.pending_focus = true;
            self.inline_edit.path = None;
            ctx.request_repaint();
            return;
        }
        self.finish_member_edit();
        let overview = self
            .inline_edit
            .overview
            .take()
            .unwrap_or_else(|| (self.camera.clone(), self.ghost));
        let viewport = self.inline_edit.viewport;
        let key = self.scene_key.clone();
        let baseline = self.workflow_snapshot();
        self.restore_member(member);
        if let Some(drawer) = drawer {
            self.restore_editor(self.members[member].preset.drawers[drawer].clone());
            self.active_drawer = Some(drawer);
        }
        self.member_edit_baseline = Some(baseline);
        self.combination_view = true;
        self.scene_key = key;
        self.explosion = 0.;
        self.dimensions = false;
        self.preview_modes.mode = preview_modes::Mode::Assembly;
        self.tab = 0;
        self.tower.print_list = false;
        self.tower.open_layer = Some(self.members[member].placement.level);
        self.inline_edit = State {
            open: true,
            target: Some(target),
            overview: Some(overview),
            pending_focus: true,
            viewport,
            ..Default::default()
        };
        if self.scene_key != "combination" {
            self.assembly(ctx);
        }
        ctx.request_repaint();
    }
    pub(super) fn close_inline_editor(&mut self, ctx: &egui::Context) {
        if !self.inline_edit.open {
            return;
        }
        let overview = self.inline_edit.overview.take();
        let key = self.scene_key.clone();
        self.finish_member_edit();
        if self.active_member < self.members.len() {
            self.restore_member(self.active_member);
        }
        self.combination_view = true;
        self.scene_key = key;
        if let Some((camera, ghost)) = overview {
            self.camera = camera;
            self.ghost = ghost;
        }
        ctx.request_repaint();
    }
    pub(super) fn local_scene_instance(&self, local: u32) -> u32 {
        if self.scene_key == "combination" {
            Target {
                member: self.active_member,
                drawer: self.active_drawer,
            }
            .instance(local)
        } else {
            local
        }
    }
    pub(super) fn focus_preview_path(&mut self, path: String, ctx: &egui::Context) {
        if self.combination_view {
            self.inline_edit.path = Some(path);
            self.inline_edit.pending_focus = true;
            ctx.request_repaint();
        } else {
            self.single(path, ctx);
        }
    }
    fn focus_inline(&mut self, viewport: egui::Rect, panel_width: f32) {
        if self.scene_key != "combination" || self.preview.has_pending() || self.busy {
            return;
        }
        let target = self.inline_edit.target.unwrap_or(Target {
            member: self.active_member,
            drawer: self.active_drawer,
        });
        let mut focused = self.scene.target_scene(target);
        if let Some(path) = &self.inline_edit.path {
            focused.parts.retain(|p| p.pose.path == *path);
        } else {
            focused
                .parts
                .retain(|p| self.bom.iter().any(|b| b.path == p.pose.path));
        }
        if focused.parts.is_empty() {
            self.inline_edit.pending_focus = false;
            self.inline_edit.path = None;
            self.status = t(
                self.zh,
                "当前目标没有可预览的零件",
                "No preview parts for this target",
            )
            .into();
            return;
        }
        for p in &mut focused.parts {
            p.visible = true;
        }
        fit_detail(&mut self.camera, &focused, viewport, panel_width);
        if self.inline_edit.path.is_some() {
            self.selected = self.scene.parts.iter().position(|p| {
                target.contains(p) && Some(&p.pose.path) == self.inline_edit.path.as_ref()
            });
        }
        self.inline_edit.pending_focus = false;
        self.inline_edit.path = None;
    }
    pub(super) fn inline_editor_ui(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        self.inline_edit.viewport = Some(rect);
        if !self.inline_edit.open {
            if self.inline_edit.pending_focus {
                self.focus_inline(rect, 0.);
            }
            return;
        }
        let width = (rect.width() * 0.43)
            .clamp(270., 340.)
            .min(rect.width() - 32.);
        if self.inline_edit.pending_focus {
            self.focus_inline(rect, width + 60.);
        }
        let z = self.zh;
        let mut close = false;
        egui::Area::new(egui::Id::new("inline-member-editor"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.left_top() + egui::vec2(16., 106.))
            .show(ctx, |ui| {
                appearance::floating_frame(ui)
                    .inner_margin(14.)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            ui.strong(format!(
                                "{}{} · {}",
                                self.selection.width, self.selection.depth, self.selection.height
                            ));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    close = ui
                                        .button("×")
                                        .on_hover_text(t(
                                            z,
                                            "完成编辑，查看整体",
                                            "Finish editing and view project",
                                        ))
                                        .clicked();
                                    if ui.small_button(t(z, "特写", "Focus")).clicked() {
                                        self.inline_edit.pending_focus = true;
                                        self.inline_edit.path = None;
                                    }
                                },
                            );
                        });
                        ui.add(
                            egui::Label::new(RichText::new(self.editor_context()).small().weak())
                                .truncate(),
                        );
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_salt("inline-member-scroll")
                            .max_height((rect.height() - 250.).max(180.))
                            .show(ui, |ui| {
                                self.inline_contents_ui(ui, ctx);
                            });
                    });
            });
        if close {
            self.close_inline_editor(ctx);
        }
    }
    fn inline_contents_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        {
            let mut kind = self.assembly_kind;
            ui.horizontal(|ui| {
                let label = ui.label(t(z, "类型", "Type"));
                if self.active_drawer.is_some() {
                    label.on_hover_text(t(
                        z,
                        "类型切换应用于整个共用外壳单元",
                        "Type changes apply to the entire shared housing",
                    ));
                }
                ui.selectable_value(&mut kind, AssemblyKind::Drawer, t(z, "抽屉", "Drawer"));
                ui.selectable_value(&mut kind, AssemblyKind::Sustainer, t(z, "翻盖箱", "Box"));
            });
            if kind != self.assembly_kind {
                let member = self.active_member;
                let drawer = self.active_drawer;
                self.close_inline_editor(ctx);
                let drawer = match self.tower_action(tower::Action::Kind(member, kind), ctx) {
                    Ok(()) => None,
                    Err(error) => {
                        self.status = error;
                        drawer
                    }
                };
                self.open_inline_target(member, drawer, ctx);
                return;
            }
        }
        if self.assembly_kind == AssemblyKind::Sustainer {
            let before = self.sustainer_options;
            if matches!(self.selection.width.as_str(), "L" | "XL") {
                ui.horizontal_wrapped(|ui| {
                    ui.label(t(z, "箱体", "Body"));
                    for (body, cn, en) in [
                        (SustainerBody::Flat, "平底", "Flat"),
                        (SustainerBody::Dividers, "隔板槽", "Dividers"),
                        (SustainerBody::Sealed, "密封分区", "Sealed"),
                    ] {
                        ui.selectable_value(&mut self.sustainer_options.body, body, t(z, cn, en));
                    }
                });
            }
            if before != self.sustainer_options {
                match self.configured_bom(&self.bom) {
                    Ok(bom) => {
                        self.bom = bom;
                        self.assembly(ctx);
                    }
                    Err(error) => {
                        self.sustainer_options = before;
                        self.status = error;
                    }
                }
            }
        }
        self.inserts_ui(ui, ctx);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(t(z, "标签", "Label"));
            if ui.small_button(t(z, "样式…", "Style…")).clicked() {
                self.label_open = true;
            }
        });
        let label = self.label.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.label.text)
                    .hint_text(t(z, "输入标签文字", "Label text"))
                    .char_limit(48)
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            self.label.enabled = !self.label.text.trim().is_empty();
            if let Err(error) = self.label.validate() {
                self.label = label;
                self.status = error;
            } else {
                self.assembly(ctx);
            }
        }
        if self.assembly_kind == AssemblyKind::Sustainer
            && ui
                .add(
                    egui::Slider::new(&mut self.sustainer_angle, 0. ..=110.).text(t(
                        z,
                        "开盖 °",
                        "Lid °",
                    )),
                )
                .changed()
        {
            self.assembly(ctx);
        }
        ui.small(t(
            z,
            "修改自动保留在当前组合中。",
            "Changes are kept in this project.",
        ));
    }
}

impl App {
    pub(super) fn inserts_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        if self.assembly_kind == AssemblyKind::Sustainer {
            ui.strong(t(z, "箱子内部配置", "Box inserts"));
            let supported =
                crate::sustainer_dividers::supported(&self.selection, self.sustainer_options);
            if !supported {
                ui.small(t(
                    z,
                    "将箱体切换为隔板槽或密封分区后，即可配置隔板。",
                    "Choose a Dividers or Sealed body to configure inserts.",
                ));
            }
            ui.add_enabled_ui(supported, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(t(z, "配置收纳隔板…", "Configure dividers…"))
                        .clicked()
                    {
                        self.sustainer_divider_editor.open = true;
                    }
                    if ui
                        .add_enabled(
                            !self.sustainer_dividers.items.is_empty(),
                            egui::Button::new(t(z, "移除全部隔板", "Remove all dividers")),
                        )
                        .clicked()
                    {
                        self.sustainer_dividers.items.clear();
                        self.sustainer_divider_editor.reset();
                        self.assembly(ctx);
                    }
                });
                ui.label(format!(
                    "{} {}",
                    self.sustainer_dividers.items.len(),
                    t(z, "块收纳隔板", "storage dividers")
                ));
            });
        }
        if self.assembly_kind == AssemblyKind::Drawer {
            if !self.inline_edit.open {
                ui.strong(t(z, "抽屉内部配置", "Drawer inserts"));
            }
            let has_dividers = self.divider_layout.enabled
                || self
                    .bom
                    .iter()
                    .any(|b| b.enabled && b.path.contains("/Grid Dividers/"));
            let mut mode = if has_dividers {
                2
            } else if self
                .bom
                .iter()
                .any(|b| b.enabled && b.path.contains("/Gridfinity Baseplate/"))
            {
                1
            } else {
                0
            };
            let previous = mode;
            let baseplate = self
                .catalog
                .accessories(&self.selection)
                .into_iter()
                .filter(|e| e.path.contains("/Gridfinity Baseplate/"))
                .min_by_key(|e| e.path.contains("Handle"))
                .map(|e| e.path.clone());
            ui.horizontal_wrapped(|ui| {
                ui.label(t(z, "内部", "Interior"));
                // Gridfinity is a baseplate on the flat floor, not a third interior type.
                let mut interior = mode == 2;
                ui.selectable_value(&mut interior, false, t(z, "平底", "Flat"));
                ui.add_enabled_ui(crate::dividers::Grid::new(&self.selection).is_ok(), |ui| {
                    ui.selectable_value(&mut interior, true, t(z, "分隔板", "Dividers"));
                });
                if interior != (mode == 2) {
                    mode = if interior { 2 } else { 0 };
                }
            });
            ui.add_enabled_ui(mode != 2 && baseplate.is_some(), |ui| {
                let mut enabled = mode == 1;
                if ui
                    .checkbox(
                        &mut enabled,
                        t(z, "Gridfinity 底板", "Gridfinity baseplate"),
                    )
                    .changed()
                {
                    mode = if enabled { 1 } else { 0 };
                }
            });
            if mode != previous {
                self.divider_layout.remove(&mut self.bom, &self.selection);
                for b in &mut self.bom {
                    if b.path.contains("/Gridfinity Baseplate/") {
                        b.enabled = false;
                    }
                }
                self.layout_cache
                    .remove(&crate::dividers::spec(&self.selection));
                self.divider_editor.open = false;
                self.divider_editor.reset();
                if mode == 1 {
                    if let Some(path) = baseplate {
                        self.select_accessory(path, true, 1);
                    }
                } else if mode == 2 {
                    self.divider_layout.enabled = true;
                }
                self.assembly(ctx);
            }
            if !self.inline_edit.open {
                ui.label(
                RichText::new(t(
                    z,
                    "底板与隔板互斥，切换会移除当前内部配置。",
                    "Baseplates and dividers are exclusive. Switching removes the current insert.",
                ))
                .small()
                .color(Color32::GRAY),
            );
            }
            if mode == 2 {
                ui.horizontal_wrapped(|ui| {
                    let label = if self.divider_layout.items.is_empty() {
                        t(z, "添加收纳隔板", "Add storage dividers")
                    } else {
                        t(z, "编辑收纳隔板", "Edit storage dividers")
                    };
                    if ui.button(label).clicked() {
                        self.divider_editor.open = true;
                    }
                    if ui
                        .button(t(z, "移除收纳隔板", "Remove storage dividers"))
                        .clicked()
                    {
                        self.remove_dividers(ctx);
                    }
                });
                ui.label(format!(
                    "{} {}",
                    self.divider_layout.items.len(),
                    t(z, "块收纳隔板", "storage dividers")
                ));
            }
            if mode == 1 {
                let plates: Vec<_> = self
                    .catalog
                    .accessories(&self.selection)
                    .into_iter()
                    .filter(|e| e.path.contains("/Gridfinity Baseplate/"))
                    .collect();
                let variant_name = |path: &str| {
                    if path.contains("2 Handles") {
                        t(z, "双提手", "Two handles")
                    } else if path.contains("1 Handle") {
                        t(z, "单提手", "One handle")
                    } else {
                        t(z, "标准版", "Standard")
                    }
                };
                let current = self
                    .bom
                    .iter()
                    .find(|b| b.enabled && b.path.contains("/Gridfinity Baseplate/"))
                    .map(|b| b.path.clone())
                    .unwrap_or_default();
                let mut chosen = current.clone();
                ui.label(t(z, "底板类型", "Baseplate type"));
                egui::ComboBox::from_id_salt("gridfinity-baseplate-type")
                    .width(ui.available_width())
                    .selected_text(variant_name(&current))
                    .show_ui(ui, |ui| {
                        for plate in &plates {
                            ui.selectable_value(
                                &mut chosen,
                                plate.path.clone(),
                                variant_name(&plate.path),
                            );
                        }
                    });
                if chosen != current {
                    let mut candidate = self.gridfinity.clone();
                    candidate.baseplate = chosen.clone();
                    match candidate.validate(&self.selection) {
                        Ok(()) => {
                            self.select_accessory(chosen, true, 1);
                            self.assembly(ctx);
                            self.gridfinity_error = None;
                        }
                        Err(e) => {
                            self.gridfinity_error = Some(format!("{e}；请先调整盒子布局"));
                        }
                    }
                }
                if let Ok(grid) =
                    crate::gridfinity::grid_for(&self.selection, &self.gridfinity.baseplate)
                {
                    ui.small(format!(
                        "{} × {} {} · {}",
                        grid[0],
                        grid[1],
                        t(z, "格", "cells"),
                        t(z, "底板随抽屉规格匹配", "Baseplate follows drawer size")
                    ));
                }
                if let Some(error) = &self.gridfinity_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                ui.add_space(6.);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            crate::gridfinity::grid(&self.selection).is_ok(),
                            egui::Button::new(t(z, "编辑 Gridfinity 盒子", "Edit Gridfinity bins")),
                        )
                        .clicked()
                    {
                        self.gridfinity_open = true;
                    }
                    if !self.gridfinity.bins.is_empty()
                        && ui.button(t(z, "移除全部盒子", "Remove all bins")).clicked()
                    {
                        self.gridfinity.bins.clear();
                        self.assembly(ctx);
                    }
                });
                ui.label(format!(
                    "{} {}",
                    self.gridfinity.bins.len(),
                    t(z, "个盒子", "bins")
                ));
                ui.small(t(
                    z,
                    "盒高默认随层高配置，可在编辑器中调整。",
                    "Bin heights follow the drawer by default; adjust them in the editor.",
                ));
            }
        }
    }
}
