use super::*;
use crate::constraints::{AssemblyRules, Joint, PartId, Relation};

// The first animated lock used a 60-degree preview and a 4 mm rack pitch per 18 degrees.
const LEGACY_KEY_TURN: f32 = 60.;
const LEGACY_RACK_TRAVEL: f32 = LEGACY_KEY_TURN / 18. * 4.;
const QUARTER_TURN: f32 = 90.;
const QUARTER_TURN_TRAVEL: f32 = QUARTER_TURN / 18. * 4.;

impl Scene {
    /// Only unversioned files enter here. Inspect every actuator on a drawer before
    /// changing its motion so a custom set of coupled limits stays intact.
    pub(super) fn upgrade_legacy_lock_ranges(&self, old: &mut Scene) -> bool {
        let mut drawers: HashMap<PartId, Vec<usize>> = HashMap::new();
        for (index, r) in old.relations.iter().enumerate() {
            if AssemblyRules::is_drawer_lock(&r.label)
                && let Some(driver) = AssemblyRules::latch_owner(old, &r.child)
            {
                drawers.entry(driver).or_default().push(index);
            }
        }
        let mut changed = false;
        for indices in drawers.values() {
            let key = |r: &Relation| r.label.ends_with(crate::front_panel::DRAWER_KEY);
            let standard = indices.iter().all(|&i| {
                let r = &old.relations[i];
                match r.joint {
                    Joint::Hinge { min, max, .. } if key(r) => {
                        min == 0. && (max - LEGACY_KEY_TURN).abs() < 1e-5
                    }
                    Joint::Slider { min, max, .. } if !key(r) => {
                        min == 0. && (max - LEGACY_RACK_TRAVEL).abs() < 1e-5
                    }
                    _ => false,
                }
            });
            if !standard || !indices.iter().any(|&i| key(&old.relations[i])) {
                continue;
            }
            for &i in indices {
                let saved = &mut old.relations[i];
                let Some(current) = self.relations.iter().find(|r| {
                    r.child == saved.child && r.parent == saved.parent && r.label == saved.label
                }) else {
                    continue;
                };
                // Keep the released installation datum and the user's axis/pivot.
                // Moving the closed datum backwards extends the motion at the locking end.
                match (&mut saved.joint, &current.joint) {
                    (
                        Joint::Hinge {
                            position,
                            rotation,
                            pivot,
                            axis,
                            angle,
                            max,
                            ..
                        },
                        Joint::Hinge { .. },
                    ) => {
                        let delta = Quat::from_axis_angle(
                            Vec3::from(*axis),
                            (*max - QUARTER_TURN).to_radians(),
                        );
                        let center = Vec3::from(*pivot);
                        *position = (center + delta * (Vec3::from(*position) - center)).to_array();
                        *rotation = (delta * Quat::from_array(*rotation)).normalize().to_array();
                        *angle = (*angle / *max) * QUARTER_TURN;
                        *max = QUARTER_TURN;
                    }
                    (
                        Joint::Slider {
                            position,
                            axis,
                            offset,
                            max,
                            ..
                        },
                        Joint::Slider { .. },
                    ) => {
                        *position = (Vec3::from(*position)
                            + Vec3::from(*axis) * (*max - QUARTER_TURN_TRAVEL))
                            .to_array();
                        *offset = (*offset / *max) * QUARTER_TURN_TRAVEL;
                        *max = QUARTER_TURN_TRAVEL;
                    }
                    _ => continue,
                }
                changed = true;
            }
        }
        changed
    }

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

    fn drawer_with_locks(turn: f32, rack_travel: f32, phase: f32) -> Scene {
        let mut scene = crate::scene::tests::tiny_scene();
        scene.parts[0].pose.instance = MEMBER_STRIDE;
        for name in ["drawer", "key", "lower", "upper"] {
            let mut p = scene.parts[0].clone();
            p.pose.path = format!("{name}.stl");
            p.source_size.y = 120.;
            scene.parts.push(p);
        }
        let id = |i: usize| PartId::of(&scene.parts[i]);
        // The member prefix, oblique axis, and displaced hinge pivot exercise
        // the same migration on manually calibrated installations.
        let slider = |axis: Vec3, max: f32, offset: f32| Joint::Slider {
            position: [3., 7., -6.],
            rotation: Quat::from_rotation_z(0.12).to_array(),
            axis: axis.to_array(),
            min: 0.,
            max,
            offset,
        };
        let rack_axis = Vec3::new(0.6, 0., 0.8);
        scene.relations = [
            (
                0,
                1,
                AssemblyRules::DRAWER_SLIDE,
                slider(Vec3::Z, 90., if phase == 1. { 18. } else { 0. }),
            ),
            (
                1,
                2,
                crate::front_panel::DRAWER_KEY,
                Joint::Hinge {
                    position: [2., -1., 9.],
                    rotation: Quat::from_rotation_x(0.8).to_array(),
                    pivot: [3., -2., 5.],
                    axis: [0., 0.8, 0.6],
                    min: 0.,
                    max: turn,
                    angle: phase * turn,
                },
            ),
            (
                1,
                3,
                crate::geared_rails::LOWER_RACK,
                slider(rack_axis, rack_travel, phase * rack_travel),
            ),
            (
                1,
                4,
                crate::geared_rails::UPPER_RACK,
                slider(-rack_axis, rack_travel, phase * rack_travel),
            ),
        ]
        .into_iter()
        .map(|(parent, child, label, joint)| Relation {
            parent: id(parent),
            child: id(child),
            joint,
            label: format!("Member 2: {label}"),
        })
        .collect();
        crate::constraints::solve(&mut scene).unwrap();
        scene.calibrated = true;
        scene
    }

