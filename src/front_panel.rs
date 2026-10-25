//! Drawer Assembly steps 1–3 and 8. Datums measured from the supplied STL
//! planar seats; source geometry remains immutable. This is not a full CAD solver.
use crate::{
    constraints::{self, Joint, PartId, Relation},
    scene::Scene,
};
use glam::{Quat, Vec3};

fn attach(
    scene: &mut Scene,
    parent: usize,
    child: usize,
    raw: Vec3,
    flip: bool,
    label: &str,
) -> Result<(), String> {
    // Mesh loader maps STL (x,y,z) to (x,z,-y).
    let relation = Relation {
        parent: PartId::of(&scene.parts[parent]),
        child: PartId::of(&scene.parts[child]),
        joint: Joint::Fixed {
            position: [raw.x, raw.z, -raw.y],
            rotation: if flip {
                Quat::from_rotation_x(std::f32::consts::PI)
            } else {
                Quat::IDENTITY
            }
            .to_array(),
        },
        label: label.into(),
    };
    scene.relations.retain(|r| r.child != relation.child);
    constraints::add(scene, relation)
}

/// Connected, circular 10 mm counterbore floors, excluding the recess hidden by the label.
fn cap_seats(mesh: &crate::mesh::Mesh, front: f32, label_lo: Vec3, label_hi: Vec3) -> Vec<Vec3> {
    use std::collections::HashSet;
    type Patch = (HashSet<(i32, i32)>, Vec3, Vec3);
    let mut patches: Vec<Patch> = Vec::new();
    for tri in mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(tri[0].position);
        let b = Vec3::from(tri[1].position);
        let c = Vec3::from(tri[2].position);
        let n = (b - a).cross(c - a);
        let center = (a + b + c) / 3.;
        if n.y.abs() <= n.length() * 0.999 || (center.y - (front - 4.1)).abs() > 0.015 {
            continue;
        }
        let mut keys: HashSet<_> = [a, b, c]
            .iter()
            .map(|v| ((v.x * 100.).round() as i32, (v.z * 100.).round() as i32))
            .collect();
        let mut lo = a.min(b).min(c);
        let mut hi = a.max(b).max(c);
        // Merge all connected patches, including triangles bridging earlier patches.
        let mut i = 0;
        while i < patches.len() {
            if !patches[i].0.is_disjoint(&keys) {
                let (other, min, max) = patches.swap_remove(i);
                keys.extend(other);
                lo = lo.min(min);
                hi = hi.max(max);
                i = 0;
            } else {
                i += 1;
            }
        }
        patches.push((keys, lo, hi));
    }
    let mut seats: Vec<_> = patches
        .into_iter()
        .filter_map(|(_, lo, hi)| {
            let center = (lo + hi) * 0.5;
            let covered = center.x >= label_lo.x
                && center.x <= label_hi.x
                && center.z >= label_lo.z
                && center.z <= label_hi.z;
            if (hi.x - lo.x - 10.).abs() < 0.08 && (hi.z - lo.z - 10.).abs() < 0.08 && !covered {
                Some(center)
            } else {
                None
            }
        })
        .collect();
    seats.sort_by_key(|p| ((p.x * 10.).round() as i32, (p.z * 10.).round() as i32));
    seats
}

