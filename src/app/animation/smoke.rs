use super::*;
use crate::constraints::{Joint, PartId};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    meshes: Vec<usize>,
    bounds: Vec<(glam::Vec3, glam::Vec3)>,
    roots: Vec<(PartId, glam::Mat4)>,
    runner_datums: HashMap<PartId, glam::Mat4>,
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
    fn check_shared_runners(&self) -> Result<(), String> {
        let closed = &self.animation.smoke.runner_datums;
        let drawers = [
            self.smoke_mechanism(1, Some(0))?,
            self.smoke_mechanism(1, Some(1))?,
        ];
        let mut guides = [0; 2];
        let mut stoppers = 0;
        for (id, datum) in closed {
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
                self.animation.smoke.runner_datums = self
                    .scene
                    .parts
                    .iter()
                    .filter(|p| {
                        Target::of(p).member == 1
                            && (p.pose.path.contains("/Runners/")
                                || p.pose.path.contains("/Boxes/"))
                    })
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
            if [3, 5, 6, 8, 9].contains(&self.smoke_stage)
                || ([2, 4].contains(&self.smoke_stage) && self.smoke_frame == 0)
            {
                self.advance_motion(0.05, ctx);
            }
            self.check_shared_runners()?;
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
            if [3, 5, 6, 8, 9].contains(&self.smoke_stage) && !self.animation.tracks.is_empty() {
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
                    if self.smoke_mechanism(1, Some(0))?.value.abs() > 0.001 {
                        return Err("Shared-shell sibling moved".into());
                    }
                    if self.smoke_stage == 5 {
                        let m = self.smoke_mechanism(1, Some(1))?;
                        if (m.value - m.max).abs() > 0.01 {
                            return Err("Drawer did not reach the allowed endpoint".into());
                        }
                    }
                }
                6 if lid.value.abs() > 0.001
                    || lid.latch.abs() > 0.001
                    || self.smoke_mechanism(1, Some(1))?.value.abs() > 0.001 =>
                {
                    return Err("Closure did not restore locks and drawers".into());
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
                    AssemblyRules::apply_motion_sample(
                        &mut self.scene,
                        &lid.child,
                        MotionSample {
                            value: lid.min,
                            latch: 0.5,
                        },
                    )?;
                    self.smoke_toggle(&lid, ctx)?;
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
                    self.advance_motion(0.1, ctx);
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
                _ => {
                    std::fs::write(folder.join("smoke-result.txt"),"PASS: staged lock release / lid opening / re-latching; independent shared-shell drawers with measured two-stage guide motion and fixed stoppers during extraction, return and rebuild; fixed housings; view-aware suspension and reversal; support and travel rejection; unchanged mesh resources; closed dimensions; persisted lid/drawer endpoints after rebuild and TOML roundtrip. Motion is a constrained display preview, not CAD validation.").map_err(|e|e.to_string())?;
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
