//! Drawer Assembly steps 4, 6, 9–11. Standard (non-XL) runner seats.
use crate::{
    constraints::{self, Relation},
    scene::Scene,
};
use glam::{Quat, Vec3};

pub const STOPPER_SEAT: &str = "Manual step 11: stopper / measured housing side port";

fn stopper_port(
    scene: &Scene,
    module: usize,
    side: f32,
    y: f32,
    length: f32,
) -> Result<(f32, f32), String> {
    let housing = &scene.parts[module];
    let (lo, hi) = constraints::world_bounds(housing);
    let outer = if side < 0. { lo.x } else { hi.x };
    let matrix = housing.pose.matrix(0.);
    let mut planes = std::collections::BTreeMap::<i32, f32>::new();
    for tri in housing.mesh.vertices.chunks_exact(3) {
        let p: [_; 3] = std::array::from_fn(|j| matrix.transform_point3(tri[j].position.into()));
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        let x = (p[0].x + p[1].x + p[2].x) / 3.;
        if (x - outer).abs() < 1. && n.x.abs() > n.length() * 0.999 {
            *planes.entry((x * 100.).round() as i32).or_default() += n.length();
        }
    }
    let outer = planes
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(x, _)| x as f32 / 100.)
        .ok_or("Runner stopper outer wall not recognized")?;
    let mut intervals = Vec::new();
    // Intersect the actual outer wall triangles with the runner's horizontal
    // plane. A port is a gap in the wall, not a distance from its front bound.
    for tri in housing.mesh.vertices.chunks_exact(3) {
        let points: [_; 3] =
            std::array::from_fn(|j| matrix.transform_point3(tri[j].position.into()));
        if points.iter().any(|p| (p.x - outer).abs() > 0.15) {
            continue;
        }
        let mut crossings = Vec::new();
        for j in 0..3 {
            let a = points[j];
            let b = points[(j + 1) % 3];
            if (a.y - y) * (b.y - y) <= 0. && (a.y - b.y).abs() > 1e-6 {
                crossings.push(a.z + (b.z - a.z) * (y - a.y) / (b.y - a.y));
            }
        }
        if crossings.len() >= 2 {
            let min = crossings.iter().copied().fold(f32::INFINITY, f32::min);
            let max = crossings.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            intervals.push((min, max));
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = f32::NEG_INFINITY;
    let mut ports = Vec::new();
    for (start, next) in intervals {
        let gap = start - end;
        if end.is_finite() && (gap - length).abs() < 2. {
            ports.push((start + end) * 0.5);
        }
        end = end.max(next);
    }
    match ports.as_slice() {
        [center] => Ok((outer, *center)),
        _ => Err(format!(
            "Runner stopper side port not uniquely recognized ({} candidates)",
            ports.len()
        )),
    }
}

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
    let (_, housing_hi) = constraints::world_bounds(&scene.parts[module]);
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
            let (outer, center) = stopper_port(scene, module, side, track_y, p.source_size.y)?;
            (
                module,
                outer - side * (p.source_size.z * 0.5 - 1.2),
                center,
                STOPPER_SEAT,
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
    fn stopper_uses_wall_opening_and_rejects_missing_or_ambiguous_ports() {
        let wall = |spans: &[(f32, f32)]| {
            let mut vertices = Vec::new();
            for &(a, b) in spans {
                let corners = [[10., -10., a], [10., 10., a], [10., 10., b], [10., -10., b]];
                for j in [0, 1, 2, 0, 2, 3] {
                    vertices.push(crate::mesh::Vertex {
                        position: corners[j],
                        normal: [1., 0., 0.],
                        shade: 1.,
                    });
                }
            }
            let mesh = std::sync::Arc::new(crate::mesh::Mesh {
                vertices,
                size: Vec3::new(20., 20., 200.),
                min: Vec3::new(10., -10., -100.),
                max: Vec3::new(10., 10., 100.),
            });
            Scene {
                parts: vec![crate::scene::Part {
                    source_size: mesh.size,
                    mesh,
                    source_bodies: 1,
                    visible: true,
                    color: [1.; 4],
                    pose: crate::scene::Pose {
                        path: "wall.stl".into(),
                        instance: 0,
                        body: None,
                        position: [0.; 3],
                        rotation: [0.; 3],
                        travel: [0.; 3],
                        travel_frame: None,
                    },
                }],
                ..Default::default()
            }
        };
        for center in [-28.5, 0., 28.5, 60.] {
            let scene = wall(&[(-100., center - 9.35), (center + 9.35, 100.)]);
            let (x, z) = stopper_port(&scene, 0, 1., 0., 17.93).unwrap();
            assert!((x - 10.).abs() < 0.001 && (z - center).abs() < 0.001);
        }
        assert!(stopper_port(&wall(&[(-100., 100.)]), 0, 1., 0., 17.93).is_err());
        assert!(
            stopper_port(
                &wall(&[(-100., -39.35), (-20.65, 20.65), (39.35, 100.)]),
                0,
                1.,
                0.,
                17.93
            )
            .is_err()
        );
    }
    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn rails_fit_and_follow_the_correct_parent() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        for (width, height, depth) in [
            ("S", "1F", 3),
            ("S", "2F", 3),
            ("M", "1F", 3),
            ("L", "1F", 3),
            ("S", "1F", 4),
            ("S", "1F", 7),
            ("M", "2F", 5),
            ("L", "4F", 5),
        ] {
            let sel = crate::catalog::Selection {
                width: width.into(),
                height: height.into(),
                depth,
                ..Default::default()
            };
            let mut s =
                Scene::load(&c.root, &c.bom(&sel), "runner regression".into(), true).unwrap();
            if s.relations.is_empty() {
                constraints::basic_relations(&mut s).unwrap();
            }
            assert!(
                s.constraint_warning.is_none(),
                "{width}{depth}/{height}: {:?}",
                s.constraint_warning
            );
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

            for &i in &indices {
                if s.parts[i].pose.path.ends_with("Stopper.stl") {
                    let p = &s.parts[i];
                    assert!(
                        (p.pose.position[2] - 28.5).abs() < 0.03,
                        "{width}{depth}/{height}: {:?}",
                        p.pose.position
                    );
                    let (lo, hi) = constraints::world_bounds(p);
                    let (ml, mh) = constraints::world_bounds(&s.parts[m]);
                    let lip = if p.pose.instance == 0 {
                        ml.x - lo.x
                    } else {
                        hi.x - mh.x
                    };
                    assert!((lip - 1.2).abs() < 0.25);
                    let side = if p.pose.instance == 0 { -1. } else { 1. };
                    let outer = if side < 0. { ml.x } else { mh.x };
                    let housing = Scene {
                        parts: vec![s.parts[m].clone()],
                        ..Default::default()
                    };
                    // Independent rays through the port and its surrounding wall:
                    // the stopper must cover an opening, not sit on solid material.
                    for (dz, hole) in [
                        (0., true),
                        (-5., true),
                        (5., true),
                        (-12., false),
                        (12., false),
                    ] {
                        let origin = Vec3::new(
                            outer + side * 2.,
                            p.pose.position[1],
                            p.pose.position[2] + dz,
                        );
                        let distance = housing.pick(origin, -side * Vec3::X, 0.).map_or(
                            f32::INFINITY,
                            |hit| {
                                origin.distance(
                                    housing.parts[0].pose.matrix(0.).transform_point3(hit.point),
                                )
                            },
                        );
                        assert!(
                            if hole { distance > 5. } else { distance < 2.3 },
                            "{width}{depth}/{height} side {side} dz {dz}: distance {distance}"
                        );
                    }
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
            // Old uncalibrated projects must not restore the wrong fixed datum.
            let mut legacy = s.clone();
            for &i in &indices {
                if legacy.parts[i].pose.path.ends_with("Stopper.stl") {
                    legacy.parts[i].pose.position[2] =
                        constraints::world_bounds(&legacy.parts[m]).1.z - 44.55;
                    let mut relation = Relation::fixed(&legacy, m, i, false).unwrap();
                    relation.label = "Manual step 11: stopper / housing side port".into();
                    legacy.relations.retain(|r| r.child != relation.child);
                    legacy.relations.push(relation);
                }
            }
            let definition = legacy.definition();
            let mut upgraded = s.clone();
            upgraded.apply(&definition).unwrap();
            for &i in &indices {
                assert!(
                    upgraded.parts[i]
                        .pose
                        .matrix(0.)
                        .abs_diff_eq(s.parts[i].pose.matrix(0.), 0.001)
                );
            }
            legacy.calibrated = true;
            let mut confirmed = s.clone();
            confirmed.apply(&legacy.definition()).unwrap();
            for &i in &indices {
                assert!(
                    confirmed.parts[i]
                        .pose
                        .matrix(0.)
                        .abs_diff_eq(legacy.parts[i].pose.matrix(0.), 0.001)
                );
            }
            let saved = s.definition();
            let decoded = toml::from_str(&toml::to_string(&saved).unwrap()).unwrap();
            let mut restored = s.clone();
            restored.apply(&decoded).unwrap();
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
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn support_removal_is_display_only() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
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
