mod exterior;
use crate::{catalog::Bom, display::Rules, mesh::Mesh};
use glam::{Mat4, Quat, Vec3};
const MAX_SCENE_TRIANGLES: usize = 16_000_000;
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
    #[serde(default)]
    pub mating_checks: Vec<crate::constraints::MateCheck>,
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
    /// Closed, world-axis envelopes measured from actual vertices on the worker.
    pub member_bounds: Vec<(Vec3, Vec3)>,
    pub sustainer_interior: Option<Box<crate::sustainer_dividers::Geometry>>,
    pub title: String,
    pub parts: Vec<Part>,
    pub calibrated: bool,
    pub assembly: bool,
    pub relations: Vec<crate::constraints::Relation>,
    pub constraint_warning: Option<String>,
    pub mating_checks: Vec<crate::constraints::MateCheck>,
}
impl Part {
    pub fn mesh_key(&self) -> usize {
        Arc::as_ptr(&self.mesh) as usize
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
                if tris > MAX_SCENE_TRIANGLES {
                    return Err(
                        "Scene exceeds 16 million unique triangles; select fewer files".into(),
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
            member_bounds: Vec::new(),
            sustainer_interior: None,
            title,
            parts,
            calibrated: !assembly,
            assembly,
            relations: Vec::new(),
            constraint_warning: None,
            mating_checks: Vec::new(),
        };
        if assembly {
            if crate::sustainer::is_scene(&scene) {
                crate::sustainer::apply(&mut scene)?;
                return Ok(scene);
            }
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
        let old_sustainer_estimates = !old.calibrated
            && old
                .relations
                .iter()
                .any(|r| r.label.starts_with("Sustainer:") && r.label.contains("(estimated)"))
            && self
                .relations
                .iter()
                .any(|r| r.label.starts_with("Sustainer datum:"));
        let old_sustainer_seats = !old.calibrated
            && old.relations.iter().any(|r| {
                r.label
                    .starts_with("Sustainer datum: upper hinge / roof seat")
                    && r.label != crate::sustainer::UPPER_SEAT
            })
            && self
                .relations
                .iter()
                .any(|r| r.label == crate::sustainer::UPPER_SEAT);
        let old_latch_defaults = !old.calibrated
            && old.relations.iter().any(|r| {
                r.label
                    .starts_with("Sustainer datum: inner lock / small pins")
                    && r.label != crate::sustainer::LOCK_INNER
            })
            && self
                .relations
                .iter()
                .any(|r| r.label == crate::sustainer::LOCK_INNER);
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
                let source_owned = legacy_hidden
                    || layout_owned
                    || old_sustainer_estimates
                    || old_sustainer_seats
                    || old_latch_defaults && part.pose.path.contains("/Locks/");
                if old_parent == new_parent && !source_owned {
                    part.pose = previous.pose.clone();
                }
                part.visible = previous.visible;
            }
        }
        for relation in &old.relations {
            if !(old_sustainer_estimates
                || old_sustainer_seats
                || old_latch_defaults && relation.child.path.contains("/Locks/"))
                && ids.contains(&relation.parent)
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
        // Sustainer explosion is derived from the restored assembled poses. Old
        // saved world-axis offsets must not replace the current display layout.
        if self.constraint_warning.is_none() && self.assembly && crate::sustainer::is_scene(self) {
            self.constraint_warning = crate::sustainer::reset_explosion(self).err();
        }
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
            mating_checks: self.mating_checks.clone(),
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

/// Source instances occupy [0, MEMBER_STRIDE); combined identities add a member offset.
pub const MEMBER_STRIDE: u32 = 10_000;
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct Placement {
    pub column: u32,
    pub level: u32,
    #[serde(default)]
    pub offset: [f32; 3],
}
impl Placement {
    pub fn validate(&self) -> Result<(), String> {
        if self.column > 63
            || self.level > 63
            || self
                .offset
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 10_000.)
        {
            return Err("组合位置无效 / Invalid combination placement".into());
        }
        Ok(())
    }
}
impl Scene {
    pub(crate) fn closed_extent(&self) -> Result<(Vec3, Vec3), String> {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.parts {
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
        if !lo.is_finite() || !hi.is_finite() {
            return Err("Empty or invalid assembly bounds".into());
        }
        Ok((lo, hi))
    }
    /// Generic envelope composer retained for identity/resource unit tests.
    /// Production combinations supply seating offsets from constraints::stacking.
    #[cfg(test)]
    pub fn combine(members: Vec<(Scene, Placement, f32)>, gap: f32) -> Result<Self, String> {
        Self::combine_positioned(members, gap, None)
    }
    pub fn combine_positioned(
        mut members: Vec<(Scene, Placement, f32)>,
        gap: f32,
        shifts: Option<Vec<Vec3>>,
    ) -> Result<Self, String> {
        if members.is_empty()
            || members.len() > 64
            || !gap.is_finite()
            || !(0. ..=100.).contains(&gap)
        {
            return Err("组合数量或间距无效 / Invalid combination size or spacing".into());
        }
        if shifts
            .as_ref()
            .is_some_and(|v| v.len() != members.len() || v.iter().any(|p| !p.is_finite()))
        {
            return Err("组合位移数量或坐标无效 / Invalid combination shifts".into());
        }
        let mut slots = std::collections::HashSet::new();
        let mut bounds = Vec::new();
        let mut widths = std::collections::BTreeMap::<u32, f32>::new();
        let mut triangles = 0usize;
        let mut shared = HashMap::<u64, Vec<Arc<Mesh>>>::new();
        let mut seen = std::collections::HashSet::new();
        for (scene, placement, _) in &mut members {
            placement.validate()?;
            if !slots.insert((placement.column, placement.level)) {
                return Err("同一列和层只能放置一个成员 / Duplicate column and level".into());
            }
            // Share identical display geometry across members, including repeated source copies.
            // Labels and generated inserts only share when every vertex (including material shading) matches.
            for part in &mut scene.parts {
                let pointer = Arc::as_ptr(&part.mesh) as usize;
                if seen.contains(&pointer) {
                    continue;
                }
                use std::hash::{Hash, Hasher};
                let bytes = bytemuck::cast_slice::<_, u8>(&part.mesh.vertices);
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                bytes.hash(&mut hash);
                for value in part
                    .mesh
                    .min
                    .to_array()
                    .into_iter()
                    .chain(part.mesh.max.to_array())
                {
                    value.to_bits().hash(&mut hash);
                }
                let bucket = shared.entry(hash.finish()).or_default();
                if let Some(mesh) = bucket.iter().find(|m| {
                    m.min == part.mesh.min
                        && m.max == part.mesh.max
                        && bytemuck::cast_slice::<_, u8>(&m.vertices) == bytes
                }) {
                    part.mesh = mesh.clone();
                } else {
                    triangles += part.mesh.vertices.len() / 3;
                    if triangles > MAX_SCENE_TRIANGLES {
                        return Err("Combination exceeds 16 million unique triangles; remove members or inserts".into());
                    }
                    bucket.push(part.mesh.clone());
                }
                seen.insert(Arc::as_ptr(&part.mesh) as usize);
            }
            if crate::sustainer::lid_angle(scene).is_some() {
                crate::sustainer::set_lid_angle(scene, 0.)?;
            }
            let (lo, hi) = scene.closed_extent()?;
            widths
                .entry(placement.column)
                .and_modify(|w| *w = w.max(hi.x - lo.x))
                .or_insert(hi.x - lo.x);
            bounds.push((lo, hi));
        }
        let mut left = std::collections::BTreeMap::new();
        let mut x = 0.;
        for (&column, &width) in &widths {
            left.insert(column, x);
            x += width + gap;
        }
        let mut result = Scene { title: format!("组合 / Combination · {}", members.len()), assembly: true,
            constraint_warning: Some("组合遵循文档规格规则；安装面按网格估算，接口啮合未验证 / Document-compatible layout; mesh seating estimates, connector engagement unverified".into()), ..Default::default() };
        for (index, (mut scene, placement, angle)) in members.iter().cloned().enumerate() {
            let (lo, hi) = bounds[index];
            let y: f32 = members
                .iter()
                .zip(&bounds)
                .filter(|((_, p, _), _)| p.column == placement.column && p.level < placement.level)
                .map(|(_, (lo, hi))| hi.y - lo.y + gap)
                .sum();
            let shift = shifts.as_ref().map(|v| v[index]).unwrap_or_else(|| {
                Vec3::new(
                    left[&placement.column] + (widths[&placement.column] - (hi.x - lo.x)) * 0.5
                        - lo.x,
                    y - lo.y,
                    -hi.z,
                ) + Vec3::from(placement.offset)
            });
            let mut closed_lo = Vec3::splat(f32::INFINITY);
            let mut closed_hi = Vec3::splat(f32::NEG_INFINITY);
            for part in &scene.parts {
                let (lo, hi) = crate::constraints::world_bounds(part);
                closed_lo = closed_lo.min(lo + shift);
                closed_hi = closed_hi.max(hi + shift);
            }
            result.member_bounds.push((closed_lo, closed_hi));
            if crate::sustainer::lid_angle(&scene).is_some() {
                crate::sustainer::set_lid_angle(&mut scene, angle)?;
            }
            crate::constraints::namespace_member(&mut scene, index)?;
            for part in &mut scene.parts {
                part.pose.position = (Vec3::from(part.pose.position) + shift).to_array();
            }
            result.parts.extend(scene.parts);
            result.relations.extend(scene.relations);
            result.mating_checks.extend(scene.mating_checks);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod combination_tests {
    use super::*;
    #[test]
    fn displayed_dimensions_measure_rotated_vertices_instead_of_rotated_aabb() {
        let mut source = unit_scene();
        source.parts.truncate(1);
        source.relations.clear();
        source.mating_checks.clear();
        source.parts[0].mesh = Arc::new(Mesh {
            vertices: [[0., 0., 0.], [10., 0., 0.], [0., 10., 0.]]
                .into_iter()
                .map(|position| crate::mesh::Vertex {
                    position,
                    normal: [0., 0., 1.],
                    shade: 1.,
                })
                .collect(),
            min: Vec3::ZERO,
            max: Vec3::new(10., 10., 0.),
            size: Vec3::new(10., 10., 0.),
        });
        source.parts[0].pose.rotation = [0., 0., 45.];
        let shift = Vec3::new(20., 30., 40.);
        let combined = Scene::combine_positioned(
            vec![(source, Placement::default(), 0.)],
            0.,
            Some(vec![shift]),
        )
        .unwrap();
        let (lo, hi) = combined.member_bounds[0];
        let expected = 10. / 2f32.sqrt();
        assert!((lo - Vec3::new(20. - expected, 30., 40.)).length() < 0.001);
        assert!((hi - Vec3::new(20. + expected, 30. + expected, 40.)).length() < 0.001);
    }
    #[test]
    fn positioned_combination_rejects_invalid_shift_vectors_instead_of_panicking() {
        for shifts in [
            vec![],
            vec![Vec3::ZERO, Vec3::ONE],
            vec![Vec3::splat(f32::NAN)],
            vec![Vec3::splat(f32::INFINITY)],
        ] {
            assert!(
                Scene::combine_positioned(
                    vec![(unit_scene(), Placement::default(), 0.)],
                    0.,
                    Some(shifts)
                )
                .is_err()
            );
        }
    }
    #[test]
    fn namespaces_remain_disjoint_at_full_member_capacity() {
        let members = (0..64)
            .map(|i| {
                (
                    unit_scene(),
                    Placement {
                        column: i,
                        ..Default::default()
                    },
                    0.,
                )
            })
            .collect();
        let scene = Scene::combine(members, 0.).unwrap();
        let ids: std::collections::HashSet<_> = scene
            .parts
            .iter()
            .map(crate::constraints::PartId::of)
            .collect();
        assert_eq!(ids.len(), 128);
        assert_eq!(scene.member_bounds.len(), 64);
        for relation in &scene.relations {
            assert!(ids.contains(&relation.parent) && ids.contains(&relation.child));
            assert_eq!(
                relation.parent.instance / MEMBER_STRIDE,
                relation.child.instance / MEMBER_STRIDE
            );
        }
    }
    fn unit_scene() -> Scene {
        let mesh = Arc::new(Mesh {
            vertices: vec![],
            min: Vec3::ZERO,
            max: Vec3::new(10., 20., 30.),
            size: Vec3::new(10., 20., 30.),
        });
        let part = Part {
            mesh,
            pose: Pose {
                path: "same.stl".into(),
                instance: 0,
                body: None,
                position: [0.; 3],
                rotation: [0.; 3],
                travel: [0.; 3],
                travel_frame: None,
            },
            color: [1.; 4],
            visible: true,
            source_size: Vec3::new(10., 20., 30.),
            source_bodies: 1,
        };
        let mut child = part.clone();
        child.pose.instance = 1;
        child.pose.position = [1., 2., 3.];
        let mut scene = Scene {
            parts: vec![part, child],
            assembly: true,
            calibrated: true,
            ..Default::default()
        };
        scene.relations.push(crate::constraints::Relation {
            parent: crate::constraints::PartId::of(&scene.parts[0]),
            child: crate::constraints::PartId::of(&scene.parts[1]),
            joint: crate::constraints::Joint::Fixed {
                position: [1., 2., 3.],
                rotation: Quat::IDENTITY.to_array(),
            },
            label: "fixed".into(),
        });
        scene
    }
    #[test]
    fn combined_members_keep_unique_identities_and_local_constraints() {
        let a = unit_scene();
        let mut b = unit_scene();
        b.parts[0].mesh = Arc::new((*b.parts[0].mesh).clone());
        assert_ne!(a.parts[0].mesh_key(), b.parts[0].mesh_key());
        let mut scene = Scene::combine(
            vec![
                (a, Placement::default(), 0.),
                (
                    b,
                    Placement {
                        level: 1,
                        ..Default::default()
                    },
                    0.,
                ),
                (
                    unit_scene(),
                    Placement {
                        column: 1,
                        ..Default::default()
                    },
                    0.,
                ),
            ],
            2.,
        )
        .unwrap();
        assert!(!scene.calibrated);
        let ids: std::collections::HashSet<_> = scene
            .parts
            .iter()
            .map(crate::constraints::PartId::of)
            .collect();
        assert_eq!(ids.len(), 6);
        assert_eq!(
            scene.parts[2].pose.position[1] - scene.parts[0].pose.position[1],
            24.
        );
        assert_eq!(
            scene.parts[4].pose.position[0] - scene.parts[0].pose.position[0],
            13.
        );
        let before: Vec<_> = scene.parts.iter().map(|p| p.pose.position).collect();
        crate::constraints::solve(&mut scene).unwrap();
        for (part, expected) in scene.parts.iter().zip(before) {
            assert!((Vec3::from(part.pose.position) - Vec3::from(expected)).length() < 0.001);
            assert_eq!(part.pose.path, "same.stl");
        }
        scene.parts[0].pose.position[0] += 5.;
        crate::constraints::solve(&mut scene).unwrap();
        assert_eq!(scene.parts[1].pose.position[0], 6.);
        assert_eq!(scene.parts[3].pose.position[0], 1.);
    }
    #[test]
    fn combined_repeated_geometry_shares_resources_but_text_shading_stays_independent() {
        let mut a = unit_scene();
        let mut mesh = (*a.parts[0].mesh).clone();
        mesh.vertices = vec![
            crate::mesh::Vertex {
                position: [0.; 3],
                normal: [0., 1., 0.],
                shade: 0.
            };
            3
        ];
        a.parts[0].mesh = Arc::new(mesh.clone());
        let mut b = unit_scene();
        b.parts[0].mesh = Arc::new(mesh.clone());
        mesh.vertices[0].shade = 1.;
        let mut c = unit_scene();
        c.parts[0].mesh = Arc::new(mesh);
        assert!(!Arc::ptr_eq(&a.parts[0].mesh, &b.parts[0].mesh));
        let scene = Scene::combine(
            vec![
                (a, Placement::default(), 0.),
                (
                    b,
                    Placement {
                        column: 1,
                        ..Default::default()
                    },
                    0.,
                ),
                (
                    c,
                    Placement {
                        column: 2,
                        ..Default::default()
                    },
                    0.,
                ),
            ],
            0.,
        )
        .unwrap();
        assert!(Arc::ptr_eq(&scene.parts[0].mesh, &scene.parts[2].mesh));
        assert!(!Arc::ptr_eq(&scene.parts[0].mesh, &scene.parts[4].mesh));
        assert_eq!(scene.parts[0].mesh_key(), scene.parts[2].mesh_key());
        assert_ne!(scene.parts[0].mesh_key(), scene.parts[4].mesh_key());
    }
    #[test]
    fn duplicate_slots_and_nonfinite_placements_are_rejected() {
        assert!(
            Scene::combine(
                vec![
                    (unit_scene(), Placement::default(), 0.),
                    (unit_scene(), Placement::default(), 0.)
                ],
                0.
            )
            .is_err()
        );
        assert!(
            Placement {
                offset: [f32::NAN, 0., 0.],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(Scene::combine(vec![(unit_scene(), Placement::default(), 0.)], f32::NAN).is_err());
    }
}
