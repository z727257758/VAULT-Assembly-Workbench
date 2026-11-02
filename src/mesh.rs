use bytemuck::{Pod, Zeroable};
use glam::Vec3;
#[cfg(test)]
use std::io::Cursor;
use std::{
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};
/// Per-source geometry budget, independent of ASCII/binary file encoding.
pub const MAX_TRIANGLES: usize = 8_000_000;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub shade: f32,
}
#[derive(Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub size: Vec3,
    pub min: Vec3,
    pub max: Vec3,
}
impl Mesh {
    pub fn load(path: &Path) -> Result<Arc<Self>, String> {
        Self::load_with_budget(path, MAX_TRIANGLES)
    }
    pub(crate) fn load_with_budget(path: &Path, remaining: usize) -> Result<Arc<Self>, String> {
        let limit = remaining.min(MAX_TRIANGLES);
        if limit == 0 {
            return Err("Scene triangle budget exhausted before reading next source".into());
        }
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        Self::read_limited(BufReader::with_capacity(1024 * 1024, file), length, limit).map(Arc::new)
    }
    #[cfg(test)]
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        Self::read(Cursor::new(data), data.len() as u64)
    }
    #[cfg(test)]
    fn read(reader: impl BufRead + Seek, length: u64) -> Result<Self, String> {
        Self::read_limited(reader, length, MAX_TRIANGLES)
    }
    fn read_limited(
        mut reader: impl BufRead + Seek,
        length: u64,
        limit: usize,
    ) -> Result<Self, String> {
        let mut header = [0u8; 84];
        let count = if length >= 84 {
            reader.read_exact(&mut header).map_err(|e| e.to_string())?;
            u32::from_le_bytes(header[80..84].try_into().unwrap()) as usize
        } else {
            0
        };
        let binary = count > 0 && 84 + count as u64 * 50 <= length;
        let mut vertices = Vec::new();
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let mut push = |p: Vec3, vertices: &mut Vec<Vertex>| -> Result<(), String> {
            if !p.is_finite() {
                return Err("Non-finite STL coordinate".into());
            }
            if vertices.len() >= limit * 3 {
                return Err(format!("STL exceeds {limit} triangles"));
            }
            if vertices.len() == vertices.capacity() {
                vertices
                    .try_reserve_exact((limit * 3 - vertices.len()).min(65536))
                    .map_err(|e| format!("STL vertex allocation: {e}"))?;
            }
            min = min.min(p);
            max = max.max(p);
            vertices.push(Vertex {
                position: p.to_array(),
                normal: [0.; 3],
                shade: 1.,
            });
            Ok(())
        };
        if binary {
            if count > limit {
                return Err(format!("STL exceeds {limit} triangles ({count})"));
            }
            vertices
                .try_reserve_exact(count * 3)
                .map_err(|e| format!("STL vertex allocation: {e}"))?;
            let mut triangle = [0u8; 50];
            for _ in 0..count {
                reader
                    .read_exact(&mut triangle)
                    .map_err(|e| format!("Truncated binary STL: {e}"))?;
                for j in 0..3 {
                    let o = 12 + j * 12;
                    push(
                        Vec3::new(
                            read_float(&triangle, o),
                            read_float(&triangle, o + 4),
                            read_float(&triangle, o + 8),
                        ),
                        &mut vertices,
                    )?;
                }
            }
        } else {
            reader.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            let mut line = Vec::new();
            let mut saw_header = false;
            loop {
                line.clear();
                // Bound ASCII record memory, even for malformed inputs without newlines.
                let n = reader
                    .by_ref()
                    .take(65537)
                    .read_until(b'\n', &mut line)
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                if n > 65536 {
                    return Err("ASCII STL line exceeds 64 KiB".into());
                }
                let text = std::str::from_utf8(&line)
                    .map_err(|_| "Invalid or truncated binary/ASCII STL")?;
                let mut words = text.split_whitespace();
                let Some(word) = words.next() else {
                    continue;
                };
                if !saw_header {
                    if word != "solid" {
                        return Err("Invalid or truncated binary/ASCII STL header".into());
                    }
                    saw_header = true;
                    continue;
                }
                if word == "vertex" {
                    let mut xyz = [0.; 3];
                    for value in &mut xyz {
                        *value = words
                            .next()
                            .ok_or("Incomplete STL vertex")?
                            .parse::<f32>()
                            .map_err(|_| "Invalid STL number")?;
                    }
                    push(Vec3::from(xyz), &mut vertices)?;
                }
            }
        }
        if vertices.is_empty() || vertices.len() % 3 != 0 {
            return Err("Empty or incomplete STL triangles".into());
        }
        let center = min * 0.5 + max * 0.5;
        let size = max - min;
        if !size.is_finite() || size.max_element() < 1e-6 {
            return Err("Degenerate or out-of-range STL".into());
        }
        // Recenter in place: no full file buffer or second array of all coordinates.
        for triangle in vertices.chunks_exact_mut(3) {
            for vertex in triangle.iter_mut() {
                let p = Vec3::from(vertex.position) - center;
                vertex.position = [p.x, p.z, -p.y];
            }
            let a = Vec3::from(triangle[0].position);
            let b = Vec3::from(triangle[1].position);
            let c = Vec3::from(triangle[2].position);
            let normal = (b - a).cross(c - a).normalize_or_zero().to_array();
            for vertex in triangle {
                vertex.normal = normal;
            }
        }
        let half = Vec3::new(size.x, size.z, size.y) * 0.5;
        Ok(Self {
            vertices,
            size,
            min: -half,
            max: half,
        })
    }
}
fn read_float(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remaining_scene_budget_limits_binary_reservation_and_ascii_streaming() {
        let mut header = [0u8; 84];
        header[80..84].copy_from_slice(&3u32.to_le_bytes());
        let error = Mesh::read_limited(Cursor::new(header), 234, 2)
            .err()
            .unwrap();
        assert!(error.contains("exceeds 2 triangles"), "{error}");
        let ascii = b"solid x\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nvertex 0 0 1\nvertex 1 0 1\nvertex 0 1 1\nendsolid";
        assert!(
            Mesh::read_limited(Cursor::new(ascii), ascii.len() as u64, 1)
                .err()
                .unwrap()
                .contains("exceeds 1 triangles")
        );
    }
    #[test]
    fn rejects_truncated_binary_and_bounds_ascii_record_memory() {
        let mut bytes = vec![0u8; 134];
        bytes[80..84].copy_from_slice(&2u32.to_le_bytes());
        assert!(Mesh::parse(&bytes).is_err());
        assert!(
            Mesh::parse(&[b'a'; 65537])
                .err()
                .unwrap()
                .contains("64 KiB")
        );
    }
    #[test]
    fn rejects_over_budget_header_before_allocating() {
        let mut header = [0u8; 84];
        header[80..84].copy_from_slice(&((MAX_TRIANGLES + 1) as u32).to_le_bytes());
        let error = Mesh::read(Cursor::new(header), 84 + (MAX_TRIANGLES as u64 + 1) * 50)
            .err()
            .unwrap();
        assert!(error.contains("triangles"));
    }
    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn large_xl_drawer_library_regression() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let root = std::path::PathBuf::from(root);
        let catalog = crate::catalog::Catalog::scan(root.clone()).unwrap();
        let selection = crate::catalog::Selection {
            width: "XL".into(),
            ..Default::default()
        };
        let scene =
            crate::scene::Scene::load(&root, &catalog.bom(&selection), "XL3".into(), true).unwrap();
        let module = scene
            .parts
            .iter()
            .find(|p| p.pose.path.contains("/Modules/"))
            .unwrap();
        assert_eq!(module.mesh.vertices.len() / 3, 6_749_642);
        assert!(
            module
                .mesh
                .vertices
                .iter()
                .all(|v| Vec3::from(v.position).is_finite())
        );
        let mut report = format!(
            "Triangles: {}\n",
            scene
                .parts
                .iter()
                .map(|p| p.mesh.vertices.len() / 3)
                .sum::<usize>()
        );
        for p in &scene.parts {
            report.push_str(&format!(
                "{} {:?} rotation={:?} position={:?}\n",
                p.pose.path, p.source_size, p.pose.rotation, p.pose.position
            ));
        }
        std::fs::create_dir_all("output/large-stl").unwrap();
        std::fs::write("output/large-stl/xl3.txt", report).unwrap();
    }
    #[test]
    fn ascii_dimensions_and_recentering() {
        let m=Mesh::parse(b"solid a\nvertex 1000 2000 3000\nvertex 1010 2000 3000\nvertex 1000 2020 3005\nendsolid").unwrap();
        assert_eq!(m.size, Vec3::new(10., 20., 5.));
        assert_eq!(m.vertices[0].position, [-5., -2.5, 10.]);
    }
    #[test]
    fn rejects_empty_and_invalid() {
        assert!(Mesh::parse(b"solid empty\nendsolid").is_err());
        assert!(Mesh::parse(b"solid x\nvertex NaN 0 0").is_err());
        assert!(Mesh::parse(&[0; 84]).is_err());
    }
    #[test]
    fn binary_header_can_start_solid() {
        let mut b = vec![0u8; 134];
        b[..5].copy_from_slice(b"solid");
        b[80..84].copy_from_slice(&1u32.to_le_bytes());
        for (j, p) in [[1f32, 0., 0.], [0., 1., 0.], [0., 0., 1.]]
            .iter()
            .enumerate()
        {
            for (k, v) in p.iter().enumerate() {
                let o = 96 + j * 12 + k * 4;
                b[o..o + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        assert_eq!(Mesh::parse(&b).unwrap().vertices.len(), 3);
    }
}
