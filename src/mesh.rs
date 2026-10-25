use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::{path::Path, sync::Arc};

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
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.len() > 250_000_000 {
            return Err("STL exceeds 250 MB".into());
        }
        Self::parse(&std::fs::read(path).map_err(|e| e.to_string())?).map(Arc::new)
    }
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let count = if data.len() >= 84 {
            u32::from_le_bytes(data[80..84].try_into().unwrap()) as usize
        } else {
            0
        };
        let binary = count > 0 && count <= 4_000_000 && 84 + count * 50 <= data.len();
        let mut points = Vec::new();
        if binary {
            points.reserve(count * 3);
            for i in 0..count {
                for j in 0..3 {
                    let o = 84 + i * 50 + 12 + j * 12;
                    let v = Vec3::new(
                        read_float(data, o),
                        read_float(data, o + 4),
                        read_float(data, o + 8),
                    );
                    if !v.is_finite() {
                        return Err("Non-finite STL coordinate".into());
                    }
                    points.push(v);
                }
            }
        } else {
            let text = std::str::from_utf8(data).map_err(|_| "Invalid binary or ASCII STL")?;
            if !text.trim_start().starts_with("solid") {
                return Err("Invalid ASCII STL header".into());
            }
            for line in text.lines() {
                let mut p = line.split_whitespace();
                if p.next() == Some("vertex") {
                    let mut xyz = [0.; 3];
                    for value in &mut xyz {
                        *value = p
                            .next()
                            .ok_or("Incomplete STL vertex")?
                            .parse::<f32>()
                            .map_err(|_| "Invalid STL number")?;
                    }
                    let v = Vec3::from(xyz);
                    if !v.is_finite() {
                        return Err("Non-finite STL coordinate".into());
                    }
                    points.push(v);
                    if points.len() > 12_000_000 {
                        return Err("Too many triangles".into());
                    }
                }
            }
        }
        if points.is_empty() || points.len() % 3 != 0 {
            return Err("Empty or incomplete STL triangles".into());
        }
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for p in &points {
            min = min.min(*p);
            max = max.max(*p);
        }
        let center = (min + max) * 0.5;
        let size = max - min;
        if size.max_element() < 1e-6 {
            return Err("Degenerate STL".into());
        }
        // Use a local origin to retain precision for files translated thousands of mm.
        for p in &mut points {
            let v = *p - center;
            *p = Vec3::new(v.x, v.z, -v.y);
        }
        let mut vertices = Vec::with_capacity(points.len());
        for tri in points.chunks_exact(3) {
            let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
            for p in tri {
                vertices.push(Vertex {
                    position: p.to_array(),
                    normal: n.to_array(),
                    shade: 1.,
                });
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
