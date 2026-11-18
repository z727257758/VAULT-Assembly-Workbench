//! Runtime joint playback; project preferences keep only the requested endpoints.
use super::*;
use crate::constraints::{AssemblyRules, Mechanism, MotionKind, MotionPlan, MotionSample};
use crate::scene::Target;
mod smoke;

pub(super) struct Track {
    plan: MotionPlan,
    pub elapsed: f32,
    last: MotionSample,
}
#[derive(Default)]
pub(super) struct State {
    pub tracks: Vec<Track>,
    suspended: bool,
    smoke: smoke::State,
}
impl App {
    pub(super) fn layer_motion_ui(
        &mut self,
        ui: &mut egui::Ui,
        members: &[usize],
        ctx: &egui::Context,
    ) {
        let z = self.zh;
        if self.scene_key != "combination" || self.busy || self.preview.has_pending() {
            ui.add_enabled(
                false,
                egui::Button::new(t(z, "开合", "Open / close")).small(),
            );
            return;
        }
        let targets = members.iter().flat_map(|&member| {
            let drawers = self.members[member].preset.drawers.len();
            (0..drawers.max(1)).map(move |drawer| Target {
                member,
                drawer: (drawers != 0).then_some(drawer),
            })
        });
        let mechanisms: Vec<_> = targets
            .filter_map(|target| AssemblyRules::mechanism(&self.scene, target).ok())
            .filter(|m| m.max > m.min + 0.001 && (m.kind != MotionKind::Lid || !m.supports_upper))
            .collect();
        let close = mechanisms.iter().any(|m| {
            self.animation
                .tracks
                .iter()
                .find(|track| track.plan.mechanism.child == m.child)
                .map_or(m.value, |track| track.plan.goal)
                > m.min + 0.001
        });
        let label = match (mechanisms.as_slice(), close) {
            ([m], false) if m.kind == MotionKind::Drawer => t(z, "拉开", "Pull out"),
            ([m], true) if m.kind == MotionKind::Drawer => t(z, "推回", "Push in"),
            (_, false) => t(z, "打开", "Open"),
            (_, true) => t(z, "关闭", "Close"),
        };
        if ui
            .add_enabled(!mechanisms.is_empty(), egui::Button::new(label).small())
            .clicked()
        {
            for m in mechanisms {
                if let Err(error) =
                    self.animate_joint_to(&m.child, if close { m.min } else { m.max }, ctx)
                {
                    self.status = error;
                }
            }
        }
    }

    fn motion_target(&self) -> Target {
        if self.scene_key == "combination" {
            Target {
                member: self.active_member,
                drawer: self.active_drawer,
            }
        } else {
            Target {
                member: 0,
                drawer: None,
            }
        }
    }

    pub(super) fn commit_motion_goal(&mut self, m: &Mechanism, goal: f32) -> Result<(), String> {
        let part = self
            .scene
            .parts
            .iter()
            .find(|p| crate::constraints::PartId::of(p) == m.child)
            .ok_or("Motion target is unavailable")?;
        let target = if self.scene_key == "combination" {
            Target::of(part)
        } else {
            Target {
                member: self.active_member,
                drawer: self.active_drawer,
            }
        };
        if m.kind == MotionKind::Lid {
            return self.commit_lid_preference(target.member, goal);
        }
        let fraction = if m.max > m.min {
            (goal - m.min) / (m.max - m.min)
        } else {
            0.
        };
        AssemblyRules::validate_drawer_extension(fraction)?;
        if !self.members.is_empty() {
            let mut preset = inline_edit::target_preset(&self.members, target)
                .ok_or("Drawer preference is unavailable")?
                .clone();
            if preset.assembly_kind != AssemblyKind::Drawer || !preset.drawers.is_empty() {
                return Err("Select an individual drawer".into());
            }
            preset.drawer_extension = fraction;
            inline_edit::store_target(&mut self.members, target, preset);
        }
        if target.member == self.active_member && target.drawer == self.active_drawer {
            self.drawer_extension = fraction;
        }
        Ok(())
    }

    pub(super) fn animate_joint_to(
        &mut self,
        child: &crate::constraints::PartId,
        goal: f32,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if self.busy || self.preview.has_pending() || !self.scene.assembly {
            return Err(t(
                self.zh,
                "请等待预览更新完成",
                "Wait for the preview to finish updating",
            )
            .into());
        }
        let plan = AssemblyRules::plan_motion(&self.scene, child, goal)?;
        self.commit_motion_goal(&plan.mechanism, goal)?;
        self.animation
            .tracks
            .retain(|t| t.plan.mechanism.child != *child);
        self.explosion = 0.;
        self.dimensions = false;
        self.preview_modes.mode = preview_modes::Mode::Assembly;
        self.fit_motion_path(&plan);
        let last = plan.sample(0.);
        self.animation.tracks.push(Track {
            plan,
            elapsed: 0.,
            last,
        });
        self.collision_report = None;
        ctx.request_repaint();
        Ok(())
    }

