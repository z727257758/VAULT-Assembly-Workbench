//! Directed assembly constraints. One parent per child; roots remain grounded.
//! Solving is deterministic/topological, not a physics relaxation simulation.
use crate::scene::{Part, Pose, Scene};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PartId {
    pub path: String,
    pub instance: u32,
    #[serde(default)]
    pub body: Option<u32>,
}
impl PartId {
    pub fn of(p: &Part) -> Self {
        Self {
            path: p.pose.path.clone(),
            instance: p.pose.instance,
            body: p.pose.body,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Joint {
    Fixed {
        position: [f32; 3],
        rotation: [f32; 4],
    },
    Face {
        parent_point: [f32; 3],
        child_point: [f32; 3],
        parent_normal: [f32; 3],
        rotation: [f32; 4],
        gap: f32,
    },
    Slider {
        position: [f32; 3],
        rotation: [f32; 4],
        axis: [f32; 3],
        offset: f32,
        min: f32,
        max: f32,
    },
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Relation {
    pub parent: PartId,
    pub child: PartId,
    pub joint: Joint,
    pub label: String,
}
#[derive(Clone, Copy, Debug)]
pub struct SurfaceHit {
    pub part: usize,
    pub point: Vec3,
    pub normal: Vec3,
}
fn pose_rotation(p: &Pose) -> Quat {
    let r = Vec3::from(p.rotation).map(f32::to_radians);
    Quat::from_euler(glam::EulerRot::XYZ, r.x, r.y, r.z)
}
fn finite(v: [f32; 3]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() < 1e6)
}
fn quaternion(v: [f32; 4]) -> Result<Quat, String> {
    let q = Quat::from_array(v);
    if !q.is_finite() || q.length() < 0.9 || q.length() > 1.1 {
        return Err("Invalid joint rotation".into());
    }
    Ok(q.normalize())
}
impl Joint {
    pub fn transform(&self) -> Result<Mat4, String> {
        let (p, q) = match *self {
            Self::Fixed { position, rotation } => {
                if !finite(position) {
                    return Err("Invalid fixed position".into());
                }
                (Vec3::from(position), quaternion(rotation)?)
            }
            Self::Face {
                parent_point,
                child_point,
                parent_normal,
                rotation,
                gap,
            } => {
                if !finite(parent_point)
                    || !finite(child_point)
                    || !finite(parent_normal)
                    || !gap.is_finite()
                    || gap.abs() > 10000.
                {
                    return Err("Invalid face anchors or gap".into());
                }
                let n = Vec3::from(parent_normal);
                if n.length() < 0.99 || n.length() > 1.01 {
                    return Err("Face normal must be unit length".into());
                }
                let q = quaternion(rotation)?;
                (
                    Vec3::from(parent_point) + n * gap - q * Vec3::from(child_point),
                    q,
                )
            }
            Self::Slider {
                position,
                rotation,
                axis,
                offset,
                min,
                max,
            } => {
                let a = Vec3::from(axis);
                if !finite(position)
                    || !a.is_finite()
                    || a.length() < 0.99
                    || a.length() > 1.01
                    || ![offset, min, max].iter().all(|v| v.is_finite())
                    || min > max
                    || offset < min
                    || offset > max
                {
                    return Err("Invalid slider axis or travel limits".into());
                }
                (Vec3::from(position) + a * offset, quaternion(rotation)?)
            }
        };
        Ok(Mat4::from_rotation_translation(q, p))
    }
}
impl Relation {
    pub fn fixed(scene: &Scene, parent: usize, child: usize, slider: bool) -> Result<Self, String> {
        if parent == child {
            return Err("Parent and child must be different parts".into());
        }
        let a = &scene.parts[parent];
        let b = &scene.parts[child];
        let relative = a.pose.matrix(0.).inverse() * b.pose.matrix(0.);
        let (_, q, p) = relative.to_scale_rotation_translation();
        let joint = if slider {
            Joint::Slider {
                position: p.to_array(),
                rotation: q.to_array(),
                axis: (pose_rotation(&a.pose).inverse() * Vec3::Z).to_array(),
                offset: 0.,
                min: -300.,
                max: 300.,
            }
        } else {
            Joint::Fixed {
                position: p.to_array(),
                rotation: q.to_array(),
            }
        };
        Ok(Self {
            parent: PartId::of(a),
            child: PartId::of(b),
            joint,
            label: if slider { "Slider" } else { "Fixed" }.into(),
        })
    }
    pub fn face(
        scene: &Scene,
        parent: SurfaceHit,
        child: SurfaceHit,
        gap: f32,
        twist: f32,
    ) -> Result<Self, String> {
        if parent.part == child.part {
            return Err("Pick faces on two different parts".into());
        }
        let a = &scene.parts[parent.part];
        let b = &scene.parts[child.part];
        let qa = pose_rotation(&a.pose);
        let qb = pose_rotation(&b.pose);
        let pn = parent.normal.normalize_or_zero();
        let cn = child.normal.normalize_or_zero();
        if pn.length() < 0.9 || cn.length() < 0.9 {
            return Err("Cannot mate degenerate faces".into());
        }
        let target_normal = qa * pn;
        let q_world = Quat::from_axis_angle(target_normal, twist.to_radians())
            * Quat::from_rotation_arc(qb * cn, -target_normal)
            * qb;
        let relative = (qa.inverse() * q_world).normalize();
        let result = Self {
            parent: PartId::of(a),
            child: PartId::of(b),
            joint: Joint::Face {
                parent_point: parent.point.to_array(),
                child_point: child.point.to_array(),
                parent_normal: pn.to_array(),
                rotation: relative.to_array(),
                gap,
            },
            label: "Face mate".into(),
        };
        result.joint.transform()?;
        Ok(result)
    }
}
pub fn solve(scene: &mut Scene) -> Result<(), String> {
    let count = scene.parts.len();
    let ids: Vec<_> = scene.parts.iter().map(PartId::of).collect();
    let mut incoming = vec![None; count];
    let mut edges = Vec::new();
    for r in &scene.relations {
        let parent = ids
            .iter()
            .position(|p| p == &r.parent)
            .ok_or("Constraint parent is missing")?;
        let child = ids
            .iter()
            .position(|p| p == &r.child)
            .ok_or("Constraint child is missing")?;
        if parent == child || incoming[child].is_some() {
            return Err("A part may have only one parent and cannot constrain itself".into());
        }
        incoming[child] = Some(parent);
        edges.push((parent, child, r.joint.transform()?));
    }
    let mut poses: Vec<_> = scene.parts.iter().map(|p| p.pose.matrix(0.)).collect();
    let mut solved: HashSet<usize> = (0..count).filter(|i| incoming[*i].is_none()).collect();
    loop {
        let before = solved.len();
        for &(parent, child, relative) in &edges {
            if solved.contains(&parent) && !solved.contains(&child) {
                poses[child] = poses[parent] * relative;
                solved.insert(child);
            }
        }
        if solved.len() == count {
            break;
        }
        if solved.len() == before {
            return Err("Assembly constraint cycle detected".into());
        }
    }
    // Transactional: validate the entire graph before changing a single pose.
    for (i, p) in scene.parts.iter_mut().enumerate() {
        if incoming[i].is_none() {
            continue;
        }
        let (_, q, pos) = poses[i].to_scale_rotation_translation();
        let (x, y, z) = q.to_euler(glam::EulerRot::XYZ);
        p.pose.position = pos.to_array();
        p.pose.rotation = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
    }
    Ok(())
}
pub fn add(scene: &mut Scene, relation: Relation) -> Result<(), String> {
    if scene.relations.iter().any(|r| r.child == relation.child) {
        return Err("Child already has a parent; remove its existing relation first".into());
    }
    scene.relations.push(relation);
    if let Err(e) = solve(scene) {
        scene.relations.pop();
        return Err(e);
    }
    scene.calibrated = false;
    Ok(())
}
/// Rotate a constrained subassembly around its own origin in world coordinates.
/// Rebase only its incoming relation; descendant local joints remain unchanged.
/// Transactional, so a failed solve cannot leave a half-rotated assembly.
pub fn rotate_subassembly(scene: &mut Scene, index: usize, rotation: Quat) -> Result<(), String> {
    if index >= scene.parts.len() || !rotation.is_finite() || (rotation.length() - 1.).abs() > 0.001
    {
        return Err("Invalid subassembly rotation".into());
    }
    let previous = scene.clone();
    let id = PartId::of(&scene.parts[index]);
    let incoming = scene.relations.iter().position(|r| r.child == id);
    if incoming.is_some_and(|i| matches!(scene.relations[i].joint, Joint::Slider { .. })) {
        return Err("Rotate the fixed subassembly, not its sliding parent".into());
    }
    let mut subtree = HashSet::from([id.clone()]);
    loop {
        let before = subtree.len();
        for r in &scene.relations {
            if subtree.contains(&r.parent) {
                subtree.insert(r.child.clone());
            }
        }
        if before == subtree.len() {
            break;
        }
    }
    let (_, q, p) = scene.parts[index]
        .pose
        .matrix(0.)
        .to_scale_rotation_translation();
    let (x, y, z) = (rotation * q).to_euler(glam::EulerRot::XYZ);
    scene.parts[index].pose.rotation = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
    scene.parts[index].pose.position = p.to_array();
    let result = (|| {
        if let Some(r) = incoming {
            let parent = scene
                .parts
                .iter()
                .position(|p| PartId::of(p) == scene.relations[r].parent)
                .ok_or("Parent missing")?;
            let mut rebased = Relation::fixed(scene, parent, index, false)?;
            rebased.label = scene.relations[r].label.clone();
            scene.relations[r] = rebased;
        }
        solve(scene)
    })();
    if result.is_err() {
        *scene = previous;
    } else {
        for p in &mut scene.parts {
            if subtree.contains(&PartId::of(p)) && p.pose.travel_frame.is_none() {
                p.pose.travel = (rotation * Vec3::from(p.pose.travel)).to_array();
            }
        }
        scene.calibrated = false;
    }
    result
}

pub fn world_bounds(p: &Part) -> (Vec3, Vec3) {
    let m = p.pose.matrix(0.);
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for v in &p.mesh.vertices {
        let p = m.transform_point3(v.position.into());
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}
fn world_anchor(scene: &Scene, index: usize, point: Vec3, normal: Vec3) -> SurfaceHit {
    let inverse = scene.parts[index].pose.matrix(0.).inverse();
    SurfaceHit {
        part: index,
        point: inverse.transform_point3(point),
        normal: inverse.transform_vector3(normal).normalize(),
    }
}
/// Conservative initial datums for the standard drawer. Bounds are only used for
/// exterior face placement, never as a cavity-fit or collision proof.
pub fn basic_relations(scene: &mut Scene) -> Result<usize, String> {
    if !scene.relations.is_empty() {
        return Err("Basic relations require an empty constraint graph".into());
    }
    let original = scene.clone();
    let result = (|| {
        let module = scene
            .parts
            .iter()
            .position(|p| p.pose.path.contains("/Modules/"));
        let drawer = scene.parts.iter().position(|p| {
            p.pose.path.contains("/Boxes/")
                && p.pose.path.contains(" Box ")
                && !p.pose.path.contains(" Part ")
        });
        let facade = scene
            .parts
            .iter()
            .position(|p| p.pose.path.contains("/Facades/"));
        let (Some(module), Some(drawer), Some(facade)) = (module, drawer, facade) else {
            return Err(
                "Basic relations currently support non-split drawers with a separate facade".into(),
            );
        };
        crate::module_assembly::prepare(scene, module, drawer, facade)?;
        let mut slider = Relation::fixed(scene, module, drawer, true)?;
        if let Joint::Slider { min, max, .. } = &mut slider.joint {
            *min = 0.;
            *max = (scene.parts[drawer].source_size.y - 20.).max(0.);
        }
        slider.label = "Manual step 10: drawer / housing slide".into();
        add(scene, slider)?;
        // Attach subassemblies before moving the facade, so descendants follow
        // its correction and subsequent drawer travel rather than staying behind.
        for i in 0..scene.parts.len() {
            if [module, drawer, facade].contains(&i) {
                continue;
            }
            let path = &scene.parts[i].pose.path;
            let name = path.rsplit('/').next().unwrap_or(path);
            let parent = if name.contains("Runner Guide")
                || name.contains("Stopper")
                || name == "Latch.stl"
                || path.contains("/Roofs/")
            {
                Some(module)
            } else if name.contains("Runner Slide")
                || name.contains("Rails")
                || name.contains("Baseplate")
            {
                Some(drawer)
            } else if name.contains("Key") || name == "Label.stl" || name == "Facade Cap.stl" {
                Some(facade)
            } else {
                None
            };
            if let Some(parent) = parent {
                let mut relation = Relation::fixed(scene, parent, i, false)?;
                relation.label = "Subassembly attachment (verify)".into();
                add(scene, relation)?;
            }
        }
        let (_, box_hi) = world_bounds(&scene.parts[drawer]);
        let (face_lo, face_hi) = world_bounds(&scene.parts[facade]);
        let face_center = (face_lo + face_hi) * 0.5;
        let anchor_a = world_anchor(
            scene,
            drawer,
            Vec3::new(face_center.x, face_center.y, box_hi.z),
            Vec3::Z,
        );
        let anchor_b = world_anchor(
            scene,
            facade,
            Vec3::new(face_center.x, face_center.y, face_lo.z),
            -Vec3::Z,
        );
        let mut mate = Relation::face(scene, anchor_a, anchor_b, 0.2, 0.)?;
        mate.label = "Facade / drawer exterior datum (verify)".into();
        add(scene, mate)?;
        // Planar support datums: spring front against facade rear. Split bodies keep
        // separate IDs and independent anchors. These are starting points, not CAD fits.
        for i in 0..scene.parts.len() {
            if scene.parts[i].pose.path.ends_with("/Springs.stl") {
                let (lo, hi) = world_bounds(&scene.parts[i]);
                let center = (lo + hi) * 0.5;
                let (facade_lo, _) = world_bounds(&scene.parts[facade]);
                let parent = world_anchor(
                    scene,
                    facade,
                    Vec3::new(center.x, center.y, facade_lo.z),
                    -Vec3::Z,
                );
                let child = world_anchor(scene, i, Vec3::new(center.x, center.y, hi.z), Vec3::Z);
                let mut mate = Relation::face(scene, parent, child, 0.2, 0.)?;
                mate.label = "Spring / facade rear datum (verify)".into();
                add(scene, mate)?;
            }
        }
        crate::front_panel::apply(scene, facade)?;
        // Manual front view: label on the left, round screw cap on the right.
        // Rotate the entire front subassembly, retaining all child-local mates.
        rotate_subassembly(scene, facade, Quat::from_rotation_z(std::f32::consts::PI))?;
        crate::module_assembly::latch(scene, module)?;
        crate::runners::apply(scene, module, drawer)?;
        crate::geared_rails::apply(scene, drawer)?;
        crate::baseplate::apply(scene, drawer)?;
        crate::explosion::reset(scene)?;
        Ok(scene.relations.len())
    })();
    if result.is_err() {
        *scene = original;
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh;
    use std::sync::Arc;
    fn part(x: f32, id: u32) -> Part {
        Part {
            mesh: Arc::new(
                Mesh::parse(b"solid a\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid")
                    .unwrap(),
            ),
            pose: Pose {
                path: "test.stl".into(),
                instance: id,
                body: None,
                position: [x, 0., 0.],
                rotation: [0.; 3],
                travel: [100., 0., 0.],
                travel_frame: None,
            },
            color: [1.; 4],
            visible: true,
            source_size: Vec3::ONE,
            source_bodies: 1,
        }
    }
    fn scene() -> Scene {
        Scene {
            parts: vec![part(0., 0), part(5., 1), part(8., 2)],
            ..Default::default()
        }
    }
    #[test]
    fn fixed_chain_follows_parent_and_explosion_is_not_solved() {
        let mut s = scene();
        add(&mut s, Relation::fixed(&scene(), 0, 1, false).unwrap()).unwrap();
        let r = Relation::fixed(&s, 1, 2, false).unwrap();
        add(&mut s, r).unwrap();
        s.parts[0].pose.position[0] = 10.;
        solve(&mut s).unwrap();
        assert!((s.parts[1].pose.position[0] - 15.).abs() < 1e-5);
        assert!((s.parts[2].pose.position[0] - 18.).abs() < 1e-5);
        assert_eq!(s.parts[1].pose.travel, [100., 0., 0.]);
    }
    #[test]
    fn cycles_are_rejected_without_moving_parts() {
        let mut s = scene();
        add(&mut s, Relation::fixed(&scene(), 0, 1, false).unwrap()).unwrap();
        let r = Relation::fixed(&s, 1, 0, false).unwrap();
        let before = s.parts[0].pose.position;
        assert!(add(&mut s, r).is_err());
        assert_eq!(s.parts[0].pose.position, before);
        assert_eq!(s.relations.len(), 1);
    }
    #[test]
    fn face_anchor_gap_and_opposed_normals() {
        let mut s = scene();
        let a = SurfaceHit {
            part: 0,
            point: Vec3::ZERO,
            normal: Vec3::Y,
        };
        let b = SurfaceHit {
            part: 1,
            point: Vec3::X,
            normal: Vec3::Y,
        };
        let r = Relation::face(&s, a, b, 0.2, 35.).unwrap();
        add(&mut s, r).unwrap();
        let m = s.parts[1].pose.matrix(0.);
        assert!((m.transform_point3(Vec3::X) - Vec3::Y * 0.2).length() < 1e-5);
        assert!(m.transform_vector3(Vec3::Y).dot(Vec3::Y) < -0.9999);
    }
    #[test]
    fn slider_locks_five_dofs_and_respects_limits() {
        let mut s = scene();
        let mut r = Relation::fixed(&s, 0, 1, true).unwrap();
        if let Joint::Slider {
            offset, min, max, ..
        } = &mut r.joint
        {
            *offset = 12.;
            *min = 0.;
            *max = 20.;
        }
        add(&mut s, r).unwrap();
        assert_eq!(s.parts[1].pose.position, [5., 0., 12.]);
        if let Joint::Slider { offset, .. } = &mut s.relations[0].joint {
            *offset = 21.;
        }
        assert!(solve(&mut s).is_err());
        assert_eq!(s.parts[1].pose.position, [5., 0., 12.]);
    }
}
