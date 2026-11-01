//! External mates from the modification manual. All transforms remain estimates.
use super::{Relation, add, stacking, world_bounds};
use crate::{
    catalog::{
        Selection,
        exterior::{Base, Exterior, Role, Roof},
    },
    scene::Scene,
};
use glam::{Quat, Vec3};
use std::collections::BTreeMap;

/// Printed bases/roofs are on edge, some rotated in their width/height plane.
/// Use a broad face normal, not the oblique bounding box, to recover orientation.
pub fn horizontal_orientation(
    part: &crate::scene::Part,
    expected_width: f32,
) -> Result<Quat, String> {
    let mut normals = BTreeMap::<(i32, i32), f32>::new();
    for t in part.mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(t[0].position);
        let b = Vec3::from(t[1].position);
        let c = Vec3::from(t[2].position);
        let cross = (b - a).cross(c - a);
        let area = cross.length();
        if area < 0.0001 {
            continue;
        }
        let mut n = cross / area;
        if n.y.abs() > 0.001 {
            continue;
        }
        if n.z < 0. {
            n = -n;
        }
        *normals
            .entry(((n.x * 10000.).round() as i32, (n.z * 10000.).round() as i32))
            .or_default() += area;
    }
    let mut candidates: Vec<_> = normals.into_iter().collect();
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    for ((nx, nz), _) in candidates.into_iter().take(12) {
        let q = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
            * Quat::from_rotation_y(-(nx as f32).atan2(nz as f32));
        let (lo, hi) = rotated_bounds(part, q);
        if (hi.x - lo.x - expected_width).abs() < 0.4 {
            return Ok(q);
        }
    }
    Err(format!(
        "{}：未找到匹配规格的配件安装面 / Exterior orientation unavailable",
        part.pose.path
    ))
}
fn rotated_bounds(p: &crate::scene::Part, q: Quat) -> (Vec3, Vec3) {
    p.mesh
        .vertices
        .iter()
        .map(|v| q * Vec3::from(v.position))
        .fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), v| (lo.min(v), hi.max(v)),
        )
}
fn pose(scene: &mut Scene, i: usize, q: Quat, pos: Vec3) {
    let (x, y, z) = q.to_euler(glam::EulerRot::XYZ);
    scene.parts[i].pose.rotation = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
    scene.parts[i].pose.position = pos.to_array();
    scene.parts[i].pose.travel = [0.; 3];
    scene.parts[i].pose.travel_frame = None;
}
fn seat(scene: &Scene, i: usize, positive: bool) -> Result<f32, String> {
    let p = &scene.parts[i];
    let m = p.pose.matrix(0.);
    let mut levels = BTreeMap::<i32, f32>::new();
    for t in p.mesh.vertices.chunks_exact(3) {
        let a = m.transform_point3(Vec3::from(t[0].position));
        let b = m.transform_point3(Vec3::from(t[1].position));
        let c = m.transform_point3(Vec3::from(t[2].position));
        let n = (b - a).cross(c - a);
        let area = n.length();
        if area > 0.0001 && n.y * (if positive { 1. } else { -1. }) > area * 0.999 {
            *levels
                .entry(((a.y + b.y + c.y) / 3. * 100.).round() as i32)
                .or_default() += area;
        }
    }
    levels
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(v, _)| v as f32 / 100.)
        .ok_or("配件安装平面缺失 / Missing exterior seating face".into())
}
fn bind(scene: &mut Scene, parent: usize, child: usize, label: &str) -> Result<(), String> {
    let mut r = Relation::fixed(scene, parent, child, false)?;
    r.label = format!("Exterior estimate: {label}");
    add(scene, r)
}
/// Specs have already been validated and meshes loaded by the worker.
pub fn attach(
    scene: &mut Scene,
    s: &Selection,
    config: &Exterior,
    parts: &[(Role, usize)],
    tower_height: f32,
) -> Result<(), String> {
    if parts.is_empty() {
        return Ok(());
    }
    let original = Scene {
        parts: scene
            .parts
            .iter()
            .enumerate()
            .filter(|(i, _)| !parts.iter().any(|(_, j)| i == j))
            .map(|(_, p)| p.clone())
            .collect(),
        ..Default::default()
    };
    let d = stacking::datum(&original)?;
    let module = scene
        .parts
        .iter()
        .position(|p| p.pose.path.contains("/Modules/"))
        .ok_or("Module missing")?;

    let w = stacking::width(&s.width)? as f32 / 10.;
    let body_top = d.bottom
        + match s.height.as_str() {
            "1F" => 47.,
            "2F" => 95.,
            _ => 191.,
        };
    let back = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let mut base = None;
    let mut backbone = None;
    let mut roof = None;
    let mut lock = None;
    // Preserve the catalogue's parent-before-child order, except locks precede pull handles.
    let mut ordered = parts.to_vec();
    ordered.sort_by_key(|(role, _)| match role {
        Role::Base => 0,
        Role::Backbone | Role::Wall => 1,
        Role::Roof => 2,
        Role::Lock => 3,
        Role::Pull => 5,
        _ => 4,
    });
    for (role, i) in ordered {
        let copy = scene.parts[i].pose.instance as f32;
        let mut parent = module;
        match role {
            Role::Base | Role::Roof => {
                let mut q = horizontal_orientation(&scene.parts[i], w)?;
                if role == Role::Base {
                    q = Quat::from_rotation_y(std::f32::consts::PI) * q;
                }
                pose(scene, i, q, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let y = if role == Role::Base {
                    d.bottom - seat(scene, i, true)?
                } else {
                    d.top - seat(scene, i, false)?
                };
                pose(
                    scene,
                    i,
                    q,
                    Vec3::new(d.center_x - (lo.x + hi.x) * 0.5, y, d.rear - lo.z),
                );
                if role == Role::Base {
                    base = Some(i);
                } else {
                    roof = Some(i);
                }
            }
            Role::Backbone | Role::Wall => {
                let q = if role == Role::Backbone {
                    back.inverse()
                } else {
                    back
                };
                pose(scene, i, q, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                pose(
                    scene,
                    i,
                    q,
                    Vec3::new(
                        d.center_x - (lo.x + hi.x) * 0.5,
                        d.bottom + (body_top - d.bottom - (hi.y - lo.y)) * 0.5 - lo.y,
                        d.rear - hi.z,
                    ),
                );
                if role == Role::Backbone {
                    backbone = Some(i);
                } else {
                    lock = Some(i);
                }
            }
            Role::Feet => {
                parent = base.ok_or("Feet require base")?;
                // Feet print on their mating face: invert so this face seats below the base.
                let q = Quat::from_rotation_z(std::f32::consts::PI);
                pose(scene, i, q, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let (blo, bhi) = world_bounds(&scene.parts[parent]);
                pose(
                    scene,
                    i,
                    q,
                    Vec3::new(d.center_x - (lo.x + hi.x) * 0.5, blo.y - hi.y, bhi.z - hi.z),
                );
            }
            Role::Disk | Role::Tyre | Role::Cover | Role::Bushing => {
                parent = base.ok_or("Wheels require base")?;
                let side = if copy < 0.5 { -1. } else { 1. };
                let q = Quat::from_rotation_z(-side * std::f32::consts::FRAC_PI_2);
                pose(scene, i, q, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let (blo, _) = world_bounds(&scene.parts[parent]);
                let axial = match role {
                    Role::Bushing => 16.5,
                    Role::Disk => 10.05,
                    Role::Tyre => 20.3,
                    Role::Cover => 32.05,
                    _ => 0.,
                };
                let center = Vec3::new(
                    d.center_x + side * (w * 0.5 + axial),
                    blo.y + 20.,
                    blo.z + 30.,
                );
                pose(
                    scene,
                    i,
                    q,
                    center - Vec3::new(0., (lo.y + hi.y) * 0.5, (lo.z + hi.z) * 0.5),
                );
                if role == Role::Tyre {
                    scene.parts[i].color = [0.13, 0.14, 0.15, 1.];
                }
            }
            Role::Lock => {
                pose(scene, i, Quat::IDENTITY, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                pose(
                    scene,
                    i,
                    Quat::IDENTITY,
                    Vec3::new(
                        d.center_x - (lo.x + hi.x) * 0.5,
                        body_top - hi.y,
                        d.rear - hi.z,
                    ),
                );
                lock = Some(i);
            }
            Role::Stationary | Role::Board => {
                parent = backbone.ok_or("Backbone missing")?;
                let q = back;
                pose(scene, i, q, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let (blo, bhi) = world_bounds(&scene.parts[parent]);
                pose(
                    scene,
                    i,
                    q,
                    Vec3::new(
                        d.center_x - (lo.x + hi.x) * 0.5,
                        (blo.y + bhi.y - lo.y - hi.y) * 0.5,
                        blo.z - hi.z,
                    ),
                );
            }
            Role::Pull => {
                parent = lock.ok_or("Pull lock missing")?;
                let q = Quat::from_rotation_z(std::f32::consts::PI);
                pose(scene, i, q, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let (blo, bhi) = world_bounds(&scene.parts[parent]);
                pose(
                    scene,
                    i,
                    q,
                    Vec3::new(
                        d.center_x - (lo.x + hi.x) * 0.5,
                        d.top + config.pull_extension - lo.y,
                        (blo.z + bhi.z - lo.z - hi.z) * 0.5 - 4.5,
                    ),
                );
                if config.pull_extension > (tower_height + body_top - d.top - 38.).max(0.) + 0.25 {
                    return Err(
                        "拉杆长度不足以保持锁座内插入量 / Insufficient rod engagement".into(),
                    );
                }
                let rod_top = d.top + config.pull_extension + 30.;
                let rod_bottom = rod_top - tower_height.max(61.);
                for side in [-1., 1.] {
                    let rod = scene.append_hardware_rod(
                        tower_height.max(61.),
                        Vec3::new(
                            d.center_x + side * (w * 0.5 - 42.),
                            (rod_top + rod_bottom) * 0.5,
                            (blo.z + bhi.z) * 0.5 - 1.5,
                        ),
                    );
                    bind(scene, i, rod, "M16 rod / purchased hardware schematic")?;
                }
            }
            Role::LockS | Role::Knob | Role::KnobCap => {
                parent = lock.ok_or("Pull lock missing")?;
                let q = back;
                pose(scene, i, q, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let (blo, bhi) = world_bounds(&scene.parts[parent]);
                let x = if role == Role::LockS {
                    0.
                } else {
                    (if copy < 0.5 { -1. } else { 1. }) * (w * 0.5 - 43.5)
                };
                let extra = if role == Role::KnobCap { 22. } else { 0. };
                pose(
                    scene,
                    i,
                    q,
                    Vec3::new(
                        d.center_x + x - (lo.x + hi.x) * 0.5,
                        (blo.y + bhi.y - lo.y - hi.y) * 0.5,
                        blo.z - hi.z - extra,
                    ),
                );
            }
            Role::TopHandle => {
                parent = roof.ok_or("Roof missing")?;
                // Source handle long axis is Y; upright across width after rotating about Z.
                let q = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
                pose(scene, i, q, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let (rlo, rhi) = world_bounds(&scene.parts[parent]);
                pose(
                    scene,
                    i,
                    q,
                    Vec3::new(
                        d.center_x - (lo.x + hi.x) * 0.5,
                        seat(scene, parent, true)? - lo.y,
                        (rlo.z + rhi.z - lo.z - hi.z) * 0.5,
                    ),
                );
            }
            Role::Receiver => {
                parent = lock.ok_or("Wall mount missing")?;
                pose(scene, i, back, Vec3::ZERO);
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let (blo, bhi) = world_bounds(&scene.parts[parent]);
                pose(
                    scene,
                    i,
                    back,
                    Vec3::new(
                        d.center_x - (lo.x + hi.x) * 0.5,
                        blo.y + (bhi.y - blo.y) * if copy < 0.5 { 0.25 } else { 0.75 }
                            - (lo.y + hi.y) * 0.5,
                        blo.z - hi.z,
                    ),
                );
            }
        }
        bind(scene, parent, i, &format!("{role:?}"))?;
    }
    scene.calibrated = false;
    Ok(())
}
/// Layout validation also runs before moves/deletes/resizes; no partial mutation.
pub fn validate_layout(
    modules: &[stacking::Module<'_>],
    layout: &stacking::Layout,
    configs: &[Exterior],
) -> Result<(), String> {
    for (i, c) in configs.iter().enumerate() {
        c.validate(modules[i].selection, modules[i].is_box)?;
        if c.pull_handle && c.pull_extension > pull_limit(modules, layout, i) + 0.01 {
            return Err(format!(
                "拉杆伸出量超过当前塔高可用行程（最多 {:.1} mm） / Handle exceeds available stroke",
                pull_limit(modules, layout, i)
            ));
        }
        if c.base != Base::None && !layout.supports[i].is_empty() {
            return Err(format!(
                "{}：底座只能安装在地面成员下方 / Base requires a ground member",
                modules[i].name
            ));
        }
        let above = layout.supports.iter().any(|s| s.contains(&i));
        if above && (c.roof != Roof::None || c.pull_handle) {
            return Err(format!(
                "{}：顶盖和伸缩拉杆需要顶部空间 / Roof/handle requires an exposed top",
                modules[i].name
            ));
        }
    }
    Ok(())
}

/// Pack independent towers by their closed external envelopes. Within a joined
/// support group, keep nominal interfaces and reject conflicting lateral space.
pub fn space_groups(
    layout: &stacking::Layout,
    scenes: &[Scene],
    shifts: &mut [Vec3],
) -> Result<(), String> {
    let mut groups: Vec<_> = (0..scenes.len()).collect();
    for (i, supports) in layout.supports.iter().enumerate() {
        for &j in supports {
            let old = groups[j];
            let new = groups[i];
            for group in &mut groups {
                if *group == old {
                    *group = new;
                }
            }
        }
    }
    for i in 0..scenes.len() {
        for j in 0..i {
            if groups[i] != groups[j] {
                continue;
            }
            let module_bounds = |k: usize| -> Result<(Vec3, Vec3), String> {
                let p = scenes[k]
                    .parts
                    .iter()
                    .find(|p| p.pose.path.contains("/Modules/"))
                    .ok_or("Module missing")?;
                let (lo, hi) = world_bounds(p);
                Ok((lo + shifts[k], hi + shifts[k]))
            };
            let (a, b) = module_bounds(i)?;
            let (c, d) = module_bounds(j)?;
            if a.x < d.x - 0.5 && c.x < b.x - 0.5 {
                continue;
            }
            // Deliberately conservative envelope-space check, not a surface crossing claim.
            for (from, to) in [(i, j), (j, i)] {
                for p in scenes[from]
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.starts_with("03. MODIFICATIONS/"))
                {
                    let (lo, hi) = world_bounds(p);
                    let lo = lo + shifts[from];
                    let hi = hi + shifts[from];
                    for other in &scenes[to].parts {
                        let (ol, oh) = world_bounds(other);
                        if (hi.min(oh + shifts[to]) - lo.max(ol + shifts[to])).min_element() > 0.5 {
                            return Err("配对支承内的配件所需空间重叠，请移除双轮或更换安装对象 / Accessory envelopes overlap within joined supports".into());
                        }
                    }
                }
            }
        }
    }
    let mut unique = Vec::new();
    for &i in &layout.order {
        if !unique.contains(&groups[i]) {
            unique.push(groups[i]);
        }
    }
    unique.sort_by(|a, b| {
        let left = |id: &usize| {
            groups
                .iter()
                .enumerate()
                .filter(|(_, g)| *g == id)
                .map(|(i, _)| layout.left[i])
                .fold(f32::INFINITY, f32::min)
        };
        left(a).total_cmp(&left(b))
    });
    let mut right = f32::NEG_INFINITY;
    for group in unique {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for (i, scene) in scenes
            .iter()
            .enumerate()
            .filter(|(i, _)| groups[*i] == group)
        {
            let (a, b) = scene.closed_extent()?;
            lo = lo.min(a.x + shifts[i].x);
            hi = hi.max(b.x + shifts[i].x);
        }
        let dx = (right - lo).max(0.);
        for (i, shift) in shifts.iter_mut().enumerate() {
            if groups[i] == group {
                shift.x += dx;
            }
        }
        right = hi + dx;
    }
    Ok(())
}

/// Conservative available stroke with at least 8 mm insertion in the top lock.
pub fn pull_limit(
    modules: &[stacking::Module<'_>],
    layout: &stacking::Layout,
    index: usize,
) -> f32 {
    let mut tops = vec![0f32; modules.len()];
    for &i in &layout.order {
        let h = match modules[i].selection.height.as_str() {
            "1F" => {
                if modules[i].is_box {
                    61.
                } else {
                    47.
                }
            }
            "2F" => 95.,
            _ => 191.,
        };
        tops[i] = layout.supports[i].first().map_or(0., |&j| tops[j]) + h;
    }
    (tops[index]
        - if modules[index].is_box && modules[index].selection.height == "1F" {
            14.
        } else {
            0.
        }
        - 38.)
        .clamp(0., 800.)
}
