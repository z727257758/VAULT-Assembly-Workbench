//! Display assembly for Sustainers. Meshes remain immutable. Surface datums
//! recover print rotations; hardware positions are estimates, never calibration.
use crate::{
    constraints::{self, Joint, Relation},
    scene::{Part, Scene},
};
use glam::{DVec3, Mat3, Quat, Vec3};
use std::collections::BTreeMap;

pub const PREFIX: &str = "02. SUSTAINERS/";
pub const LID: &str = "Sustainer: lid hinge (estimated)";

fn raw(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, z, -y)
}
fn orientation(x: Vec3, y: Vec3, z: Vec3) -> Quat {
    // Mesh::parse maps raw STL (x,y,z) to display (x,z,-y).
    Quat::from_mat3(&Mat3::from_cols(x, z, -y))
}
fn pose(part: &mut Part, q: Quat, p: Vec3) {
    let (x, y, z) = q.to_euler(glam::EulerRot::XYZ);
    part.pose.rotation = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
    part.pose.position = p.to_array();
}
fn bounds(part: &Part, q: Quat) -> (Vec3, Vec3) {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for v in &part.mesh.vertices {
        let p = q * Vec3::from(v.position);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}
fn anchor(part: &mut Part, q: Quat, local: Vec3, world: Vec3) {
    pose(part, q, world - q * local);
}
fn plane_levels(part: &Part, q: Quat) -> Vec<(f32, f32)> {
    let mut levels = BTreeMap::<i32, f32>::new();
    for tri in part.mesh.vertices.chunks_exact(3) {
        let a = q * Vec3::from(tri[0].position);
        let b = q * Vec3::from(tri[1].position);
        let c = q * Vec3::from(tri[2].position);
        let n = (b - a).cross(c - a);
        if n.length() > 1e-6 && n.y.abs() > n.length() * 0.9999 {
            *levels
                .entry(((a.y + b.y + c.y) / 3. * 10.).round() as i32)
                .or_default() += n.length();
        }
    }
    let mut levels: Vec<_> = levels
        .into_iter()
        .map(|(y, a)| (y as f32 / 10., a))
        .collect();
    levels.sort_by(|a, b| b.1.total_cmp(&a.1));
    levels
}
fn broad_rotation(part: &Part, expected_width: f32) -> Result<Quat, String> {
    let mut angles = BTreeMap::<i32, (f64, DVec3)>::new();
    for tri in part.mesh.vertices.chunks_exact(3) {
        let a = DVec3::from_array(tri[0].position.map(f64::from));
        let b = DVec3::from_array(tri[1].position.map(f64::from));
        let c = DVec3::from_array(tri[2].position.map(f64::from));
        let n = (b - a).cross(c - a);
        let area = n.length();
        if area > 1e-6 && n.y.abs() < area * 0.001 {
            let angle = n.z.atan2(n.x).to_degrees().rem_euclid(180.);
            let entry = angles.entry((angle * 10.).round() as i32).or_default();
            entry.0 += area;
            entry.1 += if n.z < 0. || n.z == 0. && n.x < 0. {
                -n
            } else {
                n
            };
        }
    }
    let largest = angles
        .values()
        .map(|v| v.0)
        .reduce(f64::max)
        .ok_or("Sustainer broad plane not found")?;
    let mut candidates = Vec::new();
    for (_, (area, normal)) in angles {
        if area < largest * 0.05 || normal.length() < 1e-6 {
            continue;
        }
        let n = DVec3::new(normal.x, 0., normal.z).normalize().as_vec3();
        let q = Quat::from_mat3(&Mat3::from_cols(n.cross(Vec3::Y), n, Vec3::Y).transpose());
        let (lo, hi) = bounds(part, q);
        candidates.push(((hi.x - lo.x - expected_width).abs(), area, q));
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| b.1.total_cmp(&a.1)));
    let (error, _, _) = candidates
        .first()
        .ok_or("Sustainer plane candidate missing")?;
    if *error > 1. {
        return Err(format!("Sustainer width datum differs by {error:.2} mm"));
    }
    // Width selects the physical axis. Among compatible axes prefer the broad
    // structural plane, not a noisy facet whose width is fractionally closer.
    Ok(candidates
        .into_iter()
        .filter(|c| c.0 <= 1.)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .2)
}
fn attach(scene: &mut Scene, parent: usize, child: usize, label: &str) -> Result<(), String> {
    let mut r = Relation::fixed(scene, parent, child, false)?;
    r.label = format!("Sustainer: {label} (estimated)");
    constraints::add(scene, r)
}
fn indices(scene: &Scene, needle: &str) -> Vec<usize> {
    scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.contains(needle))
        .map(|(i, _)| i)
        .collect()
}
fn slots(scene: &Scene, facade: usize, top: f32) -> Vec<f32> {
    let p = &scene.parts[facade];
    let m = p.pose.matrix(0.);
    let mut bins = BTreeMap::<i32, f32>::new();
    for tri in p.mesh.vertices.chunks_exact(3) {
        let a = m.transform_point3(tri[0].position.into());
        let b = m.transform_point3(tri[1].position.into());
        let c = m.transform_point3(tri[2].position.into());
        let n = (b - a).cross(c - a);
        let center = (a + b + c) / 3.;
        if n.y > n.length() * 0.999 && (center.y - (top - 13.5)).abs() < 0.2 {
            *bins.entry(center.x.round() as i32).or_default() += n.length();
        }
    }
    let mut groups = Vec::<(i32, f32, f32)>::new();
    for (x, area) in bins {
        if let Some(last) = groups.last_mut()
            && x - last.0 < 15
        {
            last.0 = x;
            last.1 += x as f32 * area;
            last.2 += area;
        } else {
            groups.push((x, x as f32 * area, area));
        }
    }
    groups
        .into_iter()
        .filter(|g| g.2 > 1.)
        .map(|g| g.1 / g.2)
        .collect()
}
// Cylinder-wall normals identify the lower hinge's two mirrored print poses.
fn lower_axis(part: &Part) -> Vec3 {
    let mut bins = BTreeMap::<(i32, i32), usize>::new();
    for tri in part.mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(tri[0].position);
        let b = Vec3::from(tri[1].position);
        let c = Vec3::from(tri[2].position);
        let n = (b - a).cross(c - a).normalize_or_zero();
        if n.y.abs() < 0.001 && n.x.abs() < 0.98 && n.z.abs() < 0.98 && n.length() > 0.99 {
            for sign in [-1., 1.] {
                let center = (a + b + c) / 3. + n * 7. * sign;
                if center.x > part.mesh.max.x * 0.4 {
                    *bins
                        .entry((
                            (center.x * 5.).round() as i32,
                            (center.z * 5.).round() as i32,
                        ))
                        .or_default() += 1;
                }
            }
        }
    }
    bins.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|((x, z), _)| Vec3::new(x as f32 / 5., 0., z as f32 / 5.))
        .unwrap_or_else(|| raw(part.mesh.max.x - 7., 5.75, 0.))
}

