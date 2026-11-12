//! Display-only separation along assembled extraction axes. Never edits mates.
use crate::{constraints::PartId, scene::Scene};
use glam::Vec3;

const MEMBER_GAP: f32 = 35.;

/// Give each member's exploded display envelope its own space. This only adds a
/// common translation to that member's existing extraction travel; closed poses,
/// joints, meshes and manufacturing layouts remain the assembly's source data.
pub(crate) fn space_combination(
    scene: &mut Scene,
    placements: &[crate::scene::Placement],
) -> Result<(), String> {
    if placements.len() < 2 {
        return Ok(());
    }
    let mut bounds =
        vec![(Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)); placements.len()];
    // Use all display parts, including hidden ones, so isolation cannot rearrange
    // the remaining members. Conservative mesh boxes are presentation envelopes,
    // not surface-crossing diagnostics or proof of mechanical clearance.
    for part in &scene.parts {
        let index = (part.pose.instance / crate::scene::MEMBER_STRIDE) as usize;
        let (lo, hi) = bounds
            .get_mut(index)
            .ok_or("Invalid explosion member identity")?;
        let matrix = part.pose.matrix(1.);
        for x in [part.mesh.min.x, part.mesh.max.x] {
            for y in [part.mesh.min.y, part.mesh.max.y] {
                for z in [part.mesh.min.z, part.mesh.max.z] {
                    let point = matrix.transform_point3(Vec3::new(x, y, z));
                    *lo = lo.min(point);
                    *hi = hi.max(point);
                }
            }
        }
    }
    if bounds
        .iter()
        .any(|(lo, hi)| !lo.is_finite() || !hi.is_finite())
    {
        return Err("Invalid member explosion envelope".into());
    }
    let mut rows = std::collections::BTreeMap::<u32, Vec<usize>>::new();
    for (i, placement) in placements.iter().enumerate() {
        rows.entry(placement.level).or_default().push(i);
    }
    let mut shifts = vec![Vec3::ZERO; placements.len()];
    let mut ceiling: Option<f32> = None;
    for members in rows.values_mut() {
        members.sort_by_key(|&i| placements[i].column);
        let mut right: Option<f32> = None;
        let mut outward = 0f32;
        for &i in members.iter() {
            let (lo, hi) = bounds[i];
            outward = right.map_or(0., |edge| (edge + MEMBER_GAP - lo.x).max(outward));
            shifts[i].x = outward;
            right = Some(hi.x + outward);
        }
        // Open a paired layer equally to either side of its assembled center.
        // A spanning whole-width member above it retains its horizontal datum.
        let center_shift = shifts[*members.last().unwrap()].x * 0.5;
        let bottom = members
            .iter()
            .map(|&i| bounds[i].0.y)
            .fold(f32::INFINITY, f32::min);
        let top = members
            .iter()
            .map(|&i| bounds[i].1.y)
            .fold(f32::NEG_INFINITY, f32::max);
        let lift = ceiling.map_or(0., |edge| (edge + MEMBER_GAP - bottom).max(0.));
        for &i in members.iter() {
            shifts[i].x -= center_shift;
            shifts[i].y = lift;
        }
        ceiling = Some(top + lift);
    }
    for part in &mut scene.parts {
        let shift = shifts[(part.pose.instance / crate::scene::MEMBER_STRIDE) as usize];
        if shift == Vec3::ZERO {
            continue;
        }
        let assembled = part.pose.matrix(0.);
        let travel = part.pose.matrix(1.).transform_point3(Vec3::ZERO)
            - assembled.transform_point3(Vec3::ZERO);
        part.pose.travel = (travel + shift).to_array();
        part.pose.travel_frame = Some(assembled.to_scale_rotation_translation().1.to_array());
    }
    Ok(())
}

