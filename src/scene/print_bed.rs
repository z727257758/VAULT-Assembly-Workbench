//! P1S sheet in machine coordinates. The 256 mm print area starts at (0, 0).
use crate::{
    label,
    mesh::{Mesh, Vertex},
};
use eframe::egui::{FontDefinitions, FontFamily};
use glam::Vec3;
use manifold_rust::{linalg::Vec2 as Point, polygon::triangulate, types::Polygons};
use std::sync::{Arc, OnceLock};

pub(crate) const PATH: &str = "@print-bed";
const BOTTOM: f32 = -0.4;
const INK_HEIGHT: f32 = 0.04;
const SHEET: f32 = 1.;
const INK: f32 = 2.;

pub(crate) fn mesh(textured: bool) -> Arc<Mesh> {
    static SHEETS: [OnceLock<Arc<Mesh>>; 2] = [const { OnceLock::new() }; 2];
    Arc::clone(SHEETS[usize::from(textured)].get_or_init(|| Arc::new(build(textured))))
}

/// Each corner gives its location and the distance to trim on its adjoining edges.
fn rounded_outline(corners: &[(f64, f64, f64)]) -> Vec<Point> {
    let point = |i: usize| Point::new(corners[i].0, corners[i].1);
    let mut outline = Vec::with_capacity(corners.len() * 7);
    for (i, &(_, _, trim)) in corners.iter().enumerate() {
        let b = point(i);
        let towards = |neighbour: Point| {
            let v = neighbour - b;
            b + v * (trim / v.x.hypot(v.y))
        };
        let a = towards(point((i + corners.len() - 1) % corners.len()));
        let c = towards(point((i + 1) % corners.len()));
        for step in 0..=6 {
            let t = f64::from(step) / 6.;
            let u = 1. - t;
            outline.push(a * (u * u) + b * (2. * t * u) + c * (t * t));
        }
    }
    outline
}

fn to_world(p: Point, height: f32) -> Vec3 {
    Vec3::new(p.x as f32, height, -p.y as f32)
}

fn triangle(vertices: &mut Vec<Vertex>, points: [Vec3; 3], shade: f32) {
    let [a, b, c] = points;
    if let Some(normal) = (b - a).cross(c - a).try_normalize() {
        vertices.extend(points.map(|point| Vertex {
            position: point.to_array(),
            normal: normal.to_array(),
            shade,
        }));
    }
}

fn faces(outlines: &Polygons, mut emit: impl FnMut([Point; 3])) {
    let points: Vec<_> = outlines.iter().flatten().copied().collect();
    for indices in triangulate(outlines, 1e-8, true) {
        emit([indices.x, indices.y, indices.z].map(|i| points[i as usize]));
    }
}

fn build(textured: bool) -> Mesh {
    // The forward lip extends asymmetrically to the right. A slotted locating
    // ear projects from the middle of the rear edge.
    let outside = rounded_outline(&[
        (0., 0., 6.),
        (71., 0., 2.),
        (82., -10., 3.),
        (258., -10., 6.),
        (258., 258., 6.),
        (164., 258., 2.),
        (155., 266., 3.),
        (103., 266., 3.),
        (94., 258., 2.),
        (0., 258., 6.),
    ]);
    let slot = rounded_outline(&[
        (107., 258., 0.8),
        (107., 260., 0.8),
        (150., 260., 0.8),
        (150., 258., 0.8),
    ]);
    let outlines = vec![outside, slot];
    let mut vertices = Vec::new();
    faces(&outlines, |points| {
        triangle(&mut vertices, points.map(|p| to_world(p, 0.)), SHEET);
        let [a, b, c] = points.map(|p| to_world(p, BOTTOM));
        triangle(&mut vertices, [c, b, a], SHEET);
    });
    for outline in &outlines {
        for (&a, &b) in outline.iter().zip(outline.iter().cycle().skip(1)) {
            let (at, bt) = (to_world(a, 0.), to_world(b, 0.));
            let (ab, bb) = (to_world(a, BOTTOM), to_world(b, BOTTOM));
            triangle(&mut vertices, [at, ab, bb], SHEET);
            triangle(&mut vertices, [at, bb, bt], SHEET);
        }
    }
    let name = if textured {
        "Textured PEI Plate"
    } else {
        "High Temp Plate"
    };
    if let Some(outlines) = inscription(name) {
        faces(&outlines, |points| {
            triangle(&mut vertices, points.map(|p| to_world(p, INK_HEIGHT)), INK);
        });
    }
    for x in [227., 232., 237.] {
        let stripe = [
            Point::new(x, -8.),
            Point::new(x + 2., -8.),
            Point::new(x + 6., -2.),
            Point::new(x + 4., -2.),
        ]
        .map(|p| to_world(p, INK_HEIGHT));
        triangle(&mut vertices, [stripe[0], stripe[1], stripe[2]], INK);
        triangle(&mut vertices, [stripe[0], stripe[2], stripe[3]], INK);
    }
    let min = Vec3::new(0., BOTTOM, -266.);
    let max = Vec3::new(258., INK_HEIGHT, 10.);
    Mesh {
        vertices,
        size: max - min,
        min,
        max,
    }
}

fn inscription(text: &str) -> Option<Polygons> {
    let fonts = FontDefinitions::default();
    let family = fonts.families.get(&FontFamily::Proportional)?;
    let font = fonts.font_data.get(family.first()?)?;
    let face = ttf_parser::Face::parse(font.font.as_ref(), font.index).ok()?;
    let mut outlines = label::line_outlines(&face, text, 10.)?;
    let (lo, hi) = outlines
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
    let scale = 7. / (hi - lo);
    let offset = Point::new(100., -5. - scale * (lo + hi) / 2.);
    for p in outlines.iter_mut().flatten() {
        *p = *p * scale + offset;
    }
    Some(outlines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_covers_bed_and_tab_but_leaves_locating_slot_open() {
        let sheet = mesh(true);
        let has_top = |x: f32, y: f32| {
            let p = Vec3::new(x, 0., -y);
            sheet.vertices.chunks_exact(3).any(|tri| {
                if tri[0].shade != SHEET || tri[0].normal[1] < 0.99 {
                    return false;
                }
                (0..3).all(|i| {
                    let a = Vec3::from_array(tri[i].position);
                    let b = Vec3::from_array(tri[(i + 1) % 3].position);
                    (b - a).cross(p - a).y >= -0.001
                })
            })
        };
        assert!(has_top(128., 128.) && has_top(128., -5.) && has_top(128., 264.));
        assert!(!has_top(128., 259.) && !has_top(48., -5.) && !has_top(200., 264.));
        assert!(
            sheet
                .vertices
                .iter()
                .any(|v| v.normal[1] < -0.99 && v.position[1] == BOTTOM)
        );
    }

    #[test]
    fn both_sheets_have_distinct_front_lip_lettering_and_share_instances() {
        let engraved = |textured| {
            let sheet = mesh(textured);
            assert!(Arc::ptr_eq(&sheet, &mesh(textured)));
            let text: Vec<_> = sheet
                .vertices
                .iter()
                .filter(|v| v.shade == INK && v.position[0] < 225.)
                .map(|v| v.position)
                .collect();
            assert!(!text.is_empty());
            assert!(text.iter().all(|&[x, y, z]| (90. ..220.).contains(&x)
                && y == INK_HEIGHT
                && (0. ..10.).contains(&z)));
            text
        };
        assert_ne!(engraved(true), engraved(false));
    }
}
