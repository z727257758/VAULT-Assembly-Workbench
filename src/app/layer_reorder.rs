use crate::scene::Scene;
use glam::Vec3;

const SECONDS: f64 = 0.42;

pub(super) struct Pending {
    pub revision: u64,
    pub sources: Vec<usize>,
    pub lifted: Vec<bool>,
}

pub(super) struct Motion {
    pub revision: u64,
    pub start: f64,
    tracks: Vec<Track>,
}

#[derive(Default)]
struct Track {
    offset: Vec3,
    explosion_offset: Vec3,
    band_delta: f32,
    lift: f32,
}

impl Motion {
    pub fn new(
        pending: &Pending,
        source: &Scene,
        target: &Scene,
        previous: Option<&Self>,
        start: f64,
    ) -> Option<Self> {
        if pending.sources.len() != target.member_shifts.len()
            || pending.sources.len() != source.member_shifts.len()
        {
            return None;
        }
        let source_ranks = source.member_layer_ranks();
        let target_ranks = target.member_layer_ranks();
        let explosion_shift = |scene: &Scene, index| {
            scene
                .member_explosion_shifts
                .get(index)
                .copied()
                .unwrap_or_default()
        };
        let tracks = pending
            .sources
            .iter()
            .zip(&pending.lifted)
            .enumerate()
            .map(|(index, (&old, &lifted))| {
                let prior =
                    previous.map_or_else(Track::default, |motion| motion.sample(old, start));
                let (lo, hi) = target.member_bounds[index];
                let source_position =
                    source.member_shifts[old] + source.member_lid_slide_offset(old);
                let target_position =
                    target.member_shifts[index] + target.member_lid_slide_offset(index);
                Track {
                    offset: source_position - target_position + prior.offset,
                    explosion_offset: explosion_shift(source, old) - explosion_shift(target, index)
                        + prior.explosion_offset,
                    band_delta: source_ranks[old] as f32 - target_ranks[index] as f32
                        + prior.band_delta,
                    lift: if lifted {
                        ((hi.z - lo.z) * 0.24).clamp(24., 64.)
                    } else {
                        0.
                    },
                }
            })
            .collect();
        Some(Self {
            revision: pending.revision,
            start,
            tracks,
        })
    }

    fn progress(&self, now: f64) -> f32 {
        ((now - self.start) / SECONDS).clamp(0., 1.) as f32
    }

    fn sample(&self, index: usize, now: f64) -> Track {
        let Some(track) = self.tracks.get(index) else {
            return Track::default();
        };
        let t = self.progress(now);
        if t == 1. {
            return Track::default();
        }
        let eased = t * t * (3. - 2. * t);
        let lift = track.lift * (std::f32::consts::PI * eased).sin();
        Track {
            offset: track.offset * (1. - eased) + Vec3::Z * lift,
            explosion_offset: track.explosion_offset * (1. - eased),
            band_delta: track.band_delta * (1. - eased),
            lift: 0.,
        }
    }

    pub fn offsets(&self, now: f64, layer_gap: f32, explosion: f32) -> Vec<Vec3> {
        if self.progress(now) == 1. {
            return Vec::new();
        }
        (0..self.tracks.len())
            .map(|index| {
                let offset = self.sample(index, now);
                offset.offset
                    + Vec3::Y * (offset.band_delta * layer_gap)
                    + offset.explosion_offset * explosion
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversing_a_swap_midway_preserves_the_visible_positions() {
        let scene = |top: f32| Scene {
            member_shifts: vec![Vec3::ZERO, Vec3::Y * top],
            member_explosion_shifts: vec![Vec3::ZERO, Vec3::Y * top * 0.5],
            member_bounds: vec![
                (Vec3::ZERO, Vec3::splat(top)),
                (Vec3::Y * top, Vec3::splat(2. * top)),
            ],
            ..Default::default()
        };
        let original = scene(50.);
        let swapped = scene(80.);
        let mut swap = Pending {
            revision: 1,
            sources: vec![1, 0],
            lifted: vec![false, true],
        };
        let motion = Motion::new(&swap, &original, &swapped, None, 0.).unwrap();
        let gap = 80.;
        let at_start = motion.offsets(0., gap, 0.);
        assert_eq!(at_start[0], Vec3::Y * (50. + gap));
        assert_eq!(at_start[1], Vec3::NEG_Y * (80. + gap));
        let exploded_start = motion.offsets(0., 0., 1.);
        assert_eq!(exploded_start, vec![Vec3::Y * 75., Vec3::NEG_Y * 120.]);

        let halfway = SECONDS * 0.5;
        let visible = motion.offsets(halfway, gap, 0.);
        swap.revision = 2;
        swap.lifted.reverse();
        let reversed = Motion::new(&swap, &swapped, &original, Some(&motion), halfway).unwrap();
        let resumed = reversed.offsets(halfway, gap, 0.);
        assert!((resumed[0] - (visible[1] + Vec3::Y * (80. + gap))).length() < 0.001);
        assert!((resumed[1] + Vec3::Y * (50. + gap) - visible[0]).length() < 0.001);
        assert!(
            reversed
                .offsets(halfway + SECONDS + 0.01, gap, 0.)
                .is_empty()
        );
    }
}