pub fn reset(scene: &mut Scene) -> Result<(), String> {
    if scene.assembly
        && scene
            .parts
            .iter()
            .all(|p| p.pose.path.starts_with(crate::sustainer::PREFIX))
    {
        return crate::sustainer::reset_explosion(scene);
    }
    if !scene.assembly || scene.relations.is_empty() {
        return Err("Explosion layout requires an assembled drawer with relations".into());
    }
    let ids: Vec<_> = scene.parts.iter().map(PartId::of).collect();
    let mut parents = vec![None; ids.len()];
    for r in &scene.relations {
        let parent = ids
            .iter()
            .position(|id| id == &r.parent)
            .ok_or("Missing explosion parent")?;
        let child = ids
            .iter()
            .position(|id| id == &r.child)
            .ok_or("Missing explosion child")?;
        if parent == child || parents[child].replace(parent).is_some() {
            return Err("Invalid explosion hierarchy".into());
        }
    }
    let drawer = scene
        .parts
        .iter()
        .find(|p| p.pose.path.contains("/Boxes/") && !p.pose.path.contains(" Part "))
        .ok_or("Unsplit drawer required for explosion layout")?;
    let frame = drawer.pose.matrix(0.);
    let front = frame.transform_vector3(-Vec3::Z);
    let up = frame.transform_vector3(Vec3::Y);
    let depth = drawer.source_size.y;
    let mut offsets = vec![None; ids.len()];
    for _ in 0..ids.len() {
        for (i, p) in scene.parts.iter().enumerate() {
            if offsets[i].is_some() {
                continue;
            }
            let inherited = match parents[i] {
                Some(parent) => match offsets[parent] {
                    Some(v) => v,
                    None => continue,
                },
                None => Vec3::ZERO,
            };
            let n = p.pose.path.rsplit('/').next().unwrap_or("");
            let matrix = p.pose.matrix(0.);
            let local_front = matrix.transform_vector3(Vec3::Y);
            let own = if parents[i].is_none() {
                Vec3::ZERO
            } else if p.pose.path.contains("/Boxes/") {
                front * (depth + 25.)
            } else if p.pose.path.contains("/Facades/") {
                local_front * 75.
            } else if n.contains("Runner Stopper") {
                -local_front * 55.
            } else if n.contains("Runner Guide") {
                front * (depth + 15.) - local_front * 65.
            } else if n.contains("Runner Slide") {
                front * 55. - local_front * 35.
            } else if p.pose.path.contains("/Geared Rails/") {
                // Both source racks have the rounded stop at local -X. Pull
                // toward local +X: opposite side entries after assembly rotation.
                // Clear the parent side wall by 20 mm without changing slot height.
                let parent = &scene.parts[parents[i].unwrap()];
                let relative = parent.pose.matrix(0.).inverse() * matrix;
                let side = relative.transform_vector3(Vec3::X).x.signum();
                let edge = if side > 0. {
                    parent.mesh.max.x
                } else {
                    parent.mesh.min.x
                };
                let center = relative.transform_point3(Vec3::ZERO).x;
                let distance = (side * (edge - center) - p.mesh.min.x + 20.).max(0.);
                matrix.transform_vector3(Vec3::X) * distance
            } else if p.pose.path.contains("/Grid Dividers/") {
                if n.contains("pin_") {
                    up * (scene.parts[parents[i].unwrap()].source_size.z + 10.)
                } else if n.contains("Edge Piece") {
                    matrix.transform_vector3(Vec3::Z) * 25.
                } else {
                    up * 45.
                }
            } else if n == "Latch.stl" {
                local_front * 65.
            } else if n == "Springs.stl" {
                // Fan the two bodies outward in the facade frame, retaining
                // the rear layer. Do not infer side from mirrored body IDs.
                let parent = &scene.parts[parents[i].unwrap()];
                let frame = parent.pose.matrix(0.);
                let lateral = frame.transform_vector3(Vec3::X);
                let relative = Vec3::from(p.pose.position) - Vec3::from(parent.pose.position);
                -frame.transform_vector3(Vec3::Y) * 35.
                    + lateral * relative.dot(lateral).signum() * 35.
            } else if n == "Key Cap.stl" {
                -local_front * 110.
            } else if n == "Hidden Key Part 2 (Turn on Supports).stl" {
                // Removable head exits the socket toward the facade exterior.
                local_front * 70.
            } else if n.contains("Key") {
                -local_front * 30.
            } else if n == "Label.stl" {
                matrix.transform_vector3(Vec3::X) * 55.
            } else if n == "Facade Cap.stl" {
                local_front * 25.
            } else if p.pose.path.contains("/Roofs/") {
                up * 70.
            } else if p.pose.path.starts_with(crate::gridfinity::PREFIX) {
                up * (p.source_size.z + 35.)
            } else if n.contains("Baseplate") {
                up * 40.
            } else {
                Vec3::ZERO
            };
            offsets[i] = Some(inherited + own);
        }
    }
    if offsets.iter().any(Option::is_none) {
        return Err("Explosion hierarchy contains a cycle".into());
    }
    for (p, travel) in scene.parts.iter_mut().zip(offsets) {
        p.pose.travel = travel.unwrap().to_array();
        p.pose.travel_frame = Some(
            p.pose
                .matrix(0.)
                .to_scale_rotation_translation()
                .1
                .to_array(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::{Catalog, Selection},
        constraints,
        scene::Definition,
    };
    use glam::{Mat4, Quat};

    fn display_member(name: &str, width: f32) -> Scene {
        use crate::{
            mesh::{Mesh, Vertex},
            scene::{Part, Pose},
        };
        let mesh = std::sync::Arc::new(Mesh {
            vertices: [
                [-width * 0.5, -10., -15.],
                [width * 0.5, 10., -15.],
                [-width * 0.5, 10., 15.],
            ]
            .into_iter()
            .map(|position| Vertex {
                position,
                normal: [0., 1., 0.],
                shade: 1.,
            })
            .collect(),
            min: Vec3::new(-width * 0.5, -10., -15.),
            max: Vec3::new(width * 0.5, 10., 15.),
            size: Vec3::new(width, 20., 30.),
        });
        let root = Part {
            source_size: mesh.size,
            mesh,
            pose: Pose {
                path: format!("test/{name}/shell.stl"),
                instance: 0,
                body: None,
                position: [0.; 3],
                rotation: [0.; 3],
                travel: [0.; 3],
                travel_frame: None,
            },
            color: [1.; 4],
            visible: true,
            source_bodies: 1,
        };
        let mut child = root.clone();
        child.pose.path = format!("test/{name}/moving.stl");
        child.pose.instance = 1;
        child.pose.position = [0., 5., 10.];
        child.pose.rotation = [25., 40., 15.];
        child.pose.travel = [-80., 60., 180.];
        child.pose.travel_frame = Some(Quat::from_rotation_y(-0.4).to_array());
        Scene {
            parts: vec![root, child],
            assembly: true,
            ..Default::default()
        }
    }

    fn envelopes(scene: &Scene) -> Vec<(Vec3, Vec3)> {
        let mut result = vec![
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            scene.member_bounds.len()
        ];
        for part in &scene.parts {
            let (lo, hi) = &mut result[(part.pose.instance / crate::scene::MEMBER_STRIDE) as usize];
            for x in [part.mesh.min.x, part.mesh.max.x] {
                for y in [part.mesh.min.y, part.mesh.max.y] {
                    for z in [part.mesh.min.z, part.mesh.max.z] {
                        let point = part.pose.matrix(1.).transform_point3(Vec3::new(x, y, z));
                        *lo = lo.min(point);
                        *hi = hi.max(point);
                    }
                }
            }
        }
        result
    }

    fn display_members() -> Vec<(Scene, crate::scene::Placement, Vec3)> {
        use crate::scene::Placement;
        vec![
            (
                display_member("left", 40.),
                Placement::default(),
                Vec3::new(-30., 0., 0.),
            ),
            (
                display_member("right", 40.),
                Placement {
                    column: 1,
                    ..Default::default()
                },
                Vec3::new(30., 0., 0.),
            ),
            (
                display_member("bridge", 100.),
                Placement {
                    level: 1,
                    ..Default::default()
                },
                Vec3::new(0., 25., 0.),
            ),
            (
                display_member("top", 100.),
                Placement {
                    level: 2,
                    ..Default::default()
                },
                Vec3::new(0., 50., 0.),
            ),
        ]
    }

    fn compose(members: &[(Scene, crate::scene::Placement, Vec3)]) -> Scene {
        Scene::combine_positioned(
            members
                .iter()
                .map(|(scene, placement, _)| (scene.clone(), *placement, 0.))
                .collect(),
            0.,
            Some(members.iter().map(|(_, _, shift)| *shift).collect()),
        )
        .unwrap()
    }

    #[test]
    fn combined_explosion_spaces_rows_and_pairs_without_changing_extraction_or_closed_poses() {
        let members = display_members();
        let scene = compose(&members);
        let boxes = envelopes(&scene);
        assert!((boxes[1].0.x - boxes[0].1.x - MEMBER_GAP).abs() < 0.001);
        assert!((boxes[2].0.y - boxes[0].1.y.max(boxes[1].1.y) - MEMBER_GAP).abs() < 0.001);
        assert!((boxes[3].0.y - boxes[2].1.y - MEMBER_GAP).abs() < 0.001);
        for (i, (original, _, shift)) in members.iter().enumerate() {
            let root_shift = scene.parts[i * 2]
                .pose
                .matrix(1.)
                .transform_point3(Vec3::ZERO)
                - original.parts[0]
                    .pose
                    .matrix(1.)
                    .transform_point3(Vec3::ZERO)
                - shift;
            for (source, part) in original.parts.iter().zip(&scene.parts[i * 2..i * 2 + 2]) {
                assert_eq!(source.pose.rotation, part.pose.rotation);
                for amount in [0., 0.25, 0.7, 1.] {
                    let expected = Mat4::from_translation(*shift + root_shift * amount)
                        * source.pose.matrix(amount);
                    assert!(part.pose.matrix(amount).abs_diff_eq(expected, 0.001));
                }
            }
        }
        assert!(scene.parts[0].pose.travel[0] < 0. && scene.parts[2].pose.travel[0] > 0.);
        assert!(scene.parts[4].pose.travel[0].abs() < 0.001);
    }

    #[test]
    fn combined_spacing_is_stable_across_visibility_and_saved_member_order() {
        let members = display_members();
        let scene = compose(&members);
        let mut reordered = vec![
            members[2].clone(),
            members[1].clone(),
            members[3].clone(),
            members[0].clone(),
        ];
        reordered[1].0.parts[1].visible = false;
        let other = compose(&reordered);
        for part in &scene.parts {
            let matching = other
                .parts
                .iter()
                .find(|p| p.pose.path == part.pose.path)
                .unwrap();
            assert!(
                part.pose
                    .matrix(1.)
                    .abs_diff_eq(matching.pose.matrix(1.), 0.001)
            );
        }
    }

    #[test]
    fn single_member_keeps_its_existing_explosion_frames() {
        let source = display_member("single", 40.);
        let original = serde_json::to_value(&source.definition().poses).unwrap();
        let combined = compose(&[(source, Default::default(), Vec3::ZERO)]);
        assert_eq!(
            original,
            serde_json::to_value(&combined.definition().poses).unwrap()
        );
    }

    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn assembled_axes_hierarchy_rotation_and_roundtrip() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let catalog = Catalog::scan(root.into()).unwrap();
        for width in ["S", "L"] {
            let selection = Selection {
                width: width.into(),
                ..Default::default()
            };
            let mut s = Scene::load(
                &catalog.root,
                &catalog.bom(&selection),
                "explosion".into(),
                true,
            )
            .unwrap();
            let assembled: Vec<_> = s.parts.iter().map(|p| p.pose.matrix(0.)).collect();
            let relations = toml::to_string(&s.definition()).unwrap();
            reset(&mut s).unwrap();
            assert_eq!(relations, toml::to_string(&s.definition()).unwrap());
            let module = s
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Modules/"))
                .unwrap();
            for (i, p) in s.parts.iter().enumerate() {
                assert!(p.pose.matrix(0.).abs_diff_eq(assembled[i], 0.0001));
                let offset =
                    p.pose.matrix(1.).transform_point3(Vec3::ZERO) - Vec3::from(p.pose.position);
                let n = p.pose.path.rsplit('/').next().unwrap();
                if let Some(relation) = s.relations.iter().find(|r| r.child == PartId::of(p)) {
                    let parent = s
                        .parts
                        .iter()
                        .find(|a| PartId::of(a) == relation.parent)
                        .unwrap();
                    let inherited = parent.pose.matrix(1.).transform_point3(Vec3::ZERO)
                        - Vec3::from(parent.pose.position);
                    let relative = offset - inherited;
                    let normal = p.pose.matrix(0.).transform_vector3(Vec3::Y);
                    if p.pose.path.contains("/Geared Rails/") {
                        let out = p.pose.matrix(0.).transform_vector3(Vec3::X);
                        assert!(relative.dot(out) > 0.);
                        assert!(relative.cross(out).length() < 0.01);
                        let parent_inv = parent.pose.matrix(1.).inverse();
                        let expanded = parent_inv * p.pose.matrix(1.);
                        let axis = expanded.transform_vector3(Vec3::X).x.signum();
                        let trailing = expanded.transform_point3(Vec3::new(p.mesh.min.x, 0., 0.)).x;
                        let edge = if axis > 0. {
                            parent.mesh.max.x
                        } else {
                            parent.mesh.min.x
                        };
                        assert!((axis * (trailing - edge) - 20.).abs() < 0.02);
                    }
                    if n == "Springs.stl" {
                        let lateral = parent.pose.matrix(0.).transform_vector3(Vec3::X);
                        let side = (Vec3::from(p.pose.position) - Vec3::from(parent.pose.position))
                            .dot(lateral);
                        assert!(relative.dot(lateral) * side > 0.);
                        assert!((relative.dot(lateral).abs() - 35.).abs() < 0.01);
                        assert!(
                            relative.dot(parent.pose.matrix(0.).transform_vector3(Vec3::Y)) < -30.
                        );
                    }
                    if n == "Key (Turn On Supports).stl" {
                        assert!(relative.dot(normal) < -25.);
                    }
                    if n == "Key Cap.stl" {
                        assert!(relative.dot(-normal) > 100.);
                    }
                    if n == "Label.stl" {
                        assert!(relative.dot(p.pose.matrix(0.).transform_vector3(Vec3::X)) > 50.);
                        assert!(relative.dot(normal).abs() < 0.01);
                    }
                }
                if n == "Latch.stl" {
                    assert!(offset.y < -60. && offset.x.abs() < 0.01 && offset.z.abs() < 0.01);
                }
                if n == "Runner Stopper.stl" {
                    assert!(
                        offset.x * p.pose.position[0] > 0.
                            && offset.y.abs() < 0.01
                            && offset.z.abs() < 0.01
                    );
                }
                if n.contains("Runner Guide") || n.contains("Runner Slide") {
                    assert!(
                        offset.z > 0.
                            && offset.x * p.pose.position[0] > 0.
                            && offset.y.abs() < 0.01
                    );
                    let lateral_gap = if n.contains("Guide") { 65. } else { 35. };
                    assert!((offset.x.abs() - lateral_gap).abs() < 0.01);
                }
            }
            assert!(Vec3::from(s.parts[module].pose.travel).length() < 0.001);
            let before = s.clone();
            let q = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.3);
            constraints::rotate_subassembly(&mut s, module, q).unwrap();
            for (old, new) in before.parts.iter().zip(&s.parts) {
                for e in [0., 0.5, 1.] {
                    assert!(
                        new.pose
                            .matrix(e)
                            .abs_diff_eq(Mat4::from_quat(q) * old.pose.matrix(e), 0.003)
                    );
                }
            }
            let def: Definition =
                toml::from_str(&toml::to_string(&s.definition()).unwrap()).unwrap();
            let mut restored = before.clone();
            restored.apply(&def).unwrap();
            for (a, b) in s.parts.iter().zip(&restored.parts) {
                assert!(a.pose.matrix(1.).abs_diff_eq(b.pose.matrix(1.), 0.003));
            }
        }
    }
}
