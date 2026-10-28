//! Native solids based on Cullenect's published dimensional interface (no SCAD dependency).
//! Coordinates in solid operations are print X/Y/Z; Mesh uses X/Z/-Y.
use crate::{
    label::Config,
    mesh::{Mesh, Vertex},
};
use glam::Vec3;
use manifold_rust::{
    linalg::{Vec2 as D2, Vec3 as D3},
    manifold::Manifold,
    types::Error,
};
use std::sync::Arc;

fn block(x: f64, y: f64, bottom: f64, height: f64) -> Manifold {
    Manifold::extrude(
        &vec![vec![
            D2::new(-x / 2., -y / 2.),
            D2::new(x / 2., -y / 2.),
            D2::new(x / 2., y / 2.),
            D2::new(-x / 2., y / 2.),
        ]],
        height,
        0,
        0.,
        D2::new(1., 1.),
    )
    .translate(D3::new(0., 0., bottom))
}
fn mesh(solid: &Manifold) -> Result<Arc<Mesh>, String> {
    if solid.status() != Error::NoError || solid.volume() <= 0. {
        return Err(format!("Cullenect solid failed: {:?}", solid.status()));
    }
    let raw = solid.get_mesh_gl64(0);
    let mut vertices = Vec::new();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for t in raw.tri_verts.chunks_exact(3) {
        let points: Vec<_> = t
            .iter()
            .map(|i| {
                let i = *i as usize * raw.num_prop as usize;
                Vec3::new(
                    raw.vert_properties[i] as f32,
                    raw.vert_properties[i + 2] as f32,
                    -raw.vert_properties[i + 1] as f32,
                )
            })
            .collect();
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        for p in points {
            min = min.min(p);
            max = max.max(p);
            vertices.push(Vertex {
                position: p.to_array(),
                normal: normal.to_array(),
                shade: 1.,
            });
        }
    }
    let e = max - min;
    Ok(Arc::new(Mesh {
        vertices,
        min,
        max,
        size: Vec3::new(e.x, e.z, e.y),
    }))
}
fn blank() -> Manifold {
    // Perimeter latch: 0.2 mm inset between Z=0.2 and 0.8 mm.
    block(36., 11., 0., 0.2)
        .union(&block(35.6, 10.6, 0.2, 0.6))
        .union(&block(36., 11., 0.8, 0.4))
}
pub fn label(config: &Config) -> Result<Arc<Mesh>, String> {
    config.validate()?;
    let source = mesh(&blank())?;
    if !config.active() {
        return Ok(source);
    }
    if config.depth > 0.4 {
        return Err("Cullenect text depth must be at most 0.4 mm".into());
    }
    let polygons = crate::label::text_polygons(config)?;
    if polygons
        .iter()
        .flatten()
        .any(|p| p.x.abs() > 17. || p.y.abs() > 5.)
    {
        return Err("Cullenect text exceeds 34 x 10 mm safe area".into());
    }
    crate::label::apply_text(&source, config, &polygons, [0., 0.], 1.2)
}
fn cavity(offset: f64, top: f64) -> Manifold {
    let pocket = block(36.3, 11.3, top - 1.2, 1.3).translate(D3::new(0., offset, 0.));
    let mut ribs = block(36.3, 0.2, top - 1., 0.4).translate(D3::new(0., offset - 5.55, 0.));
    ribs = ribs.union(&block(36.3, 0.2, top - 1., 0.4).translate(D3::new(0., offset + 5.55, 0.)));
    pocket.difference(&ribs)
}
pub fn socket(body: &Arc<Mesh>, offset: f32, top: f32) -> Result<Arc<Mesh>, String> {
    let (offset, top) = (offset as f64, top as f64);
    // Reinforce the thin existing shelf before cutting the 1.2 mm deep pocket.
    let support = block(38., 13., top - 2.2, 2.2).translate(D3::new(0., offset, 0.));
    let result = crate::label::solid(body)?
        .union(&support)
        .difference(&cavity(offset, top));
    // Front finger opening exposes the edge for removal, away from the rear wall.
    let notch = block(5., 3., top - 1.2, 1.4).translate(D3::new(0., offset + 6.3, 0.));
    mesh(&result.difference(&notch))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flat_lettering_preserves_shape_and_has_disjoint_flush_color_volumes() {
        let c = Config {
            enabled: true,
            text: "B8".into(),
            font: crate::label::Font::ArialBold,
            size: 4.,
            depth: 0.4,
            style: crate::label::Style::Flat,
            ..Default::default()
        };
        let original = blank();
        let m = label(&c).unwrap();
        let whole = crate::label::solid(&m).unwrap();
        assert_eq!(whole.status(), Error::NoError);
        assert!((whole.volume() - original.volume()).abs() < 0.001);
        assert!(whole.difference(&original).volume().abs() < 0.001);
        assert!(original.difference(&whole).volume().abs() < 0.001);
        assert!((m.size - Vec3::new(36., 11., 1.2)).abs().max_element() < 0.001);
        let regions = crate::label::color_regions(&m, &c, [0., 0.], 1.2).unwrap();
        let body = crate::label::solid(&regions[0]).unwrap();
        let ink = crate::label::solid(&regions[1]).unwrap();
        assert!(body.intersection(&ink).volume().abs() < 0.001);
        assert!((body.volume() + ink.volume() - original.volume()).abs() < 0.001);
        assert!((regions[1].min.y - 0.8).abs() < 0.001);
        assert!((regions[1].max.y - 1.2).abs() < 0.001);
        let shaded: Vec<_> = m
            .vertices
            .chunks_exact(3)
            .filter(|t| t[0].shade < 0.9)
            .collect();
        assert!(!shaded.is_empty());
        let mut area = 0.;
        let polygons = crate::label::text_polygons(&c).unwrap();
        for tri in shaded {
            assert!(
                tri.iter()
                    .all(|v| (v.position[1] - 1.2).abs() < 0.001 && v.normal[1] > 0.99)
            );
            let [a, b, c] = [
                Vec3::from(tri[0].position),
                Vec3::from(tri[1].position),
                Vec3::from(tri[2].position),
            ];
            area += (b - a).cross(c - a).length() / 2.;
            let p = (a + b + c) / 3.;
            assert!(crate::label::flat_text_contains(
                &polygons,
                p.x as f64,
                -p.z as f64
            ));
        }
        assert!((area as f64 - ink.volume() / 0.4).abs() < 0.001);
        let saved = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&saved).unwrap(), c);
    }
    #[test]
    fn nominal_latch_and_socket_clearance() {
        let plate = blank();
        assert_eq!(plate.status(), Error::NoError);
        assert!((plate.volume() - (36. * 11. * 0.6 + 35.6 * 10.6 * 0.6)).abs() < 0.001);
        let support = block(38., 13., -1., 2.2).difference(&cavity(0., 1.2));
        assert!(support.intersection(&plate).volume().abs() < 1e-6);
        let m = mesh(&plate).unwrap();
        assert!((m.size - Vec3::new(36., 11., 1.2)).abs().max_element() < 0.001);
        assert!(crate::label::solid(&m).unwrap().volume() > 0.);
    }
    #[test]
    fn lettering_keeps_latch_and_is_closed() {
        for style in [crate::label::Style::Raised, crate::label::Style::Engraved] {
            let c = Config {
                enabled: true,
                text: "M3".into(),
                font: crate::label::Font::ArialBold,
                size: 4.,
                depth: 0.2,
                style,
                ..Default::default()
            };
            let m = label(&c).unwrap();
            let solid = crate::label::solid(&m).unwrap();
            assert!(solid.volume() > 0.);
            let latch = block(38., 13., 0., 0.8);
            assert!(
                (solid.intersection(&latch).volume() - blank().intersection(&latch).volume()).abs()
                    < 0.001
            );
        }
    }
}
