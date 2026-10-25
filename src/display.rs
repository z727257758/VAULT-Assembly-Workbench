//! Display preparation is separate from source meshes and print-file quantities.
//! Connectivity is evaluated only for explicitly configured files, never blindly
//! for every STL (a disconnected shell is not always a separate physical part).
use crate::mesh::{Mesh, Vertex};
use glam::Vec3;
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
