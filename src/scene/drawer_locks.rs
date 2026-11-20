use super::*;
use crate::constraints::{AssemblyRules, Joint, PartId, Relation};

impl Scene {
    /// Older projects stored the knob and both racks as fixed parts in the manual's
    /// unlocked installation pose. Rebuild the working joints in the same parent frame.
    pub(super) fn upgraded_drawer_locks(&self, old: &Scene) -> HashMap<PartId, Relation> {
        let mut updates = HashMap::new();
        for relation in self
            .relations
            .iter()
            .filter(|r| AssemblyRules::is_drawer_lock(&r.label))
        {
            let saved = old
                .relations
                .iter()
                .find(|r| r.child == relation.child && r.parent == relation.parent);
            if saved.is_some_and(|r| {
                AssemblyRules::is_drawer_lock(&r.label)
                    && std::mem::discriminant(&r.joint) == std::mem::discriminant(&relation.joint)
            }) {
                continue;
            }
            let Some(driver) = AssemblyRules::latch_owner(self, &relation.child) else {
                continue;
            };
            let mut upgraded = relation.clone();
            // When adding racks to an existing rotating key, retain its phase. Earlier
            // versions only stored drawer travel, so an open drawer starts unlocked.
            let fraction = match AssemblyRules::mechanism_for_joint(old, &driver) {
                Ok(saved) if saved.has_latch() => saved.latch,
                _ if old.relations.iter().any(|r| {
                    r.child == driver
                        && matches!(r.joint, Joint::Slider { offset, .. } if offset > 0.001)
                }) =>
                {
                    1.
                }
                _ => 0.,
            };
            let legacy = saved.and_then(|r| match r.joint {
                Joint::Fixed { position, rotation }
                    if old.calibrated
                        || [
                            "Key / facade bore",
                            "lock body / facade bore",
                            "geared rack / front channel",
                        ]
                        .iter()
                        .any(|seat| r.label.ends_with(seat)) =>
                {
                    Some((Vec3::from(position), Quat::from_array(rotation).normalize()))
                }
                _ => None,
            });
            match &mut upgraded.joint {
                Joint::Hinge {
                    position,
                    rotation,
                    pivot,
                    axis,
                    angle,
                    max,
                    ..
                } => {
                    if let Some((seat, open_rotation)) = legacy {
                        let open_axis = Vec3::from(*axis);
                        let open = Quat::from_axis_angle(open_axis, max.to_radians())
                            * Quat::from_array(*rotation);
                        let correction = open_rotation * open.inverse();
                        *axis = (correction * open_axis).to_array();
                        *rotation = (correction * Quat::from_array(*rotation)).to_array();
                        *position = seat.to_array();
                        *pivot = *position;
                    }
                    *angle = *max * fraction;
                }
                Joint::Slider {
                    position,
                    rotation,
                    axis,
                    offset,
                    max,
                    ..
                } => {
                    if let Some((seat, open_rotation)) = legacy {
                        let correction = open_rotation * Quat::from_array(*rotation).inverse();
                        let retract = correction * Vec3::from(*axis);
                        *axis = retract.to_array();
                        *position = (seat - retract * *max).to_array();
                        *rotation = open_rotation.to_array();
                    }
                    *offset = *max * fraction;
                }
                _ => continue,
            }
            updates.insert(upgraded.child.clone(), upgraded);
        }
        updates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn fixed_drawer_lock_seats_and_open_drawer_migrate_together() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let catalog = crate::catalog::Catalog::scan(root.into()).unwrap();
        let bom = catalog.bom(&crate::catalog::Selection::default());
        let closed = Scene::load(&catalog.root, &bom, "lock migration".into(), true).unwrap();
        let target = Target {
            member: 0,
            drawer: None,
        };
        let child = AssemblyRules::mechanism(&closed, target).unwrap().child;
        let mut old = closed.clone();
        AssemblyRules::set_slider(&mut old, &child, 32.).unwrap();
        let mut seats = HashMap::new();
        for r in &mut old.relations {
            if AssemblyRules::is_drawer_lock(&r.label) {
                let seat =
                    Mat4::from_rotation_translation(Quat::from_rotation_z(0.03), Vec3::X * 0.17)
                        * r.joint.transform().unwrap();
                let (_, q, p) = seat.to_scale_rotation_translation();
                r.joint = Joint::Fixed {
                    position: p.to_array(),
                    rotation: q.to_array(),
                };
                r.label = "Custom seat".into();
                seats.insert(r.child.clone(), seat);
            }
        }
        assert_eq!(seats.len(), 3);
        old.calibrated = true;
        let mut restored = closed.clone();
        restored.apply(&old.definition()).unwrap();
        assert!(!restored.calibrated);
        let m = AssemblyRules::mechanism(&restored, target).unwrap();
        assert!((m.value - 32.).abs() < 0.001 && (m.latch - 1.).abs() < 0.001);
        for r in &restored.relations {
            if let Some(seat) = seats.get(&r.child) {
                assert!(seat.abs_diff_eq(r.joint.transform().unwrap(), 0.001));
            }
        }
        AssemblyRules::set_slider(&mut restored, &child, 0.).unwrap();
        let m = AssemblyRules::mechanism(&restored, target).unwrap();
        assert!(m.latch.abs() < 0.001);
        let mut reloaded = closed;
        reloaded.apply(&restored.definition()).unwrap();
        assert!(AssemblyRules::plan_latch(&reloaded, &child, true).is_ok());
    }
}
