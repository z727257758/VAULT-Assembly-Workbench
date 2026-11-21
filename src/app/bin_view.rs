//! Drawer-local editing against the measured sockets of the loaded baseplate.
use super::*;
use crate::{
    constraints::{MotionKind, PartId},
    gridfinity::{self, Bin, Layout},
    gridfinity_editor::{Editor, GridView},
    mesh::Mesh,
    render::MaterialOverride,
    scene::{Part, Target},
};
use glam::Vec3;
use std::sync::{Arc, mpsc};

mod geometry;
mod interaction;
use geometry::{PITCH, Projection, camera_for};

struct Sockets {
    mesh: Arc<Mesh>,
    grid: [u32; 2],
    receiver: mpsc::Receiver<Result<Vec3, String>>,
    corner: Option<Result<Vec3, String>>,
}

impl Sockets {
    fn measure(part: &Part, grid: [u32; 2], ctx: &egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        let mesh = part.mesh.clone();
        let source = mesh.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let xs = gridfinity::centers(&source, 0, grid[0] as usize)?;
                let zs = gridfinity::centers(&source, 2, grid[1] as usize)?;
                Ok(Vec3::new(
                    xs[xs.len() - 1] + PITCH * 0.5,
                    source.min.y + 1.3,
                    zs[zs.len() - 1] + PITCH * 0.5,
                ))
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        Self {
            mesh,
            grid,
            receiver,
            corner: None,
        }
    }

    fn poll(&mut self) -> Option<&Result<Vec3, String>> {
        if self.corner.is_none() {
            self.corner = match self.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "底板测量中断 / Baseplate measurement interrupted".into(),
                )),
            };
        }
        self.corner.as_ref()
    }
}

struct Footprint {
    id: u32,
    cell: [u32; 2],
    span: [u32; 2],
    height: f32,
    body: usize,
    label: Option<usize>,
}

impl Footprint {
    fn contains(&self, at: [f32; 2]) -> bool {
        (0..2).all(|axis| {
            (self.cell[axis] as f32..(self.cell[axis] + self.span[axis]) as f32).contains(&at[axis])
        })
    }

    // Intersect the pointer ray with the occupied cell prism, including its side walls.
    fn hit(&self, ground: [f32; 2], slope: [f32; 2]) -> Option<([f32; 2], f32)> {
        let (mut lower, mut upper) = (0.0f32, self.height);
        for axis in 0..2 {
            let lo = self.cell[axis] as f32 + 0.25 / PITCH;
            let hi = (self.cell[axis] + self.span[axis]) as f32 - 0.25 / PITCH;
            if slope[axis].abs() < 0.000001 {
                if !(lo..=hi).contains(&ground[axis]) {
                    return None;
                }
            } else {
                let a = (lo - ground[axis]) / slope[axis];
                let b = (hi - ground[axis]) / slope[axis];
                lower = lower.max(a.min(b));
                upper = upper.min(a.max(b));
            }
        }
        (lower <= upper).then(|| {
            (
                [ground[0] + slope[0] * upper, ground[1] + slope[1] * upper],
                upper,
            )
        })
    }
}

#[derive(Default)]
pub(super) struct State {
    opened: Option<workspace::OpenInterior>,
    sockets: Option<Sockets>,
    view: Option<Projection>,
    bins: Vec<Footprint>,
    shown: Option<Layout>,
    dragging: bool,
    confirmed_target: Option<(Target, PartId, PartId)>,
    pub(super) focus_bin: Option<u32>,
}

impl State {
    pub(super) fn project_cell(&self, at: [f32; 2]) -> Option<egui::Pos2> {
        let view = self.view.as_ref()?;
        let height = self
            .bins
            .iter()
            .find(|b| b.contains(at))
            .map_or(0., |b| b.height);
        Some(view.screen(at, height))
    }

    pub(super) fn shows_layout(&self, layout: &Layout) -> bool {
        self.shown.as_ref() == Some(layout)
    }

    fn hit(&self, view: &Projection, pos: egui::Pos2) -> Option<([f32; 2], f32)> {
        let ground = view.cell_at(pos, 0.)?;
        let above = view.cell_at(pos, 1.)?;
        let slope = [above[0] - ground[0], above[1] - ground[1]];
        self.bins
            .iter()
            .filter_map(|bin| bin.hit(ground, slope))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .or_else(|| view.inside(ground).then_some((ground, 0.)))
    }
}

