use super::*;
use crate::constraints::{AssemblyRules, Joint, PartId, Relation};

fn rotate_mount(
    position: &mut [f32; 3],
    rotation: &mut [f32; 4],
    pivot: [f32; 3],
    axis: [f32; 3],
    degrees: f32,
) {
    let turn = Quat::from_axis_angle(Vec3::from(axis), degrees.to_radians());
    let center = Vec3::from(pivot);
    *position = (center + turn * (Vec3::from(*position) - center)).to_array();
    *rotation = (turn * Quat::from_array(*rotation)).normalize().to_array();
}

impl Scene {
    pub(super) fn upgraded_lid_latches(&self, old: &Scene) -> HashMap<PartId, Relation> {
        let mut updates = HashMap::new();
        for lid in self
            .relations
            .iter()
            .filter(|r| AssemblyRules::label(&r.label) == crate::sustainer::LID)
        {
            let member = lid.child.instance / MEMBER_STRIDE;
            let hinges: Vec<_> = old
                .relations
                .iter()
                .filter(|r| {
                    r.child.instance / MEMBER_STRIDE == member
                        && AssemblyRules::lid_latch_slot(&r.label).is_some()
                })
                .collect();
            if hinges.is_empty()
                || !hinges
                    .iter()
                    .all(|r| AssemblyRules::previous_lid_latch(&r.label))
            {
                continue;
            }
            if let Some(converted) = self.convert_member_latches(old, &lid.child, &hinges) {
                for r in converted {
                    updates.insert(r.child.clone(), r);
                }
            }
        }
        updates
    }

