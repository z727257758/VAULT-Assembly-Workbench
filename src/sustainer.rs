//! Display assembly for Sustainers. Meshes remain immutable. Surface datums
//! recover print rotations and mating features. This is mesh datum positioning,
//! not a manually verified CAD or manufacturing assembly.
use crate::{
    constraints::{self, Joint, MateCheck, MateMeasure, Relation},
    display::{self, CircleDatum, PlaneDatum},
    scene::{Part, Scene},
};
use glam::{DVec3, Mat3, Mat4, Quat, Vec3};
use std::collections::BTreeMap;

pub const PREFIX: &str = "02. SUSTAINERS/";
pub const LID: &str = "Sustainer datum: lid / paired hinge axes";
pub const UPPER_SEAT: &str = "Sustainer datum: upper hinge / roof seat / inside-seat-v5";
pub const LOCK_INNER: &str = "Sustainer datum: inner lock / small pins / inside-seat-v5";
pub const LOCK_OUTER: &str = "Sustainer datum: outer lock / large pins / inside-seat-v5";

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

fn indices(scene: &Scene, needle: &str) -> Vec<usize> {
    scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.contains(needle))
        .map(|(i, _)| i)
        .collect()
}
fn raw_frame() -> Mat4 {
    Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
}
fn wide_window() -> [Vec3; 2] {
    [Vec3::splat(f32::NEG_INFINITY), Vec3::splat(f32::INFINITY)]
}
fn unit(axis: usize) -> Vec3 {
    let mut p = Vec3::ZERO;
    p[axis] = 1.;
    p
}
fn holes(
    part: &Part,
    matrix: Mat4,
    axis: usize,
    levels: [f32; 2],
    radii: [f32; 2],
) -> Result<Vec<CircleDatum>, String> {
    let first = display::section_holes(&part.mesh, matrix, axis, levels[0], radii);
    let second = display::section_holes(&part.mesh, matrix, axis, levels[1], radii);
    let mut result = Vec::new();
    for a in first {
        if let Some(b) = second.iter().min_by(|b, c| {
            let mut db = b.point - a.point;
            db[axis] = 0.;
            let mut dc = c.point - a.point;
            dc[axis] = 0.;
            db.length_squared().total_cmp(&dc.length_squared())
        }) {
            let mut drift = b.point - a.point;
            drift[axis] = 0.;
            if drift.length() < 0.06 {
                result.push(CircleDatum {
                    point: (a.point + b.point) / 2.,
                    radius: (a.radius + b.radius) / 2.,
                    residual: a.residual.max(b.residual),
                });
            }
        }
    }
    if result.is_empty() {
        return Err(format!(
            "{}: no matching bores at two sections on axis {axis}",
            part.pose.path
        ));
    }
    result.sort_by(|a, b| {
        a.point
            .x
            .total_cmp(&b.point.x)
            .then(a.point.y.total_cmp(&b.point.y))
            .then(a.point.z.total_cmp(&b.point.z))
    });
    Ok(result)
}
fn plate_holes(part: &Part, axis: usize, level: f32) -> Result<(f32, Vec<CircleDatum>), String> {
    for sign in [1., -1.] {
        if let Ok(found) = holes(
            part,
            raw_frame(),
            axis,
            [sign * level, sign * (level + 0.6)],
            [1.9, 2.3],
        ) && found.len() == 4
        {
            return Ok((sign, found));
        }
    }
    Err(format!("{}: four mounting holes not found", part.pose.path))
}
fn orient_holes(points: &[CircleDatum], matrix: Mat4) -> Vec<CircleDatum> {
    points
        .iter()
        .map(|p| CircleDatum {
            point: matrix.transform_point3(p.point),
            ..p.clone()
        })
        .collect()
}
fn window(points: &[CircleDatum], axis: usize) -> [Vec3; 2] {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for p in points {
        lo = lo.min(p.point);
        hi = hi.max(p.point);
    }
    lo -= Vec3::splat(5.);
    hi += Vec3::splat(5.);
    lo[axis] = f32::NEG_INFINITY;
    hi[axis] = f32::INFINITY;
    [lo, hi]
}
fn groups(
    points: &[CircleDatum],
    count: usize,
    label: &str,
) -> Result<Vec<Vec<CircleDatum>>, String> {
    if points.len() != count * 4 {
        return Err(format!(
            "{label}: expected {} mounting holes, found {}",
            count * 4,
            points.len()
        ));
    }
    let mut points = points.to_vec();
    points.sort_by(|a, b| a.point.x.total_cmp(&b.point.x));
    Ok(points.chunks_exact(4).map(|g| g.to_vec()).collect())
}
struct Mount {
    holes: Vec<CircleDatum>,
    plane: PlaneDatum,
    axis: usize,
}
fn record(
    scene: &mut Scene,
    pair: [usize; 2],
    label: &str,
    points: [Vec3; 2],
    axis: Vec3,
    measure: MateMeasure,
    tolerance: f32,
) {
    let check = MateCheck::world(scene, pair, label, points, axis, measure, tolerance);
    scene.mating_checks.push(check);
}
fn mount(
    scene: &mut Scene,
    pair: [usize; 2],
    q: Quat,
    parent: &Mount,
    child: &Mount,
    label: &str,
) -> Result<(), String> {
    if parent.plane.area < 1. || child.plane.area < 1. {
        return Err(format!("{label}: mounting plane patch too small"));
    }
    if parent.holes.len() != child.holes.len() {
        return Err(format!("{label}: mounting pattern count differs"));
    }
    let mut t = parent.holes.iter().map(|p| p.point).sum::<Vec3>() / parent.holes.len() as f32
        - child.holes.iter().map(|p| p.point).sum::<Vec3>() / child.holes.len() as f32;
    t[parent.axis] = parent.plane.level - child.plane.level;
    pose(&mut scene.parts[pair[1]], q, t);
    for (i, p) in child.holes.iter().enumerate() {
        let b = p.point + t;
        let a = parent
            .holes
            .iter()
            .min_by(|a, c| {
                let mut da = a.point - b;
                da[parent.axis] = 0.;
                let mut dc = c.point - b;
                dc[parent.axis] = 0.;
                da.length_squared().total_cmp(&dc.length_squared())
            })
            .unwrap()
            .point;
        let mut error = b - a;
        error[parent.axis] = 0.;
        if error.length() > 2. {
            return Err(format!(
                "{label}: hole {} differs by {:.4} mm",
                i + 1,
                error.length()
            ));
        }
        record(
            scene,
            pair,
            &format!("{label}: fastener {}", i + 1),
            [a, b],
            unit(parent.axis),
            MateMeasure::Coaxial,
            0.2,
        );
    }
    record(
        scene,
        pair,
        &format!("{label}: seating face"),
        [parent.plane.center, child.plane.center + t],
        unit(parent.axis),
        MateMeasure::Plane { gap: 0. },
        0.03,
    );
    Ok(())
}
struct Edge {
    parent: usize,
    child: usize,
    label: String,
    hinge: Option<(Vec3, Vec3)>,
}
fn edge(edges: &mut Vec<Edge>, parent: usize, child: usize, label: &str) {
    edges.push(Edge {
        parent,
        child,
        label: format!("Sustainer datum: {label}"),
        hinge: None,
    });
}
fn hinge(edges: &mut Vec<Edge>, parent: usize, child: usize, label: &str, point: Vec3, axis: Vec3) {
    edges.push(Edge {
        parent,
        child,
        label: label.into(),
        hinge: Some((point, axis)),
    });
}
fn axis_point(scene: &Scene, i: usize, datum: &CircleDatum) -> Vec3 {
    scene.parts[i].pose.matrix(0.).transform_point3(raw(
        datum.point.x,
        datum.point.y,
        datum.point.z,
    ))
}
fn section_crossings(part: &Part, y: f32, z: f32) -> Vec<f32> {
    let mut crossings = Vec::new();
    for [a, b] in display::section_segments(&part.mesh, part.pose.matrix(0.), 1, y) {
        if (a.z - z) * (b.z - z) < 0. {
            crossings.push(a.x + (b.x - a.x) * (z - a.z) / (b.z - a.z));
        }
    }
    crossings.sort_by(f32::total_cmp);
    crossings.dedup_by(|a, b| (*a - *b).abs() < 0.02);
    crossings
}
fn socket_intervals(part: &Part, y: f32, z: f32) -> Result<Vec<[f32; 2]>, String> {
    let crossings = section_crossings(part, y, z);
    if crossings.len() < 4 || !crossings.len().is_multiple_of(2) {
        return Err("Split roof socket section is incomplete".into());
    }
    let slots: Vec<_> = (1..crossings.len() - 1)
        .step_by(2)
        .map(|i| [crossings[i], crossings[i + 1]])
        .filter(|p| p[1] - p[0] > 1.)
        .collect();
    Ok(slots)
}