impl App {
    pub(super) fn bin_view_ui(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut Layout,
        selected: &mut Option<u32>,
    ) {
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
        self.bin_view.view = None;
        self.bin_view.shown = None;
        self.bin_view.bins.clear();
        let target = if self.scene_key == "combination" {
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
        let pending = self.busy || self.preview.has_pending();
        if !self.scene.assembly || self.combination_view != (self.scene_key == "combination") {
            message(t(self.zh, "正在加载抽屉装配…", "Loading drawer assembly…"));
            return;
        }
        let opened = match workspace::OpenInterior::get(
            &mut self.bin_view.opened,
            &self.scene,
            target,
            MotionKind::Drawer,
        ) {
            Ok(opened) => opened,
            Err(error) => {
                message(if pending {
                    t(self.zh, "正在加载抽屉装配…", "Loading drawer assembly…")
                } else {
                    &error
                });
                return;
            }
        };
        let parts: Vec<_> = opened
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| target.contains(p))
            .collect();
        let drawer = parts
            .iter()
            .copied()
            .find(|(_, p)| p.pose.path.contains("/Boxes/"));
        let plate = parts
            .iter()
            .copied()
            .find(|(_, p)| p.pose.path == draft.baseplate);
        let (Some((drawer_index, drawer)), Some((plate_index, plate))) = (drawer, plate) else {
            if pending {
                message(t(
                    self.zh,
                    "正在加载抽屉与底板…",
                    "Loading drawer and baseplate…",
                ));
            } else {
                message(t(
                    self.zh,
                    "当前装配没有对应的抽屉和底板",
                    "The drawer and baseplate are unavailable in this assembly",
                ));
            }
            return;
        };
        let Ok(grid) = gridfinity::grid_for(&self.selection, &draft.baseplate) else {
            return;
        };
        let state = &mut self.bin_view;
        if state
            .sockets
            .as_ref()
            .is_none_or(|s| !Arc::ptr_eq(&s.mesh, &plate.mesh) || s.grid != grid)
        {
            state.sockets = Some(Sockets::measure(plate, grid, ui.ctx()));
            state.dragging = false;
        }
        let corner = match state.sockets.as_mut().unwrap().poll() {
            Some(Ok(corner)) => *corner,
            Some(Err(error)) => {
                message(error);
                return;
            }
            None => {
                message(t(
                    self.zh,
                    "正在识别底板格位…",
                    "Locating baseplate sockets…",
                ));
                return;
            }
        };
        let plate_pose = plate.pose.matrix(0.);
        let mut view = Projection {
            camera: camera_for(drawer, plate_pose, rect, true),
            rect,
            corner,
            grid,
            plate: plate_pose,
        };
        let mut scene = (*opened).clone();
        // Preserve the assembly's order and all surrounding members. A removed
        // or resized bin can remain in the old GPU scene until the next rebuild.
        for &(index, p) in &parts {
            if p.pose.path.starts_with(gridfinity::PREFIX) {
                scene.parts[index].visible = false;
            }
        }
        scene.parts[drawer_index].visible = true;
        scene.parts[plate_index].visible = true;
        let mut complete = !pending && self.failed_preview.is_none();
        let mut all_bodies = true;
        for bin in &draft.bins {
            let path = bin.path();
            let part = parts.iter().copied().find(|(_, p)| p.pose.path == path);
            let Some((index, part, shift)) = part.and_then(|(i, p)| {
                let height = gridfinity::units(bin, &self.selection).ok()? as f32 * 7. + 3.55;
                Some((i, p, display_offset(bin, height, p, &view)?))
            }) else {
                complete = false;
                all_bodies = false;
                continue;
            };
            complete &= shift.length() < 0.2;
            let label_index = if bin.detachable() {
                let path = bin.label_path();
                let label = parts
                    .iter()
                    .find(|(_, p)| p.pose.path == path)
                    .map(|(i, _)| *i);
                complete &= label.is_some();
                label
            } else {
                None
            };
            for i in [Some(index), label_index].into_iter().flatten() {
                scene.parts[i].visible = true;
                move_display_part(&mut scene.parts[i], shift);
            }
            if state.focus_bin == Some(bin.id) {
                view.camera = camera_for(&scene.parts[index], plate_pose, rect, false);
            }
            state.bins.push(Footprint {
                id: bin.id,
                cell: bin.cell,
                span: bin.span,
                height: part.mesh.max.y - part.mesh.min.y,
                body: index,
                label: label_index,
            });
        }
        let identity = (target, PartId::of(drawer), PartId::of(plate));
        if complete {
            state.confirmed_target = Some(identity.clone());
        }
        let can_edit = if pending {
            all_bodies && state.confirmed_target.as_ref() == Some(&identity)
        } else {
            self.failed_preview.is_none()
        };
        let loaded = (complete && self.smoke).then(|| draft.clone());
        let mut edit = interaction::Edit {
            layout: draft,
            selection: &self.selection,
            selected,
            editor: &mut self.gridfinity_editor,
        };
        if can_edit {
            state.interact(ui, &response, &view, &mut edit);
        } else if std::mem::take(&mut state.dragging) {
            edit.editor.cancel();
        }
        if let Some(drag) = edit.editor.drag_preview()
            && let Some(base) = state
                .bins
                .iter()
                .find(|b| b.id == drag.id && b.span == drag.span)
        {
            let dx = base.cell[0] as f32 - drag.cell[0] as f32;
            let dz = base.cell[1] as f32 - drag.cell[1] as f32;
            let shift = plate_pose.transform_vector3(Vec3::new(dx * PITCH, 0., dz * PITCH));
            for i in [Some(base.body), base.label].into_iter().flatten() {
                move_display_part(&mut scene.parts[i], shift);
            }
        }
        let opacity = self.interior_exterior_opacity as f32 / 100.;
        let mut materials = vec![MaterialOverride::Opacity(opacity); scene.parts.len()];
        materials[plate_index] = MaterialOverride::Color([0.77, 0.80, 0.79, 1.]);
        for bin in &state.bins {
            materials[bin.body] = MaterialOverride::Color([0.99, 0.95, 0.86, 1.]);
            if let Some(i) = bin.label {
                materials[i] = MaterialOverride::Color([0.98, 0.98, 0.96, 1.]);
            }
        }
        let dpi = ui
            .ctx()
            .pixels_per_point()
            .min(4096. / rect.width().max(rect.height()));
        if self.smoke {
            self.bin_editor_smoke.observe_scene(&scene, rect);
        }
        let id = self.renderer.render_with_materials(
            0,
            [
                (rect.width() * dpi).round() as u32,
                (rect.height() * dpi).round() as u32,
            ],
            &scene,
            &view.camera,
            ViewOptions::default(),
            &materials,
        );
        painter.image(
            id,
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
        state.paint(&painter, &view, &edit, ui.visuals().dark_mode);
        if pending && !can_edit {
            painter.text(
                rect.left_bottom() + egui::vec2(16., -16.),
                egui::Align2::LEFT_BOTTOM,
                t(self.zh, "正在更新抽屉模型…", "Updating drawer model…"),
                egui::FontId::proportional(13.),
                ui.visuals().text_color(),
            );
        }
        state.view = Some(view);
        state.shown = loaded;
    }
}

fn move_display_part(part: &mut Part, shift: Vec3) {
    part.pose.position = (Vec3::from(part.pose.position) + shift).to_array();
}

/// When only a cell moved, reuse the loaded bin and label until the assembly worker catches up.
fn display_offset(bin: &Bin, height: f32, part: &Part, view: &Projection) -> Option<Vec3> {
    let offset = view.plate.inverse() * part.pose.matrix(0.);
    let foot = offset.transform_point3(Vec3::new(0., part.mesh.min.y, 0.));
    let cell = view.local_cell(foot);
    let size = part.mesh.max - part.mesh.min;
    let same_shape = (foot.y - view.corner.y).abs() < 0.1
        && (size.y - height).abs() < 0.15
        && [size.x, size.z]
            .iter()
            .enumerate()
            .all(|(axis, length)| (*length - (bin.span[axis] as f32 * PITCH - 0.5)).abs() < 0.15);
    same_shape.then(|| {
        let dx = cell[0] - bin.cell[0] as f32 - bin.span[0] as f32 * 0.5;
        let dz = cell[1] - bin.cell[1] as f32 - bin.span[1] as f32 * 0.5;
        view.plate
            .transform_vector3(Vec3::new(dx * PITCH, 0., dz * PITCH))
    })
}
