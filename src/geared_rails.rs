//! Drawer Assembly steps 5 and 7: one source STL contains two different racks.
//! Fit their rounded stops and profiles to the upper/lower front channels.
use crate::{
    constraints::{self, Joint, PartId, Relation},
    scene::Scene,
};
use glam::{Mat3, Quat, Vec3};
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
        let r = Relation {
            parent: PartId::of(&scene.parts[drawer]),
            child: PartId::of(p),
            joint: Joint::Fixed {
                position: [x, y, -size.y * 0.5 + 3.1],
                rotation: rotation.to_array(),
            },
            label: if lower {
                "Manual step 5: lower geared rack / front channel"
            } else {
                "Manual step 5: upper geared rack / front channel"
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
    #[test]
    fn racks_fit_drawer_channels_and_follow() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
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
            s.apply(&saved);
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
