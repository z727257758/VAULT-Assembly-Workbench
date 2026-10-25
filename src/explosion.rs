//! Display-only separation along assembled extraction axes. Never edits mates.
use crate::{constraints::PartId, scene::Scene};
use glam::Vec3;

pub fn reset(scene: &mut Scene) -> Result<(), String> {
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
    #[test]
    fn assembled_axes_hierarchy_rotation_and_roundtrip() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
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
            restored.apply(&def);
            for (a, b) in s.parts.iter().zip(&restored.parts) {
                assert!(a.pose.matrix(1.).abs_diff_eq(b.pose.matrix(1.), 0.003));
            }
        }
    }
}
