//! A sampled joint path. Speeds and admissible poses are supplied by AssemblyRules.
use super::PartId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionKind {
    Lid,
    Drawer,
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
}

#[derive(Clone, Copy, Debug)]
pub struct MotionSample {
    pub value: f32,
    pub latch: f32,
}

#[derive(Clone, Debug)]
pub struct MotionPlan {
    pub mechanism: Mechanism,
    pub goal: f32,
    pub duration: f32,
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
        let m = &self.mechanism;
        if self.release_time > 0. && elapsed < self.release_time {
            return MotionSample {
                value: m.value,
                latch: blend(m.latch, 1., elapsed / self.release_time),
            };
        }
        let moving = elapsed - self.release_time;
        if self.move_time > 0. && moving < self.move_time {
            return MotionSample {
                value: blend(m.value, self.goal, moving / self.move_time),
                latch: if m.kind == MotionKind::Lid { 1. } else { 0. },
            };
        }
        let latch = if m.kind == MotionKind::Drawer {
            0.
        } else if self.lock_time > 0. {
            blend(
                if self.move_time > 0. { 1. } else { m.latch },
                0.,
                (moving - self.move_time) / self.lock_time,
            )
        } else if self.goal <= m.min + 0.001 {
            0.
        } else {
            1.
        };
        MotionSample {
            value: self.goal,
            latch,
        }
    }
}
