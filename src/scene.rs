use crate::{catalog::Bom, display::Rules, mesh::Mesh};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path, sync::Arc};

#[derive(Clone, Serialize, Deserialize)]
pub struct Pose {
    pub path: String,
    pub instance: u32,
    #[serde(default)]
    pub body: Option<u32>,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub travel: [f32; 3],
    /// Orientation in which the world-space travel was authored. Old saves
    /// without this field retain their legacy world-space offsets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub travel_frame: Option<[f32; 4]>,
}
impl Pose {
    pub fn matrix(&self, explosion: f32) -> Mat4 {
        let r = Vec3::from(self.rotation).map(f32::to_radians);
        let q = Quat::from_euler(glam::EulerRot::XYZ, r.x, r.y, r.z);
        let travel = self.travel_frame.map_or(Vec3::from(self.travel), |frame| {
            q * Quat::from_array(frame).inverse() * Vec3::from(self.travel)
        });
        Mat4::from_rotation_translation(q, Vec3::from(self.position) + travel * explosion)
    }
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Definition {
    pub calibrated: bool,
    pub poses: Vec<Pose>,
    #[serde(default)]
    pub relations: Vec<crate::constraints::Relation>,
}
#[derive(Clone)]
pub struct Part {
    pub mesh: Arc<Mesh>,
    pub pose: Pose,
    pub color: [f32; 4],
    pub visible: bool,
    pub source_size: Vec3,
    pub source_bodies: usize,
}
#[derive(Clone, Default)]
pub struct Scene {
    pub title: String,
    pub parts: Vec<Part>,
    pub calibrated: bool,
    pub assembly: bool,
    pub relations: Vec<crate::constraints::Relation>,
    pub constraint_warning: Option<String>,
}
impl Part {
    pub fn mesh_key(&self) -> (String, Option<u32>) {
        (self.pose.path.clone(), self.pose.body)
    }
    pub fn label(&self, zh: bool) -> String {
        if self.pose.path.contains("/Geared Rails/") && self.pose.body.is_some() {
            let upper = self.mesh.size.y > 5.5;
            let name = match (zh, upper) {
                (true, true) => "上齿条",
                (true, false) => "下齿条",
                (false, true) => "Upper rack",
                (false, false) => "Lower rack",
            };
            let family = self
                .pose
                .path
                .rsplit('/')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .next()
                .unwrap_or("");
            return format!("{name} · {family} #{}", self.pose.instance + 1);
        }
        let metadata = crate::accessories::info(&self.pose.path, zh);
        let name = metadata.as_ref().map_or_else(
            || self.pose.path.rsplit('/').next().unwrap_or(&self.pose.path),
            |m| m.name.as_str(),
        );
        match self.pose.body {
            Some(body) => {
                if zh {
                    format!(
                        "{name} · 第{}份 / 实体 {}",
                        self.pose.instance + 1,
                        body + 1
                    )
                } else {
                    format!(
                        "{name} · copy {} / body {}",
                        self.pose.instance + 1,
                        body + 1
                    )
                }
            }
            None => format!("{name} #{}", self.pose.instance + 1),
        }
    }
}
impl Scene {
    #[cfg(test)]
    pub fn load(root: &Path, items: &[Bom], title: String, assembly: bool) -> Result<Self, String> {
        Self::load_with_rules(root, items, title, assembly, &Rules::default())
    }
    pub fn load_with_rules(
        root: &Path,
        items: &[Bom],
        title: String,
        assembly: bool,
        rules: &Rules,
    ) -> Result<Self, String> {
        let mut cache: HashMap<String, Arc<Mesh>> = HashMap::new();
        let mut tris = 0;
        for item in items.iter().filter(|i| i.enabled) {
            if !cache.contains_key(&item.path) {
                let mesh = Mesh::load(&root.join(&item.path))
                    .map_err(|e| format!("{}: {e}", item.name()))?;
                tris += mesh.vertices.len() / 3;
                if tris > 8_000_000 {
                    return Err(
                        "Scene exceeds 8 million unique triangles; select fewer files".into(),
                    );
                }
                cache.insert(item.path.clone(), mesh);
            }
        }
        let module = items
            .iter()
            .find(|i| i.name().contains(" Module "))
            .and_then(|i| cache.get(&i.path));
        let size = module.map(|m| m.size).unwrap_or(Vec3::new(160., 60., 150.));
        let (w, h, d) = (size.x, size.y, size.z);
        let mut parts = Vec::new();
        for item in items.iter().filter(|i| i.enabled) {
            let mesh = cache.get(&item.path).unwrap();
            let bodies = rules.prepare(&item.path, mesh)?;
            for copy in 0..item.qty.min(100) {
                for (body_index, body) in bodies.iter().enumerate() {
                    let display_index = copy as usize * bodies.len() + body_index;
                    let side = if display_index.is_multiple_of(2) {
                        -1.
                    } else {
                        1.
                    };
                    let n = item.name();
                    let mut pos = Vec3::ZERO;
                    let mut rot = Vec3::ZERO;
                    let mut travel = Vec3::ZERO;
                    let mut color = [0.65, 0.58, 0.85, 1.];
                    if assembly {
                        if n.contains(" Module ") {
                            rot.x = 90.;
                            travel = Vec3::new(0., h * 0.9, -d * 0.5);
                            color = [0.33, 0.54, 0.72, 1.];
                        } else if n.contains("Box") || n.contains("Back Panel") {
                            pos.y = -h * 0.1;
                            travel.z = d * 0.35;
                            color = [0.45, 0.70, 0.60, 1.];
                            if n.contains("Part 1") {
                                pos.z = -d * 0.22;
                            }
                            if n.contains("Part 2") || n.contains("Back Panel") {
                                pos.z = d * 0.22;
                                travel.z += d * 0.3;
                            }
                        } else if n.contains("Roof") {
                            rot = Vec3::new(90., -45., 0.);
                            pos.y = h * 0.5 + 6.;
                            travel.y = h * 2.;
                            color = [0.80, 0.84, 0.88, 1.];
                        } else if n.contains("Runner Guide") || n.contains("Runner Slide") {
                            rot.z = side * 90.;
                            pos = Vec3::new(side * (w * 0.5 - 9.), -h * 0.2, 0.);
                            travel = Vec3::new(
                                side * w * if n.contains("Guide") { 0.65 } else { 0.35 },
                                0.,
                                d * 0.25,
                            );
                            color = if n.contains("Guide") {
                                [0.78, 0.82, 0.88, 1.]
                            } else {
                                [0.30, 0.68, 0.73, 1.]
                            };
                        } else if n.contains("Rails") {
                            rot.x = 90.;
                            pos = Vec3::new(0., side * h * 0.22, d * 0.46);
                            travel = Vec3::new(0., side * h * 0.9, d * 0.65);
                            color = [0.87, 0.57, 0.28, 1.];
                        } else if item.path.contains("/Facades/") {
                            rot.x = 90.;
                            pos.z = d * 0.52;
                            travel.z = d * 0.9;
                            color = [0.80, 0.84, 0.88, 1.];
                        } else if n.contains("Stopper") {
                            pos = Vec3::new(side * (w * 0.5 - 8.), -h * 0.3, d * 0.47);
                            travel = Vec3::new(side * w * 0.85, -h * 0.5, d * 0.8);
                            color = [0.88, 0.60, 0.32, 1.];
                        } else if n.contains("Springs") {
                            rot.x = 90.;
                            pos = Vec3::new(side * w * 0.24, 0., d * 0.53);
                            travel = Vec3::new(side * w * 0.45, h * 0.8, d * 1.1);
                            color = [0.88, 0.60, 0.32, 1.];
                        } else if n.contains("Latch") {
                            pos.y = h * 0.5;
                            travel = Vec3::new(0., h * 1.5, -d * 0.5);
                            color = [0.88, 0.60, 0.32, 1.];
                        } else if item.group == "Key" || item.group == "Mechanism" {
                            rot.x = 90.;
                            let slot = if n.contains("Label") {
                                0.
                            } else if n.contains("Facade Cap") {
                                1.
                            } else if n.contains("Key Cap") {
                                2.
                            } else if n.contains("Part 2") {
                                4.
                            } else {
                                3.
                            };
                            pos = Vec3::new((slot - 1.5) * w * 0.08, 0., d * 0.55 + slot * 3.);
                            travel = Vec3::new(
                                (slot - 1.5) * w * 0.28,
                                -h * 0.8,
                                d * (1.1 + slot * 0.12),
                            );
                        } else {
                            pos = Vec3::new(0., -h * 0.25, 0.);
                            travel = Vec3::new(w * (copy as f32 + 1.), -h, 0.);
                        }
                    }
                    parts.push(Part {
                        mesh: body.mesh.clone(),
                        source_size: mesh.size,
                        source_bodies: bodies.len(),
                        pose: Pose {
                            path: item.path.clone(),
                            instance: copy,
                            body: body.id,
                            position: if assembly {
                                pos.to_array()
                            } else {
                                body.source_center.to_array()
                            },
                            rotation: rot.to_array(),
                            travel: travel.to_array(),
                            travel_frame: None,
                        },
                        color,
                        visible: true,
                    });
                }
            }
        }
        if parts.is_empty() {
            return Err("No enabled model files".into());
        }
        let mut scene = Self {
            title,
            parts,
            calibrated: !assembly,
            assembly,
            relations: Vec::new(),
            constraint_warning: None,
        };
        if assembly {
            for part in &mut scene.parts {
                if part.pose.path.contains("/Boxes/")
                    && part.pose.path.contains(" Box ")
                    && !part.pose.path.contains(" Part ")
                {
                    part.mesh = crate::display::remove_runner_supports(&part.mesh);
                }
            }
            let _ = crate::constraints::basic_relations(&mut scene);
        }
        Ok(scene)
    }
    pub fn apply(&mut self, definition: &Definition) {
        // Reconcile saved instances with the current BOM. A shared cap may now
        // belong to another key; absent parts must not invalidate the whole graph.
        let mut old = Scene {
            calibrated: definition.calibrated,
            relations: definition.relations.clone(),
            ..Default::default()
        };
        for p in &self.parts {
            if let Some(saved) = definition.poses.iter().find(|s| {
                s.path == p.pose.path && s.instance == p.pose.instance && s.body == p.pose.body
            }) {
                let mut part = p.clone();
                part.pose = saved.clone();
                old.parts.push(part);
            }
        }
        self.retain_assembly_state(&old);
    }
    /// Keep edits for surviving parts while retaining default mates for new parts.
    pub fn retain_assembly_state(&mut self, old: &Scene) {
        use crate::constraints::PartId;
        let layout_changed = self
            .relations
            .iter()
            .filter(|r| {
                r.label.starts_with("Divider layout:") || r.label.starts_with("Gridfinity bin /")
            })
            .any(|r| {
                old.relations
                    .iter()
                    .find(|previous| previous.child == r.child)
                    .is_none_or(|previous| {
                        previous.parent != r.parent
                            || match (previous.joint.transform(), r.joint.transform()) {
                                (Ok(a), Ok(b)) => !a.abs_diff_eq(b, 0.0001),
                                _ => true,
                            }
                    })
            });
        let ids: Vec<_> = self.parts.iter().map(PartId::of).collect();
        for part in &mut self.parts {
            if let Some(previous) = old.parts.iter().find(|p| PartId::of(p) == PartId::of(part)) {
                let id = PartId::of(part);
                let old_parent = old
                    .relations
                    .iter()
                    .find(|r| r.child == id)
                    .map(|r| &r.parent);
                let new_parent = self
                    .relations
                    .iter()
                    .find(|r| r.child == id)
                    .map(|r| &r.parent);
                let legacy_hidden = old.relations.iter().any(|r| {
                    r.child == id
                        && r.label.starts_with("Subassembly attachment")
                        && self
                            .relations
                            .iter()
                            .any(|n| n.child == id && n.label.starts_with("Hidden key:"))
                });
                let layout_owned = self.relations.iter().any(|r| {
                    r.child == id
                        && (r.label.starts_with("Divider layout:")
                            || r.label.starts_with("Gridfinity bin /"))
                });
                if old_parent == new_parent && !legacy_hidden && !layout_owned {
                    part.pose = previous.pose.clone();
                }
                part.visible = previous.visible;
            }
        }
        for relation in &old.relations {
            if ids.contains(&relation.parent)
                && ids.contains(&relation.child)
                && self
                    .relations
                    .iter()
                    .find(|r| r.child == relation.child)
                    .is_none_or(|r| {
                        !(r.label.starts_with("Divider layout:")
                            || r.label.starts_with("Gridfinity bin /"))
                            && r.parent == relation.parent
                            && !(r.label.starts_with("Hidden key:")
                                && relation.label.starts_with("Subassembly attachment"))
                    })
            {
                self.relations.retain(|r| r.child != relation.child);
                self.relations.push(relation.clone());
            }
        }
        self.constraint_warning = crate::constraints::solve(self).err();
        self.calibrated = old.calibrated
            && !layout_changed
            && self.constraint_warning.is_none()
            && ids
                .iter()
                .all(|id| old.parts.iter().any(|p| PartId::of(p) == *id));
    }
    pub fn definition(&self) -> Definition {
        Definition {
            calibrated: self.calibrated,
            relations: self.relations.clone(),
            poses: self.parts.iter().map(|p| p.pose.clone()).collect(),
        }
    }
    pub fn bounds(&self, explosion: f32) -> (Vec3, f32) {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in self.parts.iter().filter(|p| p.visible) {
            let m = p.pose.matrix(explosion);
            for x in [p.mesh.min.x, p.mesh.max.x] {
                for y in [p.mesh.min.y, p.mesh.max.y] {
                    for z in [p.mesh.min.z, p.mesh.max.z] {
                        let v = m.transform_point3(Vec3::new(x, y, z));
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
            }
        }
        if !lo.is_finite() {
            return (Vec3::ZERO, 100.);
        }
        ((lo + hi) * 0.5, (hi - lo).length().max(1.) * 0.5)
    }
    pub fn pick(
        &self,
        origin: Vec3,
        direction: Vec3,
        explosion: f32,
    ) -> Option<crate::constraints::SurfaceHit> {
        let mut best = f32::INFINITY;
        let mut hit = None;
        for (index, p) in self.parts.iter().enumerate().filter(|(_, p)| p.visible) {
            let inverse = p.pose.matrix(explosion).inverse();
            let o = inverse.transform_point3(origin);
            let d = inverse.transform_vector3(direction);
            if !ray_box(o, d, p.mesh.min, p.mesh.max) {
                continue;
            }
            for tri in p.mesh.vertices.chunks_exact(3) {
                let a = Vec3::from(tri[0].position);
                let e1 = Vec3::from(tri[1].position) - a;
                let e2 = Vec3::from(tri[2].position) - a;
                let h = d.cross(e2);
                let det = e1.dot(h);
                if det.abs() < 1e-8 {
                    continue;
                }
                let f = 1. / det;
                let s = o - a;
                let u = f * s.dot(h);
                if !(0. ..=1.).contains(&u) {
                    continue;
                }
                let q = s.cross(e1);
                let v = f * d.dot(q);
                if v < 0. || u + v > 1. {
                    continue;
                }
                let t = f * e2.dot(q);
                if t > 0. && t < best {
                    best = t;
                    hit = Some(crate::constraints::SurfaceHit {
                        part: index,
                        point: o + d * t,
                        normal: e1.cross(e2).normalize_or_zero(),
                    });
                }
            }
        }
        hit
    }
}
fn ray_box(o: Vec3, d: Vec3, min: Vec3, max: Vec3) -> bool {
    let mut near = 0f32;
    let mut far = f32::INFINITY;
    for k in 0..3 {
        if d[k].abs() < 1e-8 {
            if o[k] < min[k] || o[k] > max[k] {
                return false;
            }
        } else {
            let a = (min[k] - o[k]) / d[k];
            let b = (max[k] - o[k]) / d[k];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if far < near {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ray_parallel_outside_misses() {
        assert!(!ray_box(
            Vec3::new(2., 0., 0.),
            Vec3::Z,
            -Vec3::ONE,
            Vec3::ONE
        ));
        assert!(ray_box(
            Vec3::new(0., 0., -3.),
            Vec3::Z,
            -Vec3::ONE,
            Vec3::ONE
        ));
    }
    #[test]
    fn explosion_zero_preserves_assembly() {
        let p = Pose {
            path: String::new(),
            instance: 0,
            body: None,
            position: [10., 20., 30.],
            rotation: [0.; 3],
            travel: [100., 0., 0.],
            travel_frame: None,
        };
        assert_eq!(
            p.matrix(0.).transform_point3(Vec3::ZERO),
            Vec3::new(10., 20., 30.)
        );
        assert_eq!(
            p.matrix(1.).transform_point3(Vec3::ZERO),
            Vec3::new(110., 20., 30.)
        );
    }
}
