//! Drawer Assembly steps 5 and 7: one source STL contains two different racks.
//! Fit their rounded stops and profiles to the upper/lower front channels.
use crate::{
    constraints::{self, Joint, PartId, Relation},
    scene::Scene,
};
use glam::{Mat3, Quat, Vec3};

pub(crate) const LOWER_RACK: &str = "Drawer lock: lower geared rack / front channel";
pub(crate) const UPPER_RACK: &str = "Drawer lock: upper geared rack / front channel";
// The pinion teeth repeat every 18 degrees and the rack pitch is 4 mm.
const RACK_TRAVEL: f32 = crate::front_panel::KEY_TURN_DEGREES / 18. * 4.;

pub fn apply(scene: &mut Scene, drawer: usize) -> Result<(), String> {
    let ids: Vec<_> = scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.contains("/Geared Rails/"))
        .map(|(i, _)| i)
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    if ids.len() != 2 || ids.iter().any(|&i| scene.parts[i].source_bodies != 2) {
        return Err("Expected one source file containing two geared racks".into());
    }
    let driven = scene
        .relations
        .iter()
        .any(|r| r.label == crate::front_panel::DRAWER_KEY);
    let size = scene.parts[drawer].source_size;
    for i in ids {
        let p = &scene.parts[i];
        // The upper channel has an extra recess for the tapered backing
        // (6.22 mm profile); the lower channel takes the 4.80 mm rack.
        let lower = p.mesh.size.y < 5.5;
        let left = p.mesh.min.x;
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for v in &p.mesh.vertices {
            if v.position[0] < left + 0.01 {
                lo = lo.min(-v.position[2]);
                hi = hi.max(-v.position[2]);
            }
        }
        let round_y = (lo + hi) * 0.5;
        let depth = -size.y * 0.5 + 3.1;
        let slot_y = -size.z * 0.5 + if lower { 4.2067 } else { 32.7933 };
        let stop = slot_stop(&scene.parts[drawer].mesh, slot_y, depth, lower)
            .ok_or("Geared rack channel stop not found")?;
        let (x, y, rotation) = if lower {
            (
                stop - 0.2 + left,
                -size.z * 0.5 + 4.2067 - round_y,
                Quat::from_mat3(&Mat3::from_cols(-Vec3::X, -Vec3::Z, -Vec3::Y)),
            )
        } else {
            (
                stop + 0.2 - left,
                -size.z * 0.5 + 32.7933 + round_y,
                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            )
        };
        // Local mesh basis is (raw X, raw Z, -raw Y). The slot depth spans
        // raw Y = front -5.3 .. front -0.9; rack thickness is 3.8 mm.
        // The step 5 insertion stop is the unlocked end. At the locked end
        // the two opposite rack tips protrude from opposite sides of the box.
        let retract = if lower { Vec3::X } else { Vec3::NEG_X };
        let unlocked = Vec3::new(x, y, -size.y * 0.5 + 3.1);
        let r = Relation {
            parent: PartId::of(&scene.parts[drawer]),
            child: PartId::of(p),
            joint: if driven {
                Joint::Slider {
                    position: (unlocked - retract * RACK_TRAVEL).to_array(),
                    rotation: rotation.to_array(),
                    axis: retract.to_array(),
                    min: 0.,
                    max: RACK_TRAVEL,
                    offset: 0.,
                }
            } else {
                Joint::Fixed {
                    position: unlocked.to_array(),
                    rotation: rotation.to_array(),
                }
            },
            label: match (driven, lower) {
                (true, true) => LOWER_RACK,
                (true, false) => UPPER_RACK,
                (false, true) => "Manual step 5: lower geared rack / front channel",
                (false, false) => "Manual step 5: upper geared rack / front channel",
            }
            .into(),
        };
        scene.relations.retain(|old| old.child != r.child);
        constraints::add(scene, r)?;
    }
    Ok(())
}
// Intersect the source channel with a line along X, through its rounded
// center. Larger widths retain an inboard stop; using the outer box bounds
// would put the rack into the solid front wall.
fn slot_stop(mesh: &crate::mesh::Mesh, y: f32, z: f32, lower: bool) -> Option<f32> {
    let mut result: Option<f32> = None;
    for tri in mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(tri[0].position);
        let b = Vec3::from(tri[1].position) - a;
        let c = Vec3::from(tri[2].position) - a;
        let det = b.y * c.z - b.z * c.y;
        if det.abs() < 1e-8 {
            continue;
        }
        let u = ((y - a.y) * c.z - (z - a.z) * c.y) / det;
        let v = (b.y * (z - a.z) - b.z * (y - a.y)) / det;
        if u < -0.0001 || v < -0.0001 || u + v > 1.0001 {
            continue;
        }
        let x = a.x + u * b.x + v * c.x;
        if lower && x > 0. {
            result = Some(result.map_or(x, |old| old.min(x)));
        }
        if !lower && x < 0. {
            result = Some(result.map_or(x, |old| old.max(x)));
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;

    // Parts are in enclosure, drawer, key, lower bolt, upper bolt order.
    fn check_lock_contacts(group: &Scene, engaging_shell: bool) {
        for pair in crate::collision::check(group, 0.01).unwrap().pairs {
            match (pair.a, pair.b) {
                // Shoulder contact in the pinion/rack meshes is also present when released.
                (2, 3 | 4) => {}
                (0, moving @ 2..=4) if engaging_shell => {
                    let part = &group.parts[moving];
                    let local = part.pose.matrix(0.).inverse();
                    for hit in crate::collision::audit_pair(group, 0, moving) {
                        let p = local.transform_point3(hit);
                        if moving == 2 {
                            // The outer end of the rear cam enters the shell pocket.
                            let from_back = p.y - part.mesh.min.y;
                            assert!(
                                (-0.01..3.22).contains(&from_back) && p.x.hypot(p.z) > 19.,
                                "Housing crosses key outside rear cam: {p:?}, {from_back}"
                            );
                        } else {
                            // Only the protruding end of the bolt can enter the shell.
                            let from_tip = part.mesh.max.x - p.x;
                            assert!(
                                (-0.01..10.).contains(&from_tip),
                                "Housing crosses rack away from locking tip: {p:?}, {from_tip}"
                            );
                        }
                    }
                }
                _ => panic!(
                    "Unexpected crossing: {} / {} at {:?}",
                    group.parts[pair.a].label(false),
                    group.parts[pair.b].label(false),
                    pair.point
                ),
            }
        }
    }

    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn lock_turn_retracts_opposite_tips_and_carries_the_key_cap() {
        use crate::constraints::AssemblyRules;
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let catalog = crate::catalog::Catalog::scan(root.into()).unwrap();
        for (width, height, hidden_key, racks) in [
            ("S", "1F", false, true),
            ("M", "2F", true, true),
            ("L", "4F", false, true),
            ("S", "1F", true, false),
        ] {
            let selection = crate::catalog::Selection {
                width: width.into(),
                height: height.into(),
                hidden_key,
                ..Default::default()
            };
            let mut items = catalog.bom(&selection);
            if !racks {
                items.retain(|item| !item.path.contains("/Geared Rails/"));
            }
            let mut scene = Scene::load(&catalog.root, &items, "drawer lock".into(), true).unwrap();
            let part = |label: &str| {
                let r = scene.relations.iter().find(|r| r.label == label).unwrap();
                scene
                    .parts
                    .iter()
                    .position(|p| PartId::of(p) == r.child)
                    .unwrap()
            };
            let drawer = part(AssemblyRules::DRAWER_SLIDE);
            let key = part(crate::front_panel::DRAWER_KEY);
            let rack_ids: Vec<_> = [LOWER_RACK, UPPER_RACK]
                .into_iter()
                .filter(|_| racks)
                .map(part)
                .collect();
            let cap = scene
                .parts
                .iter()
                .position(|p| p.pose.path.ends_with("/Key Cap.stl"))
                .unwrap();
            let facade = scene
                .parts
                .iter()
                .find(|p| p.pose.path.contains("/Facades/"))
                .unwrap();
            let facade_inv = facade.pose.matrix(0.).inverse();
            let facade_part = facade.clone();
            let drawer_inv = scene.parts[drawer].pose.matrix(0.).inverse();
            let locked_centers: Vec<_> = rack_ids
                .iter()
                .map(|&i| {
                    (drawer_inv * scene.parts[i].pose.matrix(0.))
                        .w_axis
                        .truncate()
                })
                .collect();
            let driver = PartId::of(&scene.parts[drawer]);
            let locked_pivot = scene.parts[key].pose.matrix(0.).w_axis;
            let cap_seat =
                scene.parts[key].pose.matrix(0.).inverse() * scene.parts[cap].pose.matrix(0.);
            for released in [true, false] {
                let plan = AssemblyRules::plan_latch(&scene, &driver, released).unwrap();
                for t in [0., 0.25, 0.5, 1.] {
                    let sample = plan.sample(plan.duration * t);
                    AssemblyRules::apply_motion_sample(&mut scene, &driver, sample).unwrap();
                    let angle = ((1. - sample.latch) * 90.).to_radians();
                    let key_pose = scene.parts[key].pose.matrix(0.);
                    let expected_axis = Vec3::new(angle.cos(), 0., -angle.sin());
                    for i in [key, cap] {
                        let long_axis = (facade_inv * scene.parts[i].pose.matrix(0.))
                            .transform_vector3(Vec3::X);
                        assert!(long_axis.abs_diff_eq(expected_axis, 0.002));
                    }
                    assert!(key_pose.w_axis.abs_diff_eq(locked_pivot, 0.002));
                    assert!(
                        (key_pose.inverse() * scene.parts[cap].pose.matrix(0.))
                            .abs_diff_eq(cap_seat, 0.002)
                    );
                    if released {
                        let module = scene
                            .parts
                            .iter()
                            .find(|p| p.pose.path.contains("/Modules/"))
                            .unwrap();
                        let mut parts = vec![
                            module.clone(),
                            scene.parts[drawer].clone(),
                            scene.parts[key].clone(),
                        ];
                        parts.extend(rack_ids.iter().map(|&r| scene.parts[r].clone()));
                        let group = Scene {
                            parts,
                            ..Default::default()
                        };
                        check_lock_contacts(&group, angle >= 60_f32.to_radians());
                        // The close-running shoulder can touch the tessellated bore. Any
                        // reported crossing must stay on that shoulder throughout the turn.
                        let bore = Scene {
                            parts: vec![facade_part.clone(), scene.parts[key].clone()],
                            ..Default::default()
                        };
                        let inverse = key_pose.inverse();
                        for point in crate::collision::audit_pair(&bore, 0, 1) {
                            let p = inverse.transform_point3(point);
                            assert!(
                                (19.0..20.8).contains(&p.x.hypot(p.z)),
                                "Key crossed facade away from bore shoulder: {width}/{height} {p:?}"
                            );
                        }
                    }
                    for (i, &rack) in rack_ids.iter().enumerate() {
                        // Measure the outermost mesh tip against its corresponding box wall.
                        let outward = if i == 0 { -1. } else { 1. };
                        let pose = drawer_inv * scene.parts[rack].pose.matrix(0.);
                        let travel = pose.w_axis.truncate() - locked_centers[i];
                        assert!(
                            travel.abs_diff_eq(Vec3::X * (-outward * 20. * sample.latch), 0.002)
                        );
                        let tip = scene.parts[rack]
                            .mesh
                            .vertices
                            .iter()
                            .map(|v| outward * pose.transform_point3(v.position.into()).x)
                            .fold(f32::NEG_INFINITY, f32::max);
                        let extension = tip - scene.parts[drawer].source_size.x * 0.5;
                        // The source bolts retract about 12 mm inside their respective walls.
                        assert!(
                            (extension - (-12. + 20. * (1. - sample.latch))).abs() < 1.5,
                            "{width}/{height} rack {i}: {extension} at {}",
                            sample.latch
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn racks_fit_drawer_channels_and_follow() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        for (width, height, depth) in [
            ("S", "1F", 3),
            ("S", "2F", 3),
            ("M", "1F", 3),
            ("L", "1F", 3),
            ("S", "1F", 4),
        ] {
            let sel = crate::catalog::Selection {
                width: width.into(),
                height: height.into(),
                depth,
                ..Default::default()
            };
            let mut s = Scene::load(&c.root, &c.bom(&sel), "geared racks".into(), true).unwrap();
            let d = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Boxes/"))
                .unwrap();
            let rails: Vec<_> = s
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.pose.path.contains("/Geared Rails/"))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(rails.len(), 2);
            for &r in &rails {
                assert_eq!(s.parts[r].pose.instance, 0);
                assert_eq!(s.parts[r].source_bodies, 2);
                let pair = Scene {
                    parts: vec![s.parts[d].clone(), s.parts[r].clone()],
                    ..Default::default()
                };
                assert!(
                    crate::collision::check(&pair, 0.01)
                        .unwrap()
                        .pairs
                        .is_empty(),
                    "rack/channel crossing {width}{depth}/{height}"
                );
                let child = PartId::of(&s.parts[r]);
                assert_eq!(
                    s.relations
                        .iter()
                        .find(|x| x.child == child)
                        .unwrap()
                        .parent,
                    PartId::of(&s.parts[d])
                );
            }
            // Loading an older calibrated assembly must retain new rack mates.
            let mut legacy = s.definition();
            legacy.poses.retain(|p| !p.path.contains("/Geared Rails/"));
            legacy
                .relations
                .retain(|r| !r.child.path.contains("/Geared Rails/"));
            let saved: crate::scene::Definition =
                toml::from_str(&toml::to_string(&legacy).unwrap()).unwrap();
            s.apply(&saved).unwrap();
            assert!(s.constraint_warning.is_none());
            for &r in &rails {
                assert!(
                    s.relations
                        .iter()
                        .any(|relation| relation.child == PartId::of(&s.parts[r]))
                );
            }
            let before: Vec<_> = rails
                .iter()
                .map(|&i| s.parts[d].pose.matrix(0.).inverse() * s.parts[i].pose.matrix(0.))
                .collect();
            let id = PartId::of(&s.parts[d]);
            if let Joint::Slider { offset, .. } = &mut s
                .relations
                .iter_mut()
                .find(|r| r.child == id)
                .unwrap()
                .joint
            {
                *offset = 25.;
            }
            constraints::solve(&mut s).unwrap();
            for (&i, expected) in rails.iter().zip(before) {
                assert!(
                    (s.parts[d].pose.matrix(0.).inverse() * s.parts[i].pose.matrix(0.))
                        .abs_diff_eq(expected, 0.002)
                );
            }
        }
    }
}
