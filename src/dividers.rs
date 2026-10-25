//! Hole-grid layouts for the supplied, unmodified drawer divider meshes.
use crate::{
    catalog::{Bom, Catalog, Selection},
    constraints::{self, PartId, Relation},
    scene::Scene,
};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub fn interior_part(path: &str) -> bool {
    path.contains("/Boxes/") || path.contains("/Grid Dividers/")
}
pub const PITCH: f32 = 10.5;
pub const GROUP: &str = "Divider layout";
pub const PIN: &str = "01. DRAWERS/07. Universal/Grid Dividers/grid divider pin_loose.stl";
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub specification: String,
    #[serde(default)]
    pub items: Vec<Placement>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub path: String,
    /// First occupied hole cell along the divider, not a millimeter length.
    pub start: [i32; 2],
    pub vertical: bool,
}
#[derive(Clone, Copy)]
pub struct Grid {
    pub cols: i32,
    pub rows: i32,
    pub width_code: i32,
    pub left: f32,
    pub right: f32,
    pub rear: f32,
    pub front: f32,
}
pub fn spec(s: &Selection) -> String {
    format!("{}{}-{}", s.width, s.depth, s.height)
}
pub fn code(path: &str) -> Option<i32> {
    path.rsplit('/')
        .next()?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}