    fn released_pose(mut joint: Joint) -> Mat4 {
        match &mut joint {
            Joint::Slider { offset, max, .. } => *offset = *max,
            Joint::Hinge { angle, max, .. } => *angle = *max,
            _ => unreachable!(),
        }
        joint.transform().unwrap()
    }

    #[test]
    fn sixty_degree_save_keeps_release_seats_and_phase_after_upgrading_once() {
        for phase in [0., 0.4, 1.] {
            let old = drawer_with_locks(60., LEGACY_RACK_TRAVEL, phase);
            let template = drawer_with_locks(90., 20., 0.);
            let text = toml::to_string(&old.definition())
                .unwrap()
                .replace("drawer_lock_version = 1\n", "");
            let legacy: Definition = toml::from_str(&text).unwrap();
            assert_eq!(legacy.drawer_lock_version, 0);
            let mut upgraded = template.clone();
            upgraded.apply(&legacy).unwrap();
            assert!(!upgraded.calibrated);
            let driver = PartId::of(&old.parts[1]);
            let mechanism = AssemblyRules::mechanism_for_joint(&upgraded, &driver).unwrap();
            assert!((mechanism.latch - phase).abs() < 1e-6);
            assert_eq!(mechanism.value, if phase == 1. { 18. } else { 0. });
            for r in upgraded
                .relations
                .iter()
                .filter(|r| AssemblyRules::is_drawer_lock(&r.label))
            {
                let previous = old.relations.iter().find(|p| p.child == r.child).unwrap();
                assert!(
                    released_pose(r.joint.clone())
                        .abs_diff_eq(released_pose(previous.joint.clone()), 1e-5)
                );
                match (&r.joint, &previous.joint) {
                    (
                        Joint::Hinge {
                            max,
                            angle,
                            pivot,
                            axis,
                            ..
                        },
                        Joint::Hinge {
                            pivot: p, axis: a, ..
                        },
                    ) => {
                        assert_eq!((*max, *angle, pivot, axis), (90., 90. * phase, p, a));
                    }
                    (
                        Joint::Slider {
                            max,
                            offset,
                            axis,
                            rotation,
                            ..
                        },
                        Joint::Slider {
                            axis: a,
                            rotation: q,
                            ..
                        },
                    ) => {
                        assert_eq!((*max, axis, rotation), (20., a, q));
                        assert!((*offset - 20. * phase).abs() < 1e-5);
                    }
                    _ => unreachable!(),
                }
            }
            upgraded.calibrated = true;
            let saved = upgraded.definition();
            let mut reloaded = template;
            reloaded.apply(&saved).unwrap();
            assert!(reloaded.calibrated);
            assert_eq!(
                serde_json::to_value(&saved.relations).unwrap(),
                serde_json::to_value(&reloaded.relations).unwrap()
            );
        }
    }

    #[test]
    fn custom_ranges_and_current_sessions_are_preserved() {
        let template = drawer_with_locks(90., 20., 0.);
        for (turn, rack, version) in [
            (45., LEGACY_RACK_TRAVEL, 0),
            (60., 12., 0),
            (90., 20., 0),
            (60., LEGACY_RACK_TRAVEL, 1),
        ] {
            let edited = drawer_with_locks(turn, rack, 0.5);
            let mut saved = edited.definition();
            saved.drawer_lock_version = version;
            let mut restored = template.clone();
            restored.apply(&saved).unwrap();
            assert!(restored.calibrated);
            let mut rebuilt = template.clone();
            rebuilt.retain_assembly_state(&edited);
            for scene in [restored, rebuilt] {
                let mut actual = scene.relations.iter().skip(1).map(|r| match r.joint {
                    Joint::Hinge { max, angle, .. } => (max, angle),
                    Joint::Slider { max, offset, .. } => (max, offset),
                    _ => unreachable!(),
                });
                assert_eq!(actual.next(), Some((turn, turn * 0.5)));
                assert!(actual.all(|item| item == (rack, rack * 0.5)));
            }
        }
        // Loading a different file replaces the active session's custom limits.
        let mut active = drawer_with_locks(45., 12., 0.);
        let mut legacy = drawer_with_locks(60., LEGACY_RACK_TRAVEL, 0.5).definition();
        legacy.drawer_lock_version = 0;
        active.apply(&legacy).unwrap();
        assert!(matches!(
            active.relations[1].joint,
            Joint::Hinge {
                angle: 45.,
                max: 90.,
                ..
            }
        ));
    }

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
