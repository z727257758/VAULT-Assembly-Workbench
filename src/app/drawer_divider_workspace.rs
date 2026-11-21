//! Edit the selected drawer against its hole grid in a temporary opened preview.
use super::*;
use crate::{
    constraints::{MotionKind, PartId},
    divider_editor::Editor,
    dividers::{self, Grid, Layout, PITCH, Placement},
    render::MaterialOverride,
    scene::Target,
};
use egui::{Pos2, Rect, Vec2};
use glam::{Mat4, Vec3};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    opened: Option<workspace::OpenInterior>,
    orbit: Vec2,
    pan: Vec2,
    zoom_log: f32,
    drag: Option<Drag>,
    smoke_projection: Option<Projection>,
}

impl State {
    pub(super) fn project_cell(&self, cell: [f32; 2]) -> Option<Pos2> {
        let view = self.smoke_projection.as_ref()?;
        Some(view.screen(view.grid.point(cell)))
    }
}

struct Drag {
    index: usize,
    origin: Vec2,
    plane: f32,
}

struct Projection {
    camera: Camera,
    rect: Rect,
    frame: Mat4,
    floor: f32,
    grid: Grid,
}

impl Projection {
    fn cell_at(&self, pos: Pos2, plane: f32) -> Option<Vec2> {
        let (origin, ray) = self.camera.ray(pos, self.rect);
        let inverse = self.frame.inverse();
        let origin = inverse.transform_point3(origin);
        let ray = inverse.transform_vector3(ray);
        if ray.y.abs() < 0.0001 {
            return None;
        }
        let distance = (plane - origin.y) / ray.y;
        let point = origin + distance * ray;
        let [x, rear] = self.grid.point([0., 0.]);
        (distance >= 0. && point.is_finite())
            .then(|| egui::vec2(point.x - x, -point.z - rear) / PITCH)
    }

    fn hole_at(&self, pos: Pos2) -> Option<[f32; 2]> {
        let cell = self.cell_at(pos, self.floor)?;
        let [x, y] = self.grid.point([cell.x, cell.y]);
        ((self.grid.left..=self.grid.right).contains(&x)
            && (self.grid.rear..=self.grid.front).contains(&y))
        .then_some([cell.x, cell.y])
    }

    fn screen(&self, [x, y]: [f32; 2]) -> Pos2 {
        let matrix = self.camera.matrix(self.rect.aspect_ratio()) * self.frame;
        let point = matrix.project_point3(Vec3::new(x, self.floor, -y));
        self.rect.center()
            + egui::vec2(point.x * self.rect.width(), -point.y * self.rect.height()) * 0.5
    }

    fn outline(&self, p: &Placement) -> [Pos2; 4] {
        let [x0, y0, x1, y1] = self.grid.bounds(p);
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|point| self.screen(point))
    }
}

impl App {
    pub(super) fn open_drawer_divider_editor(&mut self, ctx: &egui::Context) {
        if self.assembly_kind != AssemblyKind::Drawer || Grid::new(&self.selection).is_err() {
            return;
        }
        self.close_bin_editor();
        self.close_sustainer_divider_editor();
        let target = Target {
            member: self.active_member,
            drawer: self.active_drawer,
        };
        let member_preview = !self.combination_view
            && inline_edit::target_preset(&self.members, target)
                .is_some_and(|p| p.assembly_kind == AssemblyKind::Drawer);
        if member_preview {
            self.open_inline_target(target.member, target.drawer, ctx);
        }
        self.tab = 0;
        self.tower.print_list = false;
        self.preview_modes.mode = preview_modes::Mode::Assembly;
        self.drawer_divider_view = Default::default();
        self.divider_editor.open = true;
        if !member_preview && !self.drawer_divider_scene_matches() {
            self.assembly(ctx);
        }
        ctx.request_repaint();
    }

    pub(super) fn close_drawer_divider_editor(&mut self) {
        self.divider_editor.close();
        self.drawer_divider_view = Default::default();
    }

    fn drawer_divider_scene_matches(&self) -> bool {
        self.scene.assembly
            && if self.combination_view {
                self.scene_key == "combination"
            } else {
                self.scene_key == dividers::spec(&self.selection)
            }
    }