pub fn apply(scene: &mut Scene) -> Result<(), String> {
    let module = indices(scene, "/Modules/")
        .first()
        .copied()
        .ok_or("Sustainer requires a module")?;
    let facade = indices(scene, "Facade.stl")
        .first()
        .copied()
        .ok_or("Sustainer requires a facade")?;
    let housing_q = broad_rotation(&scene.parts[module], scene.parts[facade].source_size.x)?;
    let facade_q = orientation(-Vec3::X, -Vec3::Y, Vec3::Z);
    pose(&mut scene.parts[module], housing_q, Vec3::ZERO);
    let floor = plane_levels(&scene.parts[module], housing_q)
        .iter()
        .take(2)
        .map(|p| p.0)
        .reduce(f32::min)
        .ok_or("Sustainer floor plane not found")?;
    let (module_lo, module_hi) = constraints::world_bounds(&scene.parts[module]);
    let width = module_hi.x - module_lo.x;
    let facade_height = scene.parts[facade].source_size.y;
    let top = floor + facade_height;
    let (_, face_hi) = bounds(&scene.parts[facade], facade_q);
    pose(
        &mut scene.parts[facade],
        facade_q,
        Vec3::new(0., floor + facade_height / 2., module_hi.z + face_hi.z),
    );
    attach(scene, module, facade, "facade / housing")?;
    let mut lock_x = slots(scene, facade, top);
    let lock_count = indices(scene, "/LOCK HOLDER.stl").len();
    if lock_x.len() != lock_count {
        lock_x = if lock_count == 1 {
            vec![0.]
        } else {
            vec![-width / 4., width / 4.]
        };
    }
    let pivot = Vec3::new(0., top, module_lo.z + 23.5);
    let roof: Vec<_> = scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            let n = p.pose.path.rsplit('/').next().unwrap_or("");
            n.contains("Roof") && !n.contains("Connector")
        })
        .map(|(i, _)| i)
        .collect();
    let mut rear = module_lo.z;
    for (slot, &i) in roof.iter().enumerate() {
        let q = broad_rotation(&scene.parts[i], width)?;
        let (lo, hi) = bounds(&scene.parts[i], q);
        let planes = plane_levels(&scene.parts[i], q);
        let underside = planes
            .iter()
            .take(2)
            .map(|p| p.0)
            .reduce(f32::min)
            .ok_or("Roof underside not found")?;
        pose(
            &mut scene.parts[i],
            q,
            Vec3::new(-(lo.x + hi.x) / 2., top - underside, rear - lo.z),
        );
        rear += hi.z - lo.z;
        if slot == 0 {
            let mut r = Relation::hinge(scene, module, i, pivot, -Vec3::X)?;
            r.label = LID.into();
            constraints::add(scene, r)?;
        } else {
            attach(scene, roof[0], i, "split roof / rear half")?;
        }
        scene.parts[i].color = [0.78, 0.82, 0.87, 1.];
    }
    let top_q = orientation(Vec3::Y, Vec3::Z, Vec3::X);
    let top_indices = indices(scene, "Hinge Top.stl");
    let upper_width = top_indices
        .first()
        .map(|&i| scene.parts[i].source_size.z)
        .unwrap_or(62.8);
    for (slot, i) in top_indices.into_iter().enumerate() {
        let x = if upper_width > 50. {
            0.
        } else {
            lock_x.get(slot).copied().unwrap_or(0.)
        };
        anchor(
            &mut scene.parts[i],
            top_q,
            raw(0., -21.5, 0.),
            pivot + Vec3::X * x,
        );
        attach(
            scene,
            roof.first().copied().unwrap_or(module),
            i,
            "upper hinge / lid",
        )?;
    }
    let bottom_indices: Vec<_> = indices(scene, "/Hinges/")
        .into_iter()
        .filter(|&i| !scene.parts[i].pose.path.contains("Top"))
        .collect();
    for (slot, i) in bottom_indices.into_iter().enumerate() {
        let axis = lower_axis(&scene.parts[i]);
        let sign = (-axis.z).signum();
        let q = orientation(Vec3::Y, Vec3::Z * sign, Vec3::X * sign);
        let x = if upper_width > 50. {
            (if slot == 0 { -1. } else { 1. })
                * (upper_width / 2. + scene.parts[i].mesh.size.z / 2. + 0.2)
        } else {
            lock_x.get(slot).copied().unwrap_or(0.)
        };
        anchor(&mut scene.parts[i], q, axis, pivot + Vec3::X * x);
        attach(scene, module, i, "lower hinge / housing")?;
    }
    for (slot, i) in indices(scene, "/Rods/").into_iter().enumerate() {
        let side = if slot == 0 { -1. } else { 1. };
        let center = if upper_width > 50. {
            side * (upper_width / 2. + 20.)
        } else {
            lock_x.get(slot).copied().unwrap_or(0.)
        };
        let q = orientation(Vec3::X * side, Vec3::Y, Vec3::Z * side);
        pose(&mut scene.parts[i], q, pivot + Vec3::X * center);
        attach(scene, module, i, "rod / hinge axis")?;
    }
    let (_, front) = constraints::world_bounds(&scene.parts[facade]);
    let holders = indices(scene, "/LOCK HOLDER.stl");
    for (slot, holder) in holders.into_iter().enumerate() {
        let x = lock_x.get(slot).copied().unwrap_or(0.);
        let center = Vec3::new(x, top - 11.06, front.z + 9.45);
        pose(&mut scene.parts[holder], top_q, center);
        attach(scene, facade, holder, "lock holder / facade")?;
        let small_axis = center + Vec3::new(0., -2.7, 5.8);
        let big_axis = small_axis - Vec3::Y * 15.;
        for i in indices(scene, "/LOCK INNER PART.stl")
            .into_iter()
            .filter(|&i| scene.parts[i].pose.instance as usize == slot)
            .collect::<Vec<_>>()
        {
            let q = orientation(-Vec3::Y, -Vec3::Z, Vec3::X);
            anchor(&mut scene.parts[i], q, raw(-7.5, 0., 0.), small_axis);
            attach(scene, holder, i, "lock inner / small pin axis")?;
        }
        for i in indices(scene, "/LOCK OUTER PART")
            .into_iter()
            .filter(|&i| scene.parts[i].pose.instance as usize == slot)
            .collect::<Vec<_>>()
        {
            let axis = raw(-scene.parts[i].mesh.size.x / 2. + 6., 0., 0.);
            anchor(&mut scene.parts[i], top_q, axis, big_axis);
            attach(scene, holder, i, "closed outer lock / large pin axis")?;
        }
        for i in indices(scene, "/1 SET OF PINS")
            .into_iter()
            .filter(|&i| scene.parts[i].pose.instance as usize == slot)
            .collect::<Vec<_>>()
        {
            let small = scene.parts[i].mesh.size.x < 19.5;
            let side = if scene.parts[i].pose.body.unwrap_or(0).is_multiple_of(2) {
                -1.
            } else {
                1.
            };
            let q = orientation(Vec3::X * side, Vec3::Y, Vec3::Z * side);
            pose(
                &mut scene.parts[i],
                q,
                if small {
                    small_axis + Vec3::X * side * 26.
                } else {
                    big_axis + Vec3::X * side * 22.
                },
            );
            attach(scene, holder, i, "lock pins")?;
        }
    }
    for i in indices(scene, "/Label for Sustainers.stl") {
        pose(
            &mut scene.parts[i],
            facade_q,
            Vec3::new(-width / 4., floor + 12., front.z + 1.),
        );
        attach(scene, facade, i, "blank label / facade")?;
    }
    for i in indices(scene, "/Connector For Split Roof.stl") {
        let q = orientation(Vec3::X, Vec3::Z, -Vec3::Y);
        let seam = roof
            .first()
            .map(|&r| constraints::world_bounds(&scene.parts[r]).1.z)
            .unwrap_or(0.);
        pose(&mut scene.parts[i], q, Vec3::new(0., top + 4., seam));
        attach(
            scene,
            roof.first().copied().unwrap_or(module),
            i,
            "split roof connector",
        )?;
    }
    scene.parts[module].color = [0.53, 0.62, 0.72, 1.];
    scene.parts[facade].color = [0.78, 0.82, 0.87, 1.];
    scene.calibrated = false;
    reset_explosion(scene)
}

