//! Stable assembly identities for editing and camera selection.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub member: usize,
    pub drawer: Option<usize>,
}
impl Target {
    pub fn of(part: &Part) -> Self {
        Self {
            member: (part.pose.instance / MEMBER_STRIDE) as usize,
            drawer: ((part.pose.instance % MEMBER_STRIDE)
                / crate::constraints::tower::DRAWER_STRIDE)
                .checked_sub(1)
                .map(|d| d as usize),
        }
    }
    pub fn contains(self, part: &Part) -> bool {
        let owner = Self::of(part);
        owner.member == self.member && self.drawer.is_none_or(|d| owner.drawer == Some(d))
    }
    pub fn instance(self, local: u32) -> u32 {
        self.member as u32 * MEMBER_STRIDE
            + self.drawer.map_or(0, |d| {
                (d as u32 + 1) * crate::constraints::tower::DRAWER_STRIDE
            })
            + local
    }
}
impl Scene {
    pub fn target_envelope(&self, target: Target) -> Option<(Vec3, Vec3)> {
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for part in self.parts.iter().filter(|p| target.contains(p)) {
            let matrix = part.pose.matrix(0.);
            for i in 0..8 {
                let point = matrix.transform_point3(Vec3::new(
                    if i & 1 == 0 {
                        part.mesh.min.x
                    } else {
                        part.mesh.max.x
                    },
                    if i & 2 == 0 {
                        part.mesh.min.y
                    } else {
                        part.mesh.max.y
                    },
                    if i & 4 == 0 {
                        part.mesh.min.z
                    } else {
                        part.mesh.max.z
                    },
                ));
                lo = lo.min(point);
                hi = hi.max(point);
            }
        }
        lo.is_finite().then_some((lo, hi))
    }
    pub fn target_scene(&self, target: Target) -> Self {
        Self {
            parts: self
                .parts
                .iter()
                .filter(|p| target.contains(p))
                .cloned()
                .collect(),
            assembly: self.assembly,
            ..Default::default()
        }
    }
    pub fn interior_geometry(&self, member: usize) -> Option<&crate::sustainer_dividers::Geometry> {
        if self.member_bounds.is_empty() {
            self.sustainer_interior.as_deref()
        } else {
            self.member_interiors.get(member).and_then(|g| g.as_deref())
        }
    }
}