    pub(super) fn drawer_divider_workspace(&mut self, ui: &mut egui::Ui) {
        let Ok(grid) = Grid::new(&self.selection) else {
            self.close_drawer_divider_editor();
            ui.ctx().request_repaint();
            return;
        };
        let z = self.zh;
        let mut layout = self.divider_layout.clone();
        let (model_rect, card_rect) = workspace::model_and_plan_rects(ui.max_rect());
        let mut model = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("drawer-divider-model")
                .max_rect(model_rect),
        );
        model.set_clip_rect(model_rect);
        self.drawer_divider_model(&mut model, grid, &mut layout);

        let mut card = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("drawer-divider-inspector")
                .max_rect(card_rect),
        );
        card.set_clip_rect(card_rect);
        let mut close = false;
        crate::ui_style::floating_frame(&card).show(&mut card, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let spec = dividers::spec(&self.selection);
                let title = format!("{} {spec}", t(z, "抽屉隔板", "Drawer dividers"));
                if self.combination_view
                    && let Some(member) = self.members.get(self.active_member)
                {
                    let level = member.placement.level + 1;
                    ui.strong(if z {
                        format!("第 {level} 层 · {title}")
                    } else {
                        format!("Layer {level} · {title}")
                    });
                } else {
                    ui.strong(title);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let done = ui.add(crate::ui_style::primary_button(t(z, "完成", "Done")));
                    if self.smoke {
                        self.drawer_divider_smoke.done_rect = Some(done.rect);
                    }
                    close = done.clicked();
                });
            });
            if let Some(drawer) = self.active_drawer {
                ui.weak(format!("{} {}", t(z, "子抽屉", "Drawer"), drawer + 1));
            }
            self.interior_opacity_ui(ui);
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("drawer-divider-properties")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let plan = self.divider_editor.panel(
                        ui,
                        &self.catalog,
                        &self.selection,
                        &mut layout,
                        z,
                    );
                    if self.smoke {
                        self.drawer_divider_smoke.plan_rect = Some(plan);
                        self.drawer_divider_smoke.add_rect = self.divider_editor.plan_add_rect;
                    }
                    if self.failed_preview.is_some() && !self.busy && !self.preview.has_pending() {
                        ui.colored_label(ui.visuals().error_fg_color, &self.status);
                        if ui.button(t(z, "重新加载", "Reload")).clicked() {
                            self.assembly(ui.ctx());
                        }
                    }
                });
        });
        if layout != self.divider_layout {
            self.divider_layout = layout;
            self.assembly(ui.ctx());
        }
        if close {
            self.close_drawer_divider_editor();
            ui.ctx().request_repaint();
        }
    }

    fn drawer_divider_model(&mut self, ui: &mut egui::Ui, grid: Grid, layout: &mut Layout) {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1., 1.)),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let message = |text: &str| {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(14.),
                ui.visuals().text_color(),
            );
        };
        let loading = t(self.zh, "正在加载抽屉装配…", "Loading drawer assembly…");
        let pending = self.busy || self.preview.has_pending();
        if self.smoke {
            self.drawer_divider_smoke.rendered = None;
            self.drawer_divider_view.smoke_projection = None;
        }
        if !self.drawer_divider_scene_matches() {
            message(loading);
            return;
        }
        let target = if self.combination_view {
            Target {
                member: self.active_member,
                drawer: self.active_drawer,
            }
        } else {
            Target {
                member: 0,
                drawer: None,
            }
        };
        let state = &mut self.drawer_divider_view;
        let opened = match workspace::OpenInterior::get(
            &mut state.opened,
            &self.scene,
            target,
            MotionKind::Drawer,
        ) {
            Ok(opened) => opened,
            Err(error) => {
                message(if pending { loading } else { &error });
                return;
            }
        };
        let Some(drawer_index) = opened
            .parts
            .iter()
            .position(|p| target.contains(p) && p.pose.path.contains("/Boxes/"))
        else {
            message(t(
                self.zh,
                "当前装配没有对应的抽屉",
                "Drawer unavailable in this assembly",
            ));
            return;
        };
        let drawer = &opened.parts[drawer_index];
        let frame = drawer.pose.matrix(0.);
        let mut camera = workspace::drawer_camera(drawer, frame, rect, true);
        camera.yaw += state.orbit.x;
        camera.pitch = (camera.pitch + state.orbit.y).clamp(0.5, 1.54);
        camera.pan[0] += state.pan.x;
        camera.pan[1] += state.pan.y;
        camera.zoom *= state.zoom_log.exp();
        let projection = Projection {
            camera,
            rect,
            frame,
            floor: dividers::drawer_floor(drawer),
            grid,
        };

        let opacity = self.interior_exterior_opacity as f32 / 100.;
        let mut scene = (*opened).clone();
        let mut materials = vec![MaterialOverride::Opacity(opacity); scene.parts.len()];
        scene.parts[drawer_index].visible = true;
        materials[drawer_index] = MaterialOverride::Opacity(1.);
        let divider_items = match_dividers(&mut scene, target, &projection, layout);
        let pick = |p: Pos2| {
            let (origin, direction) = projection.camera.ray(p, rect);
            let mut candidates = vec![scene.parts[drawer_index].clone()];
            let mut indices = vec![drawer_index];
            for &index in divider_items.keys() {
                candidates.push(scene.parts[index].clone());
                indices.push(index);
            }
            let hit = Scene {
                parts: candidates,
                ..Default::default()
            }
            .pick(origin, direction, 0.)?;
            let index = indices[hit.part];
            let world = scene.parts[index]
                .pose
                .matrix(0.)
                .transform_point3(hit.point);
            Some((
                divider_items.get(&index).copied(),
                frame.inverse().transform_point3(world).y,
            ))
        };

        let (press, pointer, cancel) = ui.input(|i| {
            (
                i.pointer.press_origin(),
                i.pointer.interact_pos(),
                !i.focused
                    || i.key_pressed(egui::Key::Escape)
                    || i.pointer.button_pressed(egui::PointerButton::Secondary),
            )
        });
        if response.secondary_clicked() && self.divider_editor.placing() {
            self.divider_editor.cancel_gesture();
        }
        if cancel || pending {
            state.drag = None;
        }
        let camera_before = (state.orbit, state.pan, state.zoom_log);
        let delta = ui.input(|i| i.pointer.delta());
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            state.pan += egui::vec2(-2. * delta.x / rect.width(), 2. * delta.y / rect.height());
        }
        if response.hovered() && state.drag.is_none() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            state.zoom_log = (state.zoom_log + scroll * 0.002).clamp(-1., 1.5);
        }
        if response.double_clicked_by(egui::PointerButton::Primary)
            && !self.divider_editor.placing()
        {
            state.orbit = Vec2::ZERO;
            state.pan = Vec2::ZERO;
            state.zoom_log = 0.;
        }
        let mut preview = None;
        if !cancel && !pending && self.failed_preview.is_none() {
            if self.divider_editor.placing() {
                if let Some(cell) = response.hover_pos().and_then(|p| projection.hole_at(p)) {
                    preview = self
                        .divider_editor
                        .placement_candidate(layout, &self.selection, grid, cell)
                        .map(|candidate| (layout.items.len(), candidate));
                    if response.clicked_by(egui::PointerButton::Primary) {
                        self.divider_editor.place(
                            layout,
                            &self.catalog,
                            &self.selection,
                            grid,
                            cell,
                        );
                        preview = None;
                    }
                }
            } else {
                if response.drag_started_by(egui::PointerButton::Primary)
                    && let Some(press) = press
                    && let Some((Some(index), plane)) = pick(press)
                    && let Some(origin) = projection.cell_at(press, plane)
                {
                    self.divider_editor.select(Some(index));
                    state.drag = Some(Drag {
                        index,
                        origin,
                        plane,
                    });
                }
                if let Some(drag) = &state.drag
                    && let Some(pointer) = pointer.filter(|p| rect.contains(*p))
                    && let Some(point) = projection.cell_at(pointer, drag.plane)
                {
                    preview = Editor::drag_position(layout, grid, drag.index, point - drag.origin)
                        .map(|candidate| (drag.index, candidate));
                }
                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    state.drag = None;
                    if let Some((_, candidate)) = preview.take() {
                        self.divider_editor.commit(
                            layout,
                            candidate,
                            &self.catalog,
                            &self.selection,
                        );
                    }
                } else if response.dragged_by(egui::PointerButton::Primary) && state.drag.is_none()
                {
                    state.orbit.x -= delta.x * 0.008;
                    state.orbit.y = (state.orbit.y + delta.y * 0.008).clamp(-0.65, 0.65);
                }
                if response.clicked_by(egui::PointerButton::Primary)
                    && let Some(pos) = pointer
                {
                    self.divider_editor
                        .select(pick(pos).and_then(|(index, _)| index));
                }
            }
        }
        if camera_before != (state.orbit, state.pan, state.zoom_log) {
            ui.ctx().request_repaint();
        }
        let selected = self.divider_editor.selected_index();
        for (&part, &index) in &divider_items {
            let rgba = Editor::divider_color(Some(index) == selected).to_array();
            materials[part] = MaterialOverride::Color(rgba.map(|v| f32::from(v) / 255.));
        }
        let dpi = ui
            .ctx()
            .pixels_per_point()
            .min(4096. / rect.width().max(rect.height()));
        let texture = self.renderer.render_with_materials(
            0,
            [
                (rect.width() * dpi).round() as u32,
                (rect.height() * dpi).round() as u32,
            ],
            &scene,
            &projection.camera,
            ViewOptions::default(),
            &materials,
        );
        if self.smoke {
            self.drawer_divider_smoke.rendered = Some((scene, materials));
        }
        painter.image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
        let paint_outline = |placement: &Placement, color: Color32| {
            painter.add(egui::Shape::convex_polygon(
                projection.outline(placement).to_vec(),
                color.gamma_multiply(0.28),
                egui::Stroke::new(2., color),
            ));
        };
        if pending {
            for (index, placement) in layout.items.iter().enumerate() {
                if !divider_items.values().any(|&item| item == index) {
                    paint_outline(placement, Editor::divider_color(Some(index) == selected));
                }
            }
        }
        if let Some((index, candidate)) =
            preview.or_else(|| self.divider_editor.drag_preview(layout, grid))
            && let Some(placement) = candidate.items.get(index)
        {
            let color = if candidate.validate(&self.catalog, &self.selection).is_ok() {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().error_fg_color
            };
            paint_outline(placement, color);
        }
        if pending {
            painter.text(
                rect.left_bottom() + egui::vec2(16., -16.),
                egui::Align2::LEFT_BOTTOM,
                t(self.zh, "正在更新抽屉模型…", "Updating drawer model…"),
                egui::FontId::proportional(13.),
                ui.visuals().text_color(),
            );
        }
        if self.smoke {
            state.smoke_projection = Some(projection);
        }
    }
}