pub fn height(path: &str) -> f32 {
    if path.contains("82.5MM") {
        82.5
    } else if path.contains("34.5MM") {
        34.5
    } else {
        20.
    }
}
impl Grid {
    pub fn new(s: &Selection) -> Result<Self, String> {
        let (cols, width_code, half) = match s.width.as_str() {
            "S" => (12, 10, 62.899),
            "M" => (20, 18, 107.5),
            "L" => (26, 26, 145.3),
            _ => return Err("当前分隔编辑支持 S / M / L 非分体抽屉；XL 安装网格尚未验证".into()),
        };
        if !(3..=7).contains(&s.depth) {
            return Err("不支持的抽屉深度".into());
        }
        let extra = (s.depth - 3) as f32 * 21.;
        Ok(Self {
            cols,
            rows: s.depth as i32 * 4,
            width_code,
            left: -half,
            right: half,
            rear: -68.9 - extra,
            front: 65.3 + extra,
        })
    }
    pub fn point(self, p: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - (self.cols - 1) as f32 / 2.) * PITCH,
            (p[1] - (self.rows - 1) as f32 / 2.) * PITCH - 3.5,
        ]
    }
    pub fn center(self, p: &Placement) -> [f32; 2] {
        let mut q = p.start.map(|n| n as f32);
        q[usize::from(p.vertical)] += (code(&p.path).unwrap_or(0) - 1) as f32 / 2.;
        self.point(q)
    }
    pub fn full_code(self, vertical: bool) -> i32 {
        if vertical {
            self.rows - 2
        } else {
            self.width_code
        }
    }
    pub fn full_start(self, vertical: bool) -> i32 {
        if vertical {
            1
        } else {
            (self.cols - self.width_code) / 2
        }
    }
    pub fn full(self, p: &Placement) -> bool {
        code(&p.path) == Some(self.full_code(p.vertical))
            && p.start[usize::from(p.vertical)] == self.full_start(p.vertical)
    }
    pub fn bounds(self, p: &Placement) -> [f32; 4] {
        let c = self.center(p);
        let half = code(&p.path).unwrap_or(0) as f32 * PITCH / 2.;
        let (x, y) = if p.vertical {
            (12.676, half)
        } else {
            (half, 12.676)
        };
        let mut b = [c[0] - x, c[1] - y, c[0] + x, c[1] + y];
        if self.full(p) {
            if p.vertical {
                b[1] = self.rear + 0.3;
                b[3] = self.front - 0.3;
            } else {
                b[0] = self.left + 0.3;
                b[2] = self.right - 0.3;
            }
        }
        b
    }
}
pub fn edge_path(path: &str) -> String {
    let folder = path.rsplit_once('/').unwrap().0;
    let name = if path.contains("82.5MM") {
        "Straight Edge Piece 2F.stl"
    } else if path.contains("34.5MM") {
        "Straight Edge Piece 1F.stl"
    } else {
        "Straight Edge Piece 20MM.stl"
    };
    format!("{folder}/{name}")
}
impl Layout {
    pub fn validate(&self, c: &Catalog, s: &Selection) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let g = Grid::new(s)?;
        if self.specification != spec(s) {
            return Err("布局与当前抽屉规格不一致".into());
        }
        if self.items.len() > 50 {
            return Err("布局最多支持 50 块隔板".into());
        }
        let allowed = c.accessories(s);
        for (i, p) in self.items.iter().enumerate() {
            let n = code(&p.path).ok_or("请选择带长度编号的隔板")?;
            if !p.path.contains("/Grid Dividers/")
                || !(5..=32).contains(&n)
                || !allowed.iter().any(|e| e.path == p.path)
            {
                return Err("隔板型号不兼容当前抽屉".into());
            }
            let axis = usize::from(p.vertical);
            let limits = [g.cols, g.rows];
            if p.start.iter().any(|n| *n < 0)
                || p.start[axis] + n > limits[axis]
                || p.start[1 - axis] >= limits[1 - axis]
            {
                return Err("隔板超出底孔网格".into());
            }
            let b = g.bounds(p);
            if b[0] < g.left + 0.1
                || b[2] > g.right - 0.1
                || b[1] < g.rear + 0.1
                || b[3] > g.front - 0.1
            {
                return Err("隔板底座或端件接触抽屉壁，请向内移动一格".into());
            }
            if c.resolve(PIN).is_none() || g.full(p) && c.resolve(&edge_path(&p.path)).is_none() {
                return Err("模型库缺少配套端件或连接销".into());
            }
            for other in &self.items[..i] {
                let a = g.bounds(other);
                if b[0] < a[2] + 0.1 && b[2] > a[0] - 0.1 && b[1] < a[3] + 0.1 && b[3] > a[1] - 0.1
                {
                    return Err("隔板底座重叠或交叉；当前版本不允许未验证的十字/T 形接合".into());
                }
            }
        }
        Ok(())
    }
    pub fn bom(&self, g: Grid) -> Vec<Bom> {
        if !self.enabled {
            return vec![];
        }
        let mut counts = BTreeMap::<String, u32>::new();
        for p in &self.items {
            *counts.entry(p.path.clone()).or_default() += 1;
            *counts.entry(PIN.into()).or_default() += 2;
            if g.full(p) {
                *counts.entry(edge_path(&p.path)).or_default() += 2;
            }
        }
        counts
            .into_iter()
            .map(|(path, qty)| Bom {
                path,
                qty,
                group: GROUP.into(),
                enabled: true,
            })
            .collect()
    }
    pub fn remove(&mut self, bom: &mut Vec<Bom>, s: &Selection) {
        *self = Self {
            specification: spec(s),
            ..Default::default()
        };
        bom.retain(|b| !b.path.contains("/Grid Dividers/"));
    }
    /// Both the editor and the legacy accessory list share this exclusion rule.
    pub fn select_accessory(
        &mut self,
        bom: &mut Vec<Bom>,
        s: &Selection,
        path: String,
        enabled: bool,
        qty: u32,
    ) {
        if enabled && path.contains("/Gridfinity Baseplate/") {
            self.remove(bom, s);
        }
        if enabled && path.contains("/Grid Dividers/") {
            for b in bom
                .iter_mut()
                .filter(|b| b.path.contains("/Gridfinity Baseplate/"))
            {
                b.enabled = false;
            }
        }
        Catalog::set_accessory(bom, path, enabled, qty);
    }
    pub fn sync_bom(&self, bom: &mut Vec<Bom>, s: &Selection) {
        bom.retain(|b| b.group != GROUP);
        if !self.enabled {
            // Normalize older presets which could select both insert types.
            if bom
                .iter()
                .any(|b| b.enabled && b.path.contains("/Grid Dividers/"))
            {
                for b in bom
                    .iter_mut()
                    .filter(|b| b.path.contains("/Gridfinity Baseplate/"))
                {
                    b.enabled = false;
                }
            }
            return;
        }
        bom.retain(|b| !b.path.contains("/Grid Dividers/"));
        // An empty divider configuration still reserves the drawer interior.
        for b in bom
            .iter_mut()
            .filter(|b| b.path.contains("/Gridfinity Baseplate/"))
        {
            b.enabled = false;
        }
        if let Ok(g) = Grid::new(s) {
            bom.extend(self.bom(g));
        }
    }
}
fn place(
    scene: &mut Scene,
    parent: usize,
    child: usize,
    box_matrix: Mat4,
    position: Vec3,
    rotation: Quat,
) -> Result<(), String> {
    let m = box_matrix * Mat4::from_rotation_translation(rotation, position);
    let (_, q, p) = m.to_scale_rotation_translation();
    let (x, y, z) = q.to_euler(glam::EulerRot::XYZ);
    scene.parts[child].pose.position = p.to_array();
    scene.parts[child].pose.rotation = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
    let mut relation = Relation::fixed(scene, parent, child, false)?;
    relation.label = "Divider layout: hole-grid placement".into();
    scene
        .relations
        .retain(|r| r.child != PartId::of(&scene.parts[child]));
    scene.relations.push(relation);
    Ok(())
}
pub fn apply(scene: &mut Scene, layout: &Layout, s: &Selection) -> Result<(), String> {
    if !layout.enabled || layout.items.is_empty() {
        return Ok(());
    }
    let g = Grid::new(s)?;
    let drawer = scene
        .parts
        .iter()
        .position(|p| p.pose.path.contains("/Boxes/"))
        .ok_or("分隔布局需要抽屉盒体")?;
    let b = &scene.parts[drawer];
    let matrix = b.pose.matrix(0.);
    let floor = b.mesh.min.y + 3.02;
    if (b.source_size.x - (g.right - g.left + 8.4)).abs() > 0.1 {
        return Err("抽屉尺寸与已验证底孔网格不一致".into());
    }
    let mut next = BTreeMap::<String, u32>::new();
    let mut take = |scene: &Scene, path: &str| -> Result<usize, String> {
        let count = next.entry(path.into()).or_default();
        let instance = *count;
        *count += 1;
        scene
            .parts
            .iter()
            .position(|p| p.pose.path == path && p.pose.instance == instance)
            .ok_or_else(|| format!("布局组件缺失: {path} #{instance}"))
    };
    for p in &layout.items {
        let i = take(scene, &p.path)?;
        let c = g.center(p);
        let length = code(&p.path).unwrap() as f32 * PITCH;
        if (scene.parts[i].source_size.x - length).abs() > 0.1
            || (scene.parts[i].source_size.z - height(&p.path)).abs() > 0.1
        {
            return Err("隔板打印方向或尺寸不符合安装配方".into());
        }
        let angle = if p.vertical {
            std::f32::consts::FRAC_PI_2
        } else {
            0.
        };
        let q = Quat::from_rotation_y(angle);
        place(
            scene,
            drawer,
            i,
            matrix,
            Vec3::new(c[0], floor - scene.parts[i].mesh.min.y, -c[1]),
            q,
        )?;
        for sign in [-1., 1.] {
            let pin = take(scene, PIN)?;
            let along = sign * (length / 2. - 15.75);
            let delta = q * Vec3::new(along, 0., 0.);
            // Pin source +Y is its long end; rotate upright. The collar seats
            // on the drawer floor; the short end enters the 3 mm bottom hole.
            let pin_q = q * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
            let pin_pos =
                Vec3::new(c[0], floor + 3.925, -c[1]) + delta + q * Vec3::new(0., 0., 0.55);
            place(scene, i, pin, matrix, pin_pos, pin_q)?;
            if g.full(p) {
                let edge = take(scene, &edge_path(&p.path))?;
                // End pieces slide into the divider; back plane faces the wall.
                let wall = if p.vertical {
                    if sign < 0. { g.rear } else { g.front }
                } else if sign < 0. {
                    g.left
                } else {
                    g.right
                };
                let end = wall - sign * (10.1 + 0.3);
                let mut pos = Vec3::new(c[0], floor - scene.parts[edge].mesh.min.y, -c[1]);
                if p.vertical {
                    pos.z = -end;
                } else {
                    pos.x = end;
                }
                let edge_q = q * Quat::from_rotation_y(sign * std::f32::consts::FRAC_PI_2);
                place(scene, i, edge, matrix, pos, edge_q)?;
            }
        }
    }
    constraints::solve(scene)?;
    crate::explosion::reset(scene)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> Option<Catalog> {
        std::env::var("VAULT_MODEL_ROOT")
            .ok()
            .map(|s| Catalog::scan(s.into()).unwrap())
    }
    fn plate(c: &Catalog, s: &Selection, n: i32, h: &str) -> String {
        c.accessories(s)
            .into_iter()
            .find(|e| e.path.contains(h) && code(&e.path) == Some(n))
            .unwrap()
            .path
    }
    #[test]
    fn removal_and_baseplate_exclusion_work_in_both_directions() {
        let Some(c) = root() else {
            return;
        };
        let s = Selection::default();
        let divider = plate(&c, &s, 10, "20MM");
        let base = c
            .accessories(&s)
            .into_iter()
            .find(|e| e.path.contains("/Gridfinity Baseplate/"))
            .unwrap()
            .path;
        let mut bom = c.bom(&s);
        let mut l = Layout {
            enabled: true,
            specification: spec(&s),
            items: vec![Placement {
                path: divider.clone(),
                start: [1, 5],
                vertical: false,
            }],
        };
        l.sync_bom(&mut bom, &s);
        assert!(bom.iter().any(|b| b.group == GROUP));
        l.select_accessory(&mut bom, &s, base.clone(), true, 1);
        assert!(!l.enabled && l.items.is_empty());
        assert!(!bom.iter().any(|b| b.path.contains("/Grid Dividers/")));
        assert!(bom.iter().any(|b| b.path == base && b.enabled));
        l.sync_bom(&mut bom, &s);
        assert!(bom.iter().any(|b| b.path == base && b.enabled));
        Catalog::set_accessory(&mut bom, divider.clone(), true, 1);
        l.sync_bom(&mut bom, &s);
        assert!(
            !bom.iter().any(|b| b.path == base && b.enabled),
            "legacy presets must also be normalized"
        );
        l.select_accessory(&mut bom, &s, divider.clone(), true, 1);
        assert!(!bom.iter().any(|b| b.path == base && b.enabled));
        l.remove(&mut bom, &s);
        assert!(!bom.iter().any(|b| b.path.contains("/Grid Dividers/")));
        l.select_accessory(&mut bom, &s, base.clone(), true, 1);
        l.enabled = true;
        l.sync_bom(&mut bom, &s);
        assert!(
            !bom.iter().any(|b| b.path == base && b.enabled),
            "selecting an empty divider configuration must also remove the baseplate"
        );
        let restored: Layout = toml::from_str(&toml::to_string(&l).unwrap()).unwrap();
        assert!(restored.enabled && restored.items.is_empty());
        l.items.push(Placement {
            path: divider,
            start: [1, 5],
            vertical: false,
        });
        l.sync_bom(&mut bom, &s);
        assert!(!bom.iter().any(|b| b.path == base && b.enabled));
        l.remove(&mut bom, &s);
        l.sync_bom(&mut bom, &s);
        assert!(!bom.iter().any(|b| b.group == GROUP));
        l.select_accessory(&mut bom, &s, base.clone(), true, 1);
        l.select_accessory(&mut bom, &s, base.clone(), false, 1);
        assert!(!bom.iter().any(|b| b.path == base && b.enabled));
    }
    #[test]
    fn rejects_crossings_bounds_and_incompatible_presets() {
        let Some(c) = root() else {
            return;
        };
        let s = Selection::default();
        let g = Grid::new(&s).unwrap();
        let p = Placement {
            path: plate(&c, &s, 10, "20MM"),
            start: [1, 5],
            vertical: false,
        };
        let mut l = Layout {
            enabled: true,
            specification: spec(&s),
            items: vec![p.clone()],
        };
        assert!(l.validate(&c, &s).is_ok());
        l.items.push(p.clone());
        assert!(l.validate(&c, &s).is_err());
        l.items.pop();
        l.items.push(Placement {
            vertical: true,
            start: [5, 1],
            ..p.clone()
        });
        assert!(l.validate(&c, &s).is_err());
        l.items.pop();
        l.items[0].start = [-1, 5];
        assert!(l.validate(&c, &s).is_err());
        l.items[0] = p;
        l.specification = "M3-1F".into();
        assert!(l.validate(&c, &s).is_err());
        l.specification = spec(&s);
        let saved: Layout = toml::from_str(&toml::to_string(&l).unwrap()).unwrap();
        assert_eq!(l, saved);
        let mut bom = c.bom(&s);
        l.sync_bom(&mut bom, &s);
        l.sync_bom(&mut bom, &s);
        assert_eq!(
            bom.iter()
                .filter(|b| b.group == GROUP)
                .map(|b| b.qty)
                .sum::<u32>(),
            5
        );
        assert_eq!(l.bom(g).iter().find(|b| b.path == PIN).unwrap().qty, 2);
        l.items.clear();
        l.sync_bom(&mut bom, &s);
        assert!(!bom.iter().any(|b| b.group == GROUP));
    }
    #[test]
    fn real_divider_layout_fits_follows_and_survives_refresh() {
        let Some(c) = root() else {
            return;
        };
        for (width, depth, height_name, tall, vertical) in [
            ("S", 3, "1F", "20MM", false),
            ("S", 4, "1F", "34.5MM", true),
            ("M", 3, "1F", "20MM", false),
            ("L", 3, "2F", "82.5MM", false),
        ] {
            let s = Selection {
                width: width.into(),
                depth,
                height: height_name.into(),
                ..Default::default()
            };
            let g = Grid::new(&s).unwrap();
            let path = plate(&c, &s, g.full_code(vertical), tall);
            let p = Placement {
                path: path.clone(),
                start: if vertical {
                    [5, g.full_start(true)]
                } else {
                    [g.full_start(false), 5]
                },
                vertical,
            };
            let layout = Layout {
                enabled: true,
                specification: spec(&s),
                items: vec![p.clone()],
            };
            layout.validate(&c, &s).unwrap();
            let mut bom = c.bom(&s);
            layout.sync_bom(&mut bom, &s);
            let mut scene = Scene::load(&c.root, &bom, "layout".into(), true).unwrap();
            apply(&mut scene, &layout, &s).unwrap();
            let b = scene
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Boxes/"))
                .unwrap();
            let i = scene
                .parts
                .iter()
                .position(|p| p.pose.path == path)
                .unwrap();
            let inv = scene.parts[b].pose.matrix(0.).inverse();
            let relative = inv * scene.parts[i].pose.matrix(0.);
            let center = g.center(&p);
            assert!(
                (relative.w_axis.x - center[0]).abs() < 0.002
                    && (relative.w_axis.z + center[1]).abs() < 0.002
            );
            let parts = std::iter::once(scene.parts[b].clone())
                .chain(
                    scene
                        .parts
                        .iter()
                        .filter(|p| p.pose.path.contains("/Grid Dividers/"))
                        .cloned(),
                )
                .collect();
            let pair = Scene {
                parts,
                ..Default::default()
            };
            let report = crate::collision::check(&pair, 0.01).unwrap();
            assert!(
                !report
                    .pairs
                    .iter()
                    .any(|p| p.a == 0 && !pair.parts[p.b].pose.path.contains("pin_")),
                "divider bodies and end pieces must clear the drawer"
            );
            if width == "S" && depth == 3 {
                for hit in &report.pairs {
                    let contacts = crate::collision::audit_pair(&pair, hit.a, hit.b);
                    let inv = pair.parts[hit.a].pose.matrix(0.).inverse();
                    for p in contacts {
                        let p = inv.transform_point3(p);
                        if hit.a == 0 {
                            // Source bottom-hole membrane at 0.2 mm above the
                            // underside is retained; contacts must stay at holes.
                            assert!((p.y + 18.55).abs() < 0.01);
                            assert!((p.x.abs() - 36.75).abs() < 1.5 && (p.z - 8.75).abs() < 1.5);
                        } else if pair.parts[hit.b].pose.path.contains("pin_") {
                            assert!((p.x.abs() - 36.75).abs() < 3.1 && p.z.abs() < 3.1);
                        } else {
                            assert!(p.x.abs() > 46. && p.x.abs() < 52.6 && p.z.abs() < 1.5);
                        }
                    }
                }
            }
            let id = PartId::of(&scene.parts[b]);
            if let constraints::Joint::Slider { offset, .. } = &mut scene
                .relations
                .iter_mut()
                .find(|r| r.child == id)
                .unwrap()
                .joint
            {
                *offset = 25.;
            }
            constraints::solve(&mut scene).unwrap();
            assert!(
                (scene.parts[b].pose.matrix(0.).inverse() * scene.parts[i].pose.matrix(0.))
                    .abs_diff_eq(relative, 0.002)
            );
            scene.calibrated = true;
            let mut moved = layout.clone();
            moved.items[0].start[usize::from(!vertical)] += 3;
            moved.validate(&c, &s).unwrap();
            let mut refreshed = Scene::load(&c.root, &bom, "moved".into(), true).unwrap();
            apply(&mut refreshed, &moved, &s).unwrap();
            refreshed.retain_assembly_state(&scene);
            assert!(
                !refreshed.calibrated,
                "a changed layout invalidates the previous manual verification"
            );
            let expected = g.center(&moved.items[0]);
            let m =
                refreshed.parts[b].pose.matrix(0.).inverse() * refreshed.parts[i].pose.matrix(0.);
            assert!(
                (m.w_axis.x - expected[0]).abs() < 0.002
                    && (m.w_axis.z + expected[1]).abs() < 0.002
            );
            let def = refreshed.definition();
            let mut saved = Scene::load(&c.root, &bom, "saved".into(), true).unwrap();
            apply(&mut saved, &moved, &s).unwrap();
            saved.apply(&def);
            assert!(
                saved.parts[i]
                    .pose
                    .matrix(1.)
                    .abs_diff_eq(refreshed.parts[i].pose.matrix(1.), 0.003)
            );
        }
    }
}