    /// A large case may have two clasps on one lid. Convert both together or
    /// retain the saved recipe if a custom joint cannot perform the new motion.
    fn convert_member_latches(
        &self,
        old: &Scene,
        lid: &PartId,
        hinges: &[&Relation],
    ) -> Option<Vec<Relation>> {
        let phase = AssemblyRules::mechanism_for_joint(old, lid).ok()?.latch;
        let (tall, xl4) = AssemblyRules::latch_variant(old, lid.instance / MEMBER_STRIDE);
        let (previous, _) = AssemblyRules::lid_latch_angles(tall, xl4, true);
        let (current, travel) = AssemblyRules::lid_latch_angles(tall, xl4, false);
        let mut updates = Vec::new();
        for &saved in hinges {
            let slot = AssemblyRules::lid_latch_slot(&saved.label)?;
            let target = self.relations.iter().find(|r| {
                r.child == saved.child
                    && r.parent == saved.parent
                    && AssemblyRules::lid_latch_slot(&r.label) == Some(slot)
            })?;
            let mut r = saved.clone();
            let Joint::Hinge {
                position,
                rotation,
                pivot,
                axis,
                min,
                max,
                angle,
            } = &mut r.joint
            else {
                return None;
            };
            if !(*min..=*max).contains(&travel[slot]) {
                return None;
            }
            rotate_mount(
                position,
                rotation,
                *pivot,
                *axis,
                current[slot] - previous[slot],
            );
            *angle = travel[slot] * phase;
            let tag = [crate::sustainer::LOCK_INNER, crate::sustainer::LOCK_OUTER][slot];
            r.label = target
                .label
                .replace(AssemblyRules::label(&target.label), tag);
            if slot == 1 {
                // The big pins are fixed to the inner link and share the outer
                // hinge axis. The installation draws their caps at the same clock
                // angle in the closed case. Small pins remain fixed to the holder.
                for pin in old.relations.iter().filter(|p| {
                    p.parent == saved.parent
                        && p.child.path.contains("/1 SET OF PINS")
                        && self.relations.iter().any(|n| {
                            n.child == p.child
                                && n.parent == p.parent
                                && n.label.ends_with("large elastic pin / inner lock")
                        })
                }) {
                    let mut updated = pin.clone();
                    let Joint::Fixed { position, rotation } = &mut updated.joint else {
                        return None;
                    };
                    rotate_mount(position, rotation, *pivot, *axis, previous[0] - current[0]);
                    updates.push(updated);
                }
            }
            updates.push(r);
        }
        Some(updates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Both fixtures share manually tilted hinge axes and off-center STL origins.
    fn member(previous: bool, index: usize, phase: f32, lid_angle: f32) -> Scene {
        let mut scene = crate::scene::tests::tiny_scene();
        scene.assembly = false;
        let original = scene.parts[0].clone();
        scene.parts = [
            if index == 1 {
                "parts/Modules/XL4/box"
            } else {
                "parts/Modules/S3/box"
            },
            "parts/roof",
            "parts/Locks/inner",
            "parts/Locks/LOCK OUTER PART 2-4F",
            "parts/Locks/1 SET OF PINS big",
            "parts/Locks/1 SET OF PINS small",
        ]
        .into_iter()
        .map(|path| {
            let mut part = original.clone();
            part.pose.path = path.into();
            part
        })
        .collect();
        let (closed, travel) = AssemblyRules::lid_latch_angles(true, index == 1, previous);
        let names = if previous {
            [AssemblyRules::PREVIOUS_INNER, AssemblyRules::PREVIOUS_OUTER]
        } else {
            [crate::sustainer::LOCK_INNER, crate::sustainer::LOCK_OUTER]
        };
        let axis = Vec3::new(0.8, 0., 0.6);
        let turn = |degrees: f32| Quat::from_axis_angle(axis, degrees.to_radians());
        let big = Vec3::new(1., 12., 2.);
        let hinge = |pivot: Vec3, slot: usize| {
            let orientation = turn(closed[slot]);
            Joint::Hinge {
                position: (pivot - orientation * Vec3::new(0., 1., 3.)).to_array(),
                rotation: orientation.to_array(),
                pivot: pivot.to_array(),
                axis: axis.to_array(),
                min: -180.,
                max: 180.,
                angle: travel[slot] * phase,
            }
        };
        let fixed = |p: Vec3, q: Quat| Joint::Fixed {
            position: p.to_array(),
            rotation: q.to_array(),
        };
        for (parent, child, label, joint) in [
            (
                0,
                1,
                crate::sustainer::LID,
                Joint::Hinge {
                    position: [0.; 3],
                    rotation: Quat::IDENTITY.to_array(),
                    pivot: [0.; 3],
                    axis: Vec3::X.to_array(),
                    min: 0.,
                    max: 110.,
                    angle: lid_angle,
                },
            ),
            (0, 2, names[0], hinge(Vec3::new(0.2, 14., 3.3), 0)),
            (2, 3, names[1], hinge(big, 1)),
            (
                2,
                4,
                "Sustainer datum: large elastic pin / inner lock",
                fixed(big + turn(-closed[0]) * Vec3::Y, turn(-closed[0])),
            ),
            (
                0,
                5,
                "Sustainer datum: small elastic pin / holder",
                fixed(Vec3::Y, Quat::IDENTITY),
            ),
        ] {
            scene.relations.push(Relation {
                parent: PartId::of(&scene.parts[parent]),
                child: PartId::of(&scene.parts[child]),
                label: label.into(),
                joint,
            });
        }
        crate::constraints::namespace_member(&mut scene, index).unwrap();
        crate::constraints::solve(&mut scene).unwrap();
        scene
    }

    #[test]
    fn v5_keeps_phase_pins_and_open_lid_across_members_and_reloads() {
        for phase in [0., 0.35, 1.] {
            let angle = if phase == 1. { 31. } else { 0. };
            let mut saved = member(true, 0, phase, angle);
            let mut desired = member(false, 0, phase, angle);
            let mut fresh = member(false, 0, 0., 0.);
            for (scene, extra) in [
                (&mut saved, member(true, 1, 0.68, 0.)),
                (&mut desired, member(false, 1, 0.68, 0.)),
                (&mut fresh, member(false, 1, 0., 0.)),
            ] {
                scene.parts.extend(extra.parts);
                scene.relations.extend(extra.relations);
            }
            saved.calibrated = true;
            fresh.apply(&saved.definition()).unwrap();
            assert!(!fresh.calibrated);
            for (p, expected) in fresh.parts.iter().zip(&desired.parts) {
                assert!(
                    p.pose
                        .matrix(0.)
                        .abs_diff_eq(expected.pose.matrix(0.), 0.001),
                    "{}",
                    p.pose.path
                );
            }
            let id = PartId::of(&fresh.parts[1]);
            let m = AssemblyRules::mechanism_for_joint(&fresh, &id).unwrap();
            assert!((m.latch - phase).abs() < 0.0001 && m.value == angle);
            fresh.calibrated = true;
            let encoded = fresh.definition();
            fresh.apply(&encoded).unwrap();
            assert!(fresh.calibrated);
            assert_eq!(
                serde_json::to_value(&encoded.relations).unwrap(),
                serde_json::to_value(&fresh.relations).unwrap()
            );
        }
    }

    #[test]
    fn custom_limits_keep_the_v5_pair_together_and_operable() {
        let mut old = member(true, 1, 0.41, 0.);
        let (_, previous) = AssemblyRules::lid_latch_angles(true, true, true);
        let (_, current) = AssemblyRules::lid_latch_angles(true, true, false);
        // The old recipe can still move, but the new outer clasp needs more travel.
        assert!(current[1] > previous[1]);
        let Joint::Hinge { max, .. } = &mut old.relations[2].joint else {
            unreachable!()
        };
        *max = previous[1];
        old.calibrated = true;
        let mut restored = member(false, 1, 0., 0.);
        restored.apply(&old.definition()).unwrap();
        assert!(restored.calibrated);
        assert_eq!(
            serde_json::to_value(&restored.relations).unwrap(),
            serde_json::to_value(&old.relations).unwrap()
        );
        let lid = PartId::of(&restored.parts[1]);
        let release = AssemblyRules::plan_latch(&restored, &lid, true).unwrap();
        AssemblyRules::apply_motion_sample(&mut restored, &lid, release.sample(release.duration))
            .unwrap();
        assert_eq!(
            AssemblyRules::mechanism_for_joint(&restored, &lid)
                .unwrap()
                .latch,
            1.
        );
        // Loading another file into this session must use that file's limits.
        restored
            .apply(&member(true, 1, 0.32, 0.).definition())
            .unwrap();
        assert!(
            !restored
                .relations
                .iter()
                .any(|r| AssemblyRules::previous_lid_latch(&r.label))
        );
    }
}
