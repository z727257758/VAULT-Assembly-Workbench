use super::*;

const DRAWER_ACTUATORS: [&str; 3] = [
    crate::front_panel::DRAWER_KEY,
    crate::geared_rails::LOWER_RACK,
    crate::geared_rails::UPPER_RACK,
];

impl AssemblyRules {
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
        if [crate::sustainer::LOCK_INNER, crate::sustainer::LOCK_OUTER].contains(&label) {
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
        let lid_travel = (kind == MotionKind::Lid).then(|| Self::lock_travel(scene, member));
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
            let (slot, value, released) = if let Some((inner, outer)) = lid_travel {
                let (slot, released) = match label {
                    crate::sustainer::LOCK_INNER => (0, inner),
                    crate::sustainer::LOCK_OUTER => (1, outer),
                    _ => continue,
                };
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
