//! Display preparation is separate from source meshes and print-file quantities.
//! Connectivity is evaluated only for explicitly configured files, never blindly
//! for every STL (a disconnected shell is not always a separate physical part).
use crate::mesh::{Mesh, Vertex};
use glam::{DMat3, DVec2, DVec3, Mat4, Quat, Vec3};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub path: String,
    pub split_disconnected: bool,
    pub expected_bodies: usize,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub rules: Vec<Rule>,
}
impl Default for Rules {
    fn default() -> Self {
        toml::from_str(include_str!("../config/display-rules.toml").trim_start_matches('\u{feff}'))
            .expect("Bundled display recipes are valid")
    }
}
impl Rules {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.to_string()),
        };
        let rules: Self = toml::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("Display rules: {e}"))?;
        for (i, r) in rules.rules.iter().enumerate() {
            if r.expected_bodies == 0
                || r.expected_bodies > 128
                || r.path.is_empty()
                || rules.rules[..i]
                    .iter()
                    .any(|other| other.path.eq_ignore_ascii_case(&r.path))
            {
                return Err(format!("Invalid or duplicate display rule: {}", r.path));
            }
        }
        Ok(rules)
    }
    pub fn for_path(&self, path: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| {
            r.path.eq_ignore_ascii_case(path)
                || (!path.contains('/')
                    && !path.contains('\\')
                    && r.path
                        .rsplit('/')
                        .next()
                        .is_some_and(|n| n.eq_ignore_ascii_case(path)))
        })
    }
    pub fn prepare(&self, path: &str, source: &Arc<Mesh>) -> Result<Vec<Body>, String> {
        let Some(rule) = self.for_path(path).filter(|r| r.split_disconnected) else {
            return Ok(vec![Body {
                mesh: source.clone(),
                id: None,
                source_center: Vec3::ZERO,
            }]);
        };
        let meshes = split_connected(source)?;
        if meshes.len() != rule.expected_bodies {
            return Err(format!(
                "{path}: display recipe expects {} bodies, detected {}. No source geometry was changed.",
                rule.expected_bodies,
                meshes.len()
            ));
        }
        Ok(meshes
            .into_iter()
            .enumerate()
            .map(|(id, (mesh, source_center))| Body {
                mesh: Arc::new(mesh),
                id: Some(id as u32),
                source_center,
            })
            .collect())
    }
}
#[derive(Clone)]
pub struct Body {
    pub mesh: Arc<Mesh>,
    pub id: Option<u32>,
    pub source_center: Vec3,
}

