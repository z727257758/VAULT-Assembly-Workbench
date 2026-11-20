//! A case-local editing view. Slot guides share the coordinates measured on the preview worker.
use super::*;
use crate::{
    render::MaterialOverride,
    scene::{Part, Target},
    sustainer_dividers::{self as dividers, Axis, Geometry, Layout, Placement},
};
use egui::{Pos2, Rect, Stroke};
use glam::Vec3;

struct Slot {
    placement: Placement,
    edge: [Pos2; 2],
    foot: [Pos2; 2],
    installed: bool,
    pending_mesh: bool,
    edge_depth: [f32; 2],
    drop_depth: f32,
}

impl Slot {
    fn depth_at(&self, fraction: f32) -> f32 {
        self.edge_depth[0] + (self.edge_depth[1] - self.edge_depth[0]) * fraction
    }

    fn face_depth(&self, pos: Pos2) -> Option<f32> {
        if !self.installed {
            return None;
        }
        let along = self.edge[1] - self.edge[0];
        let down = self.foot[0] - self.edge[0];
        let rel = pos - self.edge[0];
        let cross = |u: egui::Vec2, v: egui::Vec2| u.x * v.y - u.y * v.x;
        let det = cross(along, down);
        if det.abs() < 0.01 {
            return None;
        }
        let t = cross(rel, down) / det;
        let h = cross(along, rel) / det;
        ((0. ..=1.).contains(&t) && (0. ..=1.).contains(&h))
            .then(|| self.depth_at(t) + self.drop_depth * h)
    }
}

#[derive(Default)]
pub(super) struct State {
    slots: Vec<Slot>,
    shown: Option<Layout>,
    yaw: f32,
    pitch_offset: f32,
    zoom_log: f32,
    pan: [f32; 2],
}

impl State {
    fn front_face(&self, pos: Pos2) -> Option<(&Slot, f32)> {
        self.slots
            .iter()
            .filter_map(|s| Some((s, s.face_depth(pos)?)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    fn hit(&self, pos: Pos2) -> Option<&Slot> {
        // An empty floor slot can project through a tall installed divider.
        // Only its visible portion is painted and responds to the pointer.
        let face = self.front_face(pos);
        let edges = self.slots.iter().filter_map(|s| {
            let [a, b] = s.edge;
            let ab = b - a;
            let t = ((pos - a).dot(ab) / ab.length_sq().max(0.001)).clamp(0., 1.);
            let distance = pos.distance(a + t * ab);
            (distance <= 7.
                && face.is_none_or(|(front, z)| {
                    s.placement == front.placement || s.depth_at(t) <= z + 0.0001
                }))
            .then_some((s, distance))
        });
        edges
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(s, _)| s)
            .or_else(|| face.map(|(s, _)| s))
    }

    pub(super) fn slot_point(&self, placement: &Placement) -> Option<Pos2> {
        let slot = self.slots.iter().find(|s| &s.placement == placement)?;
        [0.35, 0.65, 0.5, 0.2, 0.8]
            .into_iter()
            .map(|t| slot.edge[0].lerp(slot.edge[1], t))
            .find(|&p| self.hit(p).is_some_and(|s| &s.placement == placement))
    }

    pub(super) fn loaded(&self, layout: &Layout) -> bool {
        self.shown.as_ref() == Some(layout)
    }

    fn camera(&self, rect: Rect, lo: Vec3, hi: Vec3) -> Camera {
        let mut camera = Camera {
            center: (lo + hi) * 0.5,
            radius: ((hi - lo).length() * 0.5).max(1.),
            yaw: self.yaw,
            pitch: (1.23 + self.pitch_offset).clamp(0.48, 1.54),
            ..Default::default()
        };
        let matrix = camera.matrix(rect.aspect_ratio());
        let extent = corners(lo, hi)
            .into_iter()
            .map(|p| matrix.project_point3(p).truncate().abs().max_element())
            .fold(0.01, f32::max);
        camera.zoom = 0.88 / extent * self.zoom_log.exp();
        camera.pan = self.pan;
        camera
    }
}

fn corners(lo: Vec3, hi: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        )
    })
}

fn bounds(part: &Part) -> (Vec3, Vec3) {
    let matrix = part.pose.matrix(0.);
    corners(part.mesh.min, part.mesh.max)
        .into_iter()
        .map(|p| matrix.transform_point3(p))
        .fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        )
}

