//! Drawer Assembly steps 4, 6, 9–11. Standard (non-XL) runner seats.
use crate::{
    constraints::{self, Relation},
    scene::Scene,
};
use glam::{Quat, Vec3};

pub fn apply(scene: &mut Scene, module: usize, drawer: usize) -> Result<(), String> {
    let box_part = &scene.parts[drawer];
    let half_width = box_part.source_size.x * 0.5;
    // Side-slot floor: x = outer side - 2.2 mm. Lowest track is used for
    // tall boxes; a single source BOM pair corresponds to the lower runner pair.
    let mut slot_min = f32::INFINITY;
    for tri in box_part.mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(tri[0].position);
        let b = Vec3::from(tri[1].position);
        let c = Vec3::from(tri[2].position);
        let n = (b - a).cross(c - a);
        let x = (a.x + b.x + c.x) / 3.;
        if (x - (half_width - 2.2)).abs() < 0.02 && n.x.abs() > n.length() * 0.999 {
            slot_min = slot_min.min(a.y).min(b.y).min(c.y);
        }
    }
    if !slot_min.is_finite() {
        return Err("Runner side-slot datum not recognized".into());
    }
    let track_y = box_part.pose.position[1] + slot_min + 6.2457;
    let (_, box_hi) = constraints::world_bounds(box_part);
    let (housing_lo, housing_hi) = constraints::world_bounds(&scene.parts[module]);
    let items: Vec<_> = scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.contains("/Runners/") && !p.pose.path.contains("/XL Runners/"))
        .map(|(i, _)| i)
        .collect();
    for i in items {
        let p = &scene.parts[i];
        let name = p.pose.path.rsplit('/').next().unwrap_or("");
        if p.pose.instance > 1 {
            continue;
        }
        let side = if p.pose.instance == 0 { -1. } else { 1. };
        let (parent, x, z, label) = if name.contains("Runner Guide") {
            (
                module,
                side * (half_width + 7.3),
                housing_hi.z - 0.4 - p.source_size.y * 0.5,
                "Manual step 9: guide / housing channel",
            )
        } else if name.contains("Runner Slide") {
            (
                drawer,
                side * (half_width + 2.5),
                box_hi.z - p.source_size.y * 0.5,
                "Manual step 6: slide / drawer dovetail",
            )
        } else if name == "Runner Stopper.stl" {
            let outer = if side < 0. {
                housing_lo.x
            } else {
                housing_hi.x
            };
            (
                module,
                outer - side * (p.source_size.z * 0.5 - 1.2),
                housing_hi.z - 44.55,
                "Manual step 11: stopper / housing side port",
            )
        } else {
            continue;
        };
        // Raw +Z points inward. Guide/slide raw +Y points to the facade;
        // the rear flange of the slide remains at the back. Never mirror meshes.
        let mut q = Quat::from_rotation_z((side * 90f32).to_radians());
        if name.contains("Runner Guide") || name.contains("Runner Slide") {
            q *= Quat::from_rotation_y(std::f32::consts::PI);
        }
        let (rx, ry, rz) = q.to_euler(glam::EulerRot::XYZ);
        scene.parts[i].pose.rotation = [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()];
        scene.parts[i].pose.position = [x, track_y, z];
        let mut relation = Relation::fixed(scene, parent, i, false)?;
        relation.label = label.into();
        scene.relations.retain(|r| r.child != relation.child);
        constraints::add(scene, relation)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rails_fit_and_follow_the_correct_parent() {
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
            let mut s =
                Scene::load(&c.root, &c.bom(&sel), "runner regression".into(), true).unwrap();
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
            let indices: Vec<_> = s
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.pose.path.contains("/Runners/"))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(indices.len(), 6);
            let before: Vec<_> = s.parts.iter().map(|p| p.pose.position).collect();
            for &i in &indices {
                let parent = if s.parts[i].pose.path.contains("Runner Slide") {
                    b
                } else {
                    m
                };
                assert!(
                    s.relations
                        .iter()
                        .any(|r| r.child == constraints::PartId::of(&s.parts[i])
                            && r.parent == constraints::PartId::of(&s.parts[parent]))
                );
            }
            let (_, housing_hi) = constraints::world_bounds(&s.parts[m]);
            for &i in &indices {
                if s.parts[i].pose.path.ends_with("Stopper.stl") {
                    let p = &s.parts[i];
                    assert!((p.pose.position[2] - (housing_hi.z - 44.55)).abs() < 0.002);
                    let (lo, hi) = constraints::world_bounds(p);
                    let (ml, mh) = constraints::world_bounds(&s.parts[m]);
                    let lip = if p.pose.instance == 0 {
                        ml.x - lo.x
                    } else {
                        hi.x - mh.x
                    };
                    assert!((lip - 1.2).abs() < 0.002);
                }
            }
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
                let mut parts = vec![s.parts[m].clone(), s.parts[b].clone()];
                parts.extend(
                    indices
                        .iter()
                        .filter(|&&i| !s.parts[i].pose.path.ends_with("Stopper.stl"))
                        .map(|&i| s.parts[i].clone()),
                );
                let subset = Scene {
                    parts,
                    ..Default::default()
                };
                let report = crate::collision::check(&subset, 0.01).unwrap();
                for p in &report.pairs {
                    println!(
                        "{width}{depth}/{height} {travel}: {} / {}",
                        subset.parts[p.a].label(false),
                        subset.parts[p.b].label(false)
                    );
                }
                assert!(report.pairs.is_empty());
                for &i in &indices {
                    let delta = if s.parts[i].pose.path.contains("Runner Slide") {
                        travel
                    } else {
                        0.
                    };
                    assert!((s.parts[i].pose.position[2] - before[i][2] - delta).abs() < 0.002);
                }
            }
            let saved = s.definition();
            let decoded = toml::from_str(&toml::to_string(&saved).unwrap()).unwrap();
            let mut restored = s.clone();
            restored.apply(&decoded);
            for &i in &indices {
                assert!(
                    restored.parts[i]
                        .pose
                        .matrix(0.)
                        .abs_diff_eq(s.parts[i].pose.matrix(0.), 0.001)
                );
            }
        }
    }
    #[test]
    fn support_removal_is_display_only() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        let bom = c.bom(&Default::default());
        let item = bom.iter().find(|b| b.path.contains("/Boxes/")).unwrap();
        let bytes = std::fs::read(c.root.join(&item.path)).unwrap();
        let source = crate::mesh::Mesh::load(&c.root.join(&item.path)).unwrap();
        let original = bytemuck::cast_slice::<_, u8>(&source.vertices).to_vec();
        let prepared = crate::display::remove_runner_supports(&source);
        assert!(prepared.vertices.len() < source.vertices.len());
        assert!(prepared.vertices.len() > source.vertices.len() * 9 / 10);
        assert_eq!(original, bytemuck::cast_slice::<_, u8>(&source.vertices));
        assert_eq!(source.min, prepared.min);
        assert_eq!(source.max, prepared.max);
        let standalone =
            Scene::load(&c.root, std::slice::from_ref(item), "source".into(), false).unwrap();
        assert_eq!(
            standalone.parts[0].mesh.vertices.len(),
            source.vertices.len()
        );
        assert_eq!(bytes, std::fs::read(c.root.join(&item.path)).unwrap());
    }
}