pub fn apply(scene: &mut Scene) -> Result<(), String> {
    scene.mating_checks.clear();
    let mut edges = Vec::new();
    let module = *indices(scene, "/Modules/")
        .first()
        .ok_or("Sustainer module missing")?;
    let facade = *indices(scene, "Facade.stl")
        .first()
        .ok_or("Sustainer facade missing")?;
    let mq = broad_rotation(&scene.parts[module], scene.parts[facade].source_size.x)?;
    pose(&mut scene.parts[module], mq, Vec3::ZERO);
    let (ml, mh) = constraints::world_bounds(&scene.parts[module]);
    let width = mh.x - ml.x;
    let floor = plane_levels(&scene.parts[module], mq)
        .iter()
        .take(2)
        .map(|p| p.0)
        .reduce(f32::min)
        .ok_or("Module floor missing")?;
    let fq = orientation(-Vec3::X, -Vec3::Y, Vec3::Z);
    let fm = Mat4::from_quat(fq);
    let parent_front = holes(
        &scene.parts[module],
        scene.parts[module].pose.matrix(0.),
        2,
        [mh.z - 2.1, mh.z - 3.2],
        [1.5, 2.2],
    )?;
    let face_holes = holes(&scene.parts[facade], fm, 2, [-0.2, 0.3], [1.5, 2.3])?;
    let rough = Vec3::new(
        (ml.x + mh.x) / 2.,
        floor + scene.parts[facade].source_size.y / 2.,
        0.,
    );
    let mut targets = Vec::new();
    let mut moving = Vec::new();
    for p in &parent_front {
        if let Some(c) = face_holes.iter().min_by(|a, b| {
            let mut da = a.point + rough - p.point;
            da.z = 0.;
            let mut db = b.point + rough - p.point;
            db.z = 0.;
            da.length_squared().total_cmp(&db.length_squared())
        }) {
            let mut d = c.point + rough - p.point;
            d.z = 0.;
            if d.length() < 1. {
                targets.push(p.clone());
                moving.push(c.clone());
            }
        }
    }
    if targets.len() < 4 {
        return Err("Facade / module fastener pattern missing".into());
    }
    let parent_plane = display::plane_patch(
        &scene.parts[module].mesh,
        scene.parts[module].pose.matrix(0.),
        2,
        1.,
        mh.z,
        window(&targets, 2),
        0.05,
    )?;
    let child_plane = display::plane_patch(
        &scene.parts[facade].mesh,
        fm,
        2,
        -1.,
        -5.,
        wide_window(),
        0.05,
    )?;
    mount(
        scene,
        [module, facade],
        fq,
        &Mount {
            holes: targets,
            plane: parent_plane,
            axis: 2,
        },
        &Mount {
            holes: moving,
            plane: child_plane,
            axis: 2,
        },
        "facade / module",
    )?;
    edge(
        &mut edges,
        module,
        facade,
        "facade / module mounting pattern",
    );

    let back = holes(
        &scene.parts[module],
        scene.parts[module].pose.matrix(0.),
        2,
        [ml.z + 3.3, ml.z + 4.6],
        [1.5, 2.1],
    )?;
    let lower_indices: Vec<_> = indices(scene, "/Hinges/")
        .into_iter()
        .filter(|&i| !scene.parts[i].pose.path.contains("Top"))
        .collect();
    let back_groups = groups(&back, lower_indices.len(), "lower hinges")?;
    let mut lower = Vec::new();
    for (slot, i) in lower_indices.into_iter().enumerate() {
        let (sign, raw_holes) = plate_holes(&scene.parts[i], 1, 10.)?;
        let q = orientation(Vec3::Y, Vec3::Z * sign, Vec3::X * sign);
        let m = Mat4::from_quat(q);
        let child_holes = orient_holes(&raw_holes, m * raw_frame().inverse());
        let parent_plane = display::plane_patch(
            &scene.parts[module].mesh,
            scene.parts[module].pose.matrix(0.),
            2,
            1.,
            ml.z + 6.9,
            window(&back_groups[slot], 2),
            0.3,
        )?;
        let child_plane = display::plane_patch(
            &scene.parts[i].mesh,
            m,
            2,
            -1.,
            8.75,
            window(&child_holes, 2),
            0.08,
        )?;
        mount(
            scene,
            [module, i],
            q,
            &Mount {
                holes: back_groups[slot].clone(),
                plane: parent_plane,
                axis: 2,
            },
            &Mount {
                holes: child_holes,
                plane: child_plane,
                axis: 2,
            },
            "lower hinge / housing",
        )?;
        let datum = display::cylinder_axis(
            &scene.parts[i].mesh,
            raw_frame(),
            2,
            Vec3::new(scene.parts[i].mesh.max.x - 7., -sign * 5.75, 0.),
            4.15,
        )?;
        let range = display::cylinder_limits(&scene.parts[i].mesh, raw_frame(), 2, &datum)?;
        lower.push((i, datum, range));
        edge(&mut edges, module, i, "lower hinge / back-wall seat");
    }
    let lower_mean = lower
        .iter()
        .map(|(i, d, _)| axis_point(scene, *i, d))
        .sum::<Vec3>()
        / lower.len() as f32;

    let roof: Vec<_> = scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            let n = p.pose.path.rsplit('/').next().unwrap_or("");
            n.contains(" Roof") && !n.contains("Connector")
        })
        .map(|(i, _)| i)
        .collect();
    let mut rear = ml.z;
    let mut primary = roof[0];
    let mut primary_holes = Vec::new();
    let mut outer_world = 0.;
    for (slot, &i) in roof.iter().enumerate() {
        let q = broad_rotation(&scene.parts[i], width)?;
        let m = Mat4::from_quat(q);
        let (lo, hi) = bounds(&scene.parts[i], q);
        let levels = plane_levels(&scene.parts[i], q);
        let inside = levels
            .iter()
            .take(2)
            .map(|v| v.0)
            .reduce(f32::min)
            .ok_or("Roof inner plane missing")?;
        let outside = levels.iter().take(2).map(|v| v.0).reduce(f32::max).unwrap();
        let outer =
            display::plane_patch(&scene.parts[i].mesh, m, 1, 1., outside, wide_window(), 0.05)?;
        let y = if slot == 0 {
            floor + scene.parts[facade].source_size.y - inside
        } else {
            outer_world - outer.level
        };
        pose(
            &mut scene.parts[i],
            q,
            Vec3::new((ml.x + mh.x - lo.x - hi.x) / 2., y, rear - lo.z),
        );
        rear += hi.z - lo.z;
        if slot == 0 {
            outer_world = outer.level + y;
        }
        let level = (inside + outside) / 2. + y;
        if let Ok(found) = holes(
            &scene.parts[i],
            scene.parts[i].pose.matrix(0.),
            1,
            [level - 0.4, level + 0.4],
            [1.5, 2.1],
        ) && found.len() > primary_holes.len()
        {
            primary = i;
            primary_holes = found;
        }
        scene.parts[i].color = [0.78, 0.82, 0.87, 1.];
    }
    let upper_indices = indices(scene, "Hinge Top.stl");
    let upper_groups = groups(&primary_holes, upper_indices.len(), "upper hinges")?;
    let mut upper = Vec::new();
    let (_, primary_q, _) = scene.parts[primary]
        .pose
        .matrix(0.)
        .to_scale_rotation_translation();
    let rp = plane_levels(&scene.parts[primary], primary_q)
        .iter()
        .take(2)
        .map(|p| p.0)
        .reduce(f32::min)
        .unwrap()
        + scene.parts[primary].pose.position[1];
    for (slot, i) in upper_indices.into_iter().enumerate() {
        let (sign, raw_holes) = plate_holes(&scene.parts[i], 0, 2.)?;
        // Plate lies above the shaft. The raw |X|=7 rim seats against the roof;
        // its recessed |X|=4.95 face clears the roof pad, unlike the inverted pose.
        let q = orientation(Vec3::Y * sign, Vec3::Z, Vec3::X * sign);
        let m = Mat4::from_quat(q);
        let child_holes = orient_holes(&raw_holes, m * raw_frame().inverse());
        let parent_plane = display::plane_patch(
            &scene.parts[primary].mesh,
            scene.parts[primary].pose.matrix(0.),
            1,
            -1.,
            rp,
            window(&upper_groups[slot], 1),
            0.08,
        )?;
        let child_plane = display::plane_patch(
            &scene.parts[i].mesh,
            m,
            1,
            1.,
            7.,
            window(&child_holes, 1),
            0.05,
        )?;
        mount(
            scene,
            [primary, i],
            q,
            &Mount {
                holes: upper_groups[slot].clone(),
                plane: parent_plane,
                axis: 1,
            },
            &Mount {
                holes: child_holes,
                plane: child_plane,
                axis: 1,
            },
            "upper hinge / roof",
        )?;
        let datum = display::cylinder_axis(
            &scene.parts[i].mesh,
            raw_frame(),
            2,
            Vec3::new(0., -21.5, 0.),
            4.15,
        )?;
        upper.push((i, datum));
        edge(
            &mut edges,
            primary,
            i,
            "upper hinge / roof seat / inside-seat-v5",
        );
    }
    let upper_mean = upper
        .iter()
        .map(|(i, d)| axis_point(scene, *i, d))
        .sum::<Vec3>()
        / upper.len() as f32;
    let delta = lower_mean - upper_mean;
    for i in roof.iter().copied().chain(upper.iter().map(|(i, _)| *i)) {
        scene.parts[i].pose.position =
            (Vec3::from(scene.parts[i].pose.position) + delta).to_array();
    }
    let pivot = Vec3::new(0., lower_mean.y, lower_mean.z);
    hinge(&mut edges, module, primary, LID, pivot, -Vec3::X);
    for &i in &roof {
        if i != primary {
            edge(&mut edges, primary, i, "split roof / rear panel");
        }
    }
    for (slot, (i, d, _)) in lower.iter().enumerate() {
        let (j, e) = &upper[if upper.len() == 1 { 0 } else { slot }];
        record(
            scene,
            [*i, *j],
            "upper / lower hinge shaft line",
            [axis_point(scene, *i, d), axis_point(scene, *j, e)],
            Vec3::X,
            MateMeasure::Coaxial,
            0.02,
        );
        let a = axis_point(scene, *i, d);
        let b = axis_point(scene, *j, e);
        let side = if a.x < b.x { 1. } else { -1. };
        let mut ends = Vec::new();
        for (index, sign, line) in [(*i, side, a), (*j, -side, b)] {
            let (lo, hi) = constraints::world_bounds(&scene.parts[index]);
            let seed = if sign > 0. { hi.x } else { lo.x };
            let mut region = [line - Vec3::splat(7.), line + Vec3::splat(7.)];
            region[0].x = f32::NEG_INFINITY;
            region[1].x = f32::INFINITY;
            ends.push(display::plane_patch(
                &scene.parts[index].mesh,
                scene.parts[index].pose.matrix(0.),
                0,
                sign,
                seed,
                region,
                0.08,
            )?);
        }
        let gap = (ends[1].level - ends[0].level) * side;
        let mut check = MateCheck::world(
            scene,
            [*i, *j],
            &format!("hinge barrel end planes; signed axial gap {gap:.3} mm"),
            [ends[0].center, ends[1].center],
            Vec3::X * side,
            MateMeasure::Clearance { min: 0. },
            0.03,
        );
        // Source M patterns disagree by 0.25 mm in pitch. Keep the measured
        // screw pattern; expose the end-plane discrepancy for physical review.
        check.required = false;
        scene.mating_checks.push(check);
    }
    for (slot, i) in indices(scene, "/Rods/").into_iter().enumerate() {
        let (parent, bore, range) = &lower[slot];
        let line = axis_point(scene, *parent, bore);
        let d = display::cylinder_axis(
            &scene.parts[i].mesh,
            raw_frame(),
            0,
            Vec3::new(0., 0., -2.675),
            3.95,
        )?;
        let stations = display::cylinder_crossings(
            &scene.parts[i].mesh,
            raw_frame(),
            0,
            d.point,
            bore.radius,
        )?;
        let end_points = range.map(|z| {
            scene.parts[*parent].pose.matrix(0.).transform_point3(raw(
                bore.point.x,
                bore.point.y,
                z,
            ))
        });
        let station = if slot == 0 {
            end_points[0].x.min(end_points[1].x)
        } else {
            end_points[0].x.max(end_points[1].x)
        };
        let rod_station = if slot == 0 { stations[1] } else { stations[0] };
        let q = orientation(Vec3::X, -Vec3::Z, Vec3::Y);
        let local = raw(rod_station, d.point.y, d.point.z);
        let target = Vec3::new(station, line.y, line.z);
        anchor(&mut scene.parts[i], q, local, target);
        record(
            scene,
            [*parent, i],
            &format!(
                "hinge rod / bore axis; radial gap {:.3} mm",
                bore.radius - d.radius
            ),
            [line, axis_point(scene, i, &d)],
            Vec3::X,
            MateMeasure::Coaxial,
            0.02,
        );
        record(
            scene,
            [*parent, i],
            "rod rounded head / cylinder land",
            [
                target,
                scene.parts[i].pose.matrix(0.).transform_point3(local),
            ],
            Vec3::X,
            MateMeasure::Point,
            0.03,
        );
        edge(&mut edges, *parent, i, "rod / lower hinge cylinder land");
    }

    let holders = indices(scene, "/LOCK HOLDER.stl");
    let lock_holes = holes(
        &scene.parts[facade],
        scene.parts[facade].pose.matrix(0.),
        2,
        [
            scene.parts[facade].pose.position[2] - 0.2,
            scene.parts[facade].pose.position[2] + 0.3,
        ],
        [1.7, 1.95],
    )?;
    let lock_groups = groups(&lock_holes, holders.len(), "lock holders")?;
    for (slot, holder) in holders.into_iter().enumerate() {
        // The plate's engraved UP arrow points along raw -X. Its symmetric
        // four-hole pattern also fits upside down, so holes alone cannot orient it.
        let q = orientation(-Vec3::Y, Vec3::Z, -Vec3::X);
        let m = Mat4::from_quat(q);
        let child_holes = holes(&scene.parts[holder], m, 2, [-8.2, -6.8], [1.6, 2.1])?;
        let pp = display::plane_patch(
            &scene.parts[facade].mesh,
            scene.parts[facade].pose.matrix(0.),
            2,
            -1.,
            // Manual page 2: the screw-head flange sits inside the facade.
            scene.parts[facade].pose.position[2] - 5.,
            window(&lock_groups[slot], 2),
            0.05,
        )?;
        let cp = display::plane_patch(
            &scene.parts[holder].mesh,
            m,
            2,
            1.,
            -4.45,
            window(&child_holes, 2),
            0.05,
        )?;
        mount(
            scene,
            [facade, holder],
            q,
            &Mount {
                holes: lock_groups[slot].clone(),
                plane: pp,
                axis: 2,
            },
            &Mount {
                holes: child_holes,
                plane: cp,
                axis: 2,
            },
            "lock holder / facade inside wall",
        )?;
        edge(
            &mut edges,
            facade,
            holder,
            "lock holder / facade inside wall",
        );
        let holder_axis = display::cylinder_axis(
            &scene.parts[holder].mesh,
            raw_frame(),
            2,
            Vec3::new(-2.7, 5.75, 0.),
            2.95,
        )?;
        let small_axis = axis_point(scene, holder, &holder_axis);
        let inner = *indices(scene, "/LOCK INNER PART.stl")
            .iter()
            .find(|&&i| scene.parts[i].pose.instance as usize == slot)
            .ok_or("Lock inner missing")?;
        let small = display::cylinder_axis(
            &scene.parts[inner].mesh,
            raw_frame(),
            2,
            Vec3::new(7.5, 0., 0.),
            2.85,
        )?;
        let big = display::cylinder_axis(
            &scene.parts[inner].mesh,
            raw_frame(),
            2,
            Vec3::new(-7.5, 0., 0.),
            2.95,
        )?;
        let outer = *indices(scene, "/LOCK OUTER PART")
            .iter()
            .find(|&&i| scene.parts[i].pose.instance as usize == slot)
            .ok_or("Lock outer missing")?;
        let od = display::cylinder_axis(
            &scene.parts[outer].mesh,
            raw_frame(),
            2,
            Vec3::new(scene.parts[outer].mesh.min.x + 10.3, 3.8, 0.),
            2.95,
        )?;
        // Match the two end lugs to the holder's central barrel. The other
        // inner bore is a continuous tube and mates to the outer lock's lugs.
        let recipe = constraints::latch_display_pose(
            scene.parts[outer].pose.path.contains("2-4F"),
            scene.parts[module].pose.path.contains("/XL4/"),
        );
        let base_q = orientation(Vec3::Y, Vec3::Z, Vec3::X);
        let iq = Quat::from_rotation_x(recipe.inner_degrees.to_radians()) * base_q;
        anchor(
            &mut scene.parts[inner],
            iq,
            raw(small.point.x, small.point.y, 0.),
            small_axis,
        );
        hinge(&mut edges, holder, inner, LOCK_INNER, small_axis, Vec3::X);
        record(
            scene,
            [holder, inner],
            "holder / inner lock pin axis",
            [small_axis, axis_point(scene, inner, &small)],
            Vec3::X,
            MateMeasure::Coaxial,
            0.02,
        );
        let big_axis = axis_point(scene, inner, &big);
        let oq = Quat::from_rotation_x(recipe.outer_degrees.to_radians()) * base_q;
        anchor(
            &mut scene.parts[outer],
            oq,
            raw(od.point.x, od.point.y, 0.),
            big_axis,
        );
        hinge(&mut edges, inner, outer, LOCK_OUTER, big_axis, Vec3::X);
        record(
            scene,
            [inner, outer],
            "inner / outer lock pin axis",
            [big_axis, axis_point(scene, outer, &od)],
            Vec3::X,
            MateMeasure::Coaxial,
            0.02,
        );
        for i in indices(scene, "/1 SET OF PINS")
            .into_iter()
            .filter(|&i| scene.parts[i].pose.instance as usize == slot)
            .collect::<Vec<_>>()
        {
            let small_pin = scene.parts[i].mesh.size.x < 19.5;
            let side = if scene.parts[i].pose.body.unwrap_or(0).is_multiple_of(2) {
                -1.
            } else {
                1.
            };
            let (bearing, line) = if small_pin {
                (inner, small_axis)
            } else {
                (outer, big_axis)
            };
            let (blo, bhi) = constraints::world_bounds(&scene.parts[bearing]);
            let seed = if side < 0. { blo.x } else { bhi.x };
            let mut win = [line - Vec3::splat(5.), line + Vec3::splat(5.)];
            win[0].x = f32::NEG_INFINITY;
            win[1].x = f32::INFINITY;
            let mouth = display::plane_patch(
                &scene.parts[bearing].mesh,
                scene.parts[bearing].pose.matrix(0.),
                0,
                side,
                seed,
                win,
                0.1,
            )?;
            let (cap_frame, cap) = display::elastic_pin_entry(&scene.parts[i].mesh)?;
            let inward = -side;
            let raw_to_world = Quat::from_mat3(&Mat3::from_cols(
                Vec3::X * inward,
                -Vec3::Z * inward,
                Vec3::Y,
            ));
            let pin_q = raw_to_world * cap_frame.to_scale_rotation_translation().1;
            let local = cap_frame.inverse().transform_point3(cap.point);
            let target = Vec3::new(mouth.level, line.y, line.z);
            anchor(&mut scene.parts[i], pin_q, local, target);
            record(
                scene,
                [bearing, i],
                "pin insertion face / bearing mouth (relaxed display approximation)",
                [
                    target,
                    scene.parts[i].pose.matrix(0.).transform_point3(local),
                ],
                Vec3::X,
                MateMeasure::Point,
                0.03,
            );
            edge(
                &mut edges,
                if small_pin { holder } else { inner },
                i,
                if small_pin {
                    "small elastic pin / holder"
                } else {
                    "large elastic pin / inner lock"
                },
            );
            scene.parts[i].mesh =
                display::installed_pin_display(&scene.parts[i].mesh, cap_frame, &cap)?;
        }
    }
    for i in indices(scene, "/Label for Sustainers.stl") {
        let pp = display::plane_patch(
            &scene.parts[facade].mesh,
            scene.parts[facade].pose.matrix(0.),
            2,
            1.,
            scene.parts[facade].pose.position[2] + 2.87,
            wide_window(),
            0.03,
        )?;
        let cp = display::plane_patch(
            &scene.parts[i].mesh,
            fm,
            2,
            -1.,
            -1.015,
            wide_window(),
            0.03,
        )?;
        let mut seat = pp.center;
        let mut back = cp.center;
        let profile_difference = (pp.max - pp.min).x - (cp.max - cp.min).x;
        if profile_difference > 0.2 {
            // 2F/4F facades use a longer D-ended, side-entry recess. Place
            // the plate within its rectangular land, before the round end.
            // This does not assert a unique lateral stop or a slide fit.
            let height = pp.max.y - pp.min.y;
            let right = pp.max.x - height / 2.;
            if right - pp.min.x < cp.max.x - cp.min.x - 0.1 {
                return Err("Label rectangular seating land is too short".into());
            }
            seat.x = (pp.min.x + right) / 2.;
            seat.y = (pp.min.y + pp.max.y) / 2.;
            back.x = (cp.min.x + cp.max.x) / 2.;
            back.y = (cp.min.y + cp.max.y) / 2.;
        } else if profile_difference.abs() > 0.2 {
            return Err("Label back profile is wider than facade slot".into());
        }
        pose(&mut scene.parts[i], fq, seat - back);
        record(
            scene,
            [facade, i],
            "label back / measured facade slot seating plane",
            [seat, back + Vec3::from(scene.parts[i].pose.position)],
            Vec3::Z,
            MateMeasure::Point,
            0.03,
        );
        edge(&mut edges, facade, i, "blank label / measured facade slot");
    }

    let connectors = indices(scene, "/Connector For Split Roof.stl");
    if !connectors.is_empty() {
        let secondary = *roof
            .iter()
            .find(|&&i| i != primary)
            .ok_or("Split roof front panel missing")?;
        let seam = constraints::world_bounds(&scene.parts[primary]).1.z;
        let primary_matrix = scene.parts[primary].pose.matrix(0.);
        let outer_seed = outer_world + delta.y;
        let ceil = display::plane_patch(
            &scene.parts[primary].mesh,
            primary_matrix,
            1,
            -1.,
            outer_seed - 2.,
            wide_window(),
            0.06,
        )?;
        let bottom = display::plane_patch(
            &scene.parts[primary].mesh,
            primary_matrix,
            1,
            1.,
            outer_seed - 6.,
            wide_window(),
            0.06,
        )?;
        let mid = (ceil.level + bottom.level) / 2.;
        let sockets = socket_intervals(&scene.parts[primary], mid, seam - 0.5)?;
        let other = socket_intervals(&scene.parts[secondary], mid, seam + 0.5)?;
        if sockets.len() != connectors.len() || other.len() != sockets.len() {
            return Err(format!(
                "Split roof has {} sockets, BOM has {} connectors",
                sockets.len(),
                connectors.len()
            ));
        }
        let dx = sockets.iter().map(|p| (p[0] + p[1]) / 2.).sum::<f32>() / sockets.len() as f32
            - other.iter().map(|p| (p[0] + p[1]) / 2.).sum::<f32>() / other.len() as f32;
        scene.parts[secondary].pose.position[0] += dx;
        let other = socket_intervals(&scene.parts[secondary], mid, seam + 0.5)?;
        for (slot, i) in connectors.into_iter().enumerate() {
            let center = (sockets[slot][0] + sockets[slot][1]) / 2.;
            let other_center = (other[slot][0] + other[slot][1]) / 2.;
            if (center - other_center).abs() > 0.03 {
                return Err("Split roof socket halves do not align".into());
            }
            let q = orientation(Vec3::X, Vec3::Z, -Vec3::Y);
            let (lo, hi) = bounds(&scene.parts[i], q);
            pose(
                &mut scene.parts[i],
                q,
                Vec3::new(
                    center - (lo.x + hi.x) / 2.,
                    mid - (lo.y + hi.y) / 2.,
                    seam - (lo.z + hi.z) / 2.,
                ),
            );
            let clearance = (ceil.level - bottom.level - (hi.y - lo.y)) / 2.;
            if !(0.02..=0.1).contains(&clearance) {
                return Err(format!(
                    "Split roof connector vertical gap {clearance:.4} mm"
                ));
            }
            let top_point = Vec3::new(center, hi.y + scene.parts[i].pose.position[1], seam);
            record(
                scene,
                [primary, i],
                "split connector / upper slot face; measured clearance",
                [Vec3::new(center, ceil.level, seam), top_point],
                Vec3::Y,
                MateMeasure::Plane { gap: -clearance },
                0.02,
            );
            record(
                scene,
                [secondary, i],
                "split socket centerlines across seam",
                [
                    Vec3::new(other_center, mid, seam),
                    Vec3::new(center, mid, seam),
                ],
                Vec3::X,
                MateMeasure::Point,
                0.03,
            );
            // Independent cross sections check the spline's tapered outline
            // against both actual sockets, away from the centering section.
            for offset in [-43.5, -30., -10., 10., 30., 43.5] {
                let z = seam + offset;
                let panel = if offset < 0. { primary } else { secondary };
                let slots = socket_intervals(&scene.parts[panel], mid, z)?;
                let slot = slots
                    .iter()
                    .find(|s| s[0] < center && s[1] > center)
                    .ok_or("Connector tapered socket section missing")?;
                let male = section_crossings(&scene.parts[i], mid, z);
                if male.len() < 2 {
                    return Err("Connector tapered profile section missing".into());
                }
                for (side, wall, end) in [
                    (1., slot[0], male[0]),
                    (-1., slot[1], *male.last().unwrap()),
                ] {
                    let gap = (end - wall) * side;
                    record(
                        scene,
                        [panel, i],
                        &format!("split spline outline at {offset:.1} mm; side gap {gap:.3} mm"),
                        [Vec3::new(wall, mid, z), Vec3::new(end, mid, z)],
                        Vec3::X * side,
                        MateMeasure::Clearance { min: 0. },
                        0.03,
                    );
                }
            }
            edge(
                &mut edges,
                primary,
                i,
                "split roof connector / paired socket",
            );
        }
        let a = display::plane_patch(
            &scene.parts[primary].mesh,
            scene.parts[primary].pose.matrix(0.),
            2,
            1.,
            seam,
            wide_window(),
            0.03,
        )?;
        let b = display::plane_patch(
            &scene.parts[secondary].mesh,
            scene.parts[secondary].pose.matrix(0.),
            2,
            -1.,
            seam,
            wide_window(),
            0.03,
        )?;
        record(
            scene,
            [primary, secondary],
            "split roof mating end planes",
            [a.center, b.center],
            Vec3::Z,
            MateMeasure::Plane { gap: 0. },
            0.03,
        );
    }
    scene.parts[module].color = [0.53, 0.62, 0.72, 1.];
    scene.parts[facade].color = [0.78, 0.82, 0.87, 1.];
    for e in edges {
        let mut r = if let Some((p, a)) = e.hinge {
            Relation::hinge(scene, e.parent, e.child, p, a)?
        } else {
            Relation::fixed(scene, e.parent, e.child, false)?
        };
        r.label = e.label;
        if (r.label == LOCK_INNER || r.label == LOCK_OUTER)
            && let Joint::Hinge { min, max, .. } = &mut r.joint
        {
            *min = -180.;
            *max = 180.;
        }
        constraints::add(scene, r)?;
    }
    scene.calibrated = false;
    // Source patterns can be incompatible. Retain their measured positions and
    // expose failed checks; never distort a source mesh to make a check pass.
    reset_explosion(scene)
}

