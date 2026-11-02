//! Compatibility from 01_Instructions Manual, page 8. Surface placement is an
//! estimate from source meshes; compatibility is not a verified CAD assembly.
use crate::{
    catalog::Selection,
    scene::{Placement, Scene},
};
use glam::Vec3;
use std::collections::BTreeMap;

pub struct Module<'a> {
    pub name: &'a str,
    pub selection: &'a Selection,
    pub placement: Placement,
    pub is_box: bool,
}
pub struct Layout {
    pub left: Vec<f32>,
    pub supports: Vec<Vec<usize>>,
    pub order: Vec<usize>,
}
pub fn width(name: &str) -> Result<i32, String> {
    match name {
        "S" => Ok(1648),
        "M" => Ok(2540),
        "L" => Ok(3296),
        "XL" => Ok(4188),
        _ => Err("未知模块宽度 / Unknown module width".into()),
    }
}
fn allowed(a: &[&str], b: &[&str]) -> bool {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    b.sort();
    a == b
        || matches!(
            (a.as_slice(), b.as_slice()),
            (["L"], ["S", "S"]) | (["S", "S"], ["L"]) | (["XL"], ["M", "S"]) | (["M", "S"], ["XL"])
        )
}
/// Column widths follow the narrowest member anchored in that column. Wide
/// modules may span adjacent columns. Every upper footprint must be supported.
pub fn plan(modules: &[Module<'_>], gap: f32) -> Result<Layout, String> {
    if modules.is_empty() {
        return Ok(Layout {
            left: vec![],
            supports: vec![],
            order: vec![],
        });
    }
    if modules.len() > 64 || !gap.is_finite() || gap.abs() > 0.001 {
        return Err("规则装配不使用自由间距；请将旧方案间距归零 / Rule-based assembly requires zero spacing".into());
    }
    let mut columns = BTreeMap::<u32, i32>::new();
    let mut widths = Vec::new();
    for m in modules {
        m.placement.validate()?;
        if m.placement.offset.iter().any(|v| v.abs() > 0.001) {
            return Err(format!(
                "{}：规则装配不允许自由偏移 / Free offsets are not supported",
                m.name
            ));
        }
        let w = width(&m.selection.width)?;
        widths.push(w);
        columns
            .entry(m.placement.column)
            .and_modify(|v| *v = (*v).min(w))
            .or_insert(w);
    }
    let mut starts = BTreeMap::new();
    let mut x = 0;
    for c in 0..=*columns.keys().max().unwrap() {
        starts.insert(c, x);
        x += columns.get(&c).copied().unwrap_or(1648);
    }
    let left: Vec<_> = modules
        .iter()
        .map(|m| starts[&m.placement.column])
        .collect();
    let mut order: Vec<_> = (0..modules.len()).collect();
    order.sort_by_key(|&i| (modules[i].placement.level, left[i]));
    let mut supports = vec![vec![]; modules.len()];
    for &i in &order {
        let m = &modules[i];
        let end = left[i] + widths[i];
        for &j in &order {
            if i != j
                && modules[j].placement.level == m.placement.level
                && left[i] < left[j] + widths[j]
                && left[j] < end
            {
                return Err(format!(
                    "{} 与 {} 在占用相同空间，请选择其他连接位置 / Occupied attachment space",
                    m.name, modules[j].name
                ));
            }
        }
        if m.placement.level == 0 {
            continue;
        }
        let mut below: Vec<_> = order
            .iter()
            .copied()
            .filter(|&j| {
                modules[j].placement.level + 1 == m.placement.level
                    && left[j] < end
                    && left[j] + widths[j] > left[i]
            })
            .collect();
        below.sort_by_key(|&j| left[j]);
        if below.is_empty() {
            return Err(format!(
                "{} 下方没有支承，请先添加支承模块 / No support below",
                m.name
            ));
        }
        for &j in &below {
            if modules[j].selection.depth != m.selection.depth {
                return Err(format!(
                    "{} 与 {} 深度不同，不能堆叠 / Stacked depths must match",
                    m.name, modules[j].name
                ));
            }
        }
        // Validate the entire connected group, including the other half of a pair.
        let lo = left[i].min(left[below[0]]);
        let hi = end.max(left[*below.last().unwrap()] + widths[*below.last().unwrap()]);
        let upper: Vec<_> = order
            .iter()
            .copied()
            .filter(|&j| {
                modules[j].placement.level == m.placement.level
                    && left[j] < hi
                    && left[j] + widths[j] > lo
            })
            .collect();
        let lower: Vec<_> = order
            .iter()
            .copied()
            .filter(|&j| {
                modules[j].placement.level + 1 == m.placement.level
                    && left[j] < hi
                    && left[j] + widths[j] > lo
            })
            .collect();
        let tiled = |ids: &[usize]| {
            let mut at = lo;
            for &j in ids {
                if left[j] != at || modules[j].selection.depth != m.selection.depth {
                    return false;
                }
                at += widths[j];
            }
            at == hi
        };
        let a: Vec<_> = upper
            .iter()
            .map(|&j| modules[j].selection.width.as_str())
            .collect();
        let b: Vec<_> = lower
            .iter()
            .map(|&j| modules[j].selection.width.as_str())
            .collect();
        if !tiled(&upper) || !tiled(&lower) || !allowed(&a, &b) {
            return Err(format!(
                "{}：仅支持同宽堆叠、L ↔ S＋S、XL ↔ S＋M，且深度一致 / Unsupported stacking group",
                m.name
            ));
        }
        supports[i] = std::mem::take(&mut below);
    }
    // Early UI check; worker also checks actual mesh seating planes.
    let mut tops = vec![0f32; modules.len()];
    for &i in &order {
        let base = supports[i].first().map_or(0., |&j| tops[j]);
        if supports[i].iter().any(|&j| (tops[j] - base).abs() > 0.2) {
            return Err(format!(
                "{} 的下层支承高度不齐 / Support heights differ",
                modules[i].name
            ));
        }
        let height = match modules[i].selection.height.as_str() {
            "1F" => {
                if modules[i].is_box {
                    61.
                } else {
                    47.
                }
            }
            "2F" => 95.,
            "4F" => 191.,
            _ => return Err("未知高度 / Unknown height".into()),
        };
        tops[i] = base + height;
    }
    Ok(Layout {
        left: left.into_iter().map(|v| v as f32 / 10.).collect(),
        supports,
        order,
    })
}

pub struct Datum {
    pub center_x: f32,
    pub rear: f32,
    pub bottom: f32,
    pub top: f32,
}
fn planes(part: &crate::scene::Part) -> Vec<(f32, f32)> {
    let mut levels = BTreeMap::<i32, f32>::new();
    let matrix = part.pose.matrix(0.);
    for tri in part.mesh.vertices.chunks_exact(3) {
        let a = matrix.transform_point3(Vec3::from(tri[0].position));
        let b = matrix.transform_point3(Vec3::from(tri[1].position));
        let c = matrix.transform_point3(Vec3::from(tri[2].position));
        let n = (b - a).cross(c - a);
        let area = n.length() * 0.5;
        if area > 1e-6 && n.y.abs() > n.length() * 0.9999 {
            *levels
                .entry(((a.y + b.y + c.y) / 3. * 10.).round() as i32)
                .or_default() += area;
        }
    }
    let mut values: Vec<_> = levels
        .into_iter()
        .map(|(h, a)| (h as f32 / 10., a))
        .collect();
    values.sort_by(|a, b| b.1.total_cmp(&a.1));
    values
}
/// Use broad outer shell faces, excluding projecting latches, labels and open lids.
/// This recovers seating planes, but does not prove connector engagement.
pub fn datum(scene: &Scene) -> Result<Datum, String> {
    let module = scene
        .parts
        .iter()
        .find(|p| p.pose.path.contains("/Modules/"))
        .ok_or("组合缺少箱体 / Module missing")?;
    let (lo, hi) = super::world_bounds(module);
    let name = module.pose.path.rsplit('/').next().unwrap_or("");
    let family: String = name
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    if (hi.x - lo.x - width(&family)? as f32 / 10.).abs() > 0.3 {
        return Err(format!(
            "{name}：箱体朝向尚未完成安装面校准，不能用于规则组合 / Housing seating orientation is unavailable"
        ));
    }
    let mid = (lo.y + hi.y) * 0.5;
    let levels = planes(module);
    let bottom = levels
        .iter()
        .filter(|(y, _)| *y < mid)
        .take(2)
        .map(|(y, _)| *y)
        .reduce(f32::min)
        .ok_or("底面基准缺失 / Bottom datum missing")?;
    let roofs: Vec<_> = scene
        .parts
        .iter()
        .filter(|p| {
            let n = p.pose.path.rsplit('/').next().unwrap_or("");
            n.contains(" Roof") && !n.contains("Connector")
        })
        .collect();
    let top = if roofs.is_empty() {
        levels
            .iter()
            .filter(|(y, _)| *y > mid)
            .take(2)
            .map(|(y, _)| *y)
            .reduce(f32::max)
    } else {
        roofs
            .iter()
            .filter_map(|p| planes(p).first().map(|v| v.0))
            .reduce(f32::max)
    }
    .ok_or("顶面基准缺失 / Top datum missing")?;
    if top <= bottom {
        return Err("无效安装面 / Invalid seating planes".into());
    }
    Ok(Datum {
        center_x: (lo.x + hi.x) * 0.5,
        rear: lo.z,
        bottom,
        top,
    })
}
pub fn shifts(
    layout: &Layout,
    modules: &[Module<'_>],
    scenes: &[Scene],
) -> Result<Vec<Vec3>, String> {
    let datums: Vec<_> = scenes.iter().map(datum).collect::<Result<_, _>>()?;
    let mut shifts = vec![Vec3::ZERO; scenes.len()];
    let mut tops = vec![0f32; scenes.len()];
    for &i in &layout.order {
        let base = layout.supports[i].first().map_or(0., |&j| tops[j]);
        if layout.supports[i]
            .iter()
            .any(|&j| (tops[j] - base).abs() > 0.25)
        {
            return Err(format!(
                "{}：源件安装面不等高，请调整支承规格 / Mesh support planes differ",
                modules[i].name
            ));
        }
        let d = &datums[i];
        shifts[i] = Vec3::new(
            layout.left[i] + width(&modules[i].selection.width)? as f32 / 20. - d.center_x,
            base - d.bottom,
            -d.rear,
        );
        tops[i] = base + d.top - d.bottom;
    }
    Ok(shifts)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn check(specs: &[(&str, u8, &str, bool, u32, u32)]) -> Result<Layout, String> {
        let selections: Vec<_> = specs
            .iter()
            .map(|s| Selection {
                width: s.0.into(),
                depth: s.1,
                height: s.2.into(),
                hidden_key: false,
            })
            .collect();
        let modules: Vec<_> = specs
            .iter()
            .zip(&selections)
            .map(|(s, selection)| Module {
                name: s.0,
                selection,
                is_box: s.3,
                placement: Placement {
                    column: s.4,
                    level: s.5,
                    offset: [0.; 3],
                },
            })
            .collect();
        plan(&modules, 0.)
    }
    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn library_seating_datums_and_orientation() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let root = std::path::PathBuf::from(root);
        let catalog = crate::catalog::Catalog::scan(root.clone()).unwrap();
        let mut failures = Vec::new();
        let mut report = String::new();
        for is_box in [false, true] {
            for w in ["S", "M", "L", "XL"] {
                for depth in [3, 7] {
                    for h in ["1F", "2F", "4F"] {
                        if w == "XL" && h != "1F" {
                            continue;
                        }
                        let selection = Selection {
                            width: w.into(),
                            depth,
                            height: h.into(),
                            hidden_key: false,
                        };
                        let items = if is_box {
                            catalog
                                .sustainer_bom(&selection, Default::default())
                                .unwrap()
                        } else {
                            catalog.bom(&selection)
                        };
                        let scene = match Scene::load(&root, &items, String::new(), true) {
                            Ok(scene) => scene,
                            Err(e) => {
                                report.push_str(&format!(
                                    "SOURCE ERROR box={is_box} {w}{depth} {h}: {e}\n"
                                ));
                                continue;
                            }
                        };
                        let datum = match datum(&scene) {
                            Ok(datum) => datum,
                            Err(e) => {
                                assert!(!is_box && w == "XL", "{e}");
                                report.push_str(&format!(
                                    "UNSUPPORTED box={is_box} {w}{depth} {h}: {e}\n"
                                ));
                                continue;
                            }
                        };
                        let module = scene
                            .parts
                            .iter()
                            .find(|p| p.pose.path.contains("/Modules/"))
                            .unwrap();
                        let (lo, hi) = super::super::world_bounds(module);
                        let expected = match h {
                            "1F" => {
                                if is_box {
                                    61.
                                } else {
                                    47.
                                }
                            }
                            "2F" => 95.,
                            _ => 191.,
                        };
                        let line = format!(
                            "box={is_box} {w}{depth} {h}: width={:.3}, bottom={:.3}, top={:.3}, pitch={:.3}\n",
                            hi.x - lo.x,
                            datum.bottom,
                            datum.top,
                            datum.top - datum.bottom
                        );
                        if (hi.x - lo.x - width(w).unwrap() as f32 / 10.).abs() > 0.3
                            || (datum.top - datum.bottom - expected).abs() > 0.3
                        {
                            failures.push(line.clone());
                        }
                        report.push_str(&line);
                    }
                }
            }
        }
        std::fs::create_dir_all("output/combination-rules").unwrap();
        std::fs::write("output/combination-rules/seating-datums.txt", report).unwrap();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
    #[test]
    fn documented_pairs_and_mixed_types() {
        for specs in [
            vec![
                ("L", 3, "1F", false, 0, 0),
                ("S", 3, "2F", true, 0, 1),
                ("S", 3, "2F", false, 1, 1),
            ],
            vec![
                ("S", 4, "2F", false, 0, 0),
                ("M", 4, "2F", true, 1, 0),
                ("XL", 4, "1F", true, 0, 1),
            ],
            vec![("S", 3, "1F", true, 0, 0), ("S", 3, "4F", false, 0, 1)],
        ] {
            assert!(check(&specs).is_ok());
        }
    }
    #[test]
    fn rejects_depth_partial_overlap_floating_and_uneven_support() {
        for specs in [
            vec![("S", 3, "1F", false, 0, 0), ("S", 4, "1F", false, 0, 1)],
            vec![("L", 3, "1F", false, 0, 0), ("S", 3, "1F", false, 0, 1)],
            vec![("M", 3, "1F", false, 0, 0), ("S", 3, "1F", false, 0, 1)],
            vec![("S", 3, "1F", false, 0, 2)],
            vec![
                ("S", 3, "1F", false, 0, 0),
                ("S", 3, "2F", false, 1, 0),
                ("L", 3, "1F", false, 0, 1),
            ],
        ] {
            assert!(check(&specs).is_err());
        }
    }
}
