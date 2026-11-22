//! Model-only estimates from print-height sections, without generating toolpaths.
use super::{Config, Printer};
use crate::mesh::Mesh;
use manifold_rust::cross_section::CrossSection;
use std::{collections::VecDeque, f64::consts::FRAC_PI_4};

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct GeometrySettings {
    printer: Printer,
    layer: f32,
    walls: u32,
}

impl From<&Config> for GeometrySettings {
    fn from(config: &Config) -> Self {
        Self {
            printer: config.printer,
            layer: config.layer,
            walls: config.walls,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ModelVolume {
    total: f64,
    sparse_core: f64,
}

impl ModelVolume {
    pub(crate) fn mass_g(self, infill: u32, mass_factor: f64) -> f64 {
        let unfilled = 1. - f64::from(infill.min(100)) / 100.;
        (self.total - unfilled * self.sparse_core) * mass_factor / 1000.
    }
}

impl GeometrySettings {
    pub(crate) fn measure(self, mesh: &Mesh) -> Option<ModelVolume> {
        let profile = self.printer.template_json().ok()?;
        let number = |key: &str| profile[key].as_str()?.parse::<f64>().ok();
        // Parse the same decimal value written to the 3MF.
        let layer: f64 = self.layer.to_string().parse().ok()?;
        if !(0.08..=0.28).contains(&layer) || !(1..=10).contains(&self.walls) {
            return None;
        }
        let first_layer = number("initial_layer_print_height")?;
        let shell_layers = |count, thickness| {
            let minimum = (number(thickness)? / layer - 1e-6).ceil() as usize;
            Some((number(count)? as usize).max(minimum))
        };
        let top = shell_layers("top_shell_layers", "top_shell_thickness")?;
        let bottom = shell_layers("bottom_shell_layers", "bottom_shell_thickness")?;
        let width = |key| number(key).map(|v| v.max(0.));
        let initial_width = width("initial_layer_line_width")?;
        let regular_width = width("outer_wall_line_width")?
            + f64::from(self.walls - 1) * width("inner_wall_line_width")?;

        let min_z = f64::from(mesh.min.y);
        let height = f64::from(mesh.max.y) - min_z;
        if !height.is_finite() || height <= 0. || height > f64::from(self.printer.height()) {
            return None;
        }
        let solid = mesh.closed_solid().ok()?;
        let total = solid.volume();
        if !total.is_finite() || total <= 0. {
            return None;
        }

        let first_layer = first_layer.min(height);
        let count = 1 + ((height - first_layer) / layer - 1e-6).ceil() as usize;
        let depth = |i: usize| {
            let start = if i == 0 {
                0.
            } else {
                first_layer + (i - 1) as f64 * layer
            };
            let end = (first_layer + i as f64 * layer).min(height);
            (min_z + (start + end) * 0.5, end - start)
        };
        // Keep only the sections needed to compare with the current layer.
        let mut sections = VecDeque::<CrossSection>::new();
        let mut sliced_volume = 0.;
        let mut core_volume = 0.;
        let mut next = 0;
        for i in 0..count {
            let first = i.saturating_sub(bottom);
            let last = (i + top).min(count - 1);
            while next <= last {
                sections.push_back(solid.slice(depth(next).0));
                next += 1;
            }
            let section = &sections[i - first];
            let (_, thickness) = depth(i);
            sliced_volume += section.area() * thickness;

            // Missing coverage in the neighboring shell layers becomes solid skin.
            if i >= bottom && i + top < count {
                let width = if i == 0 {
                    initial_width * f64::from(self.walls)
                } else {
                    regular_width
                };
                let spacing = width - f64::from(self.walls) * thickness * (1. - FRAC_PI_4);
                let mut core = section.offset(-spacing);
                for (j, neighbor) in sections.iter().enumerate() {
                    if j != i - first && !core.is_empty() {
                        core = core.intersection(neighbor);
                    }
                }
                core_volume += core.area() * thickness;
            }
            if i >= bottom {
                sections.pop_front();
            }
        }

        // Use the mesh's exact volume to correct integration error from sampling at layer centers.
        let fraction = (core_volume / sliced_volume).clamp(0., 1.);
        fraction.is_finite().then_some(ModelVolume {
            total,
            sparse_core: total * fraction,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manifold_rust::{linalg::Vec3, manifold::Manifold};

    #[test]
    fn estimates_sparse_core_and_keeps_shells_solid() {
        let config = Config::default();
        let geometry = GeometrySettings::from(&config);
        let cube = Mesh::from_manifold(
            &Manifold::cube(Vec3::new(20., 20., 20.), false),
            |_, _, _| 0.,
        );
        let volume = geometry.measure(&cube).unwrap();
        let hollow = volume.mass_g(0, 1.);
        let solid = volume.mass_g(100, 1.);
        assert!((solid - 8.).abs() < 1e-9);
        assert!((hollow - 1.75).abs() < 0.03);
        assert!((volume.mass_g(25, 1.) - (hollow + (solid - hollow) * 0.25)).abs() < 1e-9);
        assert!((volume.mass_g(25, 1.25) - volume.mass_g(25, 1.) * 1.25).abs() < 1e-9);

        let thin = Mesh::from_manifold(
            &Manifold::cube(Vec3::new(20., 20., 1.), false),
            |_, _, _| 0.,
        );
        let volume = geometry.measure(&thin).unwrap();
        assert!((volume.mass_g(0, 1.) - 0.4).abs() < 1e-9);
    }
}