pub fn review_notes(scene: &Scene, zh: bool) -> Vec<&'static str> {
    let mut notes = vec![if zh {
        "销钉采用伸直展示近似；打印保留源件自由形态，未做弹性受力验证。"
    } else {
        "Pins use a relaxed display approximation. Print files retain their free shape; elastic stresses are not simulated."
    }];
    if scene
        .parts
        .iter()
        .any(|p| p.pose.path.ends_with("LOCK OUTER PART 1F.stl"))
    {
        notes.push(if zh { "部分 1F 源件的支座背面 UP 凸字与盖内沿有局部相交提示；保留原几何，待实物核对。" }
            else { "Some 1F source parts have a local crossing at the holder's raised UP mark and lid lip. Source geometry is retained for physical review." });
    }
    if scene
        .parts
        .iter()
        .any(|p| p.pose.path.ends_with("Facade.stl") && p.source_size.y > 50.)
    {
        notes.push(if zh { "高面板标签位于侧入槽的矩形区域；横向止挡与滑入间隙待实物核对。" }
            else { "Tall-facade label sits on the rectangular slot land; its lateral stop and insertion clearance need physical review." });
    }
    if scene.mating_checks.iter().any(|c| {
        !c.required && {
            let r = c.evaluate(scene);
            r.active && !r.within_tolerance
        }
    }) {
        notes.push(if zh { "铰链相邻端面存在负轴向间隙；保留实测螺孔位置，需核对源件兼容性。" }
            else { "Adjacent hinge end planes have negative axial clearance. Measured fastener positions are retained; check source-part compatibility." });
    }
    if scene.mating_checks.iter().any(|c| {
        c.required && {
            let r = c.evaluate(scene);
            r.active && !r.within_tolerance
        }
    }) {
        notes.push(if zh { "安装配合存在超出检查阈值的偏差，请展开查看；本套装配仍待校准确认。" }
            else { "Some mating checks exceed their thresholds. Expand the discrepancies; this assembly still needs calibration review." });
    }
    notes
}