/// Read-only section geometry used to locate fastener bores. Sections also work
/// on faceted/tapped bores whose walls are not exact cylindrical triangles.
#[derive(Clone, Debug)]
pub struct CircleDatum {
    pub point: Vec3,
    pub radius: f32,
    pub residual: f32,
}
fn precise_point(matrix: Mat4, point: [f32; 3]) -> DVec3 {
    let p = DVec3::from_array(point.map(f64::from));
    DVec3::from_array(matrix.x_axis.truncate().to_array().map(f64::from)) * p.x
        + DVec3::from_array(matrix.y_axis.truncate().to_array().map(f64::from)) * p.y
        + DVec3::from_array(matrix.z_axis.truncate().to_array().map(f64::from)) * p.z
        + DVec3::from_array(matrix.w_axis.truncate().to_array().map(f64::from))
}
pub fn section_segments(mesh: &Mesh, matrix: Mat4, axis: usize, level: f32) -> Vec<[Vec3; 2]> {
    let level = f64::from(level);
    let mut lines = Vec::new();
    for tri in mesh.vertices.chunks_exact(3) {
        let p: [DVec3; 3] = std::array::from_fn(|i| precise_point(matrix, tri[i].position));
        let mut hits = Vec::with_capacity(2);
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            if (p[a][axis] - level) * (p[b][axis] - level) < 0. {
                let t = (level - p[a][axis]) / (p[b][axis] - p[a][axis]);
                hits.push((p[a] + (p[b] - p[a]) * t).as_vec3());
            }
        }
        if hits.len() == 2 {
            lines.push([hits[0], hits[1]]);
        }
    }
    lines
}
fn fit_circle(points: &[DVec2]) -> Option<(DVec2, f64, f64)> {
    if points.len() < 3 {
        return None;
    }
    let mean = points.iter().copied().sum::<DVec2>() / points.len() as f64;
    let mut a = DMat3::ZERO;
    let mut b = DVec3::ZERO;
    for p in points {
        let p = *p - mean;
        let row = DVec3::new(2. * p.x, 2. * p.y, 1.);
        a += DMat3::from_cols(row * row.x, row * row.y, row * row.z);
        b += row * p.length_squared();
    }
    if a.determinant().abs() < 1e-10 {
        return None;
    }
    let solution = a.inverse() * b;
    let center = mean + solution.truncate();
    let radius = (solution.z + solution.truncate().length_squared()).sqrt();
    let residual = points
        .iter()
        .map(|p| ((*p - center).length() - radius).abs())
        .fold(0., f64::max);
    (center.is_finite() && radius.is_finite()).then_some((center, radius, residual))
}
pub fn section_holes(
    mesh: &Mesh,
    matrix: Mat4,
    axis: usize,
    level: f32,
    radii: [f32; 2],
) -> Vec<CircleDatum> {
    let uv: Vec<_> = (0..3).filter(|&a| a != axis).collect();
    let mut points = Vec::<DVec2>::new();
    let mut keys = HashMap::<[i64; 2], usize>::new();
    let mut edges = Vec::new();
    for line in section_segments(mesh, matrix, axis, level) {
        let mut pair = [0; 2];
        for (i, p) in line.into_iter().enumerate() {
            let key = [
                (f64::from(p[uv[0]]) * 500.).round() as i64,
                (f64::from(p[uv[1]]) * 500.).round() as i64,
            ];
            pair[i] = *keys.entry(key).or_insert_with(|| {
                let i = points.len();
                points.push(DVec2::new(key[0] as f64 / 500., key[1] as f64 / 500.));
                i
            });
        }
        edges.push(pair);
    }
    let mut parent: Vec<_> = (0..points.len()).collect();
    for [a, b] in edges {
        let a = root(&mut parent, a);
        let b = root(&mut parent, b);
        parent[a] = b;
    }
    let mut groups = BTreeMap::<usize, Vec<DVec2>>::new();
    for (i, p) in points.into_iter().enumerate() {
        groups.entry(root(&mut parent, i)).or_default().push(p);
    }
    let mut result = Vec::new();
    for points in groups.values() {
        let Some((center, radius, residual)) = fit_circle(points) else {
            continue;
        };
        if radius < f64::from(radii[0]) || radius > f64::from(radii[1]) || residual > 0.08 {
            continue;
        }
        let mut point = Vec3::ZERO;
        point[axis] = level;
        point[uv[0]] = center.x as f32;
        point[uv[1]] = center.y as f32;
        result.push(CircleDatum {
            point,
            radius: radius as f32,
            residual: residual as f32,
        });
    }
    result.sort_by(|a, b| {
        a.point[uv[0]]
            .total_cmp(&b.point[uv[0]])
            .then(a.point[uv[1]].total_cmp(&b.point[uv[1]]))
    });
    result
}
/// Fit an arc of an actual cylinder wall, excluding planar teardrop reliefs.
/// The seed only selects the feature; the result is fitted from mesh vertices.
pub fn cylinder_axis(
    mesh: &Mesh,
    matrix: Mat4,
    axis: usize,
    seed: Vec3,
    radius: f32,
) -> Result<CircleDatum, String> {
    cylinder_arc(mesh, matrix, axis, seed, radius, 180.)
}
/// Fit a partial round mating surface. The requested coverage must be backed
/// by actual wall facets; short catch fillets are not complete shaft bores.
pub fn cylinder_arc(
    mesh: &Mesh,
    matrix: Mat4,
    axis: usize,
    seed: Vec3,
    radius: f32,
    coverage_degrees: f32,
) -> Result<CircleDatum, String> {
    let uv: Vec<_> = (0..3).filter(|&a| a != axis).collect();
    let center = DVec2::new(f64::from(seed[uv[0]]), f64::from(seed[uv[1]]));
    let mut points = BTreeMap::<[i64; 2], DVec2>::new();
    for tri in mesh.vertices.chunks_exact(3) {
        let p: [DVec3; 3] = std::array::from_fn(|i| precise_point(matrix, tri[i].position));
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        if n.length() < 1e-10 || n[axis].abs() > n.length() * 0.003 {
            continue;
        }
        let projected = p.map(|p| DVec2::new(p[uv[0]], p[uv[1]]));
        let (a, b) = [(0, 1), (1, 2), (2, 0)]
            .into_iter()
            .max_by(|&(a, b), &(c, d)| {
                (projected[a] - projected[b])
                    .length_squared()
                    .total_cmp(&(projected[c] - projected[d]).length_squared())
            })
            .unwrap();
        let radial = (projected[a] + projected[b]) / 2. - center;
        let normal = DVec2::new(n[uv[0]], n[uv[1]]);
        if (projected[a] - projected[b]).length() > f64::from(radius) * 1.5
            || radial.normalize().dot(normal.normalize()).abs() < 0.98
        {
            continue;
        }
        if coverage_degrees < 180. {
            let half_chord = (projected[a] - projected[b]).length() / 2.;
            let offset = (f64::from(radius).powi(2) - half_chord * half_chord)
                .max(0.)
                .sqrt();
            let midpoint = (projected[a] + projected[b]) / 2.;
            let direction = normal.normalize() * offset;
            if (midpoint + direction - center)
                .length()
                .min((midpoint - direction - center).length())
                > 0.06
            {
                continue;
            }
        }
        if projected
            .iter()
            .any(|p| ((*p - center).length() - f64::from(radius)).abs() > 0.18)
        {
            continue;
        }
        for p in projected {
            points.insert(
                [(p.x * 10000.).round() as i64, (p.y * 10000.).round() as i64],
                p,
            );
        }
    }
    let points: Vec<_> = points.into_values().collect();
    if points.len() < if coverage_degrees < 180. { 4 } else { 12 } {
        return Err(format!(
            "Cylinder arc has only {} distinct vertices (axis {axis}, seed {seed:?})",
            points.len()
        ));
    }
    let (center, radius, residual) = fit_circle(&points)
        .ok_or_else(|| format!("Cylinder arc could not be fitted (axis {axis}, seed {seed:?})"))?;
    if residual > 0.035 {
        return Err(format!(
            "Cylinder arc residual {residual:.4} mm (axis {axis}, seed {seed:?})"
        ));
    }
    let mut angles: Vec<_> = points
        .iter()
        .map(|p| (p.y - center.y).atan2(p.x - center.x))
        .collect();
    angles.sort_by(f64::total_cmp);
    let gap = angles.windows(2).map(|w| w[1] - w[0]).fold(
        angles[0] + std::f64::consts::TAU - angles[angles.len() - 1],
        f64::max,
    );
    if std::f64::consts::TAU - gap < f64::from(coverage_degrees.to_radians()) {
        return Err(format!(
            "Cylinder arc covers less than {coverage_degrees} degrees"
        ));
    }
    let mut point = seed;
    point[uv[0]] = center.x as f32;
    point[uv[1]] = center.y as f32;
    Ok(CircleDatum {
        point,
        radius: radius as f32,
        residual: residual as f32,
    })
}
pub fn cylinder_limits(
    mesh: &Mesh,
    matrix: Mat4,
    axis: usize,
    datum: &CircleDatum,
) -> Result<[f32; 2], String> {
    let uv: Vec<_> = (0..3).filter(|&a| a != axis).collect();
    let center = DVec2::new(f64::from(datum.point[uv[0]]), f64::from(datum.point[uv[1]]));
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for tri in mesh.vertices.chunks_exact(3) {
        let p: [DVec3; 3] = std::array::from_fn(|i| precise_point(matrix, tri[i].position));
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        if n.length() < 1e-10 || n[axis].abs() > n.length() * 0.003 {
            continue;
        }
        if p.iter().any(|p| {
            ((DVec2::new(p[uv[0]], p[uv[1]]) - center).length() - f64::from(datum.radius)).abs()
                > 0.035
        }) {
            continue;
        }
        for p in p {
            lo = lo.min(p[axis]);
            hi = hi.max(p[axis]);
        }
    }
    if !lo.is_finite() || hi - lo < 1. {
        return Err("Cylinder land interval not found".into());
    }
    Ok([lo as f32, hi as f32])
}
/// Axial stations at which a rod's rounded head reaches a bore's radius.
/// This locates a real head profile, rather than guessing from its bounding box.
pub fn cylinder_crossings(
    mesh: &Mesh,
    matrix: Mat4,
    axis: usize,
    center: Vec3,
    radius: f32,
) -> Result<[f32; 2], String> {
    let uv: Vec<_> = (0..3).filter(|&a| a != axis).collect();
    let c = DVec2::new(f64::from(center[uv[0]]), f64::from(center[uv[1]]));
    let r = f64::from(radius);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for tri in mesh.vertices.chunks_exact(3) {
        let p: [DVec3; 3] = std::array::from_fn(|i| precise_point(matrix, tri[i].position));
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let pa = DVec2::new(p[a][uv[0]], p[a][uv[1]]) - c;
            let pb = DVec2::new(p[b][uv[0]], p[b][uv[1]]) - c;
            if (pa.length() - r) * (pb.length() - r) >= 0. {
                continue;
            }
            let d = pb - pa;
            let aa = d.length_squared();
            let bb = 2. * pa.dot(d);
            let cc = pa.length_squared() - r * r;
            let disc = bb * bb - 4. * aa * cc;
            if aa < 1e-12 || disc < 0. {
                continue;
            }
            for t in [
                (-bb - disc.sqrt()) / (2. * aa),
                (-bb + disc.sqrt()) / (2. * aa),
            ] {
                if (0. ..=1.).contains(&t) {
                    let v = p[a][axis] + (p[b][axis] - p[a][axis]) * t;
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
    }
    if !lo.is_finite() {
        return Err("Rod head / bore-radius station not found".into());
    }
    Ok([lo as f32, hi as f32])
}
/// The pins are intentionally bowed free-state meshes. Only their insertion
/// face and local entry center are located here; no elastic fit is asserted.
pub fn elastic_pin_entry(mesh: &Mesh) -> Result<(Mat4, CircleDatum), String> {
    let raw = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let min_x = mesh.min.x;
    let mut normal = DVec3::ZERO;
    for tri in mesh.vertices.chunks_exact(3) {
        let p: [DVec3; 3] = std::array::from_fn(|i| precise_point(raw, tri[i].position));
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        if n.x < -n.length() * 0.98 && (p[0].x + p[1].x + p[2].x) / 3. < f64::from(min_x + 2.) {
            normal -= n;
        }
    }
    if normal.length() < 1e-6 {
        return Err("Elastic pin insertion face not found".into());
    }
    let frame = Mat4::from_quat(Quat::from_rotation_arc(
        normal.normalize().as_vec3(),
        Vec3::X,
    )) * raw;
    let lo = mesh
        .vertices
        .iter()
        .map(|v| frame.transform_point3(v.position.into()).x)
        .fold(f32::INFINITY, f32::min);
    let window = [Vec3::splat(f32::NEG_INFINITY), Vec3::splat(f32::INFINITY)];
    let cap = plane_patch(mesh, frame, 0, -1., lo, window, 1.5)?;
    let lines = section_segments(mesh, frame, 0, cap.level + 0.35);
    let min_z = lines
        .iter()
        .flatten()
        .map(|p| p.z)
        .fold(f32::INFINITY, f32::min);
    let mut unique = BTreeMap::<[i64; 2], DVec2>::new();
    for p in lines.iter().flatten().filter(|p| p.z > min_z + 0.7) {
        unique.insert(
            [
                (f64::from(p.y) * 10000.).round() as i64,
                (f64::from(p.z) * 10000.).round() as i64,
            ],
            DVec2::new(f64::from(p.y), f64::from(p.z)),
        );
    }
    let points: Vec<_> = unique.into_values().collect();
    let (center, radius, residual) =
        fit_circle(&points).ok_or("Elastic pin entry arc not found")?;
    if residual > 0.1 || !(1.5..=3.2).contains(&radius) {
        return Err(format!("Elastic pin entry arc residual {residual:.4}"));
    }
    Ok((
        frame,
        CircleDatum {
            point: Vec3::new(cap.level, center.x as f32, center.y as f32),
            radius: radius as f32,
            residual: residual as f32,
        },
    ))
}

/// Assembly-only relaxed display approximation for the deliberately bowed
/// elastic pins. Center successive sections on the insertion axis and bound
/// the nominal 5.5 mm diameter. Source/print meshes retain their free shape.
/// This is a visual approximation, not a stress/deformation solution.
pub fn installed_pin_display(
    mesh: &Mesh,
    frame: Mat4,
    cap: &CircleDatum,
) -> Result<Arc<Mesh>, String> {
    let end = mesh
        .vertices
        .iter()
        .map(|v| frame.transform_point3(v.position.into()).x)
        .fold(f32::NEG_INFINITY, f32::max);
    let length = end - cap.point.x;
    if !(10. ..30.).contains(&length) {
        return Err("Unexpected elastic pin length".into());
    }
    let mut sections = Vec::new();
    for station in 0..=48 {
        let x = cap.point.x + 0.15 + (length - 0.3) * station as f32 / 48.;
        let lines = section_segments(mesh, frame, 0, x);
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in lines.iter().flatten() {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if lo.is_finite() && (hi.y - lo.y) > 1. {
            sections.push((x, (lo + hi) / 2.));
        }
    }
    if sections.len() < 20 {
        return Err("Elastic pin sections are incomplete".into());
    }
    let inverse = frame.inverse();
    let mut vertices = mesh.vertices.clone();
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for v in &mut vertices {
        let mut p = frame.transform_point3(v.position.into());
        let right = sections
            .partition_point(|(x, _)| *x < p.x)
            .min(sections.len() - 1);
        let left = right.saturating_sub(1);
        let t = if right == left {
            0.
        } else {
            ((p.x - sections[left].0) / (sections[right].0 - sections[left].0)).clamp(0., 1.)
        };
        let center = sections[left].1.lerp(sections[right].1, t);
        let mut radial = Vec3::new(0., p.y - center.y, p.z - center.z);
        if radial.length() > 2.75 {
            radial = radial.normalize() * 2.75;
        }
        p.y = cap.point.y + radial.y;
        p.z = cap.point.z + radial.z;
        let p = inverse.transform_point3(p);
        v.position = p.to_array();
        lo = lo.min(p);
        hi = hi.max(p);
    }
    for tri in vertices.chunks_exact_mut(3) {
        let a = Vec3::from(tri[0].position);
        let b = Vec3::from(tri[1].position);
        let c = Vec3::from(tri[2].position);
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        for v in tri {
            v.normal = n;
        }
    }
    Ok(Arc::new(Mesh {
        vertices,
        size: mesh.size,
        min: lo,
        max: hi,
    }))
}

#[derive(Clone, Debug)]
pub struct PlaneDatum {
    pub level: f32,
    pub center: Vec3,
    pub min: Vec3,
    pub max: Vec3,
    pub area: f32,
}
/// Local planar patch; normal sign and a window prevent choosing the opposite
/// outside face of a wall instead of its actual recessed mounting seat.
pub fn plane_patch(
    mesh: &Mesh,
    matrix: Mat4,
    axis: usize,
    sign: f32,
    seed: f32,
    window: [Vec3; 2],
    tolerance: f32,
) -> Result<PlaneDatum, String> {
    let mut levels = BTreeMap::<i32, (f64, DVec3, DVec3, DVec3)>::new();
    for tri in mesh.vertices.chunks_exact(3) {
        let p: [DVec3; 3] = std::array::from_fn(|i| precise_point(matrix, tri[i].position));
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        let area = n.length() / 2.;
        if area < 1e-8 || n[axis] * f64::from(sign) < n.length() * 0.9999 {
            continue;
        }
        let center = (p[0] + p[1] + p[2]) / 3.;
        if (center[axis] - f64::from(seed)).abs() > f64::from(tolerance)
            || (0..3).any(|a| {
                a != axis
                    && (center[a] < f64::from(window[0][a]) || center[a] > f64::from(window[1][a]))
            })
        {
            continue;
        }
        let entry = levels
            .entry((center[axis] * 100.).round() as i32)
            .or_insert((
                0.,
                DVec3::ZERO,
                DVec3::splat(f64::INFINITY),
                DVec3::splat(f64::NEG_INFINITY),
            ));
        entry.0 += area;
        entry.1 += center * area;
        for v in p {
            entry.2 = entry.2.min(v);
            entry.3 = entry.3.max(v);
        }
    }
    // A plane may straddle two quantization bins in translated source meshes.
    let chosen = levels
        .iter()
        .max_by(|a, b| a.1.0.total_cmp(&b.1.0))
        .ok_or("Mounting plane not found")?
        .0;
    let mut area = 0.;
    let mut center = DVec3::ZERO;
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for (key, (a, c, l, h)) in &levels {
        if (key - chosen).abs() <= 1 {
            area += a;
            center += *c;
            lo = lo.min(*l);
            hi = hi.max(*h);
        }
    }
    center /= area;
    Ok(PlaneDatum {
        level: center[axis] as f32,
        center: center.as_vec3(),
        min: lo.as_vec3(),
        max: hi.as_vec3(),
        area: area as f32,
    })
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}
fn split_connected(source: &Mesh) -> Result<Vec<(Mesh, Vec3)>, String> {
    let triangles = source.vertices.len() / 3;
    let mut parent: Vec<_> = (0..triangles).collect();
    let mut rank = vec![0u8; triangles];
    let mut seen: HashMap<[i64; 3], usize> = HashMap::new();
    // Weld coincident vertices for topology only; never move source positions.
    // 0.0001 mm is below the precision of this package's translated coordinates.
    for (i, tri) in source.vertices.chunks_exact(3).enumerate() {
        for v in tri {
            let key = v.position.map(|x| (f64::from(x) * 10000.).round() as i64);
            if let Some(&other) = seen.get(&key) {
                let a = root(&mut parent, i);
                let b = root(&mut parent, other);
                if a != b {
                    if rank[a] < rank[b] {
                        parent[a] = b;
                    } else {
                        parent[b] = a;
                        if rank[a] == rank[b] {
                            rank[a] += 1;
                        }
                    }
                }
            } else {
                seen.insert(key, i);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<Vertex>> = BTreeMap::new();
    for (i, tri) in source.vertices.chunks_exact(3).enumerate() {
        groups
            .entry(root(&mut parent, i))
            .or_default()
            .extend_from_slice(tri);
    }
    let mut result = Vec::with_capacity(groups.len());
    for (_, mut vertices) in groups {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for v in &vertices {
            lo = lo.min(v.position.into());
            hi = hi.max(v.position.into());
        }
        let center = (lo + hi) * 0.5;
        let span = hi - lo;
        if !center.is_finite() {
            return Err("Invalid display body bounds".into());
        }
        for v in &mut vertices {
            v.position = (Vec3::from(v.position) - center).to_array();
        }
        let mesh = Mesh {
            vertices,
            size: Vec3::new(span.x, span.z, span.y),
            min: lo - center,
            max: hi - center,
        };
        result.push((mesh, center));
    }
    // Stable identifiers independent of STL triangle order. Use original X/Y/Z.
    result.sort_by(|a, b| {
        a.1.x
            .total_cmp(&b.1.x)
            .then((-a.1.z).total_cmp(&(-b.1.z)))
            .then(a.1.y.total_cmp(&b.1.y))
    });
    Ok(result)
}
/// Assembly-only removal of the explicitly documented sacrificial sheets in
/// standard drawer side tracks (manual step 4). Conservative triangle selection:
/// leaves tiny attachment stubs, never scales/cuts the structural dovetail walls.
/// Standalone STL viewing and export continue to use the source mesh/file.
pub fn remove_runner_supports(mesh: &Arc<Mesh>) -> Arc<Mesh> {
    let half = mesh.size.x * 0.5;
    // Recognize the 12.49 mm dovetail floor before applying this package recipe.
    let mut levels = Vec::new();
    let mut floor_triangles = Vec::new();
    for t in mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(t[0].position);
        let b = Vec3::from(t[1].position);
        let c = Vec3::from(t[2].position);
        let n = (b - a).cross(c - a);
        if ((a.x + b.x + c.x) / 3. - (half - 2.2)).abs() < 0.02 && n.x.abs() > n.length() * 0.999 {
            levels.extend([a.y, b.y, c.y]);
            floor_triangles.push([a, b, c]);
        }
    }
    if levels.is_empty() {
        return mesh.clone();
    }
    let low = levels.iter().copied().fold(f32::INFINITY, f32::min);
    let high = levels.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let rows = ((high - low - 12.4914) / 24.).round() as i32;
    if !(0..=8).contains(&rows) || (high - low - (12.4914 + rows as f32 * 24.)).abs() > 0.05 {
        return mesh.clone();
    }
    let mut tracks = Vec::new();
    for row in 0..=rows {
        let center = low + 6.2457 + row as f32 * 24.;
        let faces: Vec<_> = floor_triangles
            .iter()
            .filter(|t| t.iter().all(|p| (p.y - center).abs() < 6.3))
            .collect();
        let area: f32 = faces
            .iter()
            .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5)
            .sum();
        if area < 100. {
            continue;
        }
        let mut zmin = f32::INFINITY;
        let mut zmax = f32::NEG_INFINITY;
        for t in faces {
            for p in t {
                zmin = zmin.min(p.z);
                zmax = zmax.max(p.z);
            }
        }
        tracks.push((center, zmin, zmax));
    }
    let vertices: Vec<_> = mesh
        .vertices
        .chunks_exact(3)
        .filter(|tri| {
            !tracks.iter().any(|&(center, zmin, zmax)| {
                tri.iter().all(|v| {
                    let p = Vec3::from(v.position);
                    p.x.abs() > half - 1.26
                        && p.x.abs() < half - 0.64
                        && (p.y - center).abs() < 5.32
                        && p.z >= zmin - 0.01
                        && p.z <= zmax + 0.01
                })
            })
        })
        .flatten()
        .copied()
        .collect();
    if vertices.len() == mesh.vertices.len() {
        return mesh.clone();
    }
    Arc::new(Mesh {
        vertices,
        size: mesh.size,
        min: mesh.min,
        max: mesh.max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cylindrical_wall() -> Mesh {
        let mut vertices = Vec::new();
        for i in 0..96 {
            let a = i as f32 * std::f32::consts::TAU / 96.;
            let b = (i + 1) as f32 * std::f32::consts::TAU / 96.;
            let p = |angle: f32, y: f32| [3. + 2. * angle.cos(), y, 7. + 2. * angle.sin()];
            for position in [
                p(a, -4.),
                p(b, -4.),
                p(b, 4.),
                p(a, -4.),
                p(b, 4.),
                p(a, 4.),
            ] {
                vertices.push(Vertex {
                    position,
                    normal: [0.; 3],
                    shade: 1.,
                });
            }
        }
        Mesh {
            vertices,
            size: Vec3::new(4., 8., 4.),
            min: Vec3::new(1., -4., 5.),
            max: Vec3::new(5., 4., 9.),
        }
    }
    #[test]
    fn extracts_offset_bore_and_two_independent_sections() {
        let mesh = cylindrical_wall();
        let transform = Mat4::from_translation(Vec3::new(-1., 2., 3.));
        let axis = cylinder_axis(&mesh, transform, 1, Vec3::new(2.04, 0., 9.96), 2.).unwrap();
        assert!((axis.point.x - 2.).abs() < 0.001 && (axis.point.z - 10.).abs() < 0.001);
        assert!((axis.radius - 2.).abs() < 0.001 && axis.residual < 0.001);
        let limits = cylinder_limits(&mesh, transform, 1, &axis).unwrap();
        assert!((limits[0] + 2.).abs() < 0.001 && (limits[1] - 6.).abs() < 0.001);
        for level in [-1., 4.] {
            let holes = section_holes(&mesh, transform, 1, level, [1.9, 2.1]);
            assert_eq!(holes.len(), 1);
            assert!((holes[0].point - Vec3::new(2., level, 10.)).length() < 0.005);
        }
        assert!(cylinder_axis(&mesh, transform, 0, Vec3::ZERO, 2.).is_err());
    }
    #[test]
    fn rejects_incomplete_cylinder_arc() {
        let mut mesh = cylindrical_wall();
        mesh.vertices.truncate(12 * 6);
        assert!(cylinder_axis(&mesh, Mat4::IDENTITY, 1, Vec3::new(3., 0., 7.), 2.).is_err());
        let partial =
            cylinder_arc(&mesh, Mat4::IDENTITY, 1, Vec3::new(3., 0., 7.), 2., 30.).unwrap();
        assert!((partial.radius - 2.).abs() < 0.001);
    }
    fn pair() -> Arc<Mesh> {
        Arc::new(Mesh::parse(b"solid pair\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nvertex 0 3 0\nvertex 1 3 0\nvertex 0 4 0\nendsolid").unwrap())
    }
    #[test]
    fn splits_without_mutating_or_discarding() {
        let source = pair();
        let original = bytemuck::cast_slice::<Vertex, u8>(&source.vertices).to_vec();
        let r = Rules::default();
        let bodies = r.prepare("Springs.stl", &source).unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(
            bodies.iter().map(|b| b.mesh.vertices.len()).sum::<usize>(),
            source.vertices.len()
        );
        assert_eq!(
            original,
            bytemuck::cast_slice::<Vertex, u8>(&source.vertices)
        );
        assert!(bodies[0].source_center.z > bodies[1].source_center.z);
        assert_ne!(bodies[0].id, bodies[1].id);
    }
    #[test]
    fn unconfigured_mesh_retains_source_arc() {
        let source = pair();
        let b = Rules::default().prepare("other.stl", &source).unwrap();
        assert_eq!(b.len(), 1);
        assert!(Arc::ptr_eq(&b[0].mesh, &source));
        assert_eq!(b[0].id, None);
    }
    #[test]
    fn mismatch_is_error_not_silent_geometry_loss() {
        let source = pair();
        let r = Rules {
            rules: vec![Rule {
                path: "pair.stl".into(),
                split_disconnected: true,
                expected_bodies: 3,
            }],
        };
        assert!(r.prepare("pair.stl", &source).is_err());
    }
    #[test]
    fn triangle_order_does_not_change_body_ids() {
        let source = pair();
        let mut reversed = (*source).clone();
        reversed.vertices.rotate_left(3);
        let r = Rules::default();
        let a = r.prepare("Springs.stl", &source).unwrap();
        let b = r.prepare("Springs.stl", &Arc::new(reversed)).unwrap();
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.source_center, y.source_center);
            assert_eq!(x.id, y.id);
        }
    }
}