    fn fit_motion_path(&mut self, plan: &MotionPlan) {
        let Some(rect) = self.inline_edit.viewport else {
            return;
        };
        let Some(part) = self
            .scene
            .parts
            .iter()
            .find(|p| crate::constraints::PartId::of(p) == plan.mechanism.child)
        else {
            return;
        };
        let target = Target::of(part);
        let mut envelope = Scene::default();
        let mut sample = self.scene.clone();
        // Camera envelopes only; these boxes do not diagnose collisions.
        for fraction in [0., 0.25, 0.5, 0.75, 1.] {
            if AssemblyRules::apply_motion_sample(
                &mut sample,
                &plan.mechanism.child,
                plan.sample(plan.duration * fraction),
            )
            .is_ok()
            {
                envelope
                    .parts
                    .extend(sample.parts.iter().filter(|p| target.contains(p)).cloned());
            }
        }
        let panel = if self.inline_edit.open {
            (rect.width() * 0.43)
                .clamp(270., 340.)
                .min(rect.width() - 32.)
                + 60.
        } else {
            0.
        };
        if !envelope.parts.is_empty() {
            inline_edit::fit_detail(&mut self.camera, &envelope, rect, panel);
            self.inline_edit.clear_focus_request();
        }
    }

    pub(super) fn cancel_joint_animation(&mut self, child: &crate::constraints::PartId) {
        self.animation
            .tracks
            .retain(|t| &t.plan.mechanism.child != child);
    }

    pub(super) fn animation_tick(&mut self, ctx: &egui::Context) {
        if self.smoke && std::env::args().any(|a| a == "--smoke-motion") {
            return;
        }
        let dt = ctx.input(|i| i.stable_dt).clamp(0., 0.1);
        self.advance_motion(dt, ctx);
    }

    pub(super) fn advance_motion(&mut self, dt: f32, ctx: &egui::Context) {
        if self.busy || self.preview.has_pending() || self.animation.tracks.is_empty() {
            return;
        }
        let incompatible = self.preview_modes.mode != preview_modes::Mode::Assembly
            || self.explosion > 0.001
            || self.dimensions
            || self.tab != 0
            || self.tower.print_list;
        if incompatible {
            if !std::mem::replace(&mut self.animation.suspended, true)
                && let Err(error) = AssemblyRules::reset_explosion(&mut self.scene)
            {
                self.status = error;
            }
            return;
        }
        self.animation.suspended = false;
        let tracks = std::mem::take(&mut self.animation.tracks);
        let mut refresh = false;
        for mut track in tracks {
            track.elapsed = (track.elapsed + dt.clamp(0., 0.1)).min(track.plan.duration);
            let sample = track.plan.sample(track.elapsed);
            match AssemblyRules::apply_motion_sample(
                &mut self.scene,
                &track.plan.mechanism.child,
                sample,
            ) {
                Ok(()) => {
                    track.last = sample;
                    self.collision_report = None;
                    if track.elapsed < track.plan.duration {
                        self.animation.tracks.push(track);
                    } else {
                        refresh = true;
                    }
                }
                Err(error) => {
                    let _ = self.commit_motion_goal(&track.plan.mechanism, track.last.value);
                    self.status = error;
                    refresh = true;
                }
            }
        }
        if refresh && let Err(error) = AssemblyRules::reset_explosion(&mut self.scene) {
            self.status = error;
        }
        if !self.animation.tracks.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    pub(super) fn motion_controls_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.separator();
        ui.strong(t(self.zh, "运动预览", "Motion preview"));
        if self.busy || self.preview.has_pending() {
            ui.small(t(self.zh, "正在更新预览…", "Updating preview…"));
            return;
        }
        let m = match AssemblyRules::mechanism(&self.scene, self.motion_target()) {
            Ok(m) => m,
            Err(error) => {
                ui.small(t(
                    self.zh,
                    "当前目标暂不支持开合预览",
                    "Opening motion is unavailable for this target",
                ))
                .on_hover_text(error);
                return;
            }
        };
        let z = self.zh;
        let blocked = m.kind == MotionKind::Lid && m.supports_upper;
        let track = self
            .animation
            .tracks
            .iter()
            .find(|track| track.plan.mechanism.child == m.child);
        let goal = track.map_or(m.value, |track| track.plan.goal);
        let (open, close) = if m.kind == MotionKind::Lid {
            (t(z, "开盖", "Open lid"), t(z, "合盖", "Close lid"))
        } else {
            (t(z, "拉出", "Pull out"), t(z, "推回", "Push in"))
        };
        let mut requested = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!blocked && m.max > goal + 0.001, egui::Button::new(open))
                .clicked()
            {
                requested = Some(m.max);
            }
            if ui
                .add_enabled(
                    goal > m.min + 0.001 || (track.is_none() && m.latch > 0.001),
                    egui::Button::new(close),
                )
                .clicked()
            {
                requested = Some(m.min);
            }
        });
        if blocked {
            ui.small(t(
                z,
                "支承上层，箱盖保持闭合。",
                "Supporting an upper member; lid stays closed.",
            ));
        }
        if let Some(value) = requested
            && let Err(error) = self.animate_joint_to(&m.child, value, ctx)
        {
            self.status = error;
        }
    }
}
