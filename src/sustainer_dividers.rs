//! Sustainer inserts use the measured wall slots, never the drawer's hole grid.
use crate::{
    catalog::{Bom, Catalog, Selection, SustainerBody, SustainerOptions},
    constraints::{self, PartId, Relation},
    display,
    scene::Scene,
};
use glam::{Mat3, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const GROUP: &str = "Sustainer divider layout";
pub const PREFIX: &str = "02. SUSTAINERS/05. Universal/Dividers/";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Axis {
    #[default]
    Width,
    Depth,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Placement {
    pub axis: Axis,
    pub slot: usize,
    #[serde(default)]
    pub lane: usize,
}
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub specification: String,
    pub items: Vec<Placement>,
}
pub fn spec(s: &Selection, o: SustainerOptions) -> String {
    SustainerOptions {
        split_roof: false,
        ..o
    }
    .key(s)
}
pub fn supported(s: &Selection, o: SustainerOptions) -> bool {
    matches!(s.width.as_str(), "S" | "M") || o.body != SustainerBody::Flat
}
impl Layout {
    pub fn validate(&self, c: &Catalog, s: &Selection, o: SustainerOptions) -> Result<(), String> {
        let o = o.normalized(s);
        if self.items.is_empty() {
            return Ok(());
        }
        if self.specification != spec(s, o) || !supported(s, o) {
            return Err("隔板布局与当前箱体规格不匹配".into());
        }
        let mut occupied = BTreeSet::new();
        for p in &self.items {
            let lanes = if o.body == SustainerBody::Sealed {
                if s.width == "XL" { 3 } else { 2 }
            } else {
                1
            };
            if p.slot >= 8
                || p.lane >= lanes
                || (p.axis == Axis::Depth
                    && (o.body != SustainerBody::Dividers
                        || !matches!(s.width.as_str(), "L" | "XL")
                        || p.slot >= 2
                        || p.lane != 0))
            {
                return Err("隔板槽位超出当前箱体范围".into());
            }
            if !occupied.insert(p) {
                return Err("同一槽位已有隔板".into());
            }
            c.sustainer_divider(s, o, p.axis, p.lane)?;
        }
        Ok(())
    }
    pub fn sync_bom(
        &self,
        bom: &mut Vec<Bom>,
        c: &Catalog,
        s: &Selection,
        o: SustainerOptions,
    ) -> Result<(), String> {
        self.validate(c, s, o)?;
        let mut counts = BTreeMap::<String, u32>::new();
        for p in &self.items {
            *counts
                .entry(c.sustainer_divider(s, o, p.axis, p.lane)?)
                .or_default() += 1;
        }
        bom.retain(|b| b.group != GROUP && !b.path.starts_with(PREFIX));
        bom.extend(counts.into_iter().map(|(path, qty)| Bom {
            path,
            qty,
            group: GROUP.into(),
            enabled: true,
        }));
        Ok(())
    }
    pub fn selected_part(
        &self,
        index: usize,
        c: &Catalog,
        s: &Selection,
        o: SustainerOptions,
    ) -> Option<(String, u32)> {
        let path = c
            .sustainer_divider(s, o, self.items.get(index)?.axis, self.items[index].lane)
            .ok()?;
        let instance = self.items[..index]
            .iter()
            .filter(|p| c.sustainer_divider(s, o, p.axis, p.lane).as_ref() == Ok(&path))
            .count() as u32;
        Some((path, instance))
    }
}
#[derive(Clone, Default)]
pub struct Geometry {
    pub specification: String,
    pub bounds: [f32; 4], // left, rear, right, front in assembly coordinates
    pub floor: f32,
    pub rows: Vec<f32>,
    pub columns: Vec<f32>,
    pub lanes: Vec<[f32; 2]>,
}
fn crossings(lines: &[[Vec3; 2]], axis: usize, level: f32, other: usize) -> Vec<f32> {
    let mut hits = Vec::new();
    for [a, b] in lines {
        if (a[axis] - level) * (b[axis] - level) < 0. {
            hits.push(a[other] + (b[other] - a[other]) * (level - a[axis]) / (b[axis] - a[axis]));
        }
    }
    hits.sort_by(f32::total_cmp);
    hits.dedup_by(|a, b| (*a - *b).abs() < 0.03);
    hits
}
fn grooves(hits: &[f32], min: usize, width: [f32; 2]) -> Vec<f32> {
    let mut out = Vec::new();
    let mut start = 0;
    for end in 1..=hits.len() {
        if end == hits.len() || hits[end] - hits[end - 1] > 3. {
            if end - start >= min && (width[0]..=width[1]).contains(&(hits[end - 1] - hits[start]))
            {
                out.push((hits[start] + hits[end - 1]) / 2.);
            }
            start = end;
        }
    }
    out
}
/// Called on the preview worker after the structural parts have been assembled.
pub fn measure(scene: &Scene, s: &Selection, o: SustainerOptions) -> Result<Geometry, String> {
    let module = scene
        .parts
        .iter()
        .find(|p| p.pose.path.contains("/Modules/"))
        .ok_or("箱体缺失")?;
    let (lo, hi) = constraints::world_bounds(module);
    let mut planes = BTreeMap::<i32, f32>::new();
    let matrix = module.pose.matrix(0.);
    for tri in module.mesh.vertices.chunks_exact(3) {
        let v: Vec<_> = tri
            .iter()
            .map(|v| matrix.transform_point3(Vec3::from(v.position)))
            .collect();
        let n = (v[1] - v[0]).cross(v[2] - v[0]);
        if n.y.abs() > n.length() * 0.9999 {
            *planes
                .entry(((v[0].y + v[1].y + v[2].y) / 3. * 10.).round() as i32)
                .or_default() += n.length();
        }
    }
    let mut planes: Vec<_> = planes.into_iter().collect();
    planes.sort_by(|a, b| b.1.total_cmp(&a.1));
    let floor = planes
        .iter()
        .take(2)
        .map(|p| p.0 as f32 / 10.)
        .reduce(f32::max)
        .ok_or("未找到箱底")?;
    let lines = display::section_segments(&module.mesh, matrix, 1, floor + 20.123);
    let rows = grooves(&crossings(&lines, 0, hi.x - 4., 2), 4, [1.8, 2.2]);
    let columns = if o.body == SustainerBody::Dividers {
        grooves(&crossings(&lines, 2, lo.z + 4., 0), 2, [2.1, 2.5])
    } else {
        Vec::new()
    };
    let rear = lo.z + 5.;
    // The facade is a separate wall, assembled five mm beyond the module mouth.
    let front = hi.z + 2.;
    let lanes = if o.body == SustainerBody::Sealed {
        let hits = crossings(&lines, 2, rows.first().copied().unwrap_or(0.) + 5., 0);
        // Internal free spans between wall pairs, avoiding small facade/latch features.
        hits.windows(2)
            .enumerate()
            .filter(|(i, w)| i % 2 == 1 && w[1] - w[0] > 40.)
            .map(|(_, w)| [w[0], w[1]])
            .collect()
    } else {
        vec![[lo.x + 5., hi.x - 5.]]
    };
    if supported(s, o) && (rows.is_empty() || lanes.is_empty()) {
        return Err("未识别到完整隔板槽位；请检查当前箱体源模型".into());
    }
    Ok(Geometry {
        specification: spec(s, o),
        bounds: [lo.x + 5., rear, hi.x - 5., front],
        floor,
        rows,
        columns,
        lanes,
    })
}
impl Geometry {
    pub fn choices(&self, axis: Axis, lane: usize) -> Vec<Placement> {
        (0..if axis == Axis::Width {
            self.rows.len()
        } else {
            self.columns.len()
        })
            .map(|slot| Placement {
                axis,
                slot,
                lane: if axis == Axis::Depth { 0 } else { lane },
            })
            .collect()
    }
    pub fn line(&self, p: &Placement) -> Option<[[f32; 2]; 2]> {
        Some(match p.axis {
            Axis::Width => {
                let z = *self.rows.get(p.slot)?;
                let lane = self.lanes.get(p.lane)?;
                [[lane[0], z], [lane[1], z]]
            }
            Axis::Depth => {
                let x = *self.columns.get(p.slot)?;
                [[x, self.bounds[1]], [x, self.bounds[3]]]
            }
        })
    }
}
pub fn apply(
    scene: &mut Scene,
    layout: &Layout,
    c: &Catalog,
    s: &Selection,
    o: SustainerOptions,
) -> Result<(), String> {
    layout.validate(c, s, o)?;
    let g = measure(scene, s, o)?;
    let module = scene
        .parts
        .iter()
        .position(|p| p.pose.path.contains("/Modules/"))
        .ok_or("箱体缺失")?;
    let mut instances = BTreeMap::<String, u32>::new();
    for p in &layout.items {
        let line = g.line(p).ok_or("隔板槽位在当前源模型中不存在")?;
        let path = c.sustainer_divider(s, o, p.axis, p.lane)?;
        let copy = instances.entry(path.clone()).or_default();
        let i = scene
            .parts
            .iter()
            .position(|part| part.pose.path == path && part.pose.instance == *copy)
            .ok_or("隔板模型未载入")?;
        *copy += 1;
        // Source width panels are printed flat in XY; depth panels have height on X.
        let mut q = if p.axis == Axis::Width {
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
        } else {
            Quat::from_mat3(&Mat3::from_cols(Vec3::Y, Vec3::X, -Vec3::Z))
        };
        let mut depth_center = None;
        if p.axis == Axis::Depth {
            // Some L 2F/4F sources reverse the printed height axis. Recover the
            // upper half-lap openings and align their centres to the wall slots.
            let mut matched = None;
            for candidate in [q, Quat::from_rotation_z(std::f32::consts::PI) * q] {
                let top = scene.parts[i]
                    .mesh
                    .vertices
                    .iter()
                    .map(|v| (candidate * Vec3::from(v.position)).y)
                    .fold(f32::NEG_INFINITY, f32::max);
                let lines = display::section_segments(
                    &scene.parts[i].mesh,
                    glam::Mat4::from_quat(candidate),
                    1,
                    top - 5.123,
                );
                let slots = grooves(&crossings(&lines, 0, 0.123, 2), 2, [2.0, 2.2]);
                if slots.len() != g.rows.len() {
                    continue;
                }
                let offset =
                    g.rows.iter().zip(&slots).map(|(a, b)| a - b).sum::<f32>() / slots.len() as f32;
                if slots
                    .iter()
                    .zip(&g.rows)
                    .all(|(a, b)| (a + offset - b).abs() < 0.03)
                {
                    matched = Some((candidate, offset));
                    break;
                }
            }
            let (rotation, offset) = matched.ok_or("纵向隔板交叉槽与箱体横槽不匹配")?;
            q = rotation;
            depth_center = Some(offset);
        }
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for v in &scene.parts[i].mesh.vertices {
            let v = q * Vec3::from(v.position);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let target = Vec3::new(
            (line[0][0] + line[1][0]) / 2.,
            g.floor + 0.15,
            depth_center.unwrap_or((line[0][1] + line[1][1]) / 2.),
        );
        let pos = target - Vec3::new((lo.x + hi.x) / 2., lo.y, (lo.z + hi.z) / 2.);
        let (x, y, z) = q.to_euler(glam::EulerRot::XYZ);
        scene.parts[i].pose.position = pos.to_array();
        scene.parts[i].pose.rotation = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
        scene.parts[i].color = [0.25, 0.68, 0.68, 1.];
        let mut relation = Relation::fixed(scene, module, i, false)?;
        relation.label = "Divider layout: Sustainer wall slots (mesh estimate)".into();
        scene
            .relations
            .retain(|r| r.child != PartId::of(&scene.parts[i]));
        constraints::add(scene, relation)?;
    }
    scene.sustainer_interior = Some(Box::new(g));
    scene.calibrated = false;
    constraints::solve(scene)?;
    crate::sustainer::reset_explosion(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_rejects_duplicate_slots_and_preserves_roof_independence() {
        let s = Selection::default();
        let o = SustainerOptions::default();
        assert_eq!(
            spec(&s, o),
            spec(
                &s,
                SustainerOptions {
                    split_roof: true,
                    ..o
                }
            )
        );
        let path = format!("{PREFIX}S Dividers/S1F Divider.stl");
        let c = Catalog {
            root: Default::default(),
            entries: vec![crate::catalog::Entry {
                path: path.clone(),
                name: "S1F Divider.stl".into(),
                category: 2,
            }],
        };
        let p = Placement {
            axis: Axis::Width,
            slot: 1,
            lane: 0,
        };
        let mut l = Layout {
            specification: spec(&s, o),
            items: vec![p.clone()],
        };
        l.validate(&c, &s, o).unwrap();
        let mut bom = vec![];
        l.sync_bom(&mut bom, &c, &s, o).unwrap();
        assert_eq!(bom.len(), 1);
        assert_eq!(bom[0].qty, 1);
        assert_eq!(bom[0].path, path);
        l.items.push(p);
        assert!(l.validate(&c, &s, o).is_err());
        l.items[1].slot = 2;
        l.sync_bom(&mut bom, &c, &s, o).unwrap();
        assert_eq!(bom.len(), 1);
        assert_eq!(bom[0].qty, 2);
        let restored: Layout = toml::from_str(&toml::to_string(&l).unwrap()).unwrap();
        assert_eq!(l, restored);
        l.items.clear();
        l.sync_bom(&mut bom, &c, &s, o).unwrap();
        assert!(bom.is_empty());
    }
    #[test]
    fn sustainer_divider_library_slots_and_assembly() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let mut report = String::new();
        for (s, o) in crate::catalog::sustainer_configurations()
            .into_iter()
            .filter(|(_, o)| !o.split_roof)
        {
            if !supported(&s, o) {
                continue;
            }
            let mut bom = c.sustainer_bom(&s, o).unwrap();
            let base = Scene::load(&c.root, &bom, "slot test".into(), true).unwrap();
            let g = measure(&base, &s, o).unwrap_or_else(|e| panic!("{}: {e}", spec(&s, o)));
            let lanes = if o.body == SustainerBody::Sealed {
                if s.width == "XL" { 3 } else { 2 }
            } else {
                1
            };
            assert_eq!(g.lanes.len(), lanes, "{}", spec(&s, o));
            assert!(
                g.rows.len() >= 2 && g.rows.len() <= 7,
                "{}: {:?}",
                spec(&s, o),
                g.rows
            );
            if o.body == SustainerBody::Dividers {
                assert_eq!(g.columns.len(), 2, "{}", spec(&s, o));
            }
            let mut layout = Layout {
                specification: spec(&s, o),
                ..Default::default()
            };
            for lane in 0..lanes {
                layout.items.extend(g.choices(Axis::Width, lane));
            }
            layout.items.extend(g.choices(Axis::Depth, 0));
            layout.sync_bom(&mut bom, &c, &s, o).unwrap();
            // Loading real divider meshes also exercises source orientation, graph identities and BOM counts.
            let mut scene = Scene::load(&c.root, &bom, "slot test".into(), true).unwrap();
            apply(&mut scene, &layout, &c, &s, o).unwrap();
            assert_eq!(
                scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.starts_with(PREFIX))
                    .count(),
                layout.items.len()
            );
            for p in scene
                .parts
                .iter()
                .filter(|p| p.pose.path.starts_with(PREFIX))
            {
                let (lo, hi) = constraints::world_bounds(p);
                assert!(
                    (lo.y - g.floor - 0.15).abs() < 0.03,
                    "{} {:?}",
                    p.pose.path,
                    lo
                );
                assert!(hi.y - lo.y > 35.);
            }
            let stationary: Vec<_> = scene
                .parts
                .iter()
                .filter(|p| p.pose.path.starts_with(PREFIX))
                .map(|p| p.pose.position)
                .collect();
            crate::sustainer::set_lid_angle(&mut scene, 85.).unwrap();
            assert_eq!(
                stationary,
                scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.starts_with(PREFIX))
                    .map(|p| p.pose.position)
                    .collect::<Vec<_>>()
            );
            report.push_str(&format!(
                "{}: rows={:?}, columns={:?}, lanes={:?}, floor={}\n",
                spec(&s, o),
                g.rows,
                g.columns,
                g.lanes,
                g.floor
            ));
            std::fs::create_dir_all("output/sustainer-dividers").unwrap();
            std::fs::write("output/sustainer-dividers/slots.txt", &report).unwrap();
        }
    }

    #[test]
    fn sustainer_divider_library_surface_review() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let mut report = String::new();
        for (width, depth, height, body) in [
            ("S", 3, "1F", SustainerBody::Flat),
            ("M", 4, "2F", SustainerBody::Flat),
            ("L", 3, "1F", SustainerBody::Dividers),
            ("L", 4, "4F", SustainerBody::Dividers),
            ("L", 3, "2F", SustainerBody::Sealed),
            ("XL", 3, "1F", SustainerBody::Dividers),
            ("XL", 4, "1F", SustainerBody::Sealed),
        ] {
            let s = Selection {
                width: width.into(),
                depth,
                height: height.into(),
                hidden_key: false,
            };
            let o = SustainerOptions {
                body,
                split_roof: false,
            };
            let mut bom = c.sustainer_bom(&s, o).unwrap();
            let mut scene = Scene::load(&c.root, &bom, "surface review".into(), true).unwrap();
            let g = measure(&scene, &s, o).unwrap();
            let mut layout = Layout {
                specification: spec(&s, o),
                ..Default::default()
            };
            for lane in 0..g.lanes.len() {
                layout.items.extend(g.choices(Axis::Width, lane));
            }
            layout.items.extend(g.choices(Axis::Depth, 0));
            layout.sync_bom(&mut bom, &c, &s, o).unwrap();
            scene = Scene::load(&c.root, &bom, "surface review".into(), true).unwrap();
            apply(&mut scene, &layout, &c, &s, o).unwrap();
            std::fs::create_dir_all("output/sustainer-dividers").unwrap();
            std::fs::write(
                format!("output/sustainer-dividers/{}.toml", spec(&s, o)),
                toml::to_string(&scene.definition()).unwrap(),
            )
            .unwrap();
            scene.parts.retain(|p| {
                p.pose.path.starts_with(PREFIX)
                    || p.pose.path.contains("/Modules/")
                    || p.pose.path.ends_with("Facade.stl")
            });
            let result = crate::collision::check(&scene, 0.01).unwrap();
            report.push_str(&format!(
                "{}: {} surface crossing pairs\n",
                spec(&s, o),
                result.pairs.len()
            ));
            assert!(
                result
                    .pairs
                    .iter()
                    .all(|p| !scene.parts[p.a].pose.path.starts_with(PREFIX)
                        || !scene.parts[p.b].pose.path.starts_with(PREFIX)),
                "cross-lap alignment: {}",
                spec(&s, o)
            );
            for pair in result.pairs {
                report.push_str(&format!(
                    "  {} #{} / {} #{} @ {:?}\n",
                    scene.parts[pair.a].pose.path,
                    scene.parts[pair.a].pose.instance,
                    scene.parts[pair.b].pose.path,
                    scene.parts[pair.b].pose.instance,
                    pair.point
                ));
            }
        }
        std::fs::write("output/sustainer-dividers/surfaces.txt", report).unwrap();
    }
}
