//! Local print-support painting in source print orientation, never display poses.
use crate::mesh::Mesh;
use glam::Vec3;
use std::collections::HashMap;

/// Recognize the lower module clip undersides by their measured planar footprint.
/// Unknown geometry is left unpainted; this is not a general overhang detector.
pub fn module_clips(mesh: &Mesh, expected: usize) -> Vec<bool> {
    // L/1F sources are already rotated 45 degrees on the print bed.
    for angle in [0., std::f32::consts::FRAC_PI_4] {
        let mask = oriented_clips(mesh, expected, angle);
        if mask.iter().any(|&marked| marked) {
            return mask;
        }
    }
    vec![false; mesh.vertices.len() / 3]
}
fn oriented_clips(mesh: &Mesh, expected: usize, angle: f32) -> Vec<bool> {
    let (sin, cos) = angle.sin_cos();
    let rotate = |p: Vec3| Vec3::new(cos * p.x - sin * p.z, p.y, sin * p.x + cos * p.z);
    let front = mesh
        .vertices
        .iter()
        .map(|v| rotate(Vec3::from(v.position)).z)
        .fold(f32::NEG_INFINITY, f32::max);
    let mut result = vec![false; mesh.vertices.len() / 3];
    let mut candidates = Vec::new();
    let mut neighbours: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
    let key = |p: Vec3| p.to_array().map(|v| (v * 100.).round() as i32);
    for (index, tri) in mesh.vertices.chunks_exact(3).enumerate() {
        let p: [Vec3; 3] = std::array::from_fn(|i| rotate(Vec3::from(tri[i].position)));
        let cross = (p[1] - p[0]).cross(p[2] - p[0]);
        // Viewer Y is print Z; oriented max Z is the outside of the module front.
        if cross.y >= -0.999 * cross.length()
            || !p.iter().all(|p| {
                (p.y - mesh.min.y - 12.73).abs() < 0.12 && (0.15..=5.3).contains(&(front - p.z))
            })
        {
            continue;
        }
        let local = candidates.len();
        for point in p {
            neighbours.entry(key(point)).or_default().push(local);
        }
        candidates.push((index, p, cross.length() * 0.5));
    }
    let mut seen = vec![false; candidates.len()];
    let mut patches = Vec::new();
    for start in 0..candidates.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut indices = Vec::new();
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        let mut area = 0.;
        while let Some(i) = stack.pop() {
            let (index, points, triangle_area) = candidates[i];
            indices.push(index);
            area += triangle_area;
            for p in points {
                lo = lo.min(p);
                hi = hi.max(p);
                for &adjacent in &neighbours[&key(p)] {
                    if !seen[adjacent] {
                        seen[adjacent] = true;
                        stack.push(adjacent);
                    }
                }
            }
        }
        let size = hi - lo;
        if (8.8..=9.4).contains(&size.x)
            && (4.3..=5.1).contains(&size.z)
            && (26.0..=38.0).contains(&area)
        {
            patches.push(indices);
        }
    }
    if patches.len() == expected && expected > 0 {
        for index in patches.into_iter().flatten() {
            result[index] = true;
        }
    }
    result
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture() -> Mesh {
        let mut data = String::from("solid clips\n");
        let mut add = |p: [[f32; 3]; 3]| {
            for [x, y, z] in p {
                data += &format!("vertex {x} {y} {z}\n");
            }
        };
        // Two trapezoidal downward seats with the same footprint as the module.
        for x in [32., 122.] {
            let p = [
                [x, 0.238, 12.73],
                [x + 9.125, 0.238, 12.73],
                [x + 7.31, 5.209, 12.73],
                [x + 1.815, 5.209, 12.73],
            ];
            add([p[0], p[2], p[1]]);
            add([p[0], p[3], p[2]]);
        }
        // Establish print floor and front/back bounds, and an unpainted top.
        add([[0., 0., 0.], [165., 57., 0.], [165., 0., 0.]]);
        add([[0., 0., 30.], [165., 0., 30.], [165., 57., 30.]]);
        Mesh::parse(data.as_bytes()).unwrap()
    }
    #[test]
    fn paint_only_recognized_undersides_and_follow_translation() {
        let mut mesh = fixture();
        assert_eq!(
            module_clips(&mesh, 2),
            vec![true, true, true, true, false, false]
        );
        let shift = Vec3::new(231., -51., 83.);
        for v in &mut mesh.vertices {
            v.position = (Vec3::from(v.position) + shift).to_array();
        }
        mesh.min += shift;
        mesh.max += shift;
        assert_eq!(
            module_clips(&mesh, 2),
            vec![true, true, true, true, false, false]
        );
        assert!(!module_clips(&mesh, 4).iter().any(|&x| x));
        for v in &mut mesh.vertices {
            v.position = (glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_4)
                * Vec3::from(v.position))
            .to_array();
        }
        assert_eq!(
            module_clips(&mesh, 2),
            vec![true, true, true, true, false, false]
        );
        // Reverse one seat: do not paint its upper surface or export a partial pair.
        mesh.vertices.swap(1, 2);
        mesh.vertices.swap(4, 5);
        assert!(!module_clips(&mesh, 2).iter().any(|&x| x));
    }
    #[test]
    fn changed_geometry_is_not_silently_painted() {
        let mut mesh = fixture();
        for v in &mut mesh.vertices[..12] {
            v.position[1] += 0.2;
        }
        assert!(!module_clips(&mesh, 2).iter().any(|&x| x));
    }
    #[test]
    fn standard_module_paths_only() {
        use crate::catalog::module_clip_count;
        assert_eq!(
            module_clip_count("01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F.stl"),
            Some(2)
        );
        assert_eq!(
            module_clip_count("01. DRAWERS/03. L Families/L7/Modules/L7 Module 4F.stl"),
            Some(4)
        );
        for path in [
            "S3 Module 1F.stl",
            "01. DRAWERS/01. S Families/S3/Boxes/S3 Box 1F.stl",
            "01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F Part 1.stl",
        ] {
            assert_eq!(module_clip_count(path), None);
        }
    }
    #[test]
    fn local_library_module_clip_coverage() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let catalog = crate::catalog::Catalog::scan(root.into()).unwrap();
        let mut count = 0;
        for entry in &catalog.entries {
            let Some(expected) = crate::catalog::module_clip_count(&entry.path) else {
                continue;
            };
            // Respect the existing STL loader limit; do not expand it for this feature.
            let source = catalog.root.join(&entry.path);
            let mut header = [0u8; 84];
            std::io::Read::read_exact(&mut std::fs::File::open(&source).unwrap(), &mut header)
                .unwrap();
            if u32::from_le_bytes(header[80..84].try_into().unwrap()) > 4_000_000 {
                eprintln!(
                    "Skipped source above existing triangle limit: {}",
                    entry.path
                );
                continue;
            }
            let mesh = Mesh::load(&source).unwrap_or_else(|e| panic!("{}: {e}", entry.path));
            let mask = module_clips(&mesh, expected);
            assert!(
                mask.iter().any(|&x| x),
                "No clip seats recognized: {}",
                entry.path
            );
            count += 1;
        }
        assert!(count >= 40, "Expected standard module families in library");
        eprintln!("Recognized lower clip seats in {count} standard modules");
    }
}
