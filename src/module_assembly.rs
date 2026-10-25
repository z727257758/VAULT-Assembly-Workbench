//! Drawer Assembly steps 10 and 12: consistent housing orientation, drawer
//! closing datum, and the latch seat on the underside. Source meshes are immutable.
use crate::{
    constraints::{self, Relation},
    scene::Scene,
};
use glam::{Quat, Vec3};

pub fn prepare(
    scene: &mut Scene,
    module: usize,
    drawer: usize,
    facade: usize,
) -> Result<(), String> {
    // Raw module -Y carries the S3 text / upper connectors; +Z is the opening.
    let correction = source_rotation(&scene.parts[module]);
    let base = Quat::from_euler(
        glam::EulerRot::XYZ,
        -std::f32::consts::FRAC_PI_2,
        0.,
        std::f32::consts::PI,
    );
    let (x, y, z) = (base * correction).to_euler(glam::EulerRot::XYZ);
    scene.parts[module].pose.rotation = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
    // Raw box +Y is the facade mounting face; raw +Z is the open top.
    scene.parts[drawer].pose.rotation = [0., 180., 0.];
    let m = &scene.parts[module];
    let floor = -dominant_positive_raw_y(m).ok_or("Module floor datum missing")?;
    let back = -m.source_size.z * 0.5 + 2.;
    let box_half = scene.parts[drawer].source_size;
    // Side-runner alignment sets the floor gap to 2.0 mm; rear gap is 0.7 mm.
    scene.parts[drawer].pose.position = [
        0.,
        floor + 2.0 + box_half.z * 0.5,
        back + 0.7 + box_half.y * 0.5,
    ];
    scene.parts[facade].pose.position[1] = floor + (box_half.z + 3.5) * 0.5;
    Ok(())
}

// Some large modules are supplied rotated 45 degrees in the STL XY plane.
// Identify the broad housing planes, then undo that print-layout rotation.
fn source_rotation(part: &crate::scene::Part) -> Quat {
    use std::collections::BTreeMap;
    let mut weights = BTreeMap::<i32, f32>::new();
    for tri in part.mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(tri[0].position);
        let b = Vec3::from(tri[1].position);
        let c = Vec3::from(tri[2].position);
        let n = (b - a).cross(c - a);
        let len = n.length();
        if len > 1e-6 && n.y.abs() < len * 0.001 {
            let angle = (-n.z).atan2(n.x).to_degrees().rem_euclid(180.);
            *weights.entry((angle * 10.).round() as i32).or_default() += len;
        }
    }
    let angle = weights
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(a, _)| a as f32 / 10.)
        .unwrap_or(90.);
    Quat::from_rotation_y((90. - angle).to_radians())
}

fn dominant_positive_raw_y(part: &crate::scene::Part) -> Option<f32> {
    use std::collections::BTreeMap;
    let mut areas = BTreeMap::<i32, f32>::new();
    let q = source_rotation(part);
    for tri in part.mesh.vertices.chunks_exact(3) {
        let a = q * Vec3::from(tri[0].position);
        let b = q * Vec3::from(tri[1].position);
        let c = q * Vec3::from(tri[2].position);
        let n = (b - a).cross(c - a);
        let raw_y = -(a.z + b.z + c.z) / 3.;
        if raw_y > 0. && n.z.abs() > n.length() * 0.999 {
            *areas.entry((raw_y * 100.).round() as i32).or_default() += n.length() * 0.5;
        }
    }
    areas
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(z, _)| z as f32 / 100.)
}

