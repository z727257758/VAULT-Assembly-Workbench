//! Gridfinity insert alignment in the drawer's own coordinate system.
use crate::{
    constraints::{self, Joint, PartId, Relation},
    scene::Scene,
};
use glam::{Quat, Vec3};
use std::collections::BTreeMap;
fn plane(part: &crate::scene::Part, axis: usize, positive: bool, lo: f32, hi: f32) -> Option<f32> {
    let mut areas = BTreeMap::<i32, f32>::new();
    for t in part.mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(t[0].position);
        let b = Vec3::from(t[1].position);
        let c = Vec3::from(t[2].position);
        let n = (b - a).cross(c - a);
        let area = n.length();
        let level = (a[axis] + b[axis] + c[axis]) / 3.;
        if area > 1e-6
            && n[axis] * if positive { 1. } else { -1. } > area * 0.999
            && level > lo
            && level < hi
        {
            *areas.entry((level * 1000.).round() as i32).or_default() += area;
        }
    }
    areas
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(v, _)| v as f32 / 1000.)
}
pub fn apply(scene: &mut Scene, drawer: usize) -> Result<(), String> {
    let inserts: Vec<_> = scene
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.pose.path.contains("/Gridfinity Baseplate/"))
        .map(|(i, _)| i)
        .collect();
    if inserts.is_empty() {
        return Ok(());
    }
    let b = &scene.parts[drawer];
    let floor = plane(b, 1, true, b.mesh.min.y + 0.5, b.mesh.min.y + 6.)
        .ok_or("Drawer inner floor missing")?;
    let front = plane(b, 2, true, b.mesh.min.z + 0.5, b.mesh.min.z + 10.)
        .ok_or("Drawer inner front wall missing")?;
    let rear = plane(b, 2, false, b.mesh.max.z - 10., b.mesh.max.z - 0.5)
        .ok_or("Drawer inner rear wall missing")?;
    for i in inserts {
        // Both files use raw +Z up and raw +Y toward the front. Handles change
        // the mesh center, not the seating plane: align the actual lowest vertex.
        let r = Relation {
            parent: PartId::of(&scene.parts[drawer]),
            child: PartId::of(&scene.parts[i]),
            joint: Joint::Fixed {
                position: [
                    0.,
                    floor - scene.parts[i].mesh.min.y + 0.05,
                    (front + rear) * 0.5,
                ],
                rotation: Quat::IDENTITY.to_array(),
            },
            label: "Gridfinity insert / drawer inner floor and front orientation".into(),
        };
        scene.relations.retain(|old| old.child != r.child);
        constraints::add(scene, r)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn insert_variants_face_up_fit_and_follow() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        for (w, h, d) in [
            ("S", "1F", 3),
            ("S", "2F", 3),
            ("S", "1F", 4),
            ("M", "1F", 3),
            ("L", "1F", 3),
        ] {
            let sel = crate::catalog::Selection {
                width: w.into(),
                height: h.into(),
                depth: d,
                ..Default::default()
            };
            for entry in c
                .accessories(&sel)
                .into_iter()
                .filter(|e| e.path.contains("/Gridfinity Baseplate/"))
            {
                let mut bom = c.bom(&sel);
                crate::catalog::Catalog::set_accessory(&mut bom, entry.path.clone(), true, 1);
                let mut s = Scene::load(&c.root, &bom, "baseplate".into(), true).unwrap();
                let b = s
                    .parts
                    .iter()
                    .position(|p| p.pose.path.contains("/Boxes/"))
                    .unwrap();
                let i = s
                    .parts
                    .iter()
                    .position(|p| p.pose.path == entry.path)
                    .unwrap();
                let bm = s.parts[b].pose.matrix(0.);
                let im = s.parts[i].pose.matrix(0.);
                assert!(
                    im.transform_vector3(Vec3::Y)
                        .dot(bm.transform_vector3(Vec3::Y))
                        > 0.999
                );
                assert!(
                    im.transform_vector3(-Vec3::Z)
                        .dot(bm.transform_vector3(-Vec3::Z))
                        > 0.999
                );
                let relative = bm.inverse() * im;
                assert!(
                    (relative
                        .transform_point3(Vec3::new(0., s.parts[i].mesh.min.y, 0.))
                        .y
                        - (s.parts[b].mesh.min.y + 3.)
                        - 0.05)
                        .abs()
                        < 0.01
                );
                let pair = Scene {
                    parts: vec![s.parts[b].clone(), s.parts[i].clone()],
                    ..Default::default()
                };
                let hits = crate::collision::audit_pair(&pair, 0, 1);
                let inv = bm.inverse();
                let left = plane(
                    &s.parts[b],
                    0,
                    true,
                    s.parts[b].mesh.min.x + 0.5,
                    s.parts[b].mesh.min.x + 10.,
                )
                .unwrap();
                let right = plane(
                    &s.parts[b],
                    0,
                    false,
                    s.parts[b].mesh.max.x - 10.,
                    s.parts[b].mesh.max.x - 0.5,
                )
                .unwrap();
                let rear = plane(
                    &s.parts[b],
                    2,
                    false,
                    s.parts[b].mesh.max.z - 10.,
                    s.parts[b].mesh.max.z - 0.5,
                )
                .unwrap();
                for hit in hits {
                    let v = inv.transform_point3(hit);
                    let side = (v.x - left).abs().min((v.x - right).abs());
                    let flush_side = side < 0.01;
                    // The S standard source has a rear-corner profile mismatch;
                    // retain and bound that known diagnostic rather than scale it.
                    let rear_corner = w == "S"
                        && !entry.name.contains("Handle")
                        && side < 4.
                        && (v.z - rear).abs() < 4.;
                    assert!(
                        flush_side || rear_corner,
                        "{} / {h} unexpected interior crossing",
                        entry.name
                    );
                }
                let id = PartId::of(&s.parts[b]);
                if let Joint::Slider { offset, .. } = &mut s
                    .relations
                    .iter_mut()
                    .find(|r| r.child == id)
                    .unwrap()
                    .joint
                {
                    *offset = 25.;
                }
                constraints::solve(&mut s).unwrap();
                assert!(
                    (s.parts[b].pose.matrix(0.).inverse() * s.parts[i].pose.matrix(0.))
                        .abs_diff_eq(relative, 0.002)
                );
                if w == "S" && d == 3 && h == "1F" && entry.name.contains("1 Handle") {
                    // An accessory refresh must retain slider edits and visibility,
                    // while newly added parts keep their default assembly relation.
                    s.parts[b].visible = false;
                    let mut without =
                        Scene::load(&c.root, &c.bom(&sel), "without".into(), true).unwrap();
                    without.retain_assembly_state(&s);
                    assert!(without.constraint_warning.is_none());
                    assert!(!without.relations.iter().any(|r| r.child.path == entry.path));
                    let mut refreshed = Scene::load(&c.root, &bom, "refresh".into(), true).unwrap();
                    refreshed.retain_assembly_state(&without);
                    assert!(refreshed.constraint_warning.is_none());
                    assert!(!refreshed.parts[b].visible);
                    assert!(
                        refreshed.parts[b]
                            .pose
                            .matrix(0.)
                            .abs_diff_eq(s.parts[b].pose.matrix(0.), 0.002)
                    );
                    assert!(
                        (refreshed.parts[b].pose.matrix(0.).inverse()
                            * refreshed.parts[i].pose.matrix(0.))
                        .abs_diff_eq(relative, 0.002)
                    );
                }
                let def: crate::scene::Definition =
                    toml::from_str(&toml::to_string(&s.definition()).unwrap()).unwrap();
                let mut restored = s.clone();
                restored.apply(&def);
                assert!(
                    restored.parts[i]
                        .pose
                        .matrix(1.)
                        .abs_diff_eq(s.parts[i].pose.matrix(1.), 0.002)
                );
            }
        }
    }
}
