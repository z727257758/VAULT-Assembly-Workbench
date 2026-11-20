//! Exercise the bin editor through native pointer events and its real assembly preview.
use super::*;
use crate::constraints::{AssemblyRules, Joint, PartId};
use crate::gridfinity::{Bin, Layout};
use crate::scene::Target;
use egui::{Event, Pos2, RawInput, Rect};
use glam::{Mat4, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};

const DRAWER: Target = Target {
    member: 0,
    drawer: Some(1),
};
const SIBLING: Target = Target {
    member: 0,
    drawer: Some(0),
};
const NARROW: egui::Vec2 = egui::vec2(1050., 700.);

fn sidebar_rect(ctx: &egui::Context) -> Result<Rect, String> {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new("library"))
        .map(|state| state.rect)
        .ok_or_else(|| "Missing build sidebar".into())
}

struct Entry {
    window_size: egui::Vec2,
    central: Rect,
    sidebar: Rect,
    camera: Mat4,
    drawer_offset: f32,
    poses: HashMap<PartId, Mat4>,
    opacities: HashMap<PartId, f32>,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) plan_rect: Option<Rect>,
    pub(super) done_rect: Option<Rect>,
    initial_layouts: Option<[Layout; 2]>,
    original_seat: Option<Vec3>,
    entry: Option<Entry>,
    rendered: Option<(Scene, Rect)>,
    error: Option<String>,
}

