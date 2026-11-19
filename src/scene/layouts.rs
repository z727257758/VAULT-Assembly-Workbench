//! Read-only presentation layouts; never applied to assembly definitions.
use super::{MEMBER_STRIDE, Part, Pose, Scene};
use glam::{Quat, Vec3};
use std::sync::Arc;

fn member_band(lo: Vec3) -> i32 {
    (lo.y * 2.).round() as i32
}

pub struct LayerLayout {
    pub scene: Scene,
    pub labels: Vec<(String, Vec3)>,
}
impl Scene {
    pub fn member_layer_ranks(&self) -> Vec<usize> {
        let bands: Vec<_> = self
            .member_bounds
            .iter()
            .map(|(lo, _)| member_band(*lo))
            .collect();
        let mut sorted = bands.clone();
        sorted.sort_unstable();
        sorted.dedup();
        bands
            .into_iter()
            .map(|band| sorted.partition_point(|key| *key < band))
            .collect()
    }

    pub fn layered(&self, gap: f32, names: &[String], zh: bool) -> LayerLayout {
        let mut scene = self.clone();
        let combined = !self.member_bounds.is_empty();
        let mut keys: Vec<i32> = self
            .parts
            .iter()
            .map(|p| {
                if combined {
                    let m = (p.pose.instance / MEMBER_STRIDE) as usize;
                    self.member_bounds
                        .get(m)
                        .map_or(0, |(lo, _)| member_band(*lo))
                } else {
                    let path = &p.pose.path;
                    let file = path.rsplit('/').next().unwrap_or("");
                    if path.contains("/Roofs/")
                        || file.contains(" Roof")
                        || file.contains("Upper Hinge")
                    {
                        5
                    } else if path.starts_with(crate::gridfinity::PREFIX)
                        || path.contains("/Grid Dividers/")
                    {
                        4
                    } else if file.contains("Baseplate") {
                        3
                    } else if path.contains("/Boxes/")
                        || path.contains("/Facades/")
                        || crate::label::is_label(path)
                        || file.contains("Key")
                        || file.contains("Springs")
                        || file.contains("Facade Cap")
                    {
                        2
                    } else if path.contains("/Runners/")
                        || path.contains("/Geared Rails/")
                        || file == "Latch.stl"
                    {
                        1
                    } else {
                        0
                    }
                }
            })
            .collect();
        let part_keys = keys.clone();
        keys.sort_unstable();
        keys.dedup();
        let mut labels = Vec::new();
        for (rank, key) in keys.iter().enumerate() {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            let mut members = std::collections::BTreeSet::new();
            for (p, k) in scene.parts.iter_mut().zip(&part_keys) {
                if k != key {
                    continue;
                }
                p.pose.position[1] += rank as f32 * gap;
                p.pose.travel = [0.; 3];
                p.pose.travel_frame = None;
                if !p.visible {
                    continue;
                }
                members.insert((p.pose.instance / MEMBER_STRIDE) as usize);
                let matrix = p.pose.matrix(0.);
                for x in [p.mesh.min.x, p.mesh.max.x] {
                    for y in [p.mesh.min.y, p.mesh.max.y] {
                        for z in [p.mesh.min.z, p.mesh.max.z] {
                            let v = matrix.transform_point3(Vec3::new(x, y, z));
                            lo = lo.min(v);
                            hi = hi.max(v);
                        }
                    }
                }
            }
            if !lo.is_finite() {
                continue;
            }
            let name = if combined {
                members
                    .iter()
                    .filter_map(|i| names.get(*i))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" / ")
            } else {
                let labels = if zh {
                    [
                        "箱体 / 外壳",
                        "导轨与卡扣",
                        "抽屉与前面板",
                        "收纳底板",
                        "内部收纳",
                        "顶盖",
                    ]
                } else {
                    [
                        "Housing",
                        "Runners and latches",
                        "Drawer and facade",
                        "Baseplate",
                        "Inserts",
                        "Roof",
                    ]
                };
                labels[*key as usize].to_owned()
            };
            labels.push((
                format!("{} · {}", rank + 1, name),
                Vec3::new(lo.x, hi.y, hi.z),
            ));
        }
        LayerLayout { scene, labels }
    }
}