pub fn apply(scene: &mut Scene, facade: usize) -> Result<(), String> {
    let part = &scene.parts[facade];
    // Supported standard, unsplit facade geometry: 10 mm backing-to-front,
    // label track 2.09 mm below the front. Ignore support protrusions at max Z.
    let front = part.mesh.min.y + 10.;
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let mut area = 0.;
    for tri in part.mesh.vertices.chunks_exact(3) {
        let p: Vec<_> = tri.iter().map(|v| Vec3::from(v.position)).collect();
        let cross = (p[1] - p[0]).cross(p[2] - p[0]);
        let center = (p[0] + p[1] + p[2]) / 3.;
        if cross.y.abs() > cross.length() * 0.999
            && center.x > 25.
            && (center.y - (front - 2.09)).abs() < 0.04
        {
            area += cross.length() * 0.5;
            for v in p {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    if !(700. ..900.).contains(&area)
        || (hi.x - lo.x - 47.4).abs() > 0.5
        || (hi.z - lo.z - 19.62).abs() > 0.2
    {
        // Leave unsupported variants explicitly estimated instead of applying a false fit.
        scene.constraint_warning =
            Some("Facade slot not recognized; front attachments remain estimated".into());
        return Ok(());
    }
    let seats = cap_seats(&part.mesh, front, lo, hi);
    let row_y = -(lo.z + hi.z) * 0.5;
    let find = |name: &str| {
        scene
            .parts
            .iter()
            .position(|p| p.pose.path.rsplit('/').next() == Some(name))
    };
    let key = find("Key (Turn On Supports).stl");
    let hidden = find("Hidden Key (Turn On Supports).stl");
    let hidden_head = find("Hidden Key Part 2 (Turn on Supports).stl");
    let cap = find("Key Cap.stl");
    let label = find("Label.stl");
    let plugs: Vec<_> = scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.ends_with("/Facade Cap.stl"))
        .map(|(i, _)| i)
        .collect();
    let width = part.source_size.x;
    if let Some(key) = key {
        // The Key collar at raw Z +1.692 seats flush with facade front;
        // A 0.1 mm axial running clearance avoids coincident tessellated seats.
        // Its rear flange at -5.908 seats on the -7.6 mm counterbore.
        attach(
            scene,
            facade,
            key,
            Vec3::new(0., row_y, front - 1.692 - 0.1),
            false,
            "Manual step 1: Key / facade bore",
        )?;
        if let Some(cap) = cap {
            // Socket opens toward raw +Z in the source cap. Turn it over,
            // keeping X (and the small end hole) aligned with the Key head.
            let key_tip = scene.parts[key].mesh.max.y;
            let cap_floor = scene.parts[cap].mesh.min.y + 2.;
            attach(
                scene,
                key,
                cap,
                Vec3::new(0., 0., key_tip + cap_floor),
                true,
                "Manual step 2: cap / Key head socket",
            )?;
        }
    }
    if let Some(hidden) = hidden {
        // Same rear gear/flange as the ordinary key, but the body ends at its
        // front collar. Its bbox center differs by 7.508 mm from the full key.
        let collar = scene.parts[hidden].mesh.max.y;
        attach(
            scene,
            facade,
            hidden,
            Vec3::new(0., row_y, front - collar - 0.1),
            false,
            "Hidden key: lock body / facade bore",
        )?;
        if let Some(head) = hidden_head {
            // The socket floor seats at the lock collar, not on the lower
            // central bar (0.8 mm below it). The slot is offset from the mesh
            // bbox center: +0.1162 X / -0.002 raw Y aligns its center to the bar.
            // Keep 0.05 mm axial display clearance; retain contact diagnostics.
            let socket_floor = scene.parts[head].mesh.min.y + 7.;
            attach(
                scene,
                hidden,
                head,
                Vec3::new(0.1162, -0.002, collar - socket_floor + 0.05),
                false,
                "Hidden key: removable head / keyed collar seat",
            )?;
            if let Some(cap) = cap {
                let tip = scene.parts[head].mesh.max.y;
                let floor = scene.parts[cap].mesh.min.y + 2.;
                attach(
                    scene,
                    head,
                    cap,
                    Vec3::new(0., 0., tip + floor),
                    true,
                    "Hidden key: cap / removable head socket",
                )?;
            }
        }
    }
    if let Some(label) = label {
        let half = scene.parts[label].source_size.z * 0.5;
        // Rounded end inboard; straight edge at track entry. Measured track
        // profile is 26.25 mm inboard from the facade's outer edge.
        attach(
            scene,
            facade,
            label,
            Vec3::new(width * 0.5 - 26.25, row_y, (lo.y + hi.y) * 0.5 + half),
            false,
            "Manual step 8: label / side-entry track",
        )?;
    }
    if !plugs.is_empty() && plugs.len() != seats.len() {
        scene.constraint_warning = Some(format!(
            "Facade caps: {} copies for {} exposed recesses",
            plugs.len(),
            seats.len()
        ));
    }
    for (plug, seat) in plugs.into_iter().zip(seats) {
        let top = scene.parts[plug].mesh.max.y;
        attach(
            scene,
            facade,
            plug,
            Vec3::new(seat.x, -seat.z, front - top),
            false,
            "Manual step 8: facade cap / screw recess",
        )?;
    }
    // Step 3: the supplied file contains two identical springs, not a mirrored pair.
    // Rotate the left body 180 degrees about the facade normal. The raw -Z flat
    // side faces the drawer; the stepped +Z face seats in the rear pocket.
    let springs: Vec<_> = scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.pose.path.ends_with("/Springs.stl") && p.pose.instance == 0 && p.pose.body.is_some()
        })
        .map(|(i, _)| i)
        .collect();
    if springs.len() == 2 {
        for (side, i) in [(-1., springs[0]), (1., springs[1])] {
            let spring = &scene.parts[i];
            // Boss radius ~6.80 / notch radius ~6.79: preserve the source
            // contact geometry; do not claim clearance from these display datums.
            // Rear pocket seat has 0.05 mm axial display clearance.
            let position = [side * 38.38, front - 4.30 - spring.mesh.max.y, -row_y];
            let rotation = if side < 0. {
                Quat::from_rotation_y(std::f32::consts::PI)
            } else {
                Quat::IDENTITY
            };
            let r = Relation {
                parent: PartId::of(&scene.parts[facade]),
                child: PartId::of(spring),
                joint: Joint::Fixed {
                    position,
                    rotation: rotation.to_array(),
                },
                label: "Manual step 3: spring / rear pocket and boss".into(),
            };
            scene.relations.retain(|old| old.child != r.child);
            constraints::add(scene, r)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_standard_facade_caps_have_distinct_seats_and_follow_panel() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let catalog = crate::catalog::Catalog::scan(root.into()).unwrap();
        for width in ["S", "M", "L"] {
            for height in ["1F", "2F", "4F"] {
                let selection = crate::catalog::Selection {
                    width: width.into(),
                    height: height.into(),
                    ..Default::default()
                };
                let mut scene = Scene::load(
                    &catalog.root,
                    &catalog.bom(&selection),
                    "cap seats".into(),
                    true,
                )
                .unwrap();
                let facade = scene
                    .parts
                    .iter()
                    .position(|p| p.pose.path.contains("/Facades/"))
                    .unwrap();
                let parent = PartId::of(&scene.parts[facade]);
                let caps: Vec<_> = scene
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.pose.path.ends_with("/Facade Cap.stl"))
                    .map(|(i, _)| i)
                    .collect();
                assert_eq!(
                    caps.len(),
                    crate::catalog::facade_cap_count(&selection) as usize
                );
                let mut relative = Vec::new();
                for &i in &caps {
                    let relation = scene
                        .relations
                        .iter()
                        .find(|r| r.child == PartId::of(&scene.parts[i]))
                        .unwrap();
                    assert_eq!(relation.parent, parent);
                    assert!(relation.label.starts_with("Manual step 8: facade cap"));
                    let transform = scene.parts[facade].pose.matrix(0.).inverse()
                        * scene.parts[i].pose.matrix(0.);
                    let front =
                        transform.transform_point3(Vec3::new(0., scene.parts[i].mesh.max.y, 0.));
                    assert!((front.y - scene.parts[facade].mesh.min.y - 10.).abs() < 0.003);
                    assert!(relative.iter().all(|m: &glam::Mat4| {
                        m.w_axis.truncate().distance(transform.w_axis.truncate()) > 9.
                    }));
                    relative.push(transform);
                }
                constraints::rotate_subassembly(&mut scene, facade, Quat::from_rotation_z(0.27))
                    .unwrap();
                for (&i, expected) in caps.iter().zip(relative) {
                    let actual = scene.parts[facade].pose.matrix(0.).inverse()
                        * scene.parts[i].pose.matrix(0.);
                    assert!(actual.abs_diff_eq(expected, 0.003));
                }
            }
        }
    }
    #[test]
    fn hidden_key_seating_and_switching() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        for (width, height) in [("S", "1F"), ("S", "2F"), ("M", "1F"), ("L", "1F")] {
            let ordinary = crate::catalog::Selection {
                width: width.into(),
                height: height.into(),
                ..Default::default()
            };
            let hidden = crate::catalog::Selection {
                hidden_key: true,
                ..ordinary.clone()
            };
            let old = Scene::load(&c.root, &c.bom(&ordinary), "ordinary".into(), true).unwrap();
            let mut s = Scene::load(&c.root, &c.bom(&hidden), "hidden".into(), true).unwrap();
            let id = |s: &Scene, name: &str| {
                s.parts
                    .iter()
                    .position(|p| p.pose.path.ends_with(name))
                    .unwrap()
            };
            let body = id(&s, "/Hidden Key (Turn On Supports).stl");
            let head = id(&s, "/Hidden Key Part 2 (Turn on Supports).stl");
            let cap = id(&s, "/Key Cap.stl");
            let facade = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Facades/"))
                .unwrap();
            let intended = s.parts[cap].pose.matrix(0.);
            s.retain_assembly_state(&old);
            assert!(s.constraint_warning.is_none());
            assert!(s.parts[cap].pose.matrix(0.).abs_diff_eq(intended, 0.002));
            for (child, parent) in [(body, facade), (head, body), (cap, head)] {
                assert_eq!(
                    s.relations
                        .iter()
                        .find(|r| r.child == PartId::of(&s.parts[child]))
                        .unwrap()
                        .parent,
                    PartId::of(&s.parts[parent])
                );
            }
            let pair = Scene {
                parts: vec![
                    s.parts[facade].clone(),
                    s.parts[body].clone(),
                    s.parts[head].clone(),
                    s.parts[cap].clone(),
                ],
                ..Default::default()
            };
            let report = crate::collision::check(&pair, 0.01).unwrap();
            assert!(
                !report.pairs.iter().any(|p| p.a == 0 || p.b == 0),
                "hidden parts must clear facade"
            );
            let contacts = crate::collision::audit_pair(&pair, 1, 2);
            println!(
                "{width}{height}: hidden seat contact points {}",
                contacts.len()
            );
            // Residual source-mesh contacts are confined to the keyed rim;
            // do not call them a validated press fit or hide them in diagnostics.
            let inv = pair.parts[1].pose.matrix(0.).inverse();
            assert!(contacts.len() < 50);
            for hit in &contacts {
                let p = inv.transform_point3(*hit);
                assert!(p.y > 7.8 && p.y < 8.8 && p.x.abs() < 15. && p.z.abs() < 13.5);
            }
            let mut from_saved =
                Scene::load(&c.root, &c.bom(&hidden), "saved switch".into(), true).unwrap();
            from_saved.apply(&old.definition());
            assert!(from_saved.constraint_warning.is_none());
            assert!(
                from_saved.parts[cap]
                    .pose
                    .matrix(0.)
                    .abs_diff_eq(intended, 0.002)
            );
            let forward = s.parts[body].pose.matrix(0.).transform_vector3(Vec3::Y);
            let travel = |i: usize| {
                s.parts[i].pose.matrix(1.).w_axis.truncate()
                    - s.parts[i].pose.matrix(0.).w_axis.truncate()
            };
            assert!((travel(head) - travel(body)).dot(forward) > 69.);
            assert!((travel(cap) - travel(head)).dot(forward) > 109.);
            let relative = s.parts[body].pose.matrix(0.).inverse() * s.parts[head].pose.matrix(0.);
            constraints::rotate_subassembly(&mut s, facade, Quat::from_rotation_z(0.35)).unwrap();
            assert!(
                (s.parts[body].pose.matrix(0.).inverse() * s.parts[head].pose.matrix(0.))
                    .abs_diff_eq(relative, 0.002)
            );
            let def = s.definition();
            let mut restored = s.clone();
            restored.apply(&def);
            assert!(
                restored.parts[cap]
                    .pose
                    .matrix(1.)
                    .abs_diff_eq(s.parts[cap].pose.matrix(1.), 0.002)
            );
            let mut back = old.clone();
            back.retain_assembly_state(&s);
            assert!(back.constraint_warning.is_none());
            let cap = id(&back, "/Key Cap.stl");
            let key = id(&back, "/Key (Turn On Supports).stl");
            assert_eq!(
                back.relations
                    .iter()
                    .find(|r| r.child == PartId::of(&back.parts[cap]))
                    .unwrap()
                    .parent,
                PartId::of(&back.parts[key])
            );
        }
    }
    #[test]
    fn whole_facade_rotation_preserves_descendants_and_saved_assembly() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        for (width, height) in [("S", "1F"), ("S", "2F"), ("M", "1F"), ("L", "1F")] {
            let selection = crate::catalog::Selection {
                width: width.into(),
                height: height.into(),
                ..Default::default()
            };
            let mut s = Scene::load(
                &c.root,
                &c.bom(&selection),
                "rotation regression".into(),
                true,
            )
            .unwrap();
            let f = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Facades/"))
                .unwrap();
            let label = s
                .parts
                .iter()
                .position(|p| p.pose.path.ends_with("/Label.stl"))
                .unwrap();
            let plug = s
                .parts
                .iter()
                .position(|p| p.pose.path.ends_with("/Facade Cap.stl"))
                .unwrap();
            assert!(s.parts[label].pose.position[0] < s.parts[f].pose.position[0]);
            assert!(s.parts[plug].pose.position[0] > s.parts[f].pose.position[0]);
            assert!(
                s.parts[f]
                    .pose
                    .matrix(0.)
                    .transform_vector3(Vec3::Y)
                    .dot(Vec3::Z)
                    > 0.999
            );
            let ids: Vec<_> = s
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.pose.path.contains("/Facades/")
                        || [
                            "/Springs.stl",
                            "/Key (Turn On Supports).stl",
                            "/Key Cap.stl",
                            "/Label.stl",
                            "/Facade Cap.stl",
                        ]
                        .iter()
                        .any(|n| p.pose.path.ends_with(n))
                })
                .map(|(i, _)| i)
                .collect();
            let drawer_index = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Boxes/"))
                .unwrap();
            let mut parts = vec![s.parts[drawer_index].clone()];
            parts.extend(ids.iter().map(|&i| s.parts[i].clone()));
            let external = crate::collision::check(
                &Scene {
                    parts,
                    ..Default::default()
                },
                0.01,
            )
            .unwrap();
            assert!(
                !external.pairs.iter().any(|p| p.a == 0 || p.b == 0),
                "Rotated front must still fit drawer: {height}"
            );
            let before = s.clone();
            let pivot = Vec3::from(s.parts[f].pose.position);
            let rotation = Quat::from_rotation_z(std::f32::consts::PI);
            constraints::rotate_subassembly(&mut s, f, rotation).unwrap();
            for &i in &ids {
                let local_before =
                    before.parts[f].pose.matrix(0.).inverse() * before.parts[i].pose.matrix(0.);
                let local_after = s.parts[f].pose.matrix(0.).inverse() * s.parts[i].pose.matrix(0.);
                assert!(local_before.abs_diff_eq(local_after, 0.002));
                for e in [0., 0.5, 1.] {
                    let expected = pivot
                        + rotation
                            * (before.parts[i].pose.matrix(e).transform_point3(Vec3::ZERO) - pivot);
                    assert!(
                        (s.parts[i].pose.matrix(e).transform_point3(Vec3::ZERO) - expected)
                            .length()
                            < 0.002
                    );
                }
            }
            constraints::rotate_subassembly(&mut s, f, rotation).unwrap();
            let def: crate::scene::Definition =
                toml::from_str(&toml::to_string(&s.definition()).unwrap()).unwrap();
            let mut restored =
                Scene::load(&c.root, &c.bom(&selection), "restored".into(), true).unwrap();
            restored.apply(&def);
            for &i in &ids {
                assert!(
                    restored.parts[i]
                        .pose
                        .matrix(0.)
                        .abs_diff_eq(before.parts[i].pose.matrix(0.), 0.002)
                );
            }
            let drawer = restored
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Boxes/"))
                .unwrap();
            let id = PartId::of(&restored.parts[drawer]);
            if let Joint::Slider { offset, .. } = &mut restored
                .relations
                .iter_mut()
                .find(|r| r.child == id)
                .unwrap()
                .joint
            {
                *offset = 25.;
            }
            constraints::solve(&mut restored).unwrap();
            for &i in &ids {
                assert!(
                    (restored.parts[i].pose.position[2] - s.parts[i].pose.position[2] - 25.).abs()
                        < 0.002
                );
            }
        }
    }
    #[test]
    fn manual_front_assembly_fits_and_cap_follows_key() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        for (width, height) in [("S", "1F"), ("S", "2F"), ("M", "1F"), ("L", "1F")] {
            let selection = crate::catalog::Selection {
                width: width.into(),
                height: height.into(),
                ..Default::default()
            };
            let mut s =
                Scene::load(&c.root, &c.bom(&selection), "front regression".into(), true).unwrap();
            let fi = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Facades/"))
                .unwrap();
            let facade_matrix = s.parts[fi].pose.matrix(0.);
            let springs: Vec<_> = s
                .parts
                .iter()
                .filter(|p| p.pose.path.ends_with("/Springs.stl"))
                .collect();
            assert_eq!(springs.len(), 2);
            for (i, p) in springs.iter().enumerate() {
                let relative = facade_matrix.inverse() * p.pose.matrix(0.);
                let side = if i == 0 { -1. } else { 1. };
                // The notch center mates with the +/-51.48 mm boss; both flat
                // sides face rearward, and opposite free ends face the Key.
                let notch = relative.transform_point3(Vec3::new(13.10, 0., 0.));
                assert!((notch.x - side * 51.48).abs() < 0.02);
                assert!(relative.transform_vector3(-Vec3::Y).dot(-Vec3::Y) > 0.999);
                assert!(relative.transform_vector3(-Vec3::X).x * side < -0.999);
                let seated = relative.transform_point3(Vec3::new(0., p.mesh.max.y, 0.));
                assert!((seated.y - (s.parts[fi].mesh.min.y + 5.70)).abs() < 0.002);
            }
            let index = |name: &str| {
                s.parts
                    .iter()
                    .position(|p| p.pose.path.ends_with(name))
                    .unwrap()
            };
            let key = index("/Key (Turn On Supports).stl");
            let cap = index("/Key Cap.stl");
            let label = index("/Label.stl");
            let plug = index("/Facade Cap.stl");
            let facade = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Facades/"))
                .unwrap();
            assert!(
                s.relations
                    .iter()
                    .any(|r| r.parent == PartId::of(&s.parts[key])
                        && r.child == PartId::of(&s.parts[cap]))
            );
            let subset = Scene {
                parts: [facade, key, cap, label, plug]
                    .iter()
                    .map(|i| s.parts[*i].clone())
                    .collect(),
                ..Default::default()
            };
            let report = crate::collision::check(&subset, 0.01).unwrap();
            for pair in &report.pairs {
                println!(
                    "key-local {:?}",
                    s.parts[key]
                        .pose
                        .matrix(0.)
                        .inverse()
                        .transform_point3(pair.point)
                );
                println!(
                    "{width}{height}: {} / {}",
                    subset.parts[pair.a].label(false),
                    subset.parts[pair.b].label(false)
                );
            }
            assert!(
                report
                    .pairs
                    .iter()
                    .all(|p| (p.a == 1 && p.b == 2) || (p.a == 2 && p.b == 1)),
                "only the cap / Key press-fit contact may be reported: {width}{height}"
            );
            let before = s.parts[key].pose.matrix(0.).inverse() * s.parts[cap].pose.matrix(0.);
            let key_id = PartId::of(&s.parts[key]);
            s.relations.retain(|r| r.child != key_id);
            s.parts[key].pose.position[0] += 8.;
            s.parts[key].pose.rotation[1] += 35.;
            constraints::solve(&mut s).unwrap();
            let after = s.parts[key].pose.matrix(0.).inverse() * s.parts[cap].pose.matrix(0.);
            assert!(before.abs_diff_eq(after, 0.001));
            let encoded = toml::to_string(&s.definition()).unwrap();
            let saved: crate::scene::Definition = toml::from_str(&encoded).unwrap();
            let mut restored = s.clone();
            restored.apply(&saved);
            assert!(
                restored.parts[cap]
                    .pose
                    .matrix(0.)
                    .abs_diff_eq(s.parts[cap].pose.matrix(0.), 0.001)
            );
        }
    }
}