// The solver seats each divider on a measured row or column. Match by the
// solved slot, since instance numbers may change when a previous item is removed.
fn occupied_slot(g: &Geometry, lo: Vec3, hi: Vec3) -> Option<Placement> {
    let center = (lo + hi) * 0.5;
    let (axis, lane, coordinate, slots) = if hi.x - lo.x > hi.z - lo.z {
        let lane = g
            .lanes
            .iter()
            .position(|l| (l[0]..=l[1]).contains(&center.x))?;
        (Axis::Width, lane, center.z, &g.rows)
    } else {
        (Axis::Depth, 0, center.x, &g.columns)
    };
    let slot = slots.iter().position(|v| (v - coordinate).abs() < 0.25)?;
    Some(Placement { axis, lane, slot })
}

impl App {
    pub(super) fn divider_model_ui(
        &mut self,
        ui: &mut egui::Ui,
        g: &Geometry,
        layout: &mut Layout,
    ) -> bool {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1., 1.)),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        let state = &mut self.divider_model;
        state.slots.clear();
        state.shown = None;
        let target = Target {
            member: if self.scene_key == "combination" {
                self.active_member
            } else {
                0
            },
            drawer: None,
        };
        let housing = self.scene.parts.iter().find(|p| {
            target.contains(p)
                && p.pose.path.starts_with(crate::sustainer::PREFIX)
                && p.pose.path.contains("/Modules/")
        });
        let Some(housing) = housing else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                t(self.zh, "正在载入箱体…", "Loading case…"),
                egui::FontId::proportional(15.),
                ui.visuals().text_color(),
            );
            return false;
        };
        // Normalize a translated or rotated member to the same front-down view
        // as the inspector. The actual assembly and the user's camera stay intact.
        let local = g.housing_pose * housing.pose.matrix(0.).inverse();
        let mut scene = Scene::default();
        let mut materials = Vec::new();
        let mut heights = BTreeMap::new();
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        let mut original_count = 0;
        let selection = self
            .sustainer_divider_editor
            .selected
            .and_then(|i| layout.items.get(i));
        for source in self.scene.parts.iter().filter(|p| target.contains(p)) {
            let path = &source.pose.path;
            let board = path.starts_with(dividers::PREFIX);
            let shell = path.contains("/Modules/") || path.ends_with("Facade.stl");
            let hinge = path.contains("/Hinges/") && !path.contains("Top");
            let holder = path.ends_with("/LOCK HOLDER.stl");
            // The pin set contains both moving and fixed bodies. Only the small
            // pins have the facade's stationary holder as their parent.
            let pin = path.contains("/Rods/")
                || (path.contains("/1 SET OF PINS") && {
                    let id = crate::constraints::PartId::of(source);
                    self.scene
                        .relations
                        .iter()
                        .any(|r| r.child == id && r.parent.path.ends_with("/LOCK HOLDER.stl"))
                });
            if !(board || shell || hinge || holder || pin) {
                continue;
            }
            let mut part = source.clone();
            let (_, rotation, position) =
                (local * source.pose.matrix(0.)).to_scale_rotation_translation();
            let (x, y, z) = rotation.to_euler(glam::EulerRot::XYZ);
            part.pose.rotation = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
            part.pose.position = position.to_array();
            part.visible = true;
            let (min, max) = bounds(&part);
            let color = if board {
                original_count += 1;
                let Some(slot) = occupied_slot(g, min, max).filter(|p| layout.items.contains(p))
                else {
                    continue;
                };
                let selected = selection == Some(&slot);
                heights.insert(slot, max.y);
                if selected {
                    [0.35, 0.69, 0.51, 1.]
                } else {
                    [0.83, 0.87, 0.81, 1.]
                }
            } else if shell {
                lo = lo.min(min);
                hi = hi.max(max);
                [0.82, 0.85, 0.86, 1.]
            } else if pin {
                [0.96, 0.48, 0.18, 1.]
            } else {
                [0.44, 0.49, 0.50, 1.]
            };
            materials.push(MaterialOverride::Color(color));
            scene.parts.push(part);
        }
        let delta = response.drag_delta();
        if response.dragged_by(egui::PointerButton::Primary) {
            state.yaw -= delta.x * 0.008;
            state.pitch_offset = (state.pitch_offset + delta.y * 0.008).clamp(-0.75, 0.31);
        } else if response.dragged_by(egui::PointerButton::Secondary) {
            state.pan[0] -= delta.x * 2. / rect.width();
            state.pan[1] += delta.y * 2. / rect.height();
        }
        if response.hovered() {
            state.zoom_log =
                (state.zoom_log + ui.input(|i| i.smooth_scroll_delta.y) * 0.002).clamp(-1., 1.5);
        }
        let camera = state.camera(rect, lo, hi);
        let matrix = camera.matrix(rect.aspect_ratio());
        let screen = |p: Vec3| {
            let n = matrix.project_point3(p);
            rect.center() + egui::vec2(n.x * rect.width(), -n.y * rect.height()) * 0.5
        };
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
            &camera,
            ViewOptions::default(),
            &materials,
        );
        painter.image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
        for placement in g.slots() {
            let Some(line) = g.line(&placement) else {
                continue;
            };
            let installed = layout.items.contains(&placement);
            let height = heights.get(&placement).copied();
            let top = height.unwrap_or(if installed { hi.y - 4. } else { g.floor + 0.3 });
            let edge = line.map(|[x, z]| Vec3::new(x, top, z));
            let edge_depth = edge.map(|p| matrix.project_point3(p).z);
            let foot = line.map(|[x, z]| Vec3::new(x, g.floor + 0.3, z));
            state.slots.push(Slot {
                placement,
                edge: edge.map(screen),
                foot: foot.map(screen),
                edge_depth,
                drop_depth: matrix.project_point3(foot[0]).z - edge_depth[0],
                installed,
                pending_mesh: installed && height.is_none(),
            });
        }
        let hover = response
            .hover_pos()
            .and_then(|pos| state.hit(pos))
            .map(|s| s.placement.clone());
        for slot in &state.slots {
            let hovered = hover.as_ref() == Some(&slot.placement);
            if slot.pending_mesh {
                painter.add(egui::Shape::convex_polygon(
                    vec![slot.edge[0], slot.edge[1], slot.foot[1], slot.foot[0]],
                    Color32::from_rgba_unmultiplied(71, 140, 102, 100),
                    Stroke::NONE,
                ));
            }
            if hovered || slot.pending_mesh {
                painter.line_segment(
                    slot.edge,
                    Stroke::new(3., ui.visuals().selection.stroke.color),
                );
            } else if !slot.installed {
                let [start, end] = slot.edge;
                let length = start.distance(end).max(1.);
                let stroke = Stroke::new(1., Color32::from_black_alpha(75));
                for i in (0..length.ceil() as usize).step_by(10) {
                    let a = i as f32 / length;
                    let b = ((i as f32 + 4.) / length).min(1.);
                    let middle = (a + b) * 0.5;
                    if state
                        .front_face(start.lerp(end, middle))
                        .is_none_or(|(_, z)| slot.depth_at(middle) <= z + 0.0001)
                    {
                        painter.line_segment([start.lerp(end, a), start.lerp(end, b)], stroke);
                    }
                }
            }
        }
        if hover.is_some() {
            response
                .clone()
                .on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        if self.smoke
            && !self.busy
            && !self.preview.has_pending()
            && self.failed_preview.is_none()
            && original_count == layout.items.len()
            && heights.len() == layout.items.len()
        {
            state.shown = Some(layout.clone());
        }
        response.clicked()
            && hover.is_some_and(|p| self.sustainer_divider_editor.toggle_slot(layout, g, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_floor_guide_behind_an_installed_divider_cannot_capture_its_click() {
        let line = |x, y| [egui::pos2(x, y), egui::pos2(150., y)];
        let board = Placement {
            axis: Axis::Width,
            lane: 0,
            slot: 0,
        };
        let empty = Placement {
            slot: 1,
            ..board.clone()
        };
        let state = State {
            slots: vec![
                Slot {
                    placement: board.clone(),
                    edge: line(55., 15.),
                    foot: line(55., 50.),
                    edge_depth: [0.3; 2],
                    drop_depth: 0.1,
                    installed: true,
                    pending_mesh: false,
                },
                Slot {
                    placement: empty.clone(),
                    edge: line(10., 35.),
                    foot: line(10., 35.),
                    edge_depth: [0.5; 2],
                    drop_depth: 0.,
                    installed: false,
                    pending_mesh: false,
                },
            ],
            ..Default::default()
        };
        assert_eq!(state.hit(egui::pos2(70., 35.)).unwrap().placement, board);
        assert_eq!(state.hit(egui::pos2(40., 35.)).unwrap().placement, empty);
    }
}