pub fn lid_angle(scene: &Scene) -> Option<f32> {
    scene.relations.iter().find_map(|r| {
        if r.label == LID {
            if let Joint::Hinge { angle, .. } = r.joint {
                Some(angle)
            } else {
                None
            }
        } else {
            None
        }
    })
}
pub fn set_lid_angle(scene: &mut Scene, value: f32) -> Result<(), String> {
    let i = scene
        .relations
        .iter()
        .position(|r| r.label == LID)
        .ok_or("Sustainer lid is unavailable")?;
    let Joint::Hinge { angle, .. } = &mut scene.relations[i].joint else {
        return Err("Lid hinge missing".into());
    };
    let previous = *angle;
    *angle = value;
    if let Err(e) = constraints::solve(scene) {
        if let Joint::Hinge { angle, .. } = &mut scene.relations[i].joint {
            *angle = previous;
        }
        return Err(e);
    }
    Ok(())
}
pub fn reset_explosion(scene: &mut Scene) -> Result<(), String> {
    use constraints::PartId;
    let mut travel = BTreeMap::<usize, Vec3>::new();
    for i in 0..scene.parts.len() {
        if !scene
            .relations
            .iter()
            .any(|r| r.child == PartId::of(&scene.parts[i]))
        {
            travel.insert(i, Vec3::ZERO);
        }
    }
    for _ in 0..scene.parts.len() {
        for i in 0..scene.parts.len() {
            if travel.contains_key(&i) {
                continue;
            }
            let Some(r) = scene
                .relations
                .iter()
                .find(|r| r.child == PartId::of(&scene.parts[i]))
            else {
                continue;
            };
            let Some(parent) = scene.parts.iter().position(|p| PartId::of(p) == r.parent) else {
                return Err("Missing explosion parent".into());
            };
            let Some(&base) = travel.get(&parent) else {
                continue;
            };
            let p = &scene.parts[i];
            let name = &p.pose.path;
            let own = if r.label == LID {
                Vec3::Y * 100.
            } else if name.contains("Facade.stl") {
                Vec3::Z * 90.
            } else if name.contains("/Hinges/") {
                Vec3::Y * 35.
            } else if name.contains("/Rods/") {
                Vec3::X * if p.pose.body == Some(0) { -90. } else { 90. }
            } else if name.contains("/Locks/") {
                Vec3::Z * 35.
            } else if name.contains("Label for") {
                Vec3::new(-40., 0., 25.)
            } else {
                Vec3::Y * 25.
            };
            travel.insert(i, base + own);
        }
    }
    if travel.len() != scene.parts.len() {
        return Err("Sustainer explosion hierarchy contains a cycle".into());
    }
    for (i, p) in scene.parts.iter_mut().enumerate() {
        p.pose.travel = travel[&i].to_array();
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
    use crate::catalog::{Catalog, Selection, SustainerBody, SustainerOptions};
    #[test]
    fn sustainer_library_all_scene_datums() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let mut configurations = 0;
        for entry in c.entries.iter().filter(|e| {
            e.path.starts_with(PREFIX) && e.path.contains("/Modules/") && e.name.ends_with(".stl")
        }) {
            let size = entry.path.split('/').nth(2).unwrap();
            let width = size.trim_end_matches(|c: char| c.is_ascii_digit());
            let depth = size
                .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                .parse::<u8>()
                .unwrap();
            let height = entry
                .name
                .split_whitespace()
                .find(|s| ["1F", "2F", "4F"].contains(s))
                .unwrap();
            let body = if entry.name.contains("Sealed") {
                SustainerBody::Sealed
            } else if entry.name.contains("Dividers") {
                SustainerBody::Dividers
            } else {
                SustainerBody::Flat
            };
            for split_roof in if depth >= 6 {
                vec![false, true]
            } else {
                vec![false]
            } {
                let s = Selection {
                    width: width.into(),
                    depth,
                    height: height.into(),
                    hidden_key: false,
                };
                let bom = c
                    .sustainer_bom(&s, SustainerOptions { body, split_roof })
                    .unwrap();
                let scene = Scene::load(&c.root, &bom, "datum sweep".into(), true)
                    .unwrap_or_else(|e| panic!("{size}/{height} {body:?} split={split_roof}: {e}"));
                let module = indices(&scene, "/Modules/")[0];
                let facade = indices(&scene, "Facade.stl")[0];
                let (lo, hi) = constraints::world_bounds(&scene.parts[module]);
                let actual = hi - lo;
                assert!(
                    (actual.x - scene.parts[facade].source_size.x).abs() < 0.3,
                    "Module width: {}",
                    entry.path
                );
                let expected_height = scene.parts[facade].source_size.y
                    + if body == SustainerBody::Sealed {
                        12.3
                    } else {
                        4.8
                    };
                assert!(
                    (actual.y - expected_height).abs() < 0.3,
                    "Module up axis: {} -> {actual:?}",
                    entry.path
                );
                let mut roof_lo = Vec3::splat(f32::INFINITY);
                let mut roof_hi = Vec3::splat(f32::NEG_INFINITY);
                for part in scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.contains(" Roof") && !p.pose.path.contains("Connector"))
                {
                    let (a, b) = constraints::world_bounds(part);
                    roof_lo = roof_lo.min(a);
                    roof_hi = roof_hi.max(b);
                }
                // L7 supplied roofs include a measured ~0.35 mm lateral
                // protrusion. Preserve it; bounds agreement is not a mating fit.
                assert!(
                    (roof_hi.x - roof_lo.x - actual.x).abs() < 0.5,
                    "Roof width: {size}/{height} {body:?} split={split_roof}: {} vs {}",
                    roof_hi.x - roof_lo.x,
                    actual.x
                );
                assert!(
                    (roof_hi.z - roof_lo.z - actual.z - 10.).abs() < 0.3,
                    "Roof depth: {size}/{height}"
                );
                configurations += 1;
            }
        }
        assert_eq!(configurations, 126);
        println!(
            "all {configurations} Sustainer scenes loaded; housing up axes and cover extents checked"
        );
    }
    #[test]
    fn sustainer_library_graph_lid_motion_and_roundtrip() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        for (width, depth, height, body, split_roof) in [
            ("S", 3, "1F", SustainerBody::Flat, false),
            ("M", 4, "2F", SustainerBody::Flat, false),
            ("L", 3, "1F", SustainerBody::Flat, false),
            ("L", 4, "2F", SustainerBody::Sealed, false),
            ("L", 6, "4F", SustainerBody::Dividers, true),
            ("S", 7, "4F", SustainerBody::Flat, true),
            ("XL", 7, "1F", SustainerBody::Flat, true),
        ] {
            let selection = Selection {
                width: width.into(),
                depth,
                height: height.into(),
                hidden_key: false,
            };
            let bom = c
                .sustainer_bom(&selection, SustainerOptions { body, split_roof })
                .unwrap();
            let mut scene =
                Scene::load(&c.root, &bom, "Sustainer regression".into(), true).unwrap();
            assert!(!scene.calibrated);
            assert_eq!(
                scene.relations.len() + 1,
                scene.parts.len(),
                "{width}{depth}/{height}"
            );
            assert_eq!(
                scene
                    .relations
                    .iter()
                    .filter(|r| matches!(r.joint, Joint::Hinge { .. }))
                    .count(),
                1
            );
            assert_eq!(indices(&scene, "/Rods/").len(), 2);
            assert_eq!(
                indices(&scene, "/1 SET OF PINS").len(),
                if matches!(width, "S" | "M") { 4 } else { 8 }
            );
            let closed: Vec<_> = scene.parts.iter().map(|p| p.pose.matrix(0.)).collect();
            let roof = scene
                .parts
                .iter()
                .position(|p| p.pose.path.contains(" Roof") && !p.pose.path.contains("Connector"))
                .unwrap();
            let module = indices(&scene, "/Modules/")[0];
            let (lo, hi) = constraints::world_bounds(&scene.parts[roof]);
            let (ml, mh) = constraints::world_bounds(&scene.parts[module]);
            assert!(
                ((hi.x - lo.x) - (mh.x - ml.x)).abs() < 0.5,
                "Roof width mismatch: {width}{depth}/{height} roof={} housing={}",
                hi.x - lo.x,
                mh.x - ml.x
            );
            set_lid_angle(&mut scene, 75.).unwrap();
            assert!(scene.parts[roof].pose.position[1] > closed[roof].w_axis.y + 10.);
            assert!(
                scene.parts[module]
                    .pose
                    .matrix(0.)
                    .abs_diff_eq(closed[module], 0.0001)
            );
            for i in indices(&scene, "/Locks/") {
                assert!(
                    scene.parts[i]
                        .pose
                        .matrix(0.)
                        .abs_diff_eq(closed[i], 0.0001)
                );
            }
            let saved: crate::scene::Definition =
                toml::from_str(&toml::to_string(&scene.definition()).unwrap()).unwrap();
            let mut restored = Scene::load(&c.root, &bom, "roundtrip".into(), true).unwrap();
            restored.apply(&saved);
            assert_eq!(lid_angle(&restored), Some(75.));
            for (a, b) in restored.parts.iter().zip(&scene.parts) {
                assert!(a.pose.matrix(0.).abs_diff_eq(b.pose.matrix(0.), 0.001));
            }
            assert!(set_lid_angle(&mut scene, f32::NAN).is_err());
            assert_eq!(lid_angle(&scene), Some(75.));
            set_lid_angle(&mut scene, 0.).unwrap();
            for (part, matrix) in scene.parts.iter().zip(closed) {
                assert!(part.pose.matrix(0.).abs_diff_eq(matrix, 0.001));
            }
            println!(
                "assembled {width}{depth}/{height} {body:?} split={split_roof}: {} display parts",
                scene.parts.len()
            );
        }
    }
}