pub struct Plate {
    pub number: usize,
    pub origin: [f32; 2],
    pub name: String,
    pub copies: usize,
    pub slots: Vec<u32>,
    pub colors: Vec<[f32; 4]>,
    pub removable: bool,
}
#[derive(Clone)]
pub struct PrintPart {
    pub drawer: Option<usize>,
    pub member: usize,
    pub plate: usize,
    pub path: String,
    pub copy: u32,
}
#[derive(Default)]
pub struct PrintLayout {
    pub scene: Scene,
    pub plates: Vec<Plate>,
    pub parts: Vec<Option<PrintPart>>,
}
impl PrintLayout {
    pub fn append(
        &mut self,
        member: usize,
        name: &str,
        inputs: &[crate::catalog::Bom],
        plan: crate::export::PreparedPrint,
        config: &crate::print_plate::Config,
    ) {
        let base = self.plates.len();
        let count = plan
            .placements
            .iter()
            .map(|p| p.plate + 1)
            .max()
            .unwrap_or(0);
        for number in 0..count {
            let placed: Vec<_> = plan
                .placements
                .iter()
                .filter(|p| p.plate == number)
                .collect();
            let first = placed[0];
            let mut slots = plan.materials[first.input].clone();
            slots.sort_unstable();
            slots.dedup();
            self.plates.push(Plate {
                number,
                origin: [0.; 2],
                name: name.into(),
                copies: placed.len(),
                colors: slots.iter().map(|s| config.slot_color(*s)).collect(),
                slots,
                removable: plan.plate_groups[first.input]
                    == crate::print_plate::PlateGroup::RemovableLabel,
            });
        }
        for placement in &plan.placements {
            let down = plan.face_down[placement.input];
            let q = Quat::from_rotation_y(if placement.rotated {
                std::f32::consts::FRAC_PI_2
            } else {
                0.
            }) * Quat::from_rotation_x(if down { std::f32::consts::PI } else { 0. });
            let (rx, ry, rz) = q.to_euler(glam::EulerRot::XYZ);
            for (resource, slot) in &plan.material_ids[placement.input] {
                let (mesh, origin, _, _) = &plan.resources[*resource - 1];
                let position = Vec3::new(
                    placement.x,
                    if down {
                        plan.sizes[placement.input].z
                    } else {
                        0.
                    },
                    -placement.y,
                ) - q * *origin;
                self.scene.parts.push(Part {
                    mesh: mesh.clone(),
                    pose: Pose {
                        path: inputs[placement.input].path.clone(),
                        instance: member as u32 * MEMBER_STRIDE + placement.copy,
                        body: None,
                        position: position.to_array(),
                        rotation: [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()],
                        travel: [0.; 3],
                        travel_frame: None,
                    },
                    color: config.slot_color(*slot),
                    visible: true,
                    source_size: mesh.size,
                    source_bodies: 1,
                });
                self.parts.push(Some(PrintPart {
                    drawer: None,
                    member,
                    plate: base + placement.plate,
                    path: inputs[placement.input].path.clone(),
                    copy: placement.copy,
                }));
            }
        }
    }
    pub fn finish(&mut self) -> Result<(), String> {
        let count = self.plates.len();
        for (i, plate) in self.plates.iter_mut().enumerate() {
            plate.origin = crate::print_plate::plate_origin(i, count);
        }
        for (part, info) in self.scene.parts.iter_mut().zip(&self.parts) {
            let plate = &self.plates[info.as_ref().unwrap().plate];
            part.pose.position[0] += plate.origin[0];
            part.pose.position[2] -= plate.origin[1];
        }
        let bed = bed_mesh();
        for (i, plate) in self.plates.iter().enumerate() {
            self.scene.parts.push(Part {
                mesh: bed.clone(),
                pose: Pose {
                    path: "@print-bed".into(),
                    instance: i as u32,
                    body: None,
                    position: [plate.origin[0], -0.8, -plate.origin[1]],
                    rotation: [0.; 3],
                    travel: [0.; 3],
                    travel_frame: None,
                },
                color: [0.13, 0.18, 0.22, 1.],
                visible: true,
                source_size: bed.size,
                source_bodies: 1,
            });
            self.parts.push(None);
        }
        super::MeshPool::default().share(&mut self.scene)?;
        self.scene.title = "打印板平铺 / Print plates".into();
        Ok(())
    }
}
fn bed_mesh() -> Arc<crate::mesh::Mesh> {
    let mut vertices = Vec::new();
    let mut quad = |x: f32, z: f32, w: f32, d: f32, y: f32, shade: f32| {
        for position in [
            [x, y, z],
            [x + w, y, z],
            [x + w, y, z - d],
            [x, y, z],
            [x + w, y, z - d],
            [x, y, z - d],
        ] {
            vertices.push(crate::mesh::Vertex {
                position,
                normal: [0., 1., 0.],
                shade,
            });
        }
    };
    quad(0., 0., 256., 256., 0., 0.85);
    for n in 0..=4 {
        let value = n as f32 * 64.;
        quad(value.clamp(0., 255.6), 0., 0.4, 256., 0.04, 1.8);
        quad(0., -value.clamp(0., 255.6), 256., 0.4, 0.04, 1.8);
    }
    Arc::new(crate::mesh::Mesh {
        vertices,
        min: Vec3::new(0., 0., -256.),
        max: Vec3::new(256., 0.04, 0.),
        size: Vec3::new(256., 0.04, 256.),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn part(path: &str, instance: u32, y: f32) -> Part {
        let mesh = bed_mesh();
        Part {
            mesh: mesh.clone(),
            pose: Pose {
                path: path.into(),
                instance,
                body: None,
                position: [0., y, 0.],
                rotation: [0.; 3],
                travel: [10., 20., 30.],
                travel_frame: None,
            },
            color: [1.; 4],
            visible: true,
            source_size: mesh.size,
            source_bodies: 1,
        }
    }
    #[test]
    fn layered_members_share_height_bands_and_leave_assembly_unchanged() {
        let scene = Scene {
            parts: vec![
                part("body", 0, 0.),
                part("body", MEMBER_STRIDE, 0.),
                part("body", 2 * MEMBER_STRIDE, 40.),
            ],
            member_bounds: vec![
                (Vec3::ZERO, Vec3::splat(40.)),
                (Vec3::ZERO, Vec3::splat(40.)),
                (Vec3::new(0., 40., 0.), Vec3::splat(80.)),
            ],
            ..Default::default()
        };
        let before = toml::to_string(&scene.definition()).unwrap();
        let layout = scene.layered(75., &["A".into(), "B".into(), "C".into()], true);
        assert_eq!(layout.labels.len(), 2);
        assert_eq!(layout.scene.parts[0].pose.position[1], 0.);
        assert_eq!(layout.scene.parts[1].pose.position[1], 0.);
        assert_eq!(layout.scene.parts[2].pose.position[1], 115.);
        assert!(layout.scene.parts.iter().all(|p| p.pose.travel == [0.; 3]));
        assert_eq!(before, toml::to_string(&scene.definition()).unwrap());
        assert!(Arc::ptr_eq(
            &scene.parts[0].mesh,
            &layout.scene.parts[0].mesh
        ));
    }
    #[test]
    fn single_member_layers_follow_functional_groups() {
        let scene = Scene {
            parts: vec![
                part("01. DRAWERS/Modules/Body.stl", 0, 0.),
                part("01. DRAWERS/Boxes/Drawer.stl", 0, 4.),
                part("01. DRAWERS/Roofs/Roof.stl", 0, 40.),
            ],
            ..Default::default()
        };
        let layout = scene.layered(50., &[], true);
        assert_eq!(
            layout
                .scene
                .parts
                .iter()
                .map(|p| p.pose.position[1])
                .collect::<Vec<_>>(),
            vec![0., 54., 140.]
        );
        assert_eq!(scene.parts[1].pose.position[1], 4.);
    }
}
