//! Triangle-surface crossing diagnostic, accelerated by median-split BVHs.
//! Coplanar contact and complete containment are intentionally not classified as
//! penetration. A clean report is NOT a manufacturing clearance certificate.
use crate::{constraints::PartId, scene::Scene};
use glam::Vec3;
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
    fn new(triangles: Vec<Triangle>) -> Self {
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
        self.crossings(other, eps, 1).into_iter().next()
    }
    fn crossings(&self, other: &Self, eps: f32, limit: usize) -> Vec<Vec3> {
        let mut hits = Vec::new();
        if self.nodes.is_empty() || other.nodes.is_empty() {
            return hits;
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
                            {
                                hits.push(p);
                                if hits.len() >= limit {
                                    return hits;
                                }
                            }
                        }
                    }
                }
            }
        }
        hits
    }
}
fn inside_triangle(p: Vec3, t: [Vec3; 3]) -> bool {
    let a = t[1] - t[0];
    let b = t[2] - t[0];
    let v = p - t[0];
    let aa = a.dot(a);
    let ab = a.dot(b);
    let bb = b.dot(b);
    let av = a.dot(v);
    let bv = b.dot(v);
    let denominator = aa * bb - ab * ab;
    if denominator.abs() < 1e-12 {
        return false;
    }
    let u = (bb * av - ab * bv) / denominator;
    let v = (aa * bv - ab * av) / denominator;
    u >= -1e-5 && v >= -1e-5 && u + v <= 1. + 1e-5
}
fn cross_edges(a: [Vec3; 3], b: [Vec3; 3], eps: f32) -> Option<Vec3> {
    let n = (b[1] - b[0]).cross(b[2] - b[0]).normalize_or_zero();
    if n.length() < 0.9 {
        return None;
    }
    for i in 0..3 {
        let p = a[i];
        let q = a[(i + 1) % 3];
        let d0 = (p - b[0]).dot(n);
        let d1 = (q - b[0]).dot(n);
        if (d0 > eps && d1 < -eps) || (d1 > eps && d0 < -eps) {
            let hit = p + (q - p) * (d0 / (d0 - d1));
            if inside_triangle(hit, b) {
                return Some(hit);
            }
        }
    }
    None
}
fn cross_triangles(a: [Vec3; 3], b: [Vec3; 3], eps: f32) -> Option<Vec3> {
    cross_edges(a, b, eps).or_else(|| cross_edges(b, a, eps))
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
        let matrix = part.pose.matrix(0.);
        let mut triangles = Vec::with_capacity(part.mesh.vertices.len() / 3);
        for tri in part.mesh.vertices.chunks_exact(3) {
            let p = [
                matrix.transform_point3(tri[0].position.into()),
                matrix.transform_point3(tri[1].position.into()),
                matrix.transform_point3(tri[2].position.into()),
            ];
            let mut bounds = Bounds::empty();
            for v in p {
                bounds.add(v);
            }
            triangles.push(Triangle {
                p,
                bounds,
                center: (p[0] + p[1] + p[2]) / 3.,
            });
        }
        trees.push(Tree::new(triangles));
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
#[cfg(test)]
pub fn audit_pair(scene: &Scene, a: usize, b: usize) -> Vec<Vec3> {
    let build = |index: usize| {
        let part = &scene.parts[index];
        let m = part.pose.matrix(0.);
        Tree::new(
            part.mesh
                .vertices
                .chunks_exact(3)
                .map(|tri| {
                    let p = [
                        m.transform_point3(tri[0].position.into()),
                        m.transform_point3(tri[1].position.into()),
                        m.transform_point3(tri[2].position.into()),
                    ];
                    let mut bounds = Bounds::empty();
                    for v in p {
                        bounds.add(v);
                    }
                    Triangle {
                        p,
                        bounds,
                        center: (p[0] + p[1] + p[2]) / 3.,
                    }
                })
                .collect(),
        )
    };
    build(a).crossings(&build(b), 0.01, 100_000)
}

#[cfg(test)]
mod tests {
    use super::*;
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
