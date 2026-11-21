//! A sampled joint path. Speeds and admissible poses are supplied by AssemblyRules.
use super::PartId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionKind {
    Lid,
    Drawer,
}

#[derive(Clone, Debug)]
pub(super) struct LatchJoint {
    pub(super) relation: usize,
    pub(super) released: f32,
}

#[derive(Clone, Debug)]
pub struct Mechanism {
    pub child: PartId,
    pub kind: MotionKind,
    pub min: f32,
    pub max: f32,
    pub value: f32,
    pub latch: f32,
    pub supports_upper: bool,
    pub slide: f32,
    pub slide_distance: f32,
    /// Relation index and released joint coordinate in the scene used for this sample.
    pub(super) latch_joints: Vec<LatchJoint>,
}
impl Mechanism {
    pub fn has_latch(&self) -> bool {
        !self.latch_joints.is_empty()
    }
    pub fn can_toggle_latch(&self) -> bool {
        self.has_latch()
            && self.value <= self.min + 0.001
            && (!self.blocked_by_upper_at(self.slide) || self.latch > 0.001)
    }
    pub(crate) fn blocked_by_upper_at(&self, slide: f32) -> bool {
        self.kind == MotionKind::Lid
            && self.supports_upper
            && (self.slide_distance == 0. || slide + 0.001 < self.slide_distance)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MotionSample {
    pub value: f32,
    pub latch: f32,
    pub slide: f32,
}

#[derive(Clone, Debug)]
pub struct MotionPlan {
    pub mechanism: Mechanism,
    pub goal: f32,
    pub duration: f32,
    /// A latch-only preview keeps the drawer or lid at its closed position.
    pub latch_only: Option<bool>,
    pub(super) goal_slide: f32,
    pub(super) slide_out_time: f32,
    pub(super) slide_in_time: f32,
    pub(super) release_time: f32,
    pub(super) move_time: f32,
    pub(super) lock_time: f32,
}

#[derive(Clone)]
pub struct SliderMount {
    pub child: PartId,
    pub world_axis: glam::Vec3,
    pub offset: f32,
    pub min: f32,
    pub max: f32,
}

fn blend(a: f32, b: f32, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    a + (b - a) * t * t * (3. - 2. * t)
}
impl MotionPlan {
    pub fn sample(&self, elapsed: f32) -> MotionSample {
        let elapsed = elapsed.max(0.);
        let m = &self.mechanism;
        if elapsed >= self.duration {
            let released = self
                .latch_only
                .unwrap_or(m.has_latch() && self.goal > m.min + 0.001);
            return MotionSample {
                value: self.goal,
                latch: if released { 1. } else { 0. },
                slide: self.goal_slide,
            };
        }
        if let Some(released) = self.latch_only {
            let end = if released { 1. } else { 0. };
            return MotionSample {
                value: m.value,
                latch: if self.duration > 0. {
                    blend(m.latch, end, elapsed / self.duration)
                } else {
                    end
                },
                slide: m.slide,
            };
        }
        if elapsed < self.slide_out_time {
            return MotionSample {
                value: m.value,
                latch: m.latch,
                slide: blend(m.slide, self.goal_slide, elapsed / self.slide_out_time),
            };
        }
        let slide = m.slide.max(self.goal_slide);
        let release_elapsed = elapsed - self.slide_out_time;
        if self.release_time > 0. && release_elapsed < self.release_time {
            return MotionSample {
                value: m.value,
                latch: blend(m.latch, 1., release_elapsed / self.release_time),
                slide,
            };
        }
        let moving = release_elapsed - self.release_time;
        if self.move_time > 0. && moving < self.move_time {
            return MotionSample {
                value: blend(m.value, self.goal, moving / self.move_time),
                latch: if m.has_latch() { 1. } else { 0. },
                slide,
            };
        }
        let lock_elapsed = moving - self.move_time;
        if self.slide_in_time > 0. && lock_elapsed >= self.lock_time {
            return MotionSample {
                value: self.goal,
                latch: 0.,
                slide: blend(
                    slide,
                    self.goal_slide,
                    (lock_elapsed - self.lock_time) / self.slide_in_time,
                ),
            };
        }
        let latch = if !m.has_latch() {
            0.
        } else if self.lock_time > 0. {
            blend(
                if self.move_time > 0. { 1. } else { m.latch },
                0.,
                lock_elapsed / self.lock_time,
            )
        } else if self.goal <= m.min + 0.001 {
            0.
        } else {
            1.
        };
        MotionSample {
            value: self.goal,
            latch,
            slide,
        }
    }
}
