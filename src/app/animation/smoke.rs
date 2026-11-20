use super::*;
use crate::constraints::{Joint, PartId};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    meshes: Vec<usize>,
    bounds: Vec<(glam::Vec3, glam::Vec3)>,
    roots: Vec<(PartId, glam::Mat4)>,
    closed_poses: HashMap<PartId, glam::Mat4>,
    error: Option<String>,
}

impl App {
    pub(in crate::app) fn setup_motion_smoke(&mut self, ctx: &egui::Context) {
        self.setup_tower_smoke(ctx);
        self.animation = Default::default();
        self.members = vec![
            self.members[0].clone(),
            self.members[3].clone(),
            self.members[4].clone(),
        ];
        for (index, member) in self.members.iter_mut().enumerate() {
            member.name = format!("动画验证 · 第 {} 层", index + 1);
            member.placement = crate::scene::Placement {
                level: index as u32,
                ..Default::default()
            };
            let p = &mut member.preset;
            p.selection.width = "S".into();
            p.selection.depth = 3;
            if index != 1 {
                p.assembly_kind = AssemblyKind::Sustainer;
                p.drawers.clear();
            }
            if let Err(error) = tower::configure(p, &self.catalog, true) {
                self.animation.smoke.error = Some(error);
                return;
            }
        }
        let drawer = &mut self.members[1].preset.drawers[1];
        let Some(plate) = self
            .catalog
            .accessories(&drawer.selection)
            .into_iter()
            .find(|entry| {
                entry.path.contains("/Gridfinity Baseplate/") && !entry.name.contains("Handle")
            })
        else {
            self.animation.smoke.error =
                Some("Missing standard baseplate for the test drawer".into());
            return;
        };
        drawer.gridfinity.baseplate = plate.path.clone();
        Catalog::set_accessory(&mut drawer.bom, plate.path, true, 1);
        self.restore_member(2);
        self.combination_view = true;
        self.smoke_stage = 0;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
    }
    fn smoke_mechanism(&self, member: usize, drawer: Option<usize>) -> Result<Mechanism, String> {
        AssemblyRules::mechanism(&self.scene, Target { member, drawer })
    }
    fn check_shared_drawers(&self) -> Result<(), String> {
        let closed = &self.animation.smoke.closed_poses;
        let drawers = [
            self.smoke_mechanism(1, Some(0))?,
            self.smoke_mechanism(1, Some(1))?,
        ];
        for (slot, drawer) in drawers.iter().enumerate() {
            let target = Target {
                member: 1,
                drawer: Some(slot),
            };
            let cap = self
                .scene
                .parts
                .iter()
                .find(|p| target.contains(p) && p.pose.path.ends_with("/Key Cap.stl"))
                .ok_or("Missing visible drawer knob")?;
            // Local X follows the long grip of the source cap. The fronts of this
            // upright stack lie in world XY. Check both drawers throughout playback.
            let grip = cap
                .pose
                .matrix(0.)
                .transform_vector3(glam::Vec3::X)
                .normalize()
                .abs();
            let (horizontal, vertical) = (drawer.latch * std::f32::consts::FRAC_PI_2).sin_cos();
            if !grip.abs_diff_eq(glam::Vec3::new(horizontal, vertical, 0.), 0.002) {
                return Err(format!(
                    "Stage {}: drawer {} grip axis {grip:?} at release fraction {:.3}, expected vertical when locked and horizontal when released",
                    self.smoke_stage,
                    slot + 1,
                    drawer.latch
                ));
            }
        }
        let mut guides = [0; 2];
        let mut stoppers = 0;
        for (id, datum) in closed.iter().filter(|(id, _)| {
            id.instance / crate::scene::MEMBER_STRIDE == 1
                && (id.path.contains("/Runners/") || id.path.contains("/Boxes/"))
        }) {
            let part = self
                .scene
                .parts
                .iter()
                .find(|p| PartId::of(p) == *id)
                .ok_or("Shared-shell runner disappeared")?;
            let distance = if id.path.ends_with("/Runner Stopper.stl") {
                stoppers += 1;
                0.
            } else {
                let slot = Target::of(part)
                    .drawer
                    .filter(|i| *i < drawers.len())
                    .ok_or("Moving runner has no child drawer")?;
                let drawer = &drawers[slot];
                if id.path.contains("Runner Guide") {
                    let joint = self
                        .scene
                        .relations
                        .iter()
                        .find(|r| r.child == *id && r.parent == drawer.child);
                    let Some(Joint::RunnerGuide { inner_travel, .. }) = joint.map(|r| &r.joint)
                    else {
                        return Err("Guide is not constrained to its own drawer".into());
                    };
                    guides[slot] += 1;
                    if *inner_travel <= 0. || *inner_travel >= drawer.max {
                        return Err("Drawer cannot use both runner stages".into());
                    }
                    (drawer.value - inner_travel).max(0.)
                } else {
                    drawer.value
                }
            };
            // This test project is an unrotated stack. Compare actual world transforms
            // with the initial closed fixture, also after rebuilding the scene.
            let expected = glam::Mat4::from_translation(glam::Vec3::Z * distance) * *datum;
            if !part.pose.matrix(0.).abs_diff_eq(expected, 0.01) {
                return Err(format!(
                    "Stage {}: {} #{} did not follow its runner travel of {distance:.2} mm",
                    self.smoke_stage, id.path, id.instance
                ));
            }
        }
        if guides != [2, 2] || stoppers != 4 {
            return Err(format!(
                "Expected two guides per drawer and four housing stops, found {guides:?} and {stoppers}"
            ));
        }
        Ok(())
    }
    // The fixture has two drawers in the middle member and a lid on the top member.
    // Compare their rendered poses while operating only one lock at a time.
    fn check_independent_lock(&self, released: Option<Target>) -> Result<(), String> {
        let drawer = Target {
            member: 1,
            drawer: Some(1),
        };
        let lid = Target {
            member: 2,
            drawer: None,
        };
        let drawer_id = AssemblyRules::mechanism(&self.scene, drawer)?.child;
        let drawer_center = self
            .animation
            .smoke
            .closed_poses
            .get(&drawer_id)
            .ok_or("Missing closed drawer pose")?
            .w_axis
            .x;
        for target in [
            drawer,
            lid,
            Target {
                member: 1,
                drawer: Some(0),
            },
        ] {
            let m = AssemblyRules::mechanism(&self.scene, target)?;
            let expected = if Some(target) == released { 1. } else { 0. };
            if m.value.abs() > 0.001 || (m.latch - expected).abs() > 0.001 {
                return Err(format!(
                    "Standalone lock changed the wrong mechanism: {target:?}"
                ));
            }
        }
        let mut key_rotations = Vec::new();
        let mut rack_shifts = Vec::new();
        let mut lid_links = 0;
        for p in &self.scene.parts {
            let id = PartId::of(p);
            let closed = self
                .animation
                .smoke
                .closed_poses
                .get(&id)
                .ok_or("No initial lock pose")?;
            let pose = p.pose.matrix(0.);
            let target = Target::of(p);
            let path = &id.path;
            if Some(target) == released && target == drawer {
                if path.contains("/Geared Rails/") {
                    let shift = pose.w_axis.x - closed.w_axis.x;
                    let center = closed.transform_point3((p.mesh.min + p.mesh.max) * 0.5).x;
                    let expected = glam::Mat4::from_translation(glam::Vec3::X * shift) * *closed;
                    if !pose.abs_diff_eq(expected, 0.01) || (center - drawer_center) * shift >= 0. {
                        return Err(
                            "Released rack did not retract along its transverse channel".into()
                        );
                    }
                    rack_shifts.push(shift);
                    continue;
                }
                if path.ends_with("/Key (Turn On Supports).stl") || path.ends_with("/Key Cap.stl") {
                    let rotation =
                        glam::Quat::from_mat4(&pose) * glam::Quat::from_mat4(closed).inverse();
                    key_rotations.push(rotation);
                    continue;
                }
            }
            if Some(target) == released && target == lid && path.contains("/Locks/") {
                let link = path.contains("/LOCK INNER PART") || path.contains("/LOCK OUTER PART");
                if link || (path.contains("PINS") && p.pose.body.is_some_and(|body| body < 2)) {
                    if pose.abs_diff_eq(*closed, 0.01) {
                        return Err(format!("Released lid lock did not move {path}"));
                    }
                    lid_links += usize::from(link);
                    continue;
                }
            }
            if !pose.abs_diff_eq(*closed, 0.01) {
                return Err(format!("Standalone lock displaced {path} on {target:?}"));
            }
        }
        if released == Some(drawer) {
            let [key, cap] = key_rotations.as_slice() else {
                return Err("Missing drawer key or key cap".into());
            };
            if key.angle_between(glam::Quat::IDENTITY) < 0.05 || key.angle_between(*cap) > 0.005 {
                return Err("Drawer key and cap did not turn together".into());
            }
            let [upper, lower] = rack_shifts.as_slice() else {
                return Err("Missing geared racks in the shared drawer".into());
            };
            if upper * lower >= 0. || upper.abs() < 0.1 || (upper + lower).abs() > 0.01 {
                return Err("Drawer racks did not retract in opposite directions".into());
            }
        }
        if released == Some(lid) && lid_links != 2 {
            return Err("Missing inner or outer link of the lid lock".into());
        }
        Ok(())
    }
    fn smoke_toggle(&mut self, expected: &Mechanism, ctx: &egui::Context) -> Result<(), String> {
        let selected = AssemblyRules::mechanism(&self.scene, self.motion_target())?;
        if selected.child != expected.child {
            return Err("The editor motion target changed".into());
        }
        let goal = self
            .motion_goals(&selected)
            .toggle
            .ok_or("Editor motion button is disabled")?;
        self.animate_joint_to(&selected.child, goal, ctx)
    }
    pub(in crate::app) fn motion_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(16));
        let folder = self.home.join("output/motion-ui");
        let _ = std::fs::create_dir_all(&folder);
        if self.smoke_start.elapsed() > Duration::from_secs(240) {
            self.animation.smoke.error = Some(format!("Timeout: {}", self.status));
        }
        if !self.busy && !self.preview.has_pending() && self.failed_preview.is_some() {
            self.animation.smoke.error = Some(self.status.clone());
        }
        if let Some(error) = &self.animation.smoke.error {
            let _ = std::fs::write(folder.join("smoke-error.txt"), error);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        let ready = (|| -> Result<bool, String> {
            if self.smoke_stage == 0 && self.animation.smoke.meshes.is_empty() {
                self.animation.smoke.meshes =
                    self.scene.parts.iter().map(|p| p.mesh_key()).collect();
                self.animation.smoke.bounds = self.scene.member_bounds.clone();
                self.animation.smoke.closed_poses = self
                    .scene
                    .parts
                    .iter()
                    .map(|p| (PartId::of(p), p.pose.matrix(0.)))
                    .collect();
                self.animation.smoke.roots = self
                    .scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.contains("/Modules/"))
                    .map(|p| (PartId::of(p), p.pose.matrix(0.)))
                    .collect();
            }
            let wait_for_endpoint = matches!(self.smoke_stage, 3 | 5 | 6 | 8 | 9 | 11..=14);
            if wait_for_endpoint || ([2, 4].contains(&self.smoke_stage) && self.smoke_frame == 0) {
                self.advance_motion(0.05, ctx);
            }
            self.check_shared_drawers()?;
            let drawer = self.smoke_mechanism(1, Some(1))?;
            if drawer.value > 0.01 && drawer.latch < 0.999 {
                return Err("Drawer moved before releasing the key".into());
            }
            if self.smoke_stage == 2 {
                let m = self.smoke_mechanism(2, None)?;
                if m.value < 30. {
                    return Ok(false);
                }
            }
            if self.smoke_stage == 4 {
                let m = self.smoke_mechanism(1, Some(1))?;
                if m.value < m.max * 0.35 {
                    return Ok(false);
                }
            }
            if wait_for_endpoint && !self.animation.tracks.is_empty() {
                return Ok(false);
            }
            if self.smoke_stage < 10
                && self.animation.smoke.meshes
                    != self
                        .scene
                        .parts
                        .iter()
                        .map(|p| p.mesh_key())
                        .collect::<Vec<_>>()
            {
                return Err("Animation replaced mesh resources".into());
            }
            for (id, matrix) in &self.animation.smoke.roots {
                let part = self
                    .scene
                    .parts
                    .iter()
                    .find(|p| PartId::of(p) == *id)
                    .ok_or("Root disappeared")?;
                if !part.pose.matrix(0.).abs_diff_eq(*matrix, 0.001) {
                    return Err("Motion moved a grounded housing".into());
                }
            }
            for ((lo, hi), (a, b)) in self
                .scene
                .member_bounds
                .iter()
                .zip(&self.animation.smoke.bounds)
            {
                if (*lo - *a).length() > 0.02 || (*hi - *b).length() > 0.02 {
                    return Err("Operating pose changed the closed dimension cache".into());
                }
            }
            let lid = self.smoke_mechanism(2, None)?;
            match self.smoke_stage {
                1 if lid.value != 0.
                    || !(0.01..0.99).contains(&lid.latch)
                    || self.motion_goals(&lid).toggle != Some(lid.min) =>
                {
                    return Err("Lid release must allow immediate reversal before lifting".into());
                }
                2 if !(30. ..70.).contains(&lid.value) || lid.latch < 0.999 => {
                    return Err("Incorrect mid-opening pose".into());
                }
                3 if (lid.value - 90.).abs() > 0.01 => {
                    return Err("Lid did not reach its endpoint".into());
                }
                4 | 5 => {
                    let sibling = self.smoke_mechanism(1, Some(0))?;
                    if sibling.value.abs() > 0.001 || sibling.latch.abs() > 0.001 {
                        return Err("Shared-shell sibling moved".into());
                    }
                    if self.smoke_stage == 5 {
                        let m = self.smoke_mechanism(1, Some(1))?;
                        if (m.value - m.max).abs() > 0.01 {
                            return Err("Drawer did not reach the allowed endpoint".into());
                        }
                    }
                }
                9 | 10 => {
                    let m = self.smoke_mechanism(1, Some(1))?;
                    if (lid.value - 65.).abs() > 0.01
                        || (m.value / m.max - 0.5).abs() > 0.001
                        || self.members[2].preset.sustainer_angle != 65.
                        || self.members[1].preset.drawers[1].drawer_extension != 0.5
                    {
                        return Err("Motion endpoints did not survive a project rebuild".into());
                    }
                }
                6 | 11 | 14 => self.check_independent_lock(None)?,
                12 => self.check_independent_lock(Some(Target {
                    member: 1,
                    drawer: Some(1),
                }))?,
                13 => self.check_independent_lock(Some(Target {
                    member: 2,
                    drawer: None,
                }))?,
                _ => {}
            }
            Ok(true)
        })();
        match ready {
            Ok(false) => return,
            Err(error) => {
                self.animation.smoke.error = Some(error);
                return;
            }
            _ => {}
        }
        self.smoke_frame += 1;
        if self.smoke_frame >= 5 && !self.screenshot_requested {
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
            let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                folder.join(format!("motion-{}.png", self.smoke_stage)),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            match self.smoke_stage {
                0 => {
                    self.tower_edit_slot(2, None, ctx);
                    let m = self.smoke_mechanism(2, None)?;
                    self.animate_joint_to(&m.child, 90., ctx)?;
                    self.advance_motion(0.1, ctx);
                }
                1 | 2 | 4 => {}
                3 => {
                    self.tower_edit_slot(1, Some(1), ctx);
                    let m = self.smoke_mechanism(1, Some(1))?;
                    self.smoke_toggle(&m, ctx)?;
                    self.advance_motion(0.1, ctx);
                    let still_seated = self.smoke_mechanism(1, Some(1))?;
                    if still_seated.value.abs() > 0.001
                        || !(0.01..0.99).contains(&still_seated.latch)
                    {
                        return Err("Pulling the drawer must first turn its key in place".into());
                    }
                }
                5 => {
                    let m = self.smoke_mechanism(1, Some(1))?;
                    self.smoke_toggle(&m, ctx)?;
                    let lid = self.smoke_mechanism(2, None)?;
                    self.animate_joint_to(&lid.child, 0., ctx)?;
                }
                6 => {
                    let before =
                        toml::to_string(&self.scene.definition()).map_err(|e| e.to_string())?;
                    let lid = self.smoke_mechanism(0, None)?;
                    if self.animate_joint_to(&lid.child, 90., ctx).is_ok() {
                        return Err("Supporting lid was allowed to open".into());
                    }
                    let m = self.smoke_mechanism(1, Some(1))?;
                    for value in [-1., m.max + 1.] {
                        if self.animate_joint_to(&m.child, value, ctx).is_ok() {
                            return Err("Out-of-range drawer target was accepted".into());
                        }
                    }
                    if before
                        != toml::to_string(&self.scene.definition()).map_err(|e| e.to_string())?
                    {
                        return Err("Rejected motion changed the scene".into());
                    }
                    self.tower_edit_slot(0, None, ctx);
                    if self.motion_goals(&lid).toggle.is_some() {
                        return Err("Supporting lid offered an opening action".into());
                    }
                    if self.animate_latch_to(&lid.child, true, ctx).is_ok() {
                        return Err("Supporting lid allowed an independent clasp release".into());
                    }
                    // The upper member comes to rest on a lid whose clasp is already half open.
                    let supports = std::mem::take(&mut self.scene.member_supports);
                    let half_open = AssemblyRules::apply_motion_sample(
                        &mut self.scene,
                        &lid.child,
                        MotionSample {
                            value: lid.min,
                            latch: 0.5,
                        },
                    );
                    self.scene.member_supports = supports;
                    half_open?;
                    self.animate_latch_to(&lid.child, false, ctx)?;
                    for _ in 0..3 {
                        self.advance_motion(0.1, ctx);
                    }
                    if self.smoke_mechanism(0, None)?.latch > 0.001 {
                        return Err("Supporting lid did not finish re-latching".into());
                    }
                }
                7 => {
                    self.tower_edit_slot(1, Some(0), ctx);
                    let m = self.smoke_mechanism(1, Some(0))?;
                    self.smoke_toggle(&m, ctx)?;
                    for _ in 0..4 {
                        self.advance_motion(0.1, ctx);
                    }
                    let before = self.smoke_mechanism(1, Some(0))?.value;
                    self.preview_modes.mode = preview_modes::Mode::Layers;
                    self.advance_motion(0.1, ctx);
                    if (before - self.smoke_mechanism(1, Some(0))?.value).abs() > 0.001 {
                        return Err("Motion continued while viewing layers".into());
                    }
                    self.preview_modes.mode = preview_modes::Mode::Assembly;
                    self.advance_motion(0.1, ctx);
                    let value = self.smoke_mechanism(1, Some(0))?.value;
                    if value <= before {
                        return Err("Motion did not resume in the assembly view".into());
                    }
                    self.smoke_toggle(&m, ctx)?;
                    if (value - self.smoke_mechanism(1, Some(0))?.value).abs() > 0.001 {
                        return Err("Reversing motion jumped the model".into());
                    }
                }
                8 => {
                    self.tower_edit_slot(2, None, ctx);
                    let lid = self.smoke_mechanism(2, None)?;
                    self.animate_joint_to(&lid.child, 65., ctx)?;
                    let m = self.smoke_mechanism(1, Some(1))?;
                    self.animate_joint_to(&m.child, m.max * 0.5, ctx)?;
                }
                9 => {
                    self.commit_member();
                    let saved = self.project_snapshot(self.member_snapshot());
                    let text = toml::to_string_pretty(&saved).map_err(|e| e.to_string())?;
                    let restored: CombinationPreset =
                        toml::from_str(&text).map_err(|e| e.to_string())?;
                    restored.validate(&self.catalog)?;
                    std::fs::write(folder.join("project.toml"), text).map_err(|e| e.to_string())?;
                    self.restore_project(restored);
                    self.assembly(ctx);
                }
                10 => {
                    let lid = self.smoke_mechanism(2, None)?;
                    let drawer = self.smoke_mechanism(1, Some(1))?;
                    self.animate_joint_to(&lid.child, lid.min, ctx)?;
                    self.animate_joint_to(&drawer.child, drawer.min, ctx)?;
                }
                11 => {
                    self.tower_edit_slot(1, Some(1), ctx);
                    let drawer = self.smoke_mechanism(1, Some(1))?;
                    self.animate_latch_to(&drawer.child, true, ctx)?;
                }
                12 => {
                    let drawer = self.smoke_mechanism(1, Some(1))?;
                    self.animate_latch_to(&drawer.child, false, ctx)?;
                    self.tower_edit_slot(2, None, ctx);
                    let lid = self.smoke_mechanism(2, None)?;
                    self.animate_latch_to(&lid.child, true, ctx)?;
                }
                13 => {
                    let lid = self.smoke_mechanism(2, None)?;
                    self.animate_latch_to(&lid.child, false, ctx)?;
                }
                _ => {
                    std::fs::write(folder.join("smoke-result.txt"), "PASS: both visible drawer grips stand vertically when locked and turn horizontally when released. Independent key/cap rotation and opposing rack travel on a seated drawer. Independent lid lock release and return to its original pose after closing. Automatic unlock, opening, closing and re-latching. Shared-shell drawer isolation, two-stage guide travel, fixed stoppers and housings. Layer-view suspension, reversal, support and travel rejection, mesh reuse and closed dimensions. Lid and drawer endpoints restored after TOML roundtrip.").map_err(|e|e.to_string())?;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.animation.smoke.error = Some(error);
            return;
        }
        self.smoke_stage += 1;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
    }
}
