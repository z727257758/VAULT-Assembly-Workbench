mod drawer_locks;
pub mod drawers;
mod exterior;
pub mod layouts;
pub(crate) mod print_bed;
mod target;
use crate::{catalog::Bom, display::Rules, mesh::Mesh};
use glam::{Mat4, Quat, Vec3};
pub(crate) use target::Target;
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
    pub fn validate(&self) -> Result<(), String> {
        if self
            .position
            .iter()
            .chain(&self.rotation)
            .chain(&self.travel)
            .any(|v| !v.is_finite())
            || self.travel_frame.is_some_and(|q| {
                !q.iter().all(|v| v.is_finite())
                    || (Quat::from_array(q).length_squared() - 1.).abs() > 0.002
            })
            || !self.matrix(0.).is_finite()
            || !self.matrix(1.).is_finite()
        {
            return Err(format!("Invalid assembly pose: {}", self.path));
        }
        Ok(())
    }
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
    /// Version 1 uses the full quarter-turn drawer lock.
    #[serde(default)]
    pub drawer_lock_version: u32,
    /// Versioned, deterministic geometry stamps. Legacy files remain unverified.
    #[serde(default)]
    pub geometry_version: u32,
    #[serde(default)]
    pub geometry: Vec<GeometryStamp>,
    pub poses: Vec<Pose>,
    #[serde(default)]
    pub relations: Vec<crate::constraints::Relation>,
    #[serde(default)]
    pub mating_checks: Vec<crate::constraints::MateCheck>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeometryStamp {
    pub part: crate::constraints::PartId,
    pub fingerprint: String,
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
    /// Runtime layout metadata used to regenerate per-member display movement.
    pub member_placements: Vec<Placement>,
    /// World-space translations used to place each member's local assembly.
    pub member_shifts: Vec<Vec3>,
    pub member_explosion_shifts: Vec<Vec3>,
    /// Upper member -> supporting members, supplied by the validated assembly layout.
    pub member_supports: Vec<Vec<usize>>,
    /// Closed, world-axis envelopes measured from actual vertices on the worker.
    pub member_bounds: Vec<(Vec3, Vec3)>,
    pub sustainer_interior: Option<Box<crate::sustainer_dividers::Geometry>>,
    pub member_interiors: Vec<Option<Box<crate::sustainer_dividers::Geometry>>>,
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
/// One background load task owns its source/display cache and cumulative budget.
pub(crate) struct LoadCache {
    root: std::path::PathBuf,
    rules: Rules,
    sources: HashMap<String, Arc<Mesh>>,
    bodies: HashMap<(String, bool), Vec<crate::display::Body>>,
    remaining: usize,
}
impl LoadCache {
    pub fn new(root: &Path, rules: &Rules) -> Self {
        Self {
            root: root.to_path_buf(),
            rules: rules.clone(),
            sources: HashMap::new(),
            bodies: HashMap::new(),
            remaining: MAX_SCENE_TRIANGLES,
        }
    }
    fn source(&mut self, path: &str) -> Result<Arc<Mesh>, String> {
        if let Some(mesh) = self.sources.get(path) {
            return Ok(mesh.clone());
        }
        let mesh = Mesh::load_with_budget(&self.root.join(path), self.remaining)
            .map_err(|e| format!("{path}: {e}"))?;
        self.remaining -= mesh.vertices.len() / 3;
        self.sources.insert(path.to_owned(), mesh.clone());
        Ok(mesh)
    }
    fn prepared(
        &mut self,
        path: &str,
        assembly: bool,
    ) -> Result<Vec<crate::display::Body>, String> {
        let remove_supports = assembly
            && path.contains("/Boxes/")
            && path.contains(" Box ")
            && !path.contains(" Part ");
        let key = (path.to_owned(), remove_supports);
        if let Some(bodies) = self.bodies.get(&key) {
            return Ok(bodies.clone());
        }
        let source = self.source(path)?;
        let mut bodies = self.rules.prepare(path, &source)?;
        if remove_supports {
            for body in &mut bodies {
                body.mesh = crate::display::remove_runner_supports(&body.mesh);
            }
        }
        self.bodies.insert(key, bodies.clone());
        Ok(bodies)
    }
}
/// Deduplicate completed member geometry immediately, before loading another member.
pub(crate) struct MeshPool {
    limit: usize,
    triangles: usize,
    shared: HashMap<u64, Vec<Arc<Mesh>>>,
    seen: std::collections::HashSet<usize>,
}
impl Default for MeshPool {
    fn default() -> Self {
        Self {
            limit: MAX_SCENE_TRIANGLES,
            triangles: 0,
            shared: HashMap::new(),
            seen: Default::default(),
        }
    }
}
impl MeshPool {
    pub fn share(&mut self, scene: &mut Scene) -> Result<(), String> {
        for part in &mut scene.parts {
            let pointer = Arc::as_ptr(&part.mesh) as usize;
            if self.seen.contains(&pointer) {
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
            let bucket = self.shared.entry(hash.finish()).or_default();
            if let Some(mesh) = bucket.iter().find(|m| {
                m.min == part.mesh.min
                    && m.max == part.mesh.max
                    && bytemuck::cast_slice::<_, u8>(&m.vertices) == bytes
            }) {
                part.mesh = mesh.clone();
            } else {
                let count = part.mesh.vertices.len() / 3;
                if self.triangles + count > self.limit {
                    return Err("Combination exceeds 16 million unique triangles; remove members or inserts".into());
                }
                self.triangles += count;
                bucket.push(part.mesh.clone());
            }
            self.seen.insert(Arc::as_ptr(&part.mesh) as usize);
        }
        Ok(())
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
        Self::load_cached(&mut LoadCache::new(root, rules), items, title, assembly)
    }
    pub(crate) fn load_cached(
        loader: &mut LoadCache,
        items: &[Bom],
        title: String,
        assembly: bool,
    ) -> Result<Self, String> {
        let mut cache: HashMap<String, Arc<Mesh>> = HashMap::new();
        for item in items.iter().filter(|i| i.enabled) {
            if !cache.contains_key(&item.path) {
                cache.insert(item.path.clone(), loader.source(&item.path)?);
            }
        }
        let mut parts = Vec::new();
        for item in items.iter().filter(|i| i.enabled) {
            let mesh = cache.get(&item.path).unwrap();
            let bodies = loader.prepared(&item.path, assembly)?;
            for copy in 0..item.qty.min(100) {
                for body in bodies.iter() {
                    parts.push(Part {
                        mesh: body.mesh.clone(),
                        source_size: mesh.size,
                        source_bodies: bodies.len(),
                        pose: Pose {
                            path: item.path.clone(),
                            instance: copy,
                            body: body.id,
                            position: if assembly {
                                [0.; 3]
                            } else {
                                body.source_center.to_array()
                            },
                            rotation: [0.; 3],
                            travel: [0.; 3],
                            travel_frame: None,
                        },
                        color: [0.65, 0.58, 0.85, 1.],
                        visible: true,
                    });
                }
            }
        }
        if parts.is_empty() {
            return Err("No enabled model files".into());
        }
        let mut scene = Self {
            member_placements: Vec::new(),
            member_shifts: Vec::new(),
            member_explosion_shifts: Vec::new(),
            member_supports: Vec::new(),
            member_bounds: Vec::new(),
            sustainer_interior: None,
            member_interiors: Vec::new(),
            title,
            parts,
            calibrated: !assembly,
            assembly,
            relations: Vec::new(),
            constraint_warning: None,
            mating_checks: Vec::new(),
        };
        if assembly {
            crate::constraints::AssemblyRules::initialize(&mut scene, items)?;
        }
        Ok(scene)
    }
    pub fn apply(&mut self, definition: &Definition) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for pose in &definition.poses {
            pose.validate()?;
            if !ids.insert((&pose.path, pose.instance, pose.body)) {
                return Err("Duplicate saved assembly pose".into());
            }
        }
        for relation in &definition.relations {
            if !relation.joint.transform()?.is_finite() {
                return Err("Invalid saved joint transform".into());
            }
        }
        let geometry_matches = !definition.calibrated
            || (definition.geometry_version == 1 && definition.geometry == self.geometry_stamps());
        // Reconcile saved instances with the current BOM. A shared cap may now
        // belong to another key; absent parts must not invalidate the whole graph.
        let mut old = Scene {
            calibrated: definition.calibrated && geometry_matches,
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
        let lock_travel_changed =
            definition.drawer_lock_version == 0 && self.upgrade_legacy_lock_ranges(&mut old);
        let mut candidate = self.clone();
        candidate.retain_assembly_state(&old);
        if let Some(error) = &candidate.constraint_warning {
            return Err(error.clone());
        }
        if lock_travel_changed {
            candidate.calibrated = false;
        }
        if definition.calibrated && !geometry_matches {
            candidate.calibrated = false;
            candidate.constraint_warning = Some("源几何已改变或旧校准缺少几何指纹，请重新人工核对 / Geometry changed or legacy calibration has no fingerprint; review required".into());
        }
        *self = candidate;
        Ok(())
    }
    /// Convert the old guide/housing seats into closed guide/drawer seats. Both saved
    /// transforms are in the same housing frame, even when the project was saved open.
    fn legacy_runner_joints(
        &self,
        old: &Scene,
    ) -> HashMap<crate::constraints::PartId, crate::constraints::Joint> {
        use crate::constraints::{AssemblyRules, Joint, PartId};
        let mut updates = HashMap::new();
        for guide in &self.relations {
            let Joint::RunnerGuide {
                inner_travel,
                outer_travel,
                ..
            } = guide.joint
            else {
                continue;
            };
            let Some(old_guide) = old
                .relations
                .iter()
                .find(|r| r.child == guide.child && matches!(r.joint, Joint::Fixed { .. }))
            else {
                continue;
            };
            // For shared shells, identical source paths can occur in several drawers.
            // Full graph identities match the exact driver, guide and housing.
            let same_driver = |r: &&crate::constraints::Relation| {
                r.child == guide.parent && r.parent == old_guide.parent
            };
            let Some(old_driver) = old.relations.iter().find(same_driver) else {
                continue;
            };
            let Some(driver) = self.relations.iter().find(same_driver) else {
                continue;
            };
            let Joint::Slider {
                position,
                rotation,
                axis,
                offset,
                min,
                max,
            } = old_driver.joint
            else {
                continue;
            };
            let Joint::Slider {
                max: measured_limit,
                ..
            } = driver.joint
            else {
                continue;
            };
            let Some(drawer) = self.parts.iter().find(|p| PartId::of(p) == driver.child) else {
                continue;
            };
            if old.calibrated || !old_guide.label.ends_with("Subassembly attachment (verify)") {
                let Ok(seat) = old_guide.joint.transform() else {
                    continue;
                };
                let closed = Mat4::from_rotation_translation(
                    Quat::from_array(rotation).normalize(),
                    position.into(),
                );
                let (_, guide_rotation, guide_position) =
                    (closed.inverse() * seat).to_scale_rotation_translation();
                updates.insert(
                    guide.child.clone(),
                    Joint::RunnerGuide {
                        position: guide_position.to_array(),
                        rotation: guide_rotation.to_array(),
                        inner_travel,
                        outer_travel,
                    },
                );
            }
            updates.entry(driver.child.clone()).or_insert_with(|| {
                let old_default =
                    (drawer.source_size.y - AssemblyRules::DRAWER_REAR_RETENTION).max(0.);
                let max = if (max - old_default).abs() < 0.001 {
                    measured_limit
                } else {
                    max.clamp(0., measured_limit)
                };
                let min = min.clamp(0., max);
                Joint::Slider {
                    position,
                    rotation,
                    axis,
                    min,
                    max,
                    offset: offset.clamp(min, max),
                }
            });
        }
        updates
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
        let upgraded_stoppers: Vec<_> = self
            .relations
            .iter()
            .filter(|r| r.label == crate::runners::STOPPER_SEAT)
            .filter(|r| {
                !old.calibrated
                    && old.relations.iter().any(|previous| {
                        previous.child == r.child
                            && previous.parent == r.parent
                            && previous.label == "Manual step 11: stopper / housing side port"
                    })
            })
            .map(|r| r.child.clone())
            .collect();
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
        let mut lock_upgrades = self.upgraded_drawer_locks(old);
        let drawer_lock_changed = !lock_upgrades.is_empty();
        let mut runner_joints = self.legacy_runner_joints(old);
        let runner_motion_changed = self.relations.iter().any(|r| {
            matches!(r.joint, crate::constraints::Joint::RunnerGuide { .. })
                && !old
                    .relations
                    .iter()
                    .any(|saved| saved.child == r.child && saved.parent == r.parent)
        });
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
                let source_owned = upgraded_stoppers.contains(&id)
                    || lock_upgrades.contains_key(&id)
                    || legacy_hidden
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
            if !(upgraded_stoppers.contains(&relation.child)
                || old_sustainer_estimates
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
        for relation in &mut self.relations {
            if let Some(upgrade) = lock_upgrades.remove(&relation.child) {
                *relation = upgrade;
            }
            if let Some(joint) = runner_joints.remove(&relation.child) {
                relation.joint = joint;
            }
        }
        self.constraint_warning = crate::constraints::solve(self).err();
        // Sustainer explosion is derived from the restored assembled poses. Old
        // saved world-axis offsets must not replace the current display layout.
        if self.constraint_warning.is_none() && self.assembly && crate::sustainer::is_scene(self) {
            self.constraint_warning = crate::sustainer::reset_explosion(self).err();
        }
        self.calibrated = old.calibrated
            && self.geometry_stamps() == old.geometry_stamps()
            && !layout_changed
            && !runner_motion_changed
            && !drawer_lock_changed
            && self.constraint_warning.is_none()
            && ids
                .iter()
                .all(|id| old.parts.iter().any(|p| PartId::of(p) == *id));
    }
    fn geometry_stamps(&self) -> Vec<GeometryStamp> {
        let mut meshes = HashMap::new();
        let mut stamps: Vec<_> = self
            .parts
            .iter()
            .map(|part| {
                let hash = *meshes.entry(part.mesh_key()).or_insert_with(|| {
                    // FNV-style word hash, fixed algorithm/version across builds.
                    // Appearance (normals/shading/colors) is deliberately excluded.
                    part.mesh
                        .vertices
                        .iter()
                        .flat_map(|v| v.position)
                        .chain(part.mesh.min.to_array())
                        .chain(part.mesh.max.to_array())
                        .fold(0xcbf29ce484222325_u64, |h, v| {
                            (h ^ u64::from(v.to_bits())).wrapping_mul(0x100000001b3)
                        })
                });
                let hash = part.source_size.to_array().into_iter().fold(hash, |h, v| {
                    (h ^ u64::from(v.to_bits())).wrapping_mul(0x100000001b3)
                });
                GeometryStamp {
                    part: crate::constraints::PartId::of(part),
                    fingerprint: format!("{hash:016x}:{}", part.source_bodies),
                }
            })
            .collect();
        stamps.sort_by(|a, b| {
            (&a.part.path, a.part.instance, a.part.body).cmp(&(
                &b.part.path,
                b.part.instance,
                b.part.body,
            ))
        });
        stamps
    }
    pub fn definition(&self) -> Definition {
        Definition {
            calibrated: self.calibrated,
            drawer_lock_version: 1,
            geometry_version: 1,
            geometry: if self.calibrated {
                self.geometry_stamps()
            } else {
                Vec::new()
            },
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
    pub(super) fn tiny_scene() -> Scene {
        let mesh = Arc::new(
            Mesh::parse(b"solid x\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid").unwrap(),
        );
        Scene {
            assembly: true,
            parts: vec![Part {
                source_size: mesh.size,
                mesh,
                pose: Pose {
                    path: "synthetic.stl".into(),
                    instance: 0,
                    body: None,
                    position: [0.; 3],
                    rotation: [0.; 3],
                    travel: [0.; 3],
                    travel_frame: None,
                },
                color: [1.; 4],
                visible: true,
                source_bodies: 1,
            }],
            ..Default::default()
        }
    }
    #[test]
    fn saved_fixed_runner_seats_migrate_at_the_closed_datum_per_drawer() {
        use crate::constraints::{AssemblyRules, Joint, PartId, Relation, solve};
        let mut current = tiny_scene();
        current.parts[0].pose.instance = MEMBER_STRIDE;
        current.parts[0].pose.position = [184., 5., -20.];
        current.parts[0].pose.rotation = [0., 30., 0.];
        let housing = PartId::of(&current.parts[0]);
        // A common shell and identical file paths exercise the persisted drawer namespaces.
        let cases = [
            (80., 100., 94., 80., 80.),
            (120., 100., 70., 120., 70.),
            (110., 55., 10., 55., 10.),
        ];
        for (index, &(limit, ..)) in cases.iter().enumerate() {
            let mut drawer = current.parts[0].clone();
            drawer.pose.instance = Target {
                member: 1,
                drawer: Some(index),
            }
            .instance(0);
            drawer.pose.path = "parts/Boxes/drawer.stl".into();
            drawer.source_size.y = 120.;
            let mut guide = drawer.clone();
            guide.pose.path = "parts/Runners/Runner Guide.stl".into();
            let drawer_id = PartId::of(&drawer);
            current.relations.extend([
                Relation {
                    parent: housing.clone(),
                    child: drawer_id.clone(),
                    label: format!("Member 2: {}", AssemblyRules::DRAWER_SLIDE),
                    joint: Joint::Slider {
                        position: [3., 50. * index as f32, 10.],
                        rotation: Quat::from_rotation_y(0.7).to_array(),
                        axis: [0., 0., 1.],
                        min: 0.,
                        max: limit,
                        offset: 0.,
                    },
                },
                Relation {
                    parent: drawer_id,
                    child: PartId::of(&guide),
                    label: "Two-stage guide".into(),
                    joint: Joint::RunnerGuide {
                        position: [0.; 3],
                        rotation: Quat::IDENTITY.to_array(),
                        inner_travel: 25.,
                        outer_travel: limit - 25.,
                    },
                },
            ]);
            current.parts.extend([drawer, guide]);
        }
        solve(&mut current).unwrap();
        let mut old = current.clone();
        let mut seats = Vec::new();
        for (index, &(_, previous_max, previous_offset, ..)) in cases.iter().enumerate() {
            let Joint::Slider { max, offset, .. } = &mut old.relations[2 * index].joint else {
                unreachable!()
            };
            *max = previous_max;
            *offset = previous_offset;
            let guide = &mut old.relations[2 * index + 1];
            guide.parent = housing.clone();
            guide.label = format!("Drawer slot {} (estimated)", index + 1);
            guide.joint = Joint::Fixed {
                position: [17., 50. * index as f32 + 2., -3.],
                rotation: Quat::from_rotation_x(0.2).to_array(),
            };
            seats.push(guide.joint.transform().unwrap());
        }
        solve(&mut old).unwrap();
        old.calibrated = true;
        let saved = toml::from_str(&toml::to_string(&old.definition()).unwrap()).unwrap();
        let mut migrated = current.clone();
        migrated.apply(&saved).unwrap();
        assert!(!migrated.calibrated);
        let frame = migrated.parts[0].pose.matrix(0.);
        for (index, &(_, _, _, expected_max, expected_offset)) in cases.iter().enumerate() {
            let driver = &migrated
                .relations
                .iter()
                .find(|r| r.child == PartId::of(&migrated.parts[1 + index * 2]))
                .unwrap()
                .joint;
            let Joint::Slider { offset, max, .. } = *driver else {
                unreachable!()
            };
            assert_eq!((max, offset), (expected_max, expected_offset));
            let guide = &migrated.parts[2 + index * 2];
            let outer_offset = (offset - 25.).max(0.);
            let expected = frame * Mat4::from_translation(Vec3::Z * outer_offset) * seats[index];
            assert!(guide.pose.matrix(0.).abs_diff_eq(expected, 0.001));
        }
        // A subsequently verified two-stage assembly retains its calibration on reload.
        migrated.calibrated = true;
        current.apply(&migrated.definition()).unwrap();
        assert!(current.calibrated);
        // A saved part without its guide relation acquires a new estimated joint.
        let mut incomplete = migrated.definition();
        let missing_guide = PartId::of(&migrated.parts[2]);
        incomplete.relations.retain(|r| r.child != missing_guide);
        current.apply(&incomplete).unwrap();
        assert!(!current.calibrated);
    }
    #[test]
    fn saved_invalid_poses_and_graphs_leave_the_scene_unchanged() {
        let mut scene = tiny_scene();
        scene.calibrated = true;
        let before = toml::to_string(&scene.definition()).unwrap();
        let mut invalid = Vec::new();
        for field in 0..3 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut definition = scene.definition();
                match field {
                    0 => definition.poses[0].position[0] = value,
                    1 => definition.poses[0].rotation[0] = value,
                    _ => definition.poses[0].travel[0] = value,
                }
                invalid.push(definition);
            }
        }
        for frame in [[0.; 4], [0., 0., 0., 2.], [f32::NAN, 0., 0., 1.]] {
            let mut definition = scene.definition();
            definition.poses[0].travel_frame = Some(frame);
            invalid.push(definition);
        }
        let mut duplicate = scene.definition();
        duplicate.poses.push(duplicate.poses[0].clone());
        invalid.push(duplicate);
        let mut cycle = scene.definition();
        let id = crate::constraints::PartId::of(&scene.parts[0]);
        cycle.relations.push(crate::constraints::Relation {
            parent: id.clone(),
            child: id,
            joint: crate::constraints::Joint::Fixed {
                position: [0.; 3],
                rotation: [0., 0., 0., 1.],
            },
            label: "bad self-cycle".into(),
        });
        invalid.push(cycle);
        for definition in invalid {
            let parsed: Definition =
                toml::from_str(&toml::to_string(&definition).unwrap()).unwrap();
            assert!(scene.apply(&parsed).is_err());
            assert_eq!(toml::to_string(&scene.definition()).unwrap(), before);
            assert!(scene.parts[0].pose.matrix(1.).is_finite());
        }
        let mut invalid_root = scene.clone();
        invalid_root.parts[0].pose.position[0] = f32::NAN;
        assert!(crate::constraints::solve(&mut invalid_root).is_err());
    }
    #[test]
    fn verification_is_bound_to_geometry_and_legacy_saves_need_review() {
        let mut source = tiny_scene();
        source.calibrated = true;
        let saved: Definition =
            toml::from_str(&toml::to_string(&source.definition()).unwrap()).unwrap();
        let mut same = tiny_scene();
        same.apply(&saved).unwrap();
        assert!(same.calibrated);
        let mut changed = tiny_scene();
        changed.parts[0].mesh = Arc::new(
            Mesh::parse(b"solid x\nvertex 0 0 0\nvertex 30 0 0\nvertex 0 1 0\nendsolid").unwrap(),
        );
        changed.parts[0].source_size = changed.parts[0].mesh.size;
        changed.apply(&saved).unwrap();
        assert!(!changed.calibrated);
        assert!(
            changed
                .constraint_warning
                .as_ref()
                .unwrap()
                .contains("review required")
        );
        changed.retain_assembly_state(&source);
        assert!(!changed.calibrated);
        let mut legacy = saved.clone();
        legacy.geometry.clear();
        legacy.geometry_version = 0;
        same.apply(&legacy).unwrap();
        assert!(!same.calibrated);
        let mut appearance = tiny_scene();
        Arc::make_mut(&mut appearance.parts[0].mesh).vertices[0].shade = 0.5;
        appearance.apply(&saved).unwrap();
        assert!(appearance.calibrated);
    }
    #[test]
    fn shared_load_cache_reuses_geometry_and_stops_before_next_source() {
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/test-fixtures");
        std::fs::create_dir_all(&fixture_root).unwrap();
        let dir = tempfile::tempdir_in(fixture_root).unwrap();
        let file = dir.path().join("one.stl");
        std::fs::write(
            &file,
            b"solid x\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid",
        )
        .unwrap();
        let item = Bom {
            path: "one.stl".into(),
            qty: 1,
            group: "test".into(),
            enabled: true,
        };
        let mut cache = LoadCache::new(dir.path(), &Rules::default());
        cache.remaining = 1;
        let first =
            Scene::load_cached(&mut cache, std::slice::from_ref(&item), "one".into(), false)
                .unwrap();
        std::fs::remove_file(&file).unwrap();
        let second = Scene::load_cached(&mut cache, &[item], "two".into(), false).unwrap();
        assert!(Arc::ptr_eq(&first.parts[0].mesh, &second.parts[0].mesh));
        assert_eq!(cache.remaining, 0);
        assert!(
            cache
                .source("not-present.stl")
                .err()
                .unwrap()
                .contains("budget exhausted before reading")
        );
        let mut pool = MeshPool {
            limit: 1,
            ..Default::default()
        };
        let mut one = tiny_scene();
        let mut equal = tiny_scene();
        pool.share(&mut one).unwrap();
        pool.share(&mut equal).unwrap();
        assert!(Arc::ptr_eq(&one.parts[0].mesh, &equal.parts[0].mesh));
        let mut different = tiny_scene();
        Arc::make_mut(&mut different.parts[0].mesh).vertices[0].position[0] += 1.;
        assert!(pool.share(&mut different).is_err());
        assert_eq!(pool.triangles, 1);
    }
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
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
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
        let mut pool = MeshPool::default();
        for (scene, placement, _) in &mut members {
            placement.validate()?;
            if !slots.insert((placement.column, placement.level)) {
                return Err("同一列和层只能放置一个成员 / Duplicate column and level".into());
            }
            pool.share(scene)?;
            crate::constraints::AssemblyRules::close_drawers(scene)?;
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
            result.member_placements.push(placement);
            result.member_shifts.push(shift);
            result
                .member_interiors
                .push(scene.sustainer_interior.take());
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
        crate::constraints::AssemblyRules::space_combination(&mut result)?;
        Ok(result)
    }
}

#[cfg(test)]
mod combination_tests {
    use super::*;
    #[test]
    fn detail_target_keeps_repeated_sources_separate_inside_shared_housing() {
        let mut scene = unit_scene();
        let shell = scene.parts[0].clone();
        scene.parts = vec![shell.clone(); 4];
        let target = Target {
            member: 5,
            drawer: Some(1),
        };
        for (part, owner) in scene.parts.iter_mut().zip([
            Target {
                member: 5,
                drawer: None,
            },
            Target {
                member: 5,
                drawer: Some(0),
            },
            target,
            Target {
                member: 6,
                drawer: Some(1),
            },
        ]) {
            part.pose.instance = owner.instance(2);
            part.pose.position = [part.pose.instance as f32, 20., 30.];
        }
        let detail = scene.target_scene(target);
        assert_eq!(detail.parts.len(), 1);
        assert_eq!(Target::of(&detail.parts[0]), target);
        let (lo, hi) = scene.target_envelope(target).unwrap();
        assert_eq!(lo, Vec3::new(target.instance(2) as f32, 20., 30.));
        assert_eq!(hi - lo, shell.mesh.size);
        assert_eq!(
            scene
                .target_scene(Target {
                    member: 5,
                    drawer: None
                })
                .parts
                .len(),
            3
        );
        assert_eq!(scene.parts.len(), 4);
        assert!(scene.parts.iter().all(|p| p.visible));
    }
    #[test]
    fn combined_insert_editors_keep_each_members_local_geometry() {
        let mut source = unit_scene();
        source.sustainer_interior = Some(Box::new(crate::sustainer_dividers::Geometry {
            floor: 7.,
            specification: "first".into(),
            ..Default::default()
        }));
        let mut second = unit_scene();
        second.sustainer_interior = Some(Box::new(crate::sustainer_dividers::Geometry {
            floor: 11.,
            specification: "second".into(),
            ..Default::default()
        }));
        let combined = Scene::combine(
            vec![
                (source, Placement::default(), 0.),
                (
                    second,
                    Placement {
                        level: 1,
                        ..Default::default()
                    },
                    0.,
                ),
            ],
            0.,
        )
        .unwrap();
        assert_eq!(combined.interior_geometry(0).unwrap().floor, 7.);
        assert_eq!(combined.interior_geometry(1).unwrap().floor, 11.);
        assert_eq!(
            combined.interior_geometry(1).unwrap().specification,
            "second"
        );
        assert!(combined.interior_geometry(2).is_none());
    }
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