// Compare the solved pose with the requested hole coordinates. A pending assembly
// can still contain the previous copy at the same instance number after a deletion.
fn match_dividers(
    scene: &mut Scene,
    target: Target,
    view: &Projection,
    layout: &Layout,
) -> HashMap<usize, usize> {
    let mut parts = HashMap::new();
    let mut bodies = HashMap::new();
    let mut fittings = HashMap::new();
    let inverse = view.frame.inverse();
    for (part_index, part) in scene.parts.iter_mut().enumerate() {
        if !target.contains(part) || !part.pose.path.contains("/Grid Dividers/") {
            continue;
        }
        if !layout.enabled {
            continue;
        }
        part.visible = false;
        if dividers::code(&part.pose.path).is_none() {
            fittings.insert(PartId::of(part), part_index);
            continue;
        }
        let local = inverse * part.pose.matrix(0.);
        let point = local.transform_point3(Vec3::ZERO);
        let axis = local.transform_vector3(Vec3::X);
        let vertical = axis.z.abs() > axis.x.abs();
        if let Some(index) = layout.items.iter().position(|p| {
            let [x, y] = view.grid.center(p);
            p.path == part.pose.path
                && p.vertical == vertical
                && (point.x - x).abs() < 0.1
                && (point.z + y).abs() < 0.1
        }) {
            part.visible = true;
            parts.insert(part_index, index);
            bodies.insert(PartId::of(part), index);
        }
    }
    for relation in &scene.relations {
        if let (Some(&item), Some(&child)) =
            (bodies.get(&relation.parent), fittings.get(&relation.child))
        {
            scene.parts[child].visible = true;
            parts.insert(child, item);
        }
    }
    parts
}