// Keep the source embossing visible and reported. This narrow classification
// never suppresses collision results; the caller also checks at 0.2 mm.
fn holder_marking_crossing(scene: &Scene, pair: &crate::collision::Crossing) -> bool {
    if lid_angle(scene).is_none_or(|a| a.abs() > 0.0001)
        || !scene
            .parts
            .iter()
            .any(|p| p.pose.path.ends_with("LOCK OUTER PART 1F.stl"))
    {
        return false;
    }
    for (holder, roof) in [(pair.a, pair.b), (pair.b, pair.a)] {
        let h = &scene.parts[holder];
        let r = &scene.parts[roof];
        if !h.pose.path.ends_with("/LOCK HOLDER.stl")
            || !r.pose.path.contains(" Roof")
            || r.pose.path.contains("Connector")
        {
            continue;
        }
        let p =
            raw_frame().transform_point3(h.pose.matrix(0.).inverse().transform_point3(pair.point));
        return (-4.3..=2.8).contains(&p.x) && (-10.1..=-9.4).contains(&p.y) && p.z.abs() <= 9.2;
    }
    false
}
fn latch_crossings(
    scene: &Scene,
    report: &crate::collision::Report,
    coarse: Option<&crate::collision::Report>,
) -> Vec<serde_json::Value> {
    report.pairs.iter().filter(|pair|[pair.a,pair.b].iter().any(|&i|scene.parts[i].pose.path.contains("/Locks/")))
        .map(|pair| {
            let marking_review = holder_marking_crossing(scene, pair)
                && coarse.is_some_and(|r| !r.pairs.iter().any(|p| p.a == pair.a && p.b == pair.b));
            serde_json::json!({"a":constraints::PartId::of(&scene.parts[pair.a]),"b":constraints::PartId::of(&scene.parts[pair.b]),"point":pair.point.to_array(),"holder_marking_review":marking_review})
        }).collect()
}

