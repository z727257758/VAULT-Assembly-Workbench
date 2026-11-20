//! Triangle-surface crossing diagnostic, accelerated by median-split BVHs.
//! Coplanar contact and complete containment are intentionally not classified as
//! penetration. A clean report is NOT a manufacturing clearance certificate.
use crate::{
    constraints::PartId,
    scene::{Part, Scene},
};
use glam::{DVec3, Vec3};
#[derive(Clone, Copy)]
struct Bounds {
    lo: Vec3,
    hi: Vec3,
}
impl Bounds {
    fn empty() -> Self {
        Self {
            lo: Vec3::splat(f32::INFINITY),
            hi: Vec3::splat(f32::NEG_INFINITY),
        }
    }
    fn add(&mut self, p: Vec3) {
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }
    fn overlaps(self, b: Self, eps: f32) -> bool {
        (self.lo - b.hi).max_element() <= eps && (b.lo - self.hi).max_element() <= eps
    }
}
#[derive(Clone, Copy)]
struct Triangle {
    p: [Vec3; 3],
    bounds: Bounds,
    center: Vec3,
}
struct Node {
    bounds: Bounds,
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}
struct Tree {
    triangles: Vec<Triangle>,
    order: Vec<usize>,
    nodes: Vec<Node>,
}
impl Tree {
    fn from_part(part: &Part, explosion: f32) -> Self {
        let transform = part.pose.matrix(explosion);
        let triangles: Vec<_> = part
            .mesh
            .vertices
            .chunks_exact(3)
            .map(|tri| {
                let p = std::array::from_fn(|i| transform.transform_point3(tri[i].position.into()));
                let mut bounds = Bounds::empty();
                for point in p {
                    bounds.add(point);
                }
                Triangle {
                    p,
                    bounds,
                    center: (p[0] + p[1] + p[2]) / 3.,
                }
            })
            .collect();
        let mut tree = Self {
            order: (0..triangles.len()).collect(),
            triangles,
            nodes: Vec::new(),
        };
        if !tree.order.is_empty() {
            tree.build(0, tree.order.len());
        }
        tree
    }
    fn build(&mut self, start: usize, end: usize) -> usize {
        let mut bounds = Bounds::empty();
        for &i in &self.order[start..end] {
            bounds.add(self.triangles[i].bounds.lo);
            bounds.add(self.triangles[i].bounds.hi);
        }
        let node = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            start,
            end,
            children: None,
        });
        if end - start > 8 {
            let extent = bounds.hi - bounds.lo;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            let middle = (start + end) / 2;
            let triangles = &self.triangles;
            self.order[start..end].select_nth_unstable_by(middle - start, |a, b| {
                triangles[*a].center[axis].total_cmp(&triangles[*b].center[axis])
            });
            let a = self.build(start, middle);
            let b = self.build(middle, end);
            self.nodes[node].children = Some((a, b));
        }
        node
    }
    fn crossing(&self, other: &Self, eps: f32) -> Option<Vec3> {
        let mut first = None;
        self.visit_crossings(other, eps, |segment| {
            first = Some(segment.midpoint());
            false
        });
        first
    }
    /// Stops when the visitor returns false. Returns true on a complete traversal.
    fn visit_crossings(
        &self,
        other: &Self,
        eps: f32,
        mut visit: impl FnMut(CrossingSegment) -> bool,
    ) -> bool {
        if self.nodes.is_empty() || other.nodes.is_empty() {
            return true;
        }
        let mut stack = vec![(0, 0)];
        while let Some((a, b)) = stack.pop() {
            let na = &self.nodes[a];
            let nb = &other.nodes[b];
            if !na.bounds.overlaps(nb.bounds, eps) {
                continue;
            }
            match (na.children, nb.children) {
                (Some((x, y)), _) => {
                    stack.push((x, b));
                    stack.push((y, b));
                }
                (None, Some((x, y))) => {
                    stack.push((a, x));
                    stack.push((a, y));
                }
                (None, None) => {
                    for &i in &self.order[na.start..na.end] {
                        for &j in &other.order[nb.start..nb.end] {
                            let a = &self.triangles[i];
                            let b = &other.triangles[j];
                            if a.bounds.overlaps(b.bounds, eps)
                                && let Some(p) = cross_triangles(a.p, b.p, eps)
                                && !visit(p)
                            {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        true
    }
}
struct CrossingSegment {
    start: Vec3,
    end: Vec3,
}
impl CrossingSegment {
    fn midpoint(&self) -> Vec3 {
        self.start.midpoint(self.end)
    }
}
fn cross_triangles(a: [Vec3; 3], b: [Vec3; 3], eps: f32) -> Option<CrossingSegment> {
    let a = a.map(|p| p.as_dvec3());
    let b = b.map(|p| p.as_dvec3());
    let na = (a[1] - a[0]).cross(a[2] - a[0]).normalize_or_zero();
    let nb = (b[1] - b[0]).cross(b[2] - b[0]).normalize_or_zero();
    let da = a.map(|p| (p - b[0]).dot(nb));
    let db = b.map(|p| (p - a[0]).dot(na));
    let eps = f64::from(eps);
    let straddles = |d: [f64; 3]| d.iter().any(|&v| v > eps) && d.iter().any(|&v| v < -eps);
    if !straddles(da) || !straddles(db) {
        return None;
    }
    let direction = na.cross(nb).normalize_or_zero();
    if direction.length_squared() < 0.9 {
        return None;
    }
    let cut = |t: [DVec3; 3], d: [f64; 3]| {
        let mut points = Vec::with_capacity(3);
        for i in 0..3 {
            let j = (i + 1) % 3;
            if d[i].abs() < 1e-10 {
                points.push(t[i]);
            }
            if d[i] * d[j] < 0. {
                points.push(t[i] + (t[j] - t[i]) * (d[i] / (d[i] - d[j])));
            }
        }
        points
    };
    let ca = cut(a, da);
    let cb = cut(b, db);
    let origin = *ca.first()?;
    let interval = |points: &[DVec3]| {
        points
            .iter()
            .map(|p| (*p - origin).dot(direction))
            .fold([f64::INFINITY, f64::NEG_INFINITY], |r, v| {
                [r[0].min(v), r[1].max(v)]
            })
    };
    let ia = interval(&ca);
    let ib = interval(&cb);
    let lo = ia[0].max(ib[0]);
    let hi = ia[1].min(ib[1]);
    // Both finite plane-cut segments must overlap. An inclusive barycentric
    // edge test can mistake adjacent notch/flange boundaries for a crossing.
    (hi - lo > eps).then(|| CrossingSegment {
        start: (origin + direction * lo).as_vec3(),
        end: (origin + direction * hi).as_vec3(),
    })
}
#[derive(Clone)]
pub struct Crossing {
    pub a: usize,
    pub b: usize,
    pub point: Vec3,
}
#[derive(Clone)]
pub struct Report {
    pub pairs: Vec<Crossing>,
    pub tolerance: f32,
    snapshot: Vec<(PartId, [f32; 3], [f32; 3])>,
}
impl Report {
    pub fn is_current(&self, scene: &Scene) -> bool {
        self.snapshot.len() == scene.parts.len()
            && self
                .snapshot
                .iter()
                .zip(&scene.parts)
                .all(|((id, p, r), part)| {
                    *id == PartId::of(part) && *p == part.pose.position && *r == part.pose.rotation
                })
    }
}
pub fn check(scene: &Scene, tolerance: f32) -> Result<Report, String> {
    if !tolerance.is_finite() || !(0.0001..=1.).contains(&tolerance) {
        return Err("Tolerance must be between 0.0001 and 1 mm".into());
    }
    let mut trees = Vec::with_capacity(scene.parts.len());
    let mut total = 0usize;
    for part in &scene.parts {
        total += part.mesh.vertices.len() / 3;
        if total > 10_000_000 {
            return Err("Collision check limited to 10 million displayed triangles".into());
        }
        trees.push(Tree::from_part(part, 0.));
    }
    let mut pairs = Vec::new();
    for a in 0..trees.len() {
        for b in a + 1..trees.len() {
            if let Some(point) = trees[a].crossing(&trees[b], tolerance) {
                pairs.push(Crossing { a, b, point });
            }
        }
    }
    Ok(Report {
        pairs,
        tolerance,
        snapshot: scene
            .parts
            .iter()
            .map(|p| (PartId::of(p), p.pose.position, p.pose.rotation))
            .collect(),
    })
}

/// Cross-member surface diagnostics at one display pose. Envelope rejection only
/// selects triangle pairs to examine; it never counts as a positive crossing.
pub fn check_between_members(
    scene: &Scene,
    tolerance: f32,
    explosion: f32,
) -> Result<Vec<Crossing>, String> {
    check_between_members_with_budget(scene, tolerance, explosion, 10_000_000)
}
fn check_between_members_with_budget(
    scene: &Scene,
    tolerance: f32,
    explosion: f32,
    triangle_budget: usize,
) -> Result<Vec<Crossing>, String> {
    if !tolerance.is_finite()
        || !(0.0001..=1.).contains(&tolerance)
        || !explosion.is_finite()
        || !(0. ..=1.).contains(&explosion)
    {
        return Err("Invalid crossing tolerance or explosion amount".into());
    }
    let mut bounds = Vec::with_capacity(scene.parts.len());
    for part in &scene.parts {
        let matrix = part.pose.matrix(explosion);
        let mut envelope = Bounds::empty();
        for x in [part.mesh.min.x, part.mesh.max.x] {
            for y in [part.mesh.min.y, part.mesh.max.y] {
                for z in [part.mesh.min.z, part.mesh.max.z] {
                    envelope.add(matrix.transform_point3(Vec3::new(x, y, z)));
                }
            }
        }
        if !envelope.lo.is_finite() || !envelope.hi.is_finite() {
            return Err("Non-finite crossing envelope".into());
        }
        bounds.push(envelope);
    }
    let mut trees: Vec<Option<Tree>> = (0..scene.parts.len()).map(|_| None).collect();
    let mut total = 0usize;
    let mut pairs = Vec::new();
    for a in 0..scene.parts.len() {
        for b in a + 1..scene.parts.len() {
            if scene.parts[a].pose.instance / crate::scene::MEMBER_STRIDE
                == scene.parts[b].pose.instance / crate::scene::MEMBER_STRIDE
                || !bounds[a].overlaps(bounds[b], tolerance)
            {
                continue;
            }
            let missing: usize = [a, b]
                .into_iter()
                .filter(|&i| trees[i].is_none())
                .map(|i| scene.parts[i].mesh.vertices.len() / 3)
                .sum();
            let required =
                scene.parts[a].mesh.vertices.len() / 3 + scene.parts[b].mesh.vertices.len() / 3;
            if required > triangle_budget {
                return Err("Cross-member pair exceeds the diagnostic triangle budget".into());
            }
            // Bound retained BVH memory, rather than refusing a large assembly
            // just because many small, separate candidate pairs were visited.
            while total + missing > triangle_budget {
                let evict = trees
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| *i != a && *i != b && t.is_some())
                    .max_by_key(|(_, t)| t.as_ref().unwrap().triangles.len())
                    .map(|(i, _)| i)
                    .ok_or("Cross-member diagnostic memory budget exhausted")?;
                total -= trees[evict].take().unwrap().triangles.len();
            }
            for index in [a, b] {
                if trees[index].is_some() {
                    continue;
                }
                let part = &scene.parts[index];
                total += part.mesh.vertices.len() / 3;
                trees[index] = Some(Tree::from_part(part, explosion));
            }
            if let Some(point) = trees[a]
                .as_ref()
                .unwrap()
                .crossing(trees[b].as_ref().unwrap(), tolerance)
            {
                pairs.push(Crossing { a, b, point });
            }
        }
    }
    Ok(pairs)
}

/// Both ends of every intersection segment must lie in the convex contact
/// region, so the entire segment is inside. Empty pairs have no contact.
pub(crate) fn all_crossings_match(
    a: &Part,
    b: &Part,
    tolerance: f32,
    mut matches: impl FnMut(Vec3) -> bool,
) -> bool {
    let first = Tree::from_part(a, 0.);
    let second = Tree::from_part(b, 0.);
    let mut found = false;
    let completed = first.visit_crossings(&second, tolerance, |segment| {
        found = true;
        matches(segment.start) && matches(segment.end)
    });
    found && completed
}

#[cfg(test)]
pub fn audit_pair(scene: &Scene, a: usize, b: usize) -> Vec<Vec3> {
    let first = Tree::from_part(&scene.parts[a], 0.);
    let second = Tree::from_part(&scene.parts[b], 0.);
    let mut hits = Vec::new();
    first.visit_crossings(&second, 0.01, |segment| {
        hits.push(segment.midpoint());
        hits.len() < 100_000
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    fn member_triangles() -> Scene {
        use crate::{
            mesh::{Mesh, Vertex},
            scene::{Part, Pose},
        };
        let make = |points: [[f32; 3]; 3], instance| {
            let lo = points
                .iter()
                .copied()
                .map(Vec3::from)
                .fold(Vec3::splat(f32::INFINITY), Vec3::min);
            let hi = points
                .iter()
                .copied()
                .map(Vec3::from)
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
            Part {
                mesh: std::sync::Arc::new(Mesh {
                    vertices: points
                        .into_iter()
                        .map(|position| Vertex {
                            position,
                            normal: [0., 0., 1.],
                            shade: 1.,
                        })
                        .collect(),
                    min: lo,
                    max: hi,
                    size: hi - lo,
                }),
                pose: Pose {
                    path: format!("triangle-{instance}"),
                    instance,
                    body: None,
                    position: [0.; 3],
                    rotation: [0.; 3],
                    travel: [0.; 3],
                    travel_frame: None,
                },
                source_size: hi - lo,
                source_bodies: 1,
                color: [1.; 4],
                visible: true,
            }
        };
        Scene {
            parts: vec![
                make([[-2., -2., 0.], [2., -2., 0.], [0., 2., 0.]], 0),
                make(
                    [[0., -1., -1.], [0., 1., 1.], [0., 1., -1.]],
                    crate::scene::MEMBER_STRIDE,
                ),
            ],
            ..Default::default()
        }
    }
    #[test]
    fn localized_contact_requires_every_crossing_in_the_region() {
        let mut scene = member_triangles();
        let mesh = std::sync::Arc::make_mut(&mut scene.parts[1].mesh);
        let mut second = mesh.vertices.clone();
        for vertex in &mut second {
            vertex.position[0] += 0.25;
        }
        mesh.vertices.extend(second);
        mesh.max.x += 0.25;
        mesh.size = mesh.max - mesh.min;
        let [a, b] = &scene.parts[..] else {
            unreachable!()
        };
        assert!(all_crossings_match(a, b, 0.01, |p| (-0.01..0.26).contains(&p.x)));
        assert!(!all_crossings_match(a, b, 0.01, |p| p.x < 0.1));
        assert!(!all_crossings_match(a, b, 0.01, |p| p.x > 0.1));
        assert!(!all_crossings_match(a, b, 0.01, |p| p.y < 0.75));
        assert!(!all_crossings_match(a, a, 0.01, |_| true));
    }

    #[test]
    fn cross_member_check_uses_actual_triangles_and_the_requested_explosion_pose() {
        let mut scene = member_triangles();
        assert_eq!(check_between_members(&scene, 0.01, 0.).unwrap().len(), 1);
        scene.parts[1].pose.travel = [10., 0., 0.];
        assert!(check_between_members(&scene, 0.01, 1.).unwrap().is_empty());
        assert_eq!(check_between_members(&scene, 0.01, 0.).unwrap().len(), 1);
        scene.parts[1].pose.instance = 1;
        assert!(check_between_members(&scene, 0.01, 0.).unwrap().is_empty());
        assert_eq!(check(&scene, 0.01).unwrap().pairs.len(), 1);
    }
    #[test]
    fn cross_member_check_rejects_invalid_parameters_and_does_not_ignore_hidden_parts() {
        let mut scene = member_triangles();
        scene.parts[1].visible = false;
        assert_eq!(check_between_members(&scene, 0.01, 0.).unwrap().len(), 1);
        for amount in [-0.1, 1.1, f32::NAN] {
            assert!(check_between_members(&scene, 0.01, amount).is_err());
        }
        for tolerance in [0., -1., f32::INFINITY] {
            assert!(check_between_members(&scene, tolerance, 0.).is_err());
        }
    }
    #[test]
    fn cross_member_envelope_overlap_alone_does_not_create_a_crossing() {
        let mut scene = member_triangles();
        scene.parts[1].mesh = std::sync::Arc::clone(&scene.parts[0].mesh);
        assert!(check_between_members(&scene, 0.01, 0.).unwrap().is_empty());
    }
    #[test]
    fn bounded_cross_member_cache_keeps_checking_later_pairs_after_eviction() {
        let mut scene = member_triangles();
        let mut last = scene.parts[0].clone();
        last.pose.instance = 2 * crate::scene::MEMBER_STRIDE;
        scene.parts.push(last);
        let pairs = check_between_members_with_budget(&scene, 0.01, 0., 2).unwrap();
        assert_eq!(
            pairs.iter().map(|p| (p.a, p.b)).collect::<Vec<_>>(),
            [(0, 1), (1, 2)]
        );
        assert!(check_between_members_with_budget(&scene, 0.01, 0., 1).is_err());
    }
    #[test]
    fn seating_edge_contact_is_not_a_surface_crossing() {
        let face = [
            Vec3::new(-3., -3., 0.),
            Vec3::new(3., -3., 0.),
            Vec3::new(0., 3., 0.),
        ];
        let edge = [
            Vec3::new(-1., 0., 0.),
            Vec3::new(1., 0., 0.),
            Vec3::new(0., 0., 2.),
        ];
        assert!(cross_triangles(face, edge, 0.01).is_none());
        let penetrating = edge.map(|p| p - Vec3::Z * 0.5);
        assert!(cross_triangles(face, penetrating, 0.01).is_some());
    }
    #[test]
    fn plane_cut_segments_must_overlap_beyond_edge_tolerance() {
        let a = [
            Vec3::new(-1., -1., 0.),
            Vec3::new(1., -1., 0.),
            Vec3::new(0., 1., 0.),
        ];
        let b = [
            Vec3::new(0., 1.00001, -1.),
            Vec3::new(0., 1.00001, 1.),
            Vec3::new(0., 2., 0.),
        ];
        // Both triangles straddle the other's infinite plane, but their
        // intersection segments only approach an endpoint of the finite face.
        for offset in [Vec3::ZERO, Vec3::new(97., 84., 157.)] {
            let a = a.map(|p| p + offset);
            let b = b.map(|p| p + offset);
            assert!(cross_triangles(a, b, 0.001).is_none());
            assert!(cross_triangles(b, a, 0.001).is_none());
            let crossing = b.map(|p| p - Vec3::Y);
            assert!(cross_triangles(a, crossing, 0.001).is_some());
            assert!(cross_triangles(crossing, a, 0.001).is_some());
        }
    }
    #[test]
    fn crossing_triangles_are_found() {
        let a = [
            Vec3::new(-1., -1., 0.),
            Vec3::new(1., -1., 0.),
            Vec3::new(0., 1., 0.),
        ];
        let b = [
            Vec3::new(0., 0., -1.),
            Vec3::new(0., 0., 1.),
            Vec3::new(1., 0., 0.5),
        ];
        assert!(cross_triangles(a, b, 0.001).is_some());
    }
    #[test]
    fn coplanar_contact_is_not_reported_as_penetration() {
        let a = [Vec3::ZERO, Vec3::X, Vec3::Y];
        assert!(cross_triangles(a, a, 0.001).is_none());
    }
    #[test]
    fn bounds_overlap_is_not_enough() {
        let a = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let b = a.map(|p| p + Vec3::new(0.8, 0.8, 0.));
        assert!(cross_triangles(a, b, 0.001).is_none());
    }
    #[test]
    fn separated_parallel_surfaces_do_not_intersect() {
        let a = [Vec3::ZERO, Vec3::X, Vec3::Y];
        assert!(cross_triangles(a, a.map(|p| p + Vec3::Z), 0.001).is_none());
    }
}