pub fn latch(scene: &mut Scene, module: usize) -> Result<(), String> {
    let indices: Vec<_> = scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.ends_with("/Latch.stl"))
        .map(|(i, _)| i)
        .collect();
    if indices.is_empty() {
        return Ok(());
    }
    let floor =
        dominant_positive_raw_y(&scene.parts[module]).ok_or("Module floor datum missing")?;
    let paired = indices.len() == 2;
    for (slot, index) in indices.into_iter().enumerate() {
        let half = scene.parts[index].source_size.z * 0.5;
        scene.parts[index].pose.rotation = [180., 0., 0.];
        // Step 12: the rounded heads seat at the INNER pocket end, 63.48 mm
        // from the rear, not the outer frame end (65.74 mm). The old datum
        // pushed both heads 2.26 mm into the frame. L modules have two seats
        // on the 164.8 mm module pitch; S/M have one centered seat.
        let head_from_center = scene.parts[index].source_size.y * 0.5;
        scene.parts[index].pose.position = [
            if paired {
                if slot == 0 { -82.4 } else { 82.4 }
            } else {
                0.
            },
            -floor - 3. - half - 0.05,
            -scene.parts[module].source_size.z * 0.5 + 63.48 - head_from_center,
        ];
        let mut r = Relation::fixed(scene, module, index, false)?;
        r.label = "Manual step 12: latch heads / inner housing pocket".into();
        scene.relations.retain(|old| old.child != r.child);
        constraints::add(scene, r)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drawer_orientation_clearance_travel_and_latch_follow() {
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
            ("L", "2F", 4),
        ] {
            let selection = crate::catalog::Selection {
                width: width.into(),
                height: height.into(),
                depth,
                ..Default::default()
            };
            let mut s = Scene::load(
                &c.root,
                &c.bom(&selection),
                "housing regression".into(),
                true,
            )
            .unwrap();
            let m = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Modules/"))
                .unwrap();
            let b = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Boxes/"))
                .unwrap();
            let l = s
                .parts
                .iter()
                .position(|p| p.pose.path.ends_with("/Latch.stl"))
                .unwrap();
            // Source module -Y -> up; source box +Z -> up, +Y -> front.
            assert!(
                s.parts[m]
                    .pose
                    .matrix(0.)
                    .transform_vector3(source_rotation(&s.parts[m]).inverse() * Vec3::Z)
                    .dot(Vec3::Y)
                    > 0.999
            );
            assert!(
                s.parts[b]
                    .pose
                    .matrix(0.)
                    .transform_vector3(Vec3::Y)
                    .dot(Vec3::Y)
                    > 0.999
            );
            assert!(
                s.parts[b]
                    .pose
                    .matrix(0.)
                    .transform_vector3(-Vec3::Z)
                    .dot(Vec3::Z)
                    > 0.999
            );
            let (lo, _) = constraints::world_bounds(&s.parts[b]);
            let floor = -dominant_positive_raw_y(&s.parts[m]).unwrap();
            assert!((lo.y - floor - 2.0).abs() < 0.01);
            assert!((lo.z - (-s.parts[m].source_size.z * 0.5 + 2.) - 0.7).abs() < 0.01);
            let latch_indices: Vec<_> = s
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.pose.path.ends_with("/Latch.stl"))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(latch_indices.len(), if width == "L" { 2 } else { 1 });
            // Measure both rear-facing inner seat surfaces from source triangles,
            // independently of the placement formula. This rejects the old outer
            // frame datum and the old single, centered latch on L modules.
            let mm = s.parts[m].pose.matrix(0.);
            for &li in &latch_indices {
                let (_, latch_hi) = constraints::world_bounds(&s.parts[li]);
                assert!((latch_hi.y - (floor - 3. - 0.05)).abs() < 0.01);
                let cx = s.parts[li].pose.position[0];
                for side in [-1., 1.] {
                    let inner_end = s.parts[m]
                        .mesh
                        .vertices
                        .chunks_exact(3)
                        .filter_map(|tri| {
                            let a = mm.transform_point3(Vec3::from(tri[0].position));
                            let b = mm.transform_point3(Vec3::from(tri[1].position));
                            let c = mm.transform_point3(Vec3::from(tri[2].position));
                            let center = (a + b + c) / 3.;
                            let n = (b - a).cross(c - a);
                            (n.length() > 1e-6
                                && n.z < -0.999 * n.length()
                                && (28. ..36.).contains(&(side * (center.x - cx)))
                                && center.y < floor - 3.01
                                && center.y > floor - 7.69
                                && center.z < 0.)
                                .then_some(center.z)
                        })
                        .reduce(f32::max)
                        .unwrap_or_else(|| {
                            panic!("inner latch seat missing: {width}{depth}/{height} side {side}")
                        });
                    assert!(
                        (latch_hi.z - inner_end).abs() < 0.02,
                        "Latch misses inner seat: {width}{depth}/{height} side {side}"
                    );
                }
                let child = constraints::PartId::of(&s.parts[li]);
                assert_eq!(
                    s.relations
                        .iter()
                        .find(|r| r.child == child)
                        .unwrap()
                        .parent,
                    constraints::PartId::of(&s.parts[m])
                );
            }
            let all_latches_before: Vec<_> = latch_indices
                .iter()
                .map(|&li| s.parts[li].pose.matrix(0.))
                .collect();
            let latch_before = s.parts[l].pose.matrix(0.);
            let bid = constraints::PartId::of(&s.parts[b]);
            for travel in [0., 25., 50.] {
                if let constraints::Joint::Slider { offset, .. } = &mut s
                    .relations
                    .iter_mut()
                    .find(|r| r.child == bid)
                    .unwrap()
                    .joint
                {
                    *offset = travel;
                }
                constraints::solve(&mut s).unwrap();
                let pair = Scene {
                    parts: vec![s.parts[m].clone(), s.parts[b].clone()],
                    ..Default::default()
                };
                assert!(
                    crate::collision::check(&pair, 0.01)
                        .unwrap()
                        .pairs
                        .is_empty(),
                    "Module / drawer crossing: {width}{depth}/{height} at {travel}"
                );
                assert!(s.parts[l].pose.matrix(0.).abs_diff_eq(latch_before, 0.001));
            }
            for (&li, before) in latch_indices.iter().zip(&all_latches_before) {
                assert!(s.parts[li].pose.matrix(0.).abs_diff_eq(*before, 0.001));
            }
            let module_before = s.parts[m].pose.matrix(0.);
            let before = s.parts[m].pose.matrix(0.).inverse() * s.parts[l].pose.matrix(0.);
            s.parts[m].pose.position = [12., 4., -7.];
            s.parts[m].pose.rotation[1] = 20.;
            constraints::solve(&mut s).unwrap();
            let after = s.parts[m].pose.matrix(0.).inverse() * s.parts[l].pose.matrix(0.);
            assert!(before.abs_diff_eq(after, 0.001));
            for (&li, before) in latch_indices.iter().zip(&all_latches_before) {
                assert!(
                    (s.parts[m].pose.matrix(0.).inverse() * s.parts[li].pose.matrix(0.))
                        .abs_diff_eq(module_before.inverse() * *before, 0.001)
                );
            }
            let def = s.definition();
            let encoded = toml::to_string(&def).unwrap();
            let saved = toml::from_str(&encoded).unwrap();
            let mut restored = s.clone();
            restored.apply(&saved);
            for &li in &latch_indices {
                assert!(
                    restored.parts[li]
                        .pose
                        .matrix(0.)
                        .abs_diff_eq(s.parts[li].pose.matrix(0.), 0.001)
                );
            }
            assert!(
                restored.parts[l]
                    .pose
                    .matrix(0.)
                    .abs_diff_eq(s.parts[l].pose.matrix(0.), 0.001)
            );
        }
    }
}
