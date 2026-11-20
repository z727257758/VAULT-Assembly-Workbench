use super::*;

const DRAWER_ACTUATORS: [&str; 3] = [
    crate::front_panel::DRAWER_KEY,
    crate::geared_rails::LOWER_RACK,
    crate::geared_rails::UPPER_RACK,
];

impl AssemblyRules {
    pub(crate) const PREVIOUS_INNER: &str =
        "Sustainer datum: inner lock / small pins / inside-seat-v5";
    pub(crate) const PREVIOUS_OUTER: &str =
        "Sustainer datum: outer lock / large pins / inside-seat-v5";

    pub(crate) fn lid_latch_slot(label: &str) -> Option<usize> {
        let label = Self::label(label);
        if [crate::sustainer::LOCK_INNER, Self::PREVIOUS_INNER].contains(&label) {
            Some(0)
        } else if [crate::sustainer::LOCK_OUTER, Self::PREVIOUS_OUTER].contains(&label) {
            Some(1)
        } else {
            None
        }
    }

    pub(crate) fn previous_lid_latch(label: &str) -> bool {
        [Self::PREVIOUS_INNER, Self::PREVIOUS_OUTER].contains(&Self::label(label))
    }

    /// Both recipe angles are measured in the same frame. The second hinge is
    /// mounted on the moving inner link, so its coordinate is their difference.
    pub(crate) fn lid_latch_angles(tall: bool, xl4: bool, previous: bool) -> ([f32; 2], [f32; 2]) {
        let (closed, released) = if previous {
            (
                LatchDisplayPose {
                    inner_degrees: -8.,
                    outer_degrees: if xl4 { 10.5 } else { 9.5 },
                },
                LatchDisplayPose {
                    inner_degrees: -90.,
                    outer_degrees: 0.,
                },
            )
        } else {
            (
                Self::latch_display_pose(tall, xl4),
                Self::latch_release_pose(),
            )
        };
        let relative =
            |pose: LatchDisplayPose| [pose.inner_degrees, pose.outer_degrees - pose.inner_degrees];
        let closed = relative(closed);
        let released = relative(released);
        (closed, [released[0] - closed[0], released[1] - closed[1]])
    }

    pub(crate) fn is_drawer_lock(label: &str) -> bool {
        DRAWER_ACTUATORS.contains(&Self::label(label))
    }

