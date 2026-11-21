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

fn motion_labels(kind: MotionKind, zh: bool) -> (&'static str, &'static str) {
    match kind {
        MotionKind::Drawer => (t(zh, "拉出", "Pull out"), t(zh, "推回", "Push in")),
        MotionKind::Lid => (t(zh, "开盖", "Open lid"), t(zh, "合盖", "Close lid")),
    }
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
            .filter(|m| {
                m.max > m.min + 0.001
                    && (m.kind != MotionKind::Lid || !m.supports_upper || m.slide_distance > 0.)
            })
            .collect();
        let close = mechanisms.iter().any(|m| {
            self.animation
                .tracks
                .iter()
                .find(|track| track.plan.mechanism.child == m.child)
                .map_or(m.value > m.min + 0.001 || m.slide > 0.001, |track| {
                    track.plan.goal > m.min + 0.001
                })
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

    fn motion_toggle_goal(&self, m: &Mechanism) -> Option<f32> {
        let plan = self
            .animation
            .tracks
            .iter()
            .map(|track| &track.plan)
            .find(|plan| plan.mechanism.child == m.child && plan.latch_only.is_none());
        let goal = plan.map_or(m.value, |plan| plan.goal);
        let can_open = (m.kind != MotionKind::Lid || !m.supports_upper || m.slide_distance > 0.)
            && m.max > goal + 0.001;
        let can_close = goal > m.min + 0.001
            || (plan.is_none()
                && m.kind == MotionKind::Lid
                && m.supports_upper
                && (m.latch > 0.001 || m.slide > 0.001));
        match (can_open, can_close) {
            (true, true) if plan.is_some_and(|plan| plan.goal < plan.mechanism.value) => {
                Some(m.max)
            }
            (_, true) => Some(m.min),
            (true, false) => Some(m.max),
            (false, false) => None,
        }
    }

    fn latch_toggle_ui(&mut self, ui: &mut egui::Ui, m: &Mechanism, ctx: &egui::Context) {
        if !m.has_latch() {
            return;
        }
        let playing = self
            .animation
            .tracks
            .iter()
            .find(|t| t.plan.mechanism.child == m.child);
        let supported = m.blocked_by_upper_at(m.slide);
        let release = match playing.and_then(|t| t.plan.latch_only) {
            Some(released) => !released,
            None if supported => m.latch <= 0.001,
            None => m.latch < 0.5,
        };
        let enabled = m.can_toggle_latch()
            && !(release && supported)
            && playing.is_none_or(|t| t.plan.latch_only.is_some());
        let z = self.zh;
        let (label, hint) = match (m.kind, release) {
            (MotionKind::Drawer, true) => (
                t(z, "解锁", "Unlock"),
                t(z, "转动抽屉旋钮解锁", "Turn the drawer knob to unlock"),
            ),
            (MotionKind::Drawer, false) => (
                t(z, "上锁", "Lock"),
                t(z, "转回抽屉旋钮上锁", "Turn the drawer knob back to lock"),
            ),
            (MotionKind::Lid, true) => (
                t(z, "松扣", "Unlatch"),
                t(z, "松开箱盖正面的搭扣", "Release the front lid clasp"),
            ),
            (MotionKind::Lid, false) => (
                t(z, "扣合", "Latch"),
                t(z, "扣紧箱盖正面的搭扣", "Fasten the front lid clasp"),
            ),
        };
        if ui
            .add_enabled(enabled, egui::Button::new(label))
            .on_hover_text(hint)
            .on_disabled_hover_text(if supported {
                t(
                    z,
                    "箱盖支承着上层，搭扣应保持扣合",
                    "Keep the clasp fastened while the lid supports an upper member",
                )
            } else {
                t(
                    z,
                    "完全闭合并停止开合后可操作",
                    "Available when fully closed and stationary",
                )
            })
            .clicked()
            && let Err(error) = self.animate_latch_to(&m.child, release, ctx)
        {
            self.status = error;
        }
    }

    pub(super) fn inline_motion_toggle_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        let mechanism = if self.busy || self.preview.has_pending() {
            Err(t(z, "正在更新预览…", "Updating preview…").to_owned())
        } else if self.scene_key != "combination" {
            Err(t(z, "装配预览尚未就绪", "Assembly preview is unavailable").to_owned())
        } else {
            AssemblyRules::mechanism(&self.scene, self.motion_target())
        };
        let m = match mechanism {
            Ok(m) => m,
            Err(reason) => {
                let label = match self.assembly_kind {
                    AssemblyKind::Drawer => t(z, "抽屉开合", "Drawer motion"),
                    AssemblyKind::Sustainer => t(z, "箱盖开合", "Lid motion"),
                };
                ui.add_enabled(false, egui::Button::new(label))
                    .on_disabled_hover_text(reason);
                return;
            }
        };
        let goal = self.motion_toggle_goal(&m);
        let (open, close) = motion_labels(m.kind, z);
        let label = if goal == Some(m.min) { close } else { open };
        let response = ui.add_enabled(goal.is_some(), egui::Button::new(label));
        if let Some(goal) = goal {
            let response = if m.slide_distance > 0. && goal > m.min {
                response.on_hover_text(t(
                    z,
                    "箱体先向左滑出，再开盖",
                    "Slide left, then open the lid",
                ))
            } else {
                response
            };
            if response.clicked()
                && let Err(error) = self.animate_joint_to(&m.child, goal, ctx)
            {
                self.status = error;
            }
        } else {
            response.on_disabled_hover_text(if m.kind == MotionKind::Lid && m.supports_upper {
                t(
                    z,
                    "支承上层，箱盖保持闭合。",
                    "Supporting an upper member; lid stays closed.",
                )
            } else {
                t(
                    z,
                    "此关节没有可用行程",
                    "No travel is available for this joint",
                )
            });
        }
        self.latch_toggle_ui(ui, &m, ctx);
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
        self.ensure_motion_ready()?;
        let plan = AssemblyRules::plan_motion(&self.scene, child, goal)?;
        self.commit_motion_goal(&plan.mechanism, goal)?;
        self.play_motion(plan, ctx);
        Ok(())
    }

    fn ensure_motion_ready(&self) -> Result<(), String> {
        if self.busy || self.preview.has_pending() || !self.scene.assembly {
            return Err(t(
                self.zh,
                "请等待预览更新完成",
                "Wait for the preview to finish updating",
            )
            .into());
        }
        Ok(())
    }

    fn animate_latch_to(
        &mut self,
        child: &crate::constraints::PartId,
        released: bool,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        self.ensure_motion_ready()?;
        let plan = AssemblyRules::plan_latch(&self.scene, child, released)?;
        if self.animating_opening(child) {
            return Err(t(
                self.zh,
                "请等待开合动作完成",
                "Wait for the opening or closing motion to finish",
            )
            .into());
        }
        self.play_motion(plan, ctx);
        Ok(())
    }

    fn play_motion(&mut self, plan: MotionPlan, ctx: &egui::Context) {
        self.cancel_joint_animation(&plan.mechanism.child);
        self.explosion = 0.;
        self.dimensions = false;
        self.preview_modes.mode = preview_modes::Mode::Assembly;
        if plan.latch_only.is_none() {
            self.fit_motion_path(&plan);
        }
        let last = plan.sample(0.);
        self.animation.tracks.push(Track {
            plan,
            elapsed: 0.,
            last,
        });
        self.collision_report = None;
        ctx.request_repaint();
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

    pub(super) fn animating_opening(&self, child: &crate::constraints::PartId) -> bool {
        self.animation
            .tracks
            .iter()
            .any(|t| &t.plan.mechanism.child == child && t.plan.latch_only.is_none())
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
                    if track.plan.latch_only.is_none() {
                        let _ = self.commit_motion_goal(&track.plan.mechanism, track.last.value);
                    }
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
}