fn pointer(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

impl State {
    pub(super) fn observe_scene(&mut self, scene: &Scene, rect: Rect) {
        self.rendered = Some((scene.clone(), rect));
    }
}

#[derive(Default)]
pub(super) struct Pointer {
    inputs: VecDeque<Vec<Event>>,
}

impl Pointer {
    pub(super) fn click(&mut self, pos: Pos2) {
        self.inputs
            .extend([vec![Event::PointerMoved(pos)], vec![pointer(pos, true)]]);
        self.release(pos);
    }

    pub(super) fn pending(&self) -> bool {
        !self.inputs.is_empty()
    }

    pub(super) fn inject(&mut self, input: &mut RawInput) {
        input.focused = true;
        input.events.retain(|event| {
            !matches!(
                event,
                Event::PointerMoved(_) | Event::PointerButton { .. } | Event::PointerGone
            )
        });
        if let Some(events) = self.inputs.pop_front() {
            input.events.extend(events);
        }
    }

    fn drag(&mut self, start: Pos2, end: Pos2, release: bool) {
        self.inputs.extend([
            vec![Event::PointerMoved(start)],
            vec![pointer(start, true)],
            vec![Event::PointerMoved(
                start + (end - start).normalized() * 12.,
            )],
            vec![Event::PointerMoved(end)],
        ]);
        if release {
            self.release(end);
        }
    }

    fn release(&mut self, pos: Pos2) {
        self.inputs
            .extend([vec![pointer(pos, false)], vec![Event::PointerGone]]);
    }
}

impl App {
    pub(super) fn setup_bin_editor_smoke(&mut self, ctx: &egui::Context) {
        let setup = (|| -> Result<(), String> {
            let mut preset = self.member_snapshot();
            preset.selection = Selection {
                width: "S".into(),
                depth: 4,
                height: "2F".into(),
                hidden_key: false,
            };
            preset.assembly_kind = AssemblyKind::Drawer;
            let mut housing = tower::drawer_pattern(&preset, &self.catalog, &["1F", "1F"], true)?;
            for (index, drawer) in housing.drawers.iter_mut().enumerate() {
                let plate = self
                    .catalog
                    .accessories(&drawer.selection)
                    .into_iter()
                    .find(|entry| {
                        entry.path.contains("/Gridfinity Baseplate/")
                            && !entry.path.contains("Handle")
                    })
                    .ok_or("Missing S4 standard baseplate")?;
                drawer.gridfinity.baseplate = plate.path.clone();
                Catalog::set_accessory(&mut drawer.bom, plate.path, true, 1);
                let cells = if index == 0 {
                    vec![[2, 0], [2, 2]]
                } else {
                    let [cols, rows] = crate::gridfinity::grid_for(
                        &drawer.selection,
                        &drawer.gridfinity.baseplate,
                    )?;
                    (0..rows)
                        .flat_map(|y| (0..cols).map(move |x| [x, y]))
                        .filter(|cell| ![[1, 0], [1, 1]].contains(cell))
                        .collect()
                };
                drawer.gridfinity.bins = cells
                    .into_iter()
                    .enumerate()
                    .map(|(i, cell)| Bin {
                        id: i as u32 + 1,
                        cell,
                        ..Default::default()
                    })
                    .collect();
                drawer.gridfinity.shared.detachable_label = true;
                drawer.gridfinity.shared.depth = 0.4;
                drawer.gridfinity.bins[0].label.enabled = true;
                drawer.gridfinity.bins[0].label.text = "M3".into();
                drawer.gridfinity.sync_shared();
                drawer.gridfinity.validate(&drawer.selection)?;
                drawer.gridfinity.sync_bom(&mut drawer.bom);
            }
            self.bin_editor_smoke.initial_layouts = Some([
                housing.drawers[0].gridfinity.clone(),
                housing.drawers[1].gridfinity.clone(),
            ]);
            // A partly opened target detects a close action that blindly resets it to zero.
            housing.drawers[1].drawer_extension = 0.2;
            preset.selection.height = "1F".into();
            tower::configure(&mut preset, &self.catalog, true)?;
            self.members = vec![
                Member {
                    name: "S4".into(),
                    preset: housing,
                    placement: Default::default(),
                    exterior: Default::default(),
                },
                Member {
                    name: "S4".into(),
                    preset,
                    placement: crate::scene::Placement {
                        level: 1,
                        ..Default::default()
                    },
                    exterior: Default::default(),
                },
            ];
            self.restore_member(0);
            self.tower_edit_slot(0, Some(1), ctx);
            Ok(())
        })();
        self.bin_editor_smoke.error = setup.err();
    }

    fn check_bin_smoke_layout(&mut self, cell: [u32; 2]) -> Result<(), String> {
        let [sibling, edited] = self
            .bin_editor_smoke
            .initial_layouts
            .as_ref()
            .ok_or("Missing drawer fixture")?;
        let mut expected = edited.clone();
        expected.bins[0].cell = cell;
        if expected != self.gridfinity
            || self.inline_edit.target != Some(DRAWER)
            || self.members[0].preset.drawers[1].gridfinity != self.gridfinity
            || *sibling != self.members[0].preset.drawers[0].gridfinity
        {
            return Err(format!(
                "Unexpected drawer or bin data, expected bin 1 at {cell:?}, found {:?}",
                self.gridfinity.bins.first().map(|b| b.cell)
            ));
        }
        let path = expected.bins[0].path();
        let part = self
            .scene
            .parts
            .iter()
            .find(|part| DRAWER.contains(part) && part.pose.path == path)
            .ok_or("Missing bin mesh in the edited drawer")?;
        let id = PartId::of(part);
        let seat = self
            .scene
            .relations
            .iter()
            .find_map(|r| match &r.joint {
                Joint::Fixed { position, .. }
                    if r.child == id && r.parent.path.contains("/Gridfinity Baseplate/") =>
                {
                    Some(Vec3::from(*position))
                }
                _ => None,
            })
            .ok_or("Missing baseplate-to-bin mounting relation")?;
        let initial = *self.bin_editor_smoke.original_seat.get_or_insert(seat);
        let expected_shift = Vec3::new(-42. * cell[0] as f32, 0., -42. * cell[1] as f32);
        if !(seat - initial).abs_diff_eq(expected_shift, 0.6) {
            return Err(format!(
                "Bin mesh seat moved by {:?}, expected {expected_shift:?}",
                seat - initial
            ));
        }
        Ok(())
    }

    pub(super) fn capture_native_smoke(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Result<bool, String> {
        self.smoke_frame += 1;
        if self.smoke_frame >= 4 && !self.screenshot_requested {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else { return Ok(false) };
        let pixels: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        image::save_buffer(
            path,
            &pixels,
            image.width() as u32,
            image.height() as u32,
            image::ColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
        Ok(true)
    }

    fn check_bin_smoke_context(&self, ctx: &egui::Context, stage: u8) -> Result<(), String> {
        let entry = self
            .bin_editor_smoke
            .entry
            .as_ref()
            .ok_or("Missing entry scene")?;
        let sidebar = sidebar_rect(ctx)?;
        let central = ctx.available_rect();
        if central != entry.central
            || sidebar != entry.sidebar
            || (central.left() - sidebar.right()).abs() > 1.
        {
            return Err(format!(
                "Build sidebar or preview bounds changed: {sidebar:?}, {central:?}"
            ));
        }
        let motion = AssemblyRules::mechanism(&self.scene, DRAWER)?;
        if (motion.value - entry.drawer_offset).abs() > 0.01
            || (self.members[0].preset.drawers[1].drawer_extension - 0.2).abs() > 0.001
        {
            return Err("Gridfinity editing changed the stored drawer extension".into());
        }
        let moved_meshes = [
            self.gridfinity.bins[0].path(),
            self.gridfinity.bins[0].label_path(),
        ];
        if self.scene.parts.len() != entry.poses.len()
            || self.scene.parts.iter().any(|part| {
                !(DRAWER.contains(part) && moved_meshes.contains(&part.pose.path))
                    && entry
                        .poses
                        .get(&PartId::of(part))
                        .is_none_or(|original| !original.abs_diff_eq(part.pose.matrix(0.), 0.025))
            })
        {
            return Err("An assembly part outside the edited bin changed pose".into());
        }
        if matches!(stage, 6 | 9) {
            if self.bin_smoke_opacities(&self.scene)? != entry.opacities {
                return Err("Done did not restore the original assembly material opacities".into());
            }
            if !entry
                .camera
                .abs_diff_eq(self.camera.matrix(central.aspect_ratio()), 0.0001)
            {
                return Err("Done changed the entry camera".into());
            }
            return Ok(());
        }
        let (drawn, model_rect) = self
            .bin_editor_smoke
            .rendered
            .as_ref()
            .ok_or("Missing rendered assembly")?;
        let exterior_alpha = match stage {
            1 | 2 => 0.20,
            3 | 4 => 0.50,
            _ => 0.30,
        };
        self.check_bin_smoke_materials(drawn, exterior_alpha)?;
        let plan = self.bin_editor_smoke.plan_rect.ok_or("Missing plan view")?;
        let done = self
            .bin_editor_smoke
            .done_rect
            .ok_or("Missing Done button")?;
        if ![model_rect, &plan, &done]
            .into_iter()
            .all(|rect| central.expand(1.).contains_rect(*rect))
            || model_rect.intersects(plan)
        {
            return Err(
                "Model or plan overlaps the build sidebar, toolbar or the other view".into(),
            );
        }
        if stage == 8 {
            let [cols, rows] =
                crate::gridfinity::grid_for(&self.selection, &self.gridfinity.baseplate)?;
            for row in 0..rows {
                for col in 0..cols {
                    let cell = [col as f32 + 0.5, row as f32 + 0.5];
                    if !self
                        .bin_view
                        .project_cell(cell)
                        .is_some_and(|p| model_rect.contains(p))
                    {
                        return Err(format!("Narrow model viewport clips cell {cell:?}"));
                    }
                }
            }
        }
        let opened = AssemblyRules::mechanism(drawn, DRAWER)?;
        if (opened.value - motion.max).abs() > 0.01 || motion.max <= entry.drawer_offset + 1. {
            return Err(format!(
                "Displayed drawer offset is {}, expected {}",
                opened.value, motion.max
            ));
        }
        // Compare the complete displayed assembly against its solved pose, including
        // the shared shell, sibling drawer, upper module and attached accessories.
        let mut expected = self.scene.clone();
        AssemblyRules::set_slider(&mut expected, &motion.child, motion.max)?;
        if drawn.parts.len() != expected.parts.len() {
            return Err("The editor omitted parts of the surrounding assembly".into());
        }
        for (actual, wanted) in drawn.parts.iter().zip(&expected.parts) {
            let candidate = matches!(stage, 1 | 4)
                && DRAWER.contains(wanted)
                && moved_meshes.contains(&wanted.pose.path);
            if PartId::of(actual) != PartId::of(wanted)
                || (wanted.visible && !actual.visible)
                || (!candidate
                    && !actual
                        .pose
                        .matrix(0.)
                        .abs_diff_eq(wanted.pose.matrix(0.), 0.025))
            {
                return Err(format!(
                    "Unexpected displayed part or transform at stage {stage}: {:?}",
                    PartId::of(wanted)
                ));
            }
        }
        Ok(())
    }

    fn bin_smoke_opacities(&self, scene: &Scene) -> Result<HashMap<PartId, f32>, String> {
        let alphas = self.renderer.part_opacities();
        if alphas.len() != scene.parts.len() {
            return Err(format!(
                "Renderer submitted {} materials for {} assembly parts",
                alphas.len(),
                scene.parts.len()
            ));
        }
        Ok(scene
            .parts
            .iter()
            .map(PartId::of)
            .zip(alphas.iter().copied())
            .collect())
    }

    fn check_bin_smoke_materials(&self, scene: &Scene, exterior: f32) -> Result<(), String> {
        let alphas = self.bin_smoke_opacities(scene)?;
        let mut opaque = HashSet::from([self.gridfinity.baseplate.clone()]);
        for bin in &self.gridfinity.bins {
            opaque.insert(bin.path());
            opaque.insert(bin.label_path());
        }
        let mut visible_opaque = HashSet::new();
        for part in scene.parts.iter().filter(|part| part.visible) {
            let expected = if DRAWER.contains(part) && opaque.contains(&part.pose.path) {
                visible_opaque.insert(&part.pose.path);
                1.
            } else {
                exterior
            };
            let id = PartId::of(part);
            let actual = alphas[&id];
            if !actual.is_finite() || (actual - expected).abs() > 0.0001 {
                return Err(format!(
                    "Submitted opacity {actual} for {id:?}, expected {expected}"
                ));
            }
        }
        if visible_opaque.len() != opaque.len() {
            return Err(
                "The selected baseplate, bin bodies and detachable labels must all be visible"
                    .into(),
            );
        }
        Ok(())
    }

    fn smoke_draw_bins(&mut self, ctx: &egui::Context, folder: &Path) -> Result<(), String> {
        let stage = self.smoke_stage;
        let added = usize::from((stage - 10).min(2));
        let depth = if stage == 13 { 1 } else { 2 };
        let expected = [([0, 2], [1, 2]), ([0, 0], [2, depth])];
        let [original, other] = self.bin_editor_smoke.initial_layouts.as_ref().unwrap();
        let mut other = other.clone();
        other.bins[0].cell = [1, 1];
        if self.inline_edit.target != Some(SIBLING)
            || self.members[0].preset.drawers[1].gridfinity != other
            || self.gridfinity != self.members[0].preset.drawers[0].gridfinity
            || self.gridfinity.shared != original.shared
            || self.gridfinity.bins.len() != original.bins.len() + added
            || !self.gridfinity.bins.starts_with(&original.bins)
        {
            return Err(format!(
                "Stage {stage} changed existing bins, shared defaults or the expected count"
            ));
        }
        for (bin, (cell, span)) in self.gridfinity.bins[original.bins.len()..]
            .iter()
            .zip(expected)
        {
            if (bin.cell, bin.span) != (cell, span) {
                return Err(format!(
                    "Expected bin at {cell:?} with span {span:?}, got {:?} {:?}",
                    bin.cell, bin.span
                ));
            }
            let path = bin.path();
            let mesh = &self
                .scene
                .parts
                .iter()
                .find(|p| SIBLING.contains(p) && p.visible && p.pose.path == path)
                .ok_or_else(|| format!("Missing new bin mesh {path}"))?
                .mesh;
            let size = mesh.max - mesh.min;
            for (length, cells) in [(size.x, span[0]), (size.z, span[1])] {
                if (length - (42. * cells as f32 - 0.5)).abs() > 0.15 {
                    return Err(format!(
                        "Wrong physical dimensions for {path}: {size:?} mm, expected {span:?} cells"
                    ));
                }
            }
        }
        // The preview confirms that each mesh is seated on its measured baseplate cells.
        let window_size = self.bin_editor_smoke.entry.as_ref().unwrap().window_size;
        if !self.bin_view.shows_layout(&self.gridfinity)
            || (ctx.content_rect().size() - window_size).length() > 1.
        {
            return Ok(());
        }
        if stage >= 12 && self.gridfinity_selected != self.gridfinity.bins.last().map(|bin| bin.id)
        {
            return Err("The diagonal bin lost selection before or during resizing".into());
        }
        match stage {
            10 | 13 => {
                let rect = self
                    .bin_editor_smoke
                    .plan_rect
                    .ok_or("Missing plan for vertical draw")?;
                let [cols, rows] =
                    crate::gridfinity::grid_for(&self.selection, &self.gridfinity.baseplate)?;
                let (col, from, to) = if stage == 10 {
                    (0.5, 3.5, 2.5)
                } else {
                    (1., 0.98, 1.98)
                };
                let point = |row: f32| {
                    rect.min
                        + egui::vec2(
                            rect.width() * col / cols as f32,
                            rect.height() * row / rows as f32,
                        )
                };
                self.editor_smoke_pointer.drag(point(from), point(to), true);
            }
            11 | 12 => {
                let (from, to) = if stage == 11 {
                    ([1.5, 1.5], [0.5, 0.5])
                } else {
                    if !self.capture_native_smoke(ctx, &folder.join("drawn-bins.png"))? {
                        return Ok(());
                    }
                    ([1., 1.98], [1., 0.98])
                };
                let from = self
                    .bin_view
                    .project_cell(from)
                    .ok_or("Missing 3D drag origin")?;
                let to = self
                    .bin_view
                    .project_cell(to)
                    .ok_or("Missing 3D drag endpoint")?;
                self.editor_smoke_pointer.drag(from, to, true);
            }
            _ => {
                std::fs::write(folder.join("smoke-result.txt"), "PASS: The build sidebar kept its width. The complete assembly showed only the selected drawer opening along its joint. The selected baseplate, bins and labels stayed opaque with exterior opacity at 30%, 20% and 50%. 3D and plan drags moved the same bin by two 42 mm steps. Overlap was rejected. At 1050 x 700 all cell centers fit the model view. Done restored the assembly opacities, camera and drawer extension at both sizes, with the edited layout retained. A vertical plan drag created a 1x2 bin and a diagonal 3D drag created a 2x2 bin at the expected sockets. Dragging its front edge in 3D resized it to 2x1 and dragging the plan edge restored 2x2. Mesh dimensions matched at each step, with the earlier bins and shared defaults retained.").map_err(|e| e.to_string())?;
                let _ = std::fs::remove_file(folder.join("smoke-error.txt"));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return Ok(());
            }
        }
        self.smoke_stage += 1;
        Ok(())
    }

    pub(super) fn bin_editor_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(16));
        let folder = self.home.join("output/bin-editor-ui");
        let run = (|| -> Result<(), String> {
            if let Some(error) = self.bin_editor_smoke.error.take() {
                return Err(error);
            }
            if self.smoke_start.elapsed() > Duration::from_secs(180)
                || (!self.busy && !self.preview.has_pending() && self.failed_preview.is_some())
            {
                return Err(format!("Stage {}: {}", self.smoke_stage, self.status));
            }
            if self.editor_smoke_pointer.pending() || self.busy || self.preview.has_pending() {
                return Ok(());
            }
            if self.smoke_stage >= 10 {
                return self.smoke_draw_bins(ctx, &folder);
            }
            if self.bin_editor_smoke.entry.is_none() {
                if self.inline_edit.target != Some(DRAWER)
                    || self.inline_edit.pending_focus
                    || self.scene_key != "combination"
                {
                    return Ok(());
                }
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                if !self.capture_native_smoke(ctx, &folder.join("before.png"))? {
                    return Ok(());
                }
                let central = ctx.available_rect();
                let sidebar = sidebar_rect(ctx)?;
                let motion = AssemblyRules::mechanism(&self.scene, DRAWER)?;
                for category in ["/Boxes/", "/Facades/"] {
                    if !self
                        .scene
                        .parts
                        .iter()
                        .any(|p| p.visible && DRAWER.contains(p) && p.pose.path.contains(category))
                    {
                        return Err(format!("Missing visible exterior test part {category}"));
                    }
                }
                if motion.value <= motion.min
                    || motion.value >= motion.max
                    || !self
                        .scene
                        .parts
                        .iter()
                        .any(|p| Target::of(p).member == 1 && p.pose.path.contains("/Modules/"))
                {
                    return Err(
                        "The test fixture needs a partly opened drawer and a second layer".into(),
                    );
                }
                self.bin_editor_smoke.entry = Some(Entry {
                    window_size: ctx.content_rect().size(),
                    central,
                    sidebar,
                    camera: self.camera.matrix(central.aspect_ratio()),
                    drawer_offset: motion.value,
                    opacities: self.bin_smoke_opacities(&self.scene)?,
                    poses: self
                        .scene
                        .parts
                        .iter()
                        .map(|p| (PartId::of(p), p.pose.matrix(0.)))
                        .collect(),
                });
                self.open_bin_editor(ctx);
                return Ok(());
            }
            let Some(rect) = self.bin_editor_smoke.plan_rect else {
                return Ok(());
            };
            let stage = self.smoke_stage;
            if stage == 7 {
                let delta = ctx.content_rect().size() - NARROW;
                if delta.x.abs() > 1. || delta.y.abs() > 1. {
                    return Ok(());
                }
                let entry = self
                    .bin_editor_smoke
                    .entry
                    .as_mut()
                    .ok_or("Missing entry scene")?;
                entry.central = ctx.available_rect();
                entry.sidebar = sidebar_rect(ctx)?;
                entry.camera = self.camera.matrix(entry.central.aspect_ratio());
                self.open_bin_editor(ctx);
                self.smoke_stage += 1;
                return Ok(());
            }
            if !matches!(stage, 1 | 4 | 6 | 9) && !self.bin_view.shows_layout(&self.gridfinity) {
                return Ok(());
            }
            let cell = match stage {
                0 | 1 => [0, 0],
                2 => [1, 0],
                _ => [1, 1],
            };
            self.check_bin_smoke_layout(cell)?;
            if (1..=5).contains(&stage) && self.gridfinity_selected != Some(1) {
                return Err(format!("Stage {stage}: pointer did not select bin 1"));
            }
            if matches!(stage, 6 | 9) && (self.gridfinity_open || !self.inline_edit.open) {
                return Err("Done did not return to the drawer editor".into());
            }
            if matches!(stage, 1 | 4) {
                let valid = stage == 1;
                let destination = if valid { [1, 0] } else { [2, 2] };
                if !self
                    .gridfinity_editor
                    .drag_preview()
                    .is_some_and(|p| p.id == 1 && p.cell == destination && p.valid == valid)
                {
                    return Err(format!(
                        "Stage {stage}: incorrect live drag candidate at {destination:?}"
                    ));
                }
            }
            let names = [
                "initial",
                "drag-3d",
                "moved-3d",
                "moved-plan",
                "overlap",
                "rollback",
                "returned",
            ];
            if stage != 9 {
                let name = if stage == 8 {
                    "narrow"
                } else {
                    names[stage as usize]
                };
                if !self.capture_native_smoke(ctx, &folder.join(format!("{name}.png")))? {
                    return Ok(());
                }
            }
            self.check_bin_smoke_context(ctx, stage)?;
            let [cols, rows] =
                crate::gridfinity::grid_for(&self.selection, &self.gridfinity.baseplate)?;
            let plan_point = |x: f32, y: f32| {
                rect.min
                    + egui::vec2(
                        x * rect.width() / cols as f32,
                        y * rect.height() / rows as f32,
                    )
            };
            match stage {
                0 => {
                    self.bin_exterior_opacity = 20;
                    let from = self
                        .bin_view
                        .project_cell([0.5, 0.5])
                        .ok_or("Missing 3D drag origin")?;
                    let to = self
                        .bin_view
                        .project_cell([1.5, 0.5])
                        .ok_or("Missing 3D drag destination")?;
                    self.editor_smoke_pointer.drag(from, to, false);
                }
                1 => {
                    let to = self
                        .bin_view
                        .project_cell([1.5, 0.5])
                        .ok_or("Missing 3D drop position")?;
                    self.editor_smoke_pointer.release(to);
                }
                2 => {
                    self.bin_exterior_opacity = 50;
                    self.editor_smoke_pointer.drag(
                        plan_point(1.5, 0.5),
                        plan_point(1.5, 1.5),
                        true,
                    );
                }
                3 => self.editor_smoke_pointer.drag(
                    plan_point(1.5, 1.5),
                    plan_point(2.5, 2.5),
                    false,
                ),
                4 => {
                    self.bin_exterior_opacity = 30;
                    self.editor_smoke_pointer.release(plan_point(2.5, 2.5));
                }
                5 | 8 => {
                    let done = self
                        .bin_editor_smoke
                        .done_rect
                        .ok_or("Missing Done button")?
                        .center();
                    self.editor_smoke_pointer.click(done);
                }
                6 => ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(NARROW)),
                _ => {
                    let window_size = self.bin_editor_smoke.entry.as_ref().unwrap().window_size;
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(window_size));
                    self.tower_edit_slot(SIBLING.member, SIBLING.drawer, ctx);
                    self.open_bin_editor(ctx);
                }
            }
            self.smoke_stage += 1;
            Ok(())
        })();
        if let Err(error) = run {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("smoke-error.txt"), error);
            let _ = std::fs::remove_file(folder.join("smoke-result.txt"));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