    /// The key is mounted on the facade, and both racks are mounted on the drawer.
    /// Follow the actual parents so siblings in a shared shell keep independent controls.
    fn drawer_driver<'a>(scene: &'a Scene, part: &PartId) -> Option<&'a PartId> {
        let mut part = part;
        for _ in 0..scene.relations.len() {
            let r = scene.relations.iter().find(|r| &r.child == part)?;
            if Self::label(&r.label) == Self::DRAWER_SLIDE {
                return Some(&r.child);
            }
            part = &r.parent;
        }
        None
    }

    pub fn latch_owner(scene: &Scene, child: &PartId) -> Option<PartId> {
        let r = scene.relations.iter().find(|r| &r.child == child)?;
        let label = Self::label(&r.label);
        if Self::is_drawer_lock(&r.label) {
            return Self::drawer_driver(scene, &r.parent).cloned();
        }
        if Self::lid_latch_slot(label).is_some() {
            let member = child.instance / crate::scene::MEMBER_STRIDE;
            return scene
                .relations
                .iter()
                .find(|r| {
                    r.child.instance / crate::scene::MEMBER_STRIDE == member
                        && Self::label(&r.label) == crate::sustainer::LID
                })
                .map(|r| r.child.clone());
        }
        None
    }

    pub(super) fn latch_state(
        scene: &Scene,
        kind: MotionKind,
        driver: &PartId,
    ) -> Result<(Vec<LatchJoint>, f32), String> {
        let member = driver.instance / crate::scene::MEMBER_STRIDE;
        let lid_variant = (kind == MotionKind::Lid).then(|| Self::latch_variant(scene, member));
        let mut joints = Vec::new();
        let mut fraction: Option<f32> = None;
        let mut count = [0; 3];
        for (index, r) in scene
            .relations
            .iter()
            .enumerate()
            .filter(|(_, r)| r.child.instance / crate::scene::MEMBER_STRIDE == member)
        {
            let label = Self::label(&r.label);
            let (slot, value, released) = if let Some((tall, xl4)) = lid_variant {
                let Some(slot) = Self::lid_latch_slot(label) else {
                    continue;
                };
                let (_, travel) =
                    Self::lid_latch_angles(tall, xl4, Self::previous_lid_latch(label));
                let released = travel[slot];
                let Joint::Hinge {
                    angle, min, max, ..
                } = r.joint
                else {
                    return Err("A lid lock requires a hinge".into());
                };
                if !(min..=max).contains(&0.) || !(min..=max).contains(&released) {
                    return Err("Lid lock travel excludes a required endpoint".into());
                }
                (slot, angle, released)
            } else {
                let Some(slot) = DRAWER_ACTUATORS.iter().position(|&name| name == label) else {
                    continue;
                };
                if Self::drawer_driver(scene, &r.parent) != Some(driver) {
                    continue;
                }
                let (value, min, max) = match (&r.joint, slot) {
                    (
                        Joint::Hinge {
                            angle, min, max, ..
                        },
                        0,
                    ) => (*angle, *min, *max),
                    (
                        Joint::Slider {
                            offset, min, max, ..
                        },
                        1 | 2,
                    ) => (*offset, *min, *max),
                    _ => return Err("Drawer lock joint has the wrong motion type".into()),
                };
                if min != 0. || max <= 0.001 {
                    return Err("Drawer lock requires a closed datum and release travel".into());
                }
                (slot, value, max)
            };
            r.joint.transform()?;
            if released.abs() > 0.001 {
                let f = value / released;
                if !(-0.001..=1.001).contains(&f)
                    || fraction.is_some_and(|first| (f - first).abs() > 0.01)
                {
                    return Err("Lock components have inconsistent release positions".into());
                }
                fraction.get_or_insert(f);
            } else if value.abs() > 0.001 {
                return Err("Lock joint moved beyond its release travel".into());
            }
            joints.push(LatchJoint {
                relation: index,
                released,
            });
            count[slot] += 1;
        }
        if kind == MotionKind::Lid && (count[0] == 0 || count[0] != count[1]) {
            return Err("Opening requires matching inner and outer lid locks".into());
        }
        if kind == MotionKind::Lid {
            for inner in joints
                .iter()
                .map(|j| &scene.relations[j.relation])
                .filter(|r| Self::lid_latch_slot(&r.label) == Some(0))
            {
                let pairs = joints
                    .iter()
                    .map(|j| &scene.relations[j.relation])
                    .filter(|r| {
                        r.parent == inner.child && Self::lid_latch_slot(&r.label) == Some(1)
                    })
                    .count();
                if pairs != 1 {
                    return Err("Each inner lock requires its own outer clasp".into());
                }
            }
        }
        if kind == MotionKind::Drawer && !matches!(count, [0, 0, 0] | [1, 0, 0] | [1, 1, 1]) {
            return Err(
                "A drawer lock requires one key and an optional matched pair of geared racks"
                    .into(),
            );
        }
        Ok((joints, fraction.unwrap_or(0.).clamp(0., 1.)))
    }

    pub fn plan_latch(scene: &Scene, child: &PartId, released: bool) -> Result<MotionPlan, String> {
        let m = Self::mechanism_for_joint(scene, child)?;
        if !m.can_toggle_latch() {
            return Err(
                "Operate the lock on a closed drawer or lid with a complete lock assembly".into(),
            );
        }
        if released && m.kind == MotionKind::Lid && m.supports_upper {
            return Err("A supporting lid cannot be unlocked".into());
        }
        let goal = m.value;
        let fraction = if released { 1. } else { 0. };
        let duration = (fraction - m.latch).abs() * Self::LATCH_SECONDS;
        let plan = MotionPlan {
            mechanism: m,
            goal,
            duration,
            latch_only: Some(released),
            release_time: 0.,
            move_time: 0.,
            lock_time: 0.,
        };
        Self::solve_sample(&mut scene.clone(), &plan.mechanism, plan.sample(duration))?;
        Ok(plan)
    }
}