/// Explicit offline audit. All parsing/fitting runs before the GUI starts.
pub fn audit(root: &std::path::Path, home: &std::path::Path) -> Result<(), String> {
    use crate::catalog::{self, Catalog};
    use std::{fmt::Write as _, fs};
    let catalog = Catalog::scan(root.into())?;
    let folder = home.join("output/sustainer-calibration");
    let state = home.join("state");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut report = String::from(
        "# Sustainer 装配几何核对\n\n依据源 STL 的双截面孔轴、圆柱弧及有向安装面定位；单位假定为 mm。软件检查阈值不是制造公差。所有自动生成定义保持 calibrated=false，待人工和实物确认。\n\n装配销钉采用伸直展示近似，打印保留源件自由形态，未做弹性受力验证；高面板标签横向止挡待核对。螺钉、支撑移除和 0–110° 机械行程未验证。端面负间隙提示不等同于已证明实体穿透。\n\n| 型号 | 检查通过/总数 | 最大必要误差 mm | 提示数 | 坐标文件 |\n| --- | ---: | ---: | ---: | --- |\n",
    );
    let mut failures = 0;
    let mut latch_failures = 0;
    let mut marking_reviews = 0;
    for (selection, options) in catalog::sustainer_configurations() {
        let key = options.key(&selection);
        let result = catalog.sustainer_bom(&selection, options).and_then(|bom| {
            Scene::load_with_rules(root, &bom, key.clone(), true, &display::Rules::default())
        });
        match result {
            Ok(scene) => {
                let surface = crate::collision::check(&scene, 0.05)?;
                let coarse = if surface
                    .pairs
                    .iter()
                    .any(|p| holder_marking_crossing(&scene, p))
                {
                    Some(crate::collision::check(&scene, 0.2)?)
                } else {
                    None
                };
                let latch_surface_crossings = latch_crossings(&scene, &surface, coarse.as_ref());
                let other_surface_crossings = surface.pairs.len() - latch_surface_crossings.len();
                if latch_surface_crossings
                    .iter()
                    .any(|p| p["holder_marking_review"] != true)
                {
                    latch_failures += 1;
                }
                if latch_surface_crossings
                    .iter()
                    .any(|p| p["holder_marking_review"] == true)
                {
                    marking_reviews += 1;
                }
                let checks: Vec<_> = scene
                    .mating_checks
                    .iter()
                    .map(|c| c.evaluate(&scene))
                    .collect();
                let passed = checks.iter().filter(|c| c.within_tolerance).count();
                let warnings = checks.iter().filter(|c| !c.within_tolerance).count();
                let max_error = checks
                    .iter()
                    .filter(|c| c.required)
                    .map(|c| c.error_mm)
                    .fold(0., f32::max);
                let file = state.join(format!("assembly-{key}.toml"));
                // Preserve any existing manually confirmed assembly definition.
                let disposition = if file.is_file() {
                    let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
                    let previous: crate::scene::Definition =
                        toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
                    if previous.calibrated {
                        "preserved_manual"
                    } else {
                        "updated"
                    }
                } else {
                    "created"
                };
                let candidate =
                    toml::to_string_pretty(&scene.definition()).map_err(|e| e.to_string())?;
                if disposition == "preserved_manual" {
                    fs::write(folder.join(format!("assembly-{key}.toml")), candidate)
                        .map_err(|e| e.to_string())?;
                } else {
                    fs::write(&file, candidate).map_err(|e| e.to_string())?;
                }
                writeln!(
                    report,
                    "| {key} | {passed}/{} | {max_error:.4} | {warnings} | {disposition} |",
                    checks.len()
                )
                .unwrap();
                rows.push(serde_json::json!({"key":key,"checks":checks,"notes":review_notes(&scene,true),"definition":file,"disposition":disposition,"latch_surface_crossings":latch_surface_crossings,"surface_tolerance_mm":0.05,"other_surface_crossings":other_surface_crossings}));
            }
            Err(error) => {
                failures += 1;
                writeln!(report, "| {key} | FAILED | — | — | {error} |").unwrap();
                rows.push(serde_json::json!({"key":key,"error":error}));
            }
        }
    }
    writeln!(
        report,
        "\n共 {} 组，加载失败 {failures} 组。详细检查见同目录 report.json。",
        rows.len()
    )
    .unwrap();
    writeln!(report,"\n卡扣局部表面检查：{latch_failures} 组检出未归类相交；另有 {marking_reviews} 组支座 UP 凸字与盖内沿的微小相交待核对（阈值 0.05 mm），涵盖支座、内外扣和装配展示销钉。销钉采用伸直近似；其他箱体相交、弹性及制造配合未验证。详见 report.json 的 latch_surface_crossings。凸字提示仍保留在报告中，不表示无干涉；同一对在 0.2 mm 复核仍检出时视为失败。\n").unwrap();
    report.push_str("\n主要待核对项：M 全部 21 组的相邻铰链端面有约 −0.125 mm 轴向间隙；XL4 三种箱体的后安装孔竖距为 14 mm、下铰链为 15.4 mm，居中配对后每孔偏约 0.7 mm。软件保留两件的原始孔阵列；原始 STL 不变，只有装配展示销钉采用伸直近似。\n\n分盖连接件按 S/M/L/XL 分别配置 2/3/4/5 个；在接缝两侧共六个截面检查其渐变轮廓及槽壁间隙。\n\n代表规格的 0°、1°、5°、15°、30°、45°、75°、110° 三角面相交诊断另见 surface-report.json。箱体、顶盖等保留打印支撑的展示网格仍报告其他相交；不能据此视作完整无干涉装配。孔轴/安装面通过也不代表 CAD 或实物配合通过。\n");
    fs::write(folder.join("report.md"), report).map_err(|e| e.to_string())?;
    fs::write(
        folder.join("report.json"),
        serde_json::to_vec_pretty(&rows).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if failures > 0 || latch_failures > 0 {
        Err(format!(
            "{failures} loads failed; {latch_failures} latch surface checks failed; see {}",
            folder.display()
        ))
    } else {
        Ok(())
    }
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
    if !value.is_finite() || !(0. ..=110.).contains(&value) {
        return Err("Invalid lid angle".into());
    }
    let i = scene
        .relations
        .iter()
        .position(|r| r.label == LID)
        .ok_or("Sustainer lid is unavailable")?;
    let closed = constraints::latch_display_pose(
        scene
            .parts
            .iter()
            .any(|p| p.pose.path.contains("/LOCK OUTER PART 2-4F")),
        scene
            .parts
            .iter()
            .any(|p| p.pose.path.contains("/Modules/") && p.pose.path.contains("/XL4/")),
    );
    let released = constraints::latch_release_pose();
    let previous: Vec<_> = scene.relations.iter().map(|r| r.joint.clone()).collect();
    // Select the released display state before any cover lift. Interpolating
    // the cover through a still-closed clasp produces a real mesh crossing.
    for (index, r) in scene.relations.iter_mut().enumerate() {
        if let Joint::Hinge { angle, .. } = &mut r.joint {
            if index == i {
                *angle = value;
            } else if r.label == LOCK_INNER {
                *angle = if value > 0. {
                    released.inner_degrees - closed.inner_degrees
                } else {
                    0.
                };
            } else if r.label == LOCK_OUTER {
                // The outer joint is relative to the moving inner link.
                *angle = if value > 0. {
                    released.outer_degrees
                        - closed.outer_degrees
                        - (released.inner_degrees - closed.inner_degrees)
                } else {
                    0.
                };
            }
        }
    }
    if let Err(e) = constraints::solve(scene) {
        for (r, joint) in scene.relations.iter_mut().zip(previous) {
            r.joint = joint;
        }
        return Err(e);
    }
    reset_explosion(scene)
}
// Bounding projections provide display spacing only, not a surface-interference check.
fn explosion_projection(part: &Part, axis: Vec3) -> (f32, f32) {
    let frame = part.pose.matrix(0.);
    let center = frame
        .transform_point3((part.mesh.min + part.mesh.max) * 0.5)
        .dot(axis);
    let half = (part.mesh.max - part.mesh.min) * 0.5;
    let local_axis = frame.inverse().transform_vector3(axis).abs();
    let extent = half.dot(local_axis);
    (center - extent, center + extent)
}
// Rotated print orientations make a roof's source bounding box much taller
// than the assembled lid. Use its mesh edges for compact vertical spacing.
fn explosion_mesh_projection(part: &Part, axis: Vec3) -> (f32, f32) {
    let frame = part.pose.matrix(0.);
    let local_axis = frame.inverse().transform_vector3(axis);
    let origin = frame.transform_point3(Vec3::ZERO).dot(axis);
    part.mesh
        .vertices
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), vertex| {
            let value = Vec3::from(vertex.position).dot(local_axis) + origin;
            (lo.min(value), hi.max(value))
        })
}
fn explosion_check<'a>(scene: &'a Scene, index: usize, label: &str) -> Option<&'a MateCheck> {
    let id = constraints::PartId::of(&scene.parts[index]);
    scene
        .mating_checks
        .iter()
        .find(|check| check.child == id && check.label.contains(label))
}
fn explosion_axis(scene: &Scene, index: usize, label: &str, fallback: Vec3) -> Vec3 {
    explosion_check(scene, index, label).map_or(fallback, |check| {
        scene.parts[index]
            .pose
            .matrix(0.)
            .transform_vector3(Vec3::from(check.child_axis))
            .normalize()
    })
}
pub fn reset_explosion(scene: &mut Scene) -> Result<(), String> {
    use constraints::PartId;
    let facade = *indices(scene, "Facade.stl")
        .first()
        .ok_or("Sustainer facade missing")?;
    let module = *indices(scene, "/Modules/")
        .first()
        .ok_or("Sustainer module missing")?;
    // Facade mesh +Y faces out, +Z faces up in the assembled frame.
    let front = scene.parts[facade]
        .pose
        .matrix(0.)
        .transform_vector3(Vec3::Y)
        .normalize();
    let up = scene.parts[facade]
        .pose
        .matrix(0.)
        .transform_vector3(Vec3::Z)
        .normalize();
    let right = up.cross(front).normalize();
    let ids: Vec<_> = scene.parts.iter().map(PartId::of).collect();
    let mut parents = vec![None; ids.len()];
    for relation in &scene.relations {
        let parent = ids
            .iter()
            .position(|id| id == &relation.parent)
            .ok_or("Missing explosion parent")?;
        let child = ids
            .iter()
            .position(|id| id == &relation.child)
            .ok_or("Missing explosion child")?;
        if parent == child || parents[child].replace(parent).is_some() {
            return Err("Invalid Sustainer explosion hierarchy".into());
        }
    }
    // In the exploded view, place the removable lock links outside the facade.
    // The holder stays inside; the solving graph and assembled poses are unchanged.
    for relation in scene.relations.iter().filter(|r| r.label == LOCK_INNER) {
        let child = ids
            .iter()
            .position(|id| id == &relation.child)
            .ok_or("Missing inner lock")?;
        parents[child] = Some(facade);
    }
    // Pins visually follow the bearing they pass through. Their solving parent
    // can instead be the stationary link, which is correct for lid motion.
    for (i, parent) in parents.iter_mut().enumerate() {
        if scene.parts[i].pose.path.contains("/1 SET OF PINS")
            && let Some(check) = explosion_check(scene, i, "pin insertion face")
        {
            *parent = Some(
                ids.iter()
                    .position(|id| id == &check.parent)
                    .ok_or("Missing pin bearing")?,
            );
        }
    }
    let mut travel = vec![None; ids.len()];
    for _ in 0..ids.len() {
        for (i, p) in scene.parts.iter().enumerate() {
            if travel[i].is_some() {
                continue;
            }
            let Some(parent) = parents[i] else {
                travel[i] = Some(Vec3::ZERO);
                continue;
            };
            let Some(base) = travel[parent] else {
                continue;
            };
            let name = &p.pose.path;
            let relation = scene
                .relations
                .iter()
                .find(|r| r.child == ids[i])
                .ok_or("Missing explosion relation")?;
            let own = if relation.label == LID || name.contains("/Hinges/") {
                // Set the common vertical layers after all axial offsets exist.
                Vec3::ZERO
            } else if i == facade {
                front * 90.
            } else if name.contains("/Rods/") || name.contains("/1 SET OF PINS") {
                let label = if name.contains("/Rods/") {
                    "rod rounded head"
                } else {
                    "pin insertion face"
                };
                let axis = explosion_axis(scene, i, label, right);
                let head =
                    explosion_check(scene, i, label).map_or(Vec3::from(p.pose.position), |check| {
                        p.pose
                            .matrix(0.)
                            .transform_point3(Vec3::from(check.child_point))
                    });
                let side = if (head - Vec3::from(scene.parts[parent].pose.position)).dot(axis) < 0.
                {
                    -1.
                } else {
                    1.
                };
                let out = axis * side;
                let (_, end) = explosion_projection(&scene.parts[parent], out);
                let (start, _) = explosion_projection(p, out);
                out * (end - start + 20.).max(20.)
            } else if name.contains("/Locks/") {
                // Holder -> facade -> inner link -> outer clasp, from inside out.
                // Pins inherit their corresponding link's display offset.
                let out = if name.ends_with("/LOCK HOLDER.stl") {
                    -front
                } else {
                    front
                };
                let (_, end) = explosion_projection(&scene.parts[parent], out);
                let (start, _) = explosion_projection(p, out);
                out * (end - start + 25.).max(25.)
            } else if name.contains("Label for") {
                explosion_axis(scene, i, "label back", front) * 30.
            } else if relation.label.contains("split roof / rear panel") {
                let axis = explosion_axis(scene, i, "split roof mating end planes", front);
                let sign = if (Vec3::from(p.pose.position)
                    - Vec3::from(scene.parts[parent].pose.position))
                .dot(axis)
                    < 0.
                {
                    -1.
                } else {
                    1.
                };
                axis * sign * 90.
            } else if relation.label.contains("split roof connector") {
                let other = scene
                    .relations
                    .iter()
                    .find(|r| {
                        r.parent == relation.parent && r.label.contains("split roof / rear panel")
                    })
                    .and_then(|r| ids.iter().position(|id| id == &r.child));
                let axis = other.map_or(front, |index| {
                    explosion_axis(scene, index, "split roof mating end planes", front)
                });
                let sign = other.map_or(1., |index| {
                    if (Vec3::from(scene.parts[index].pose.position)
                        - Vec3::from(scene.parts[parent].pose.position))
                    .dot(axis)
                        < 0.
                    {
                        -1.
                    } else {
                        1.
                    }
                });
                axis * sign * 45.
            } else if name.starts_with(crate::sustainer_dividers::PREFIX) {
                let (_, top) = explosion_projection(&scene.parts[module], up);
                let (bottom, _) = explosion_projection(p, up);
                up * (top - bottom + 30.).max(30.)
            } else {
                up * 25.
            };
            travel[i] = Some(base + own);
        }
    }
    if travel.iter().any(Option::is_none) {
        return Err("Sustainer explosion hierarchy contains a cycle".into());
    }
    let descends_from = |mut index: usize, root: usize| {
        for _ in 0..ids.len() {
            if index == root {
                return true;
            }
            let Some(parent) = parents[index] else {
                break;
            };
            index = parent;
        }
        false
    };
    let roots = |label: &str| -> Vec<usize> {
        scene
            .relations
            .iter()
            .filter(|r| r.label.contains(label))
            .filter_map(|r| ids.iter().position(|id| id == &r.child))
            .collect()
    };
    let lower_roots = roots("lower hinge / back-wall seat");
    let upper_roots = roots(UPPER_SEAT);
    let roof_roots = roots(LID);
    // Shaft checks identify mating partners even when one upper hinge spans
    // two lower hinges. Align their depth, retaining their axial placement.
    for check in scene
        .mating_checks
        .iter()
        .filter(|c| c.label == "upper / lower hinge shaft line")
    {
        let lower = ids
            .iter()
            .position(|id| id == &check.parent)
            .ok_or("Missing lower hinge")?;
        let upper = ids
            .iter()
            .position(|id| id == &check.child)
            .ok_or("Missing upper hinge")?;
        let lower_frame = scene.parts[lower].pose.matrix(0.);
        let upper_frame = scene.parts[upper].pose.matrix(0.);
        let axis = lower_frame
            .transform_vector3(Vec3::from(check.parent_axis))
            .normalize();
        let depth = up.cross(axis).normalize_or_zero();
        let delta = upper_frame.transform_point3(Vec3::from(check.child_point))
            + travel[upper].unwrap()
            - lower_frame.transform_point3(Vec3::from(check.parent_point))
            - travel[lower].unwrap();
        let correction = depth * delta.dot(depth);
        for (i, offset) in travel.iter_mut().enumerate() {
            if descends_from(i, lower) {
                *offset = Some(offset.unwrap() + correction);
            }
        }
    }
    let lower_layer: Vec<_> = (0..ids.len())
        .filter(|&i| lower_roots.iter().any(|&root| descends_from(i, root)))
        .collect();
    let upper_layer: Vec<_> = (0..ids.len())
        .filter(|&i| upper_roots.iter().any(|&root| descends_from(i, root)))
        .collect();
    let roof_layer: Vec<_> = (0..ids.len())
        .filter(|&i| {
            !upper_layer.contains(&i) && roof_roots.iter().any(|&root| descends_from(i, root))
        })
        .collect();
    let mut ceiling = explosion_projection(&scene.parts[module], up).1;
    for (i, part) in scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.starts_with(crate::sustainer_dividers::PREFIX))
    {
        ceiling = ceiling.max(explosion_projection(part, up).1 + travel[i].unwrap().dot(up));
    }
    // Pack the exploded vertical layers using their actual assembled extents.
    // This is display spacing, not a collision or mechanical-travel validation.
    for (layer, gap, exact) in [
        (lower_layer, 25., false),
        (upper_layer, 25., false),
        (roof_layer, 15., true),
    ] {
        if layer.is_empty() {
            continue;
        }
        let projection = |i: usize| {
            if exact {
                explosion_mesh_projection(&scene.parts[i], up)
            } else {
                explosion_projection(&scene.parts[i], up)
            }
        };
        let bottom = layer
            .iter()
            .map(|&i| projection(i).0 + travel[i].unwrap().dot(up))
            .fold(f32::INFINITY, f32::min);
        let top = layer
            .iter()
            .map(|&i| projection(i).1 + travel[i].unwrap().dot(up))
            .fold(f32::NEG_INFINITY, f32::max);
        let lift = (ceiling + gap - bottom).max(0.);
        for i in layer {
            travel[i] = Some(travel[i].unwrap() + up * lift);
        }
        ceiling = top + lift;
    }
    // Place the label beyond the facade and the exploded lock-pin envelope,
    // on its assembled side. Keep its front offset and orientation unchanged.
    for (i, part) in scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.ends_with("/Label for Sustainers.stl"))
    {
        let side = if (Vec3::from(part.pose.position)
            - Vec3::from(scene.parts[facade].pose.position))
        .dot(right)
            > 0.
        {
            1.
        } else {
            -1.
        };
        let out = right * side;
        let edge = scene
            .parts
            .iter()
            .enumerate()
            .filter(|(j, p)| *j == facade || p.pose.path.contains("/Locks/"))
            .map(|(j, p)| explosion_projection(p, out).1 + travel[j].unwrap().dot(out))
            .fold(f32::NEG_INFINITY, f32::max);
        let offset = travel[i].unwrap();
        let start = explosion_projection(part, out).0 + offset.dot(out);
        travel[i] = Some(offset + out * (edge + 25. - start).max(0.));
    }
    let front_branch: Vec<_> = (0..ids.len())
        .filter(|&i| {
            let mut current = Some(i);
            for _ in 0..ids.len() {
                let Some(index) = current else {
                    break;
                };
                if index == facade {
                    return true;
                }
                current = parents[index];
            }
            false
        })
        .collect();
    let rear = front_branch
        .iter()
        .map(|&i| explosion_projection(&scene.parts[i], front).0 + travel[i].unwrap().dot(front))
        .fold(f32::INFINITY, f32::min);
    let (_, housing_front) = explosion_projection(&scene.parts[module], front);
    let shift = front * (housing_front + 35. - rear).max(0.);
    for i in front_branch {
        travel[i] = Some(travel[i].unwrap() + shift);
    }
    for (p, travel) in scene.parts.iter_mut().zip(travel) {
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
    use crate::catalog::{Catalog, Selection, SustainerBody, SustainerOptions};
    #[test]
    fn sustainer_explosion_follows_assembled_axes_and_restored_poses() {
        use constraints::PartId;
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let catalog = Catalog::scan(root.into()).unwrap();
        for (width, depth, height, body, split_roof) in [
            ("S", 3, "1F", SustainerBody::Flat, false),
            ("M", 4, "2F", SustainerBody::Flat, false),
            ("L", 6, "4F", SustainerBody::Dividers, true),
            ("XL", 7, "1F", SustainerBody::Sealed, true),
        ] {
            let selection = Selection {
                width: width.into(),
                depth,
                height: height.into(),
                hidden_key: false,
            };
            let bom = catalog
                .sustainer_bom(&selection, SustainerOptions { body, split_roof })
                .unwrap();
            let mut scene =
                Scene::load(&catalog.root, &bom, "Explosion regression".into(), true).unwrap();
            for angle in [0., 75.] {
                set_lid_angle(&mut scene, angle).unwrap();
                let before = scene.clone();
                let relations = toml::to_string(&scene.definition()).unwrap();
                reset_explosion(&mut scene).unwrap();
                assert_eq!(relations, toml::to_string(&scene.definition()).unwrap());
                for (i, part) in scene.parts.iter().enumerate() {
                    assert!(
                        part.pose
                            .matrix(0.)
                            .abs_diff_eq(before.parts[i].pose.matrix(0.), 0.001)
                    );
                    let check = if part.pose.path.contains("/Rods/") {
                        explosion_check(&scene, i, "rod rounded head")
                    } else if part.pose.path.contains("/1 SET OF PINS") {
                        explosion_check(&scene, i, "pin insertion face")
                    } else {
                        None
                    };
                    if let Some(check) = check {
                        let parent = scene
                            .parts
                            .iter()
                            .find(|p| PartId::of(p) == check.parent)
                            .unwrap();
                        let axis = part
                            .pose
                            .matrix(0.)
                            .transform_vector3(Vec3::from(check.child_axis))
                            .normalize();
                        let head = part
                            .pose
                            .matrix(0.)
                            .transform_point3(Vec3::from(check.child_point));
                        let relative =
                            Vec3::from(part.pose.travel) - Vec3::from(parent.pose.travel);
                        assert!(
                            relative.cross(axis).length() < 0.01,
                            "pin / rod must follow assembled bore"
                        );
                        assert!(
                            relative.dot(axis)
                                * (head - Vec3::from(parent.pose.position)).dot(axis)
                                > 0.,
                            "head must exit its own side"
                        );
                        let out = relative.normalize();
                        assert!(
                            explosion_projection(part, out).0 + relative.length()
                                >= explosion_projection(parent, out).1 + 19.99
                        );
                    }
                }
                let facade = indices(&scene, "Facade.stl")[0];
                let front = scene.parts[facade]
                    .pose
                    .matrix(0.)
                    .transform_vector3(Vec3::Y);
                let module = indices(&scene, "/Modules/")[0];
                let (_, edge) = explosion_projection(&scene.parts[module], front);
                for part in scene.parts.iter().filter(|p| {
                    p.pose.path.contains("/Locks/") || p.pose.path.contains("Facade.stl")
                }) {
                    assert!(
                        explosion_projection(part, front).0
                            + Vec3::from(part.pose.travel).dot(front)
                            >= edge + 34.99
                    );
                }
                let panel = &scene.parts[facade];
                let (panel_back, panel_front) = explosion_projection(panel, front);
                let panel_travel = Vec3::from(panel.pose.travel).dot(front);
                for part in scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.contains("/Locks/"))
                {
                    let (lo, hi) = explosion_projection(part, front);
                    let offset = Vec3::from(part.pose.travel).dot(front);
                    if part.pose.path.ends_with("/LOCK HOLDER.stl") {
                        assert!(
                            hi + offset <= panel_back + panel_travel - 24.99,
                            "holder stays inside"
                        );
                    } else {
                        assert!(
                            lo + offset > panel_front + panel_travel,
                            "lock links and pins outside facade: {}",
                            part.pose.path
                        );
                    }
                    if part.pose.path.contains("/LOCK OUTER PART") {
                        let relation = scene
                            .relations
                            .iter()
                            .find(|r| r.child == PartId::of(part))
                            .unwrap();
                        let inner = scene
                            .parts
                            .iter()
                            .find(|p| PartId::of(p) == relation.parent)
                            .unwrap();
                        let inner_front = explosion_projection(inner, front).1
                            + Vec3::from(inner.pose.travel).dot(front);
                        assert!(
                            lo + offset >= inner_front + 24.99,
                            "outer clasp follows inner link"
                        );
                    }
                }
                let right = panel.pose.matrix(0.).transform_vector3(-Vec3::X);
                for label in scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.ends_with("/Label for Sustainers.stl"))
                {
                    let side = if (Vec3::from(label.pose.position)
                        - Vec3::from(panel.pose.position))
                    .dot(right)
                        > 0.
                    {
                        1.
                    } else {
                        -1.
                    };
                    let out = right * side;
                    let label_edge =
                        explosion_projection(label, out).0 + Vec3::from(label.pose.travel).dot(out);
                    for part in scene.parts.iter().filter(|p| {
                        p.pose.path.contains("/Locks/") || p.pose.path.contains("Facade.stl")
                    }) {
                        let edge = explosion_projection(part, out).1
                            + Vec3::from(part.pose.travel).dot(out);
                        assert!(
                            label_edge >= edge + 24.99,
                            "label must clear facade and exploded pins: {}",
                            part.pose.path
                        );
                    }
                }
                let up = panel.pose.matrix(0.).transform_vector3(Vec3::Z);
                let projected = |p: &Part| {
                    let (lo, hi) = explosion_projection(p, up);
                    let offset = Vec3::from(p.pose.travel).dot(up);
                    (lo + offset, hi + offset)
                };
                let lower: Vec<_> = scene
                    .parts
                    .iter()
                    .filter(|p| {
                        p.pose.path.contains("/Rods/")
                            || (p.pose.path.contains("/Hinges/")
                                && !p.pose.path.contains("Hinge Top.stl"))
                    })
                    .collect();
                let upper: Vec<_> = scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.contains("Hinge Top.stl"))
                    .collect();
                let roofs: Vec<_> = scene
                    .parts
                    .iter()
                    .filter(|p| {
                        p.pose
                            .path
                            .rsplit('/')
                            .next()
                            .is_some_and(|name| name.contains(" Roof"))
                    })
                    .collect();
                let mut ceiling = projected(&scene.parts[module]).1;
                for (layer, gap, exact) in [
                    (&lower, 25., false),
                    (&upper, 25., false),
                    (&roofs, 15., true),
                ] {
                    assert!(!layer.is_empty());
                    let bounds = |part: &Part| {
                        if exact {
                            let frame = part.pose.matrix(1.);
                            part.mesh.vertices.iter().fold(
                                (f32::INFINITY, f32::NEG_INFINITY),
                                |(lo, hi), v| {
                                    let value =
                                        frame.transform_point3(Vec3::from(v.position)).dot(up);
                                    (lo.min(value), hi.max(value))
                                },
                            )
                        } else {
                            projected(part)
                        }
                    };
                    let bottom = layer
                        .iter()
                        .map(|p| bounds(p).0)
                        .fold(f32::INFINITY, f32::min);
                    let top = layer
                        .iter()
                        .map(|p| bounds(p).1)
                        .fold(f32::NEG_INFINITY, f32::max);
                    assert!(
                        bottom >= ceiling + gap - 0.01,
                        "vertical layer must retain its display gap"
                    );
                    if exact {
                        assert!(
                            (bottom - ceiling - 15.).abs() < 0.01,
                            "roof uses a compact actual-mesh gap"
                        );
                    }
                    ceiling = top;
                }
                for check in scene
                    .mating_checks
                    .iter()
                    .filter(|c| c.label == "upper / lower hinge shaft line")
                {
                    let lower = scene
                        .parts
                        .iter()
                        .find(|p| PartId::of(p) == check.parent)
                        .unwrap();
                    let upper = scene
                        .parts
                        .iter()
                        .find(|p| PartId::of(p) == check.child)
                        .unwrap();
                    let axis = lower
                        .pose
                        .matrix(0.)
                        .transform_vector3(Vec3::from(check.parent_axis))
                        .normalize();
                    let delta = upper
                        .pose
                        .matrix(1.)
                        .transform_point3(Vec3::from(check.child_point))
                        - lower
                            .pose
                            .matrix(1.)
                            .transform_point3(Vec3::from(check.parent_point));
                    assert!(delta.dot(up) > 25., "upper shaft stays above lower shaft");
                    assert!(
                        delta.dot(up.cross(axis)).abs() < 0.01,
                        "hinge shaft lines must align vertically"
                    );
                }
                // The whole exploded layout must rotate with its assembled root,
                // including pin directions and the inherited parent offsets.
                let q = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.3);
                let mut rotated = scene.clone();
                constraints::rotate_subassembly(&mut rotated, module, q).unwrap();
                reset_explosion(&mut rotated).unwrap();
                for (a, b) in scene.parts.iter().zip(&rotated.parts) {
                    for amount in [0., 0.5, 1.] {
                        assert!(
                            b.pose
                                .matrix(amount)
                                .abs_diff_eq(Mat4::from_quat(q) * a.pose.matrix(amount), 0.01),
                            "rotation: {}",
                            a.pose.path
                        );
                    }
                }
                let mut saved = rotated.definition();
                for pose in &mut saved.poses {
                    pose.travel = [777.; 3];
                    pose.travel_frame = None;
                }
                let mut restored =
                    Scene::load(&catalog.root, &bom, "Restored explosion".into(), true).unwrap();
                restored.apply(&saved);
                assert!(restored.constraint_warning.is_none());
                for (a, b) in rotated.parts.iter().zip(&restored.parts) {
                    for amount in [0., 0.5, 1.] {
                        assert!(
                            a.pose
                                .matrix(amount)
                                .abs_diff_eq(b.pose.matrix(amount), 0.01)
                        );
                    }
                }
                assert_eq!(restored.calibrated, rotated.calibrated);
            }
        }
    }
    #[test]
    fn sustainer_library_latch_bearing_pairing() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let bom = c
            .sustainer_bom(&Selection::default(), SustainerOptions::default())
            .unwrap();
        let s = Scene::load(&c.root, &bom, "bearing regression".into(), true).unwrap();
        let inner = indices(&s, "/LOCK INNER PART.stl")[0];
        let holder = indices(&s, "/LOCK HOLDER.stl")[0];
        let middle =
            display::section_holes(&s.parts[inner].mesh, raw_frame(), 2, 0.013, [2.7, 3.1]);
        assert_eq!(middle.len(), 1);
        assert!((middle[0].point.x + 7.5).abs() < 0.01);
        let ends = display::section_holes(&s.parts[inner].mesh, raw_frame(), 2, 16., [2.7, 3.1]);
        assert_eq!(ends.len(), 2);
        let small = ends.iter().find(|h| h.point.x > 0.).unwrap();
        let holder_hole = display::cylinder_axis(
            &s.parts[holder].mesh,
            raw_frame(),
            2,
            Vec3::new(-2.7, 5.75, 0.),
            2.95,
        )
        .unwrap();
        let delta = axis_point(&s, inner, small) - axis_point(&s, holder, &holder_hole);
        assert!(delta.y.abs() < 0.001 && delta.z.abs() < 0.001);
        assert!(
            s.parts[holder]
                .pose
                .matrix(0.)
                .transform_vector3(raw(-1., 0., 0.))
                .normalize()
                .dot(Vec3::Y)
                > 0.999
        );
    }
    #[test]
    fn sustainer_library_all_scene_datums() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let mut configurations = 0;
        let mut source_hole_discrepancies = 0;
        let mut end_gap_discrepancies = 0;
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
                for result in scene
                    .mating_checks
                    .iter()
                    .map(|c| c.evaluate(&scene))
                    .filter(|r| !r.within_tolerance)
                {
                    if result.required {
                        assert_eq!(size, "XL4");
                        assert!(result.label.starts_with("lower hinge / housing: fastener"));
                        assert!((result.error_mm - 0.7).abs() < 0.005);
                        source_hole_discrepancies += 1;
                    } else {
                        assert_eq!(width, "M");
                        assert!(result.label.starts_with("hinge barrel end planes"));
                        assert!((result.error_mm - 0.125).abs() < 0.005);
                        end_gap_discrepancies += 1;
                    }
                }
                let module = indices(&scene, "/Modules/")[0];
                let facade = indices(&scene, "Facade.stl")[0];
                let (facade_lo, facade_hi) = constraints::world_bounds(&scene.parts[facade]);
                for holder in indices(&scene, "/LOCK HOLDER.stl") {
                    let part = &scene.parts[holder];
                    // The two outer screw lands belong entirely on the inside
                    // of the panel. Their forward-most vertices must touch it.
                    let flange_front = part
                        .mesh
                        .vertices
                        .iter()
                        .filter_map(|v| {
                            let local = Vec3::from(v.position);
                            (raw_frame().transform_point3(local).z.abs() > 26.)
                                .then(|| part.pose.matrix(0.).transform_point3(local).z)
                        })
                        .reduce(f32::max)
                        .unwrap();
                    assert!(
                        (flange_front - facade_lo.z).abs() < 0.005,
                        "Holder flange is not on inside wall: {size}/{height}"
                    );
                }
                for upper in indices(&scene, "Hinge Top.stl") {
                    let part = &scene.parts[upper];
                    let matrix = part.pose.matrix(0.);
                    let shaft_y = matrix.transform_point3(raw(0., -21.5, 0.)).y;
                    // L/XL's two source bodies are mirrored. Sample the actual
                    // screw plate away from the barrel, independent of raw sign.
                    let plate_lo = part
                        .mesh
                        .vertices
                        .iter()
                        .filter_map(|v| {
                            let local = Vec3::from(v.position);
                            (raw_frame().transform_point3(local).y > 0.)
                                .then(|| matrix.transform_point3(local).y)
                        })
                        .reduce(f32::min)
                        .unwrap();
                    assert!(
                        plate_lo >= shaft_y - 0.005,
                        "Upper plate is below shaft: {size}/{height}"
                    );
                }
                let roof_points = scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.contains(" Roof") && !p.pose.path.contains("Connector"))
                    .flat_map(|p| {
                        let matrix = p.pose.matrix(0.);
                        p.mesh
                            .vertices
                            .iter()
                            .map(move |v| matrix.transform_point3(v.position.into()))
                    });
                let roof_front = roof_points.clone().map(|p| p.z).reduce(f32::max).unwrap();
                let front_rim = roof_points
                    .filter(|p| p.z > roof_front - 0.2)
                    .map(|p| p.y)
                    .reduce(f32::min)
                    .unwrap();
                assert!(
                    front_rim >= facade_hi.y - 0.03,
                    "Roof rim below facade: {size}/{height}"
                );
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
        assert_eq!(source_hole_discrepancies, 24);
        assert_eq!(end_gap_discrepancies, 42);
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
        let mut surface_reports = Vec::new();
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
            assert!(
                scene
                    .mating_checks
                    .iter()
                    .filter(|c| c.required)
                    .all(|c| c.evaluate(&scene).within_tolerance)
            );
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
                if matches!(width, "S" | "M") { 3 } else { 5 }
            );
            assert_eq!(indices(&scene, "/Rods/").len(), 2);
            assert_eq!(
                indices(&scene, "/1 SET OF PINS").len(),
                if matches!(width, "S" | "M") { 4 } else { 8 }
            );
            let closed: Vec<_> = scene.parts.iter().map(|p| p.pose.matrix(0.)).collect();
            for holder in indices(&scene, "/LOCK HOLDER.stl") {
                let up = scene.parts[holder]
                    .pose
                    .matrix(0.)
                    .transform_vector3(raw(-1., 0., 0.));
                assert!(up.normalize().dot(Vec3::Y) > 0.999);
            }
            if width == "S" && depth == 3 {
                std::fs::create_dir_all("output/sustainer-calibration").unwrap();
                std::fs::write(
                    "output/sustainer-calibration/latch-s3.toml",
                    toml::to_string_pretty(&scene.definition()).unwrap(),
                )
                .unwrap();
            }
            let mut old_latch = scene.clone();
            for relation in &mut old_latch.relations {
                if relation.label == LOCK_INNER {
                    relation.label = "Sustainer datum: inner lock / small pins".into();
                    if let Joint::Hinge { angle, .. } = &mut relation.joint {
                        *angle = 25.;
                    }
                }
            }
            constraints::solve(&mut old_latch).unwrap();
            let mut upgraded = scene.clone();
            upgraded.retain_assembly_state(&old_latch);
            assert!(
                upgraded
                    .parts
                    .iter()
                    .zip(&closed)
                    .all(|(p, m)| p.pose.matrix(0.).abs_diff_eq(*m, 0.001))
            );
            let mut old_seats = scene.clone();
            for relation in &mut old_seats.relations {
                if relation.label == UPPER_SEAT {
                    relation.label = "Sustainer datum: upper hinge / roof seat".into();
                }
                if relation.label == LID
                    && let Joint::Hinge { angle, .. } = &mut relation.joint
                {
                    *angle = 12.;
                }
                if relation.child.path.ends_with("/LOCK HOLDER.stl")
                    && let Joint::Fixed { position, .. } = &mut relation.joint
                {
                    position[0] += 15.;
                }
            }
            constraints::solve(&mut old_seats).unwrap();
            let mut upgraded_seats = scene.clone();
            upgraded_seats.apply(&old_seats.definition());
            assert!(
                upgraded_seats
                    .parts
                    .iter()
                    .zip(&closed)
                    .all(|(p, m)| p.pose.matrix(0.).abs_diff_eq(*m, 0.001))
            );
            // A user's confirmed calibration is never silently replaced.
            old_seats.calibrated = true;
            let mut manual = scene.clone();
            manual.apply(&old_seats.definition());
            assert!(manual.calibrated);
            assert_eq!(lid_angle(&manual), Some(12.));
            assert!(
                manual
                    .parts
                    .iter()
                    .zip(&old_seats.parts)
                    .all(|(a, b)| a.pose.matrix(0.).abs_diff_eq(b.pose.matrix(0.), 0.001))
            );
            let mut legacy = scene.clone();
            for relation in &mut legacy.relations {
                relation.label = "Sustainer: legacy (estimated)".into();
            }
            for part in &mut legacy.parts {
                part.pose.position[1] += 100.;
            }
            let mut migrated = scene.clone();
            migrated.retain_assembly_state(&legacy);
            assert!(
                migrated
                    .parts
                    .iter()
                    .zip(&closed)
                    .all(|(p, m)| p.pose.matrix(0.).abs_diff_eq(*m, 0.001))
            );
            let small_joint = scene
                .relations
                .iter()
                .position(|r| r.label == LOCK_INNER)
                .unwrap();
            if let Joint::Hinge { angle, .. } = &mut scene.relations[small_joint].joint {
                *angle = 15.;
            }
            constraints::solve(&mut scene).unwrap();
            let outer = indices(&scene, "/LOCK OUTER PART")[0];
            assert!(
                !scene.parts[outer]
                    .pose
                    .matrix(0.)
                    .abs_diff_eq(closed[outer], 0.001)
            );
            let holder = indices(&scene, "/LOCK HOLDER.stl")[0];
            assert!(
                scene.parts[holder]
                    .pose
                    .matrix(0.)
                    .abs_diff_eq(closed[holder], 0.001)
            );
            if let Joint::Hinge { angle, .. } = &mut scene.relations[small_joint].joint {
                *angle = 0.;
            }
            constraints::solve(&mut scene).unwrap();
            for angle in [0., 1., 5., 15., 30., 45., 75., 110.] {
                set_lid_angle(&mut scene, angle).unwrap();
                let diagnostic = crate::collision::check(&scene, 0.05).unwrap();
                for pair in &diagnostic.pairs {
                    let a = &scene.parts[pair.a];
                    let b = &scene.parts[pair.b];
                    if [a, b].iter().any(|p| p.pose.path.contains("/Locks/")) {
                        assert!(
                            holder_marking_crossing(&scene, pair),
                            "Unexpected latch crossing {width}{depth}/{height} angle={angle}: {} / {} @ {:?}",
                            a.label(false),
                            b.label(false),
                            pair.point
                        );
                        // Do not accept a larger crossing at the same feature.
                        let coarse = crate::collision::check(&scene, 0.2).unwrap();
                        assert!(
                            !coarse.pairs.iter().any(|p| p.a == pair.a && p.b == pair.b),
                            "Holder marking crossing exceeds review threshold"
                        );
                    }
                }

                let pairs:Vec<_>=diagnostic.pairs.iter().map(|p|serde_json::json!({
                    "a":constraints::PartId::of(&scene.parts[p.a]),"b":constraints::PartId::of(&scene.parts[p.b]),"point":p.point.to_array()
                })).collect();
                surface_reports.push(serde_json::json!({"size":format!("{width}{depth}/{height}"),"body":format!("{body:?}"),"split":split_roof,"angle":angle,"pairs":pairs}));
            }
            set_lid_angle(&mut scene, 0.).unwrap();
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
                let p = &scene.parts[i];
                let stays_fixed = p.pose.path.ends_with("/LOCK HOLDER.stl")
                    || (p.pose.path.contains("PINS") && p.pose.body.is_some_and(|b| b >= 2));
                assert_eq!(p.pose.matrix(0.).abs_diff_eq(closed[i], 0.001), stays_fixed);
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
        let folder = std::path::Path::new("output/sustainer-calibration");
        std::fs::create_dir_all(folder).unwrap();
        std::fs::write(
            folder.join("surface-report.json"),
            serde_json::to_vec_pretty(&surface_reports).unwrap(),
        )
        .unwrap();
    }
}
