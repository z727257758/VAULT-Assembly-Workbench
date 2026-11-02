use super::*;
use crate::catalog::exterior::{Component, Role};
impl Scene {
    /// Called only by the background combination loader; source files remain read-only.
    pub(crate) fn append_exterior(
        &mut self,
        loader: &mut LoadCache,
        components: &[Component],
    ) -> Result<Vec<(Role, usize)>, String> {
        if components.is_empty() {
            return Ok(vec![]);
        }
        let items: Vec<_> = components.iter().map(|c| c.bom.clone()).collect();
        let extra = Self::load_cached(loader, &items, "Exterior".into(), false)?;
        let mut result = Vec::new();
        for part in extra.parts {
            if self.parts.iter().any(|p| {
                p.pose.path == part.pose.path
                    && p.pose.instance == part.pose.instance
                    && p.pose.body == part.pose.body
            }) {
                return Err("配件已存在于单件清单，请先移除重复配置 / Duplicate exterior part in member BOM".into());
            }
            let role = components
                .iter()
                .find(|c| c.bom.path == part.pose.path)
                .ok_or("Unknown exterior source")?
                .role;
            result.push((role, self.parts.len()));
            self.parts.push(part);
        }
        Ok(result)
    }
}

impl Scene {
    pub fn append_hardware_rod(&mut self, length: f32, position: Vec3) -> usize {
        let mesh = crate::display::hardware_rod(length);
        let index = self.parts.len();
        self.parts.push(Part {
            source_size: mesh.size,
            source_bodies: 1,
            mesh,
            pose: Pose {
                path: format!("@hardware/M16-rod-{length:.1}mm"),
                instance: 8000 + index as u32,
                body: None,
                position: position.to_array(),
                rotation: [0.; 3],
                travel: [0.; 3],
                travel_frame: None,
            },
            color: [0.65, 0.68, 0.72, 1.],
            visible: true,
        });
        index
    }
}
