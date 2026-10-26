//! P1S print-bed packing and Bambu project settings. No assembly transforms.
use glam::Vec3;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub margin: f32,
    pub brim: f32,
    pub gap: f32,
    pub rotate: bool,
    pub split: bool,
    pub center: bool,
    pub layer: f32,
    pub walls: u32,
    pub infill: u32,
    pub textured: bool,
    pub support_by_name: bool,
    pub filament: u32,
    pub palette: [[u8; 3]; 6],
    pub part_filaments: std::collections::BTreeMap<String, u32>,
    pub preview_colors: bool,
    pub text_filaments: std::collections::BTreeMap<String, u32>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            margin: 0.5,
            brim: 0.,
            gap: 6.,
            rotate: true,
            split: true,
            center: false,
            layer: 0.2,
            walls: 2,
            infill: 15,
            textured: true,
            support_by_name: true,
            filament: 1,
            palette: [
                [22, 22, 22],
                [249, 140, 54],
                [255, 255, 255],
                [188, 188, 188],
                [22, 22, 22],
                [246, 246, 243],
            ],
            part_filaments: Default::default(),
            preview_colors: false,
            text_filaments: Default::default(),
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !self.margin.is_finite()
            || !(0.0..=30.).contains(&self.margin)
            || !self.brim.is_finite()
            || !(0.0..=10.).contains(&self.brim)
            || !self.gap.is_finite()
            || !(0.0..=30.).contains(&self.gap)
            || !self.layer.is_finite()
            || !(0.08..=0.28).contains(&self.layer)
            || !(1..=10).contains(&self.walls)
            || self.infill > 100
            || !(1..=6).contains(&self.filament)
            || self
                .part_filaments
                .values()
                .chain(self.text_filaments.values())
                .any(|slot| !(1..=6).contains(slot))
        {
            return Err("无效的 P1S 打印或摆盘参数".into());
        }
        Ok(())
    }
    pub fn slot(&self, path: &str) -> u32 {
        self.part_filaments
            .get(path)
            .copied()
            .unwrap_or(self.filament)
            .clamp(1, 6)
    }
    pub fn text_slot(&self, path: &str) -> u32 {
        self.text_filaments
            .get(path)
            .copied()
            .unwrap_or_else(|| self.slot(path))
            .clamp(1, 6)
    }
    pub fn slot_color(&self, slot: u32) -> [f32; 4] {
        let [r, g, b] = self.palette[(slot.clamp(1, 6) - 1) as usize];
        [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
    }
    pub fn color_hex(&self, slot: u32) -> String {
        let [r, g, b] = self.palette[(slot.clamp(1, 6) - 1) as usize];
        format!("#{r:02X}{g:02X}{b:02X}")
    }
    pub fn color(&self, path: &str) -> [f32; 4] {
        let [r, g, b] = self.palette[(self.slot(path) - 1) as usize];
        [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
    }
    pub fn project_json(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut d: serde_json::Value =
            serde_json::from_str(include_str!("../config/export/p1s-0.4.json"))
                .map_err(|e| e.to_string())?;
        for (key, value) in [
            ("layer_height", self.layer.to_string()),
            ("wall_loops", self.walls.to_string()),
            ("sparse_infill_density", format!("{}%", self.infill)),
            (
                "curr_bed_type",
                if self.textured {
                    "Textured PEI Plate"
                } else {
                    "High Temp Plate"
                }
                .into(),
            ),
            ("print_sequence", "by layer".into()),
            ("enable_prime_tower", "0".into()),
            (
                "brim_type",
                if self.brim > 0. {
                    "outer_only"
                } else {
                    "no_brim"
                }
                .into(),
            ),
            ("brim_width", self.brim.to_string()),
            ("skirt_loops", "0".into()),
            ("enable_support", "0".into()),
            ("print_settings_id", "VAULT P1S".into()),
        ] {
            d[key] = serde_json::Value::String(value);
        }
        let colors: Vec<_> = (1..=6).map(|slot| self.color_hex(slot)).collect();
        d["filament_colour"] = serde_json::json!(colors);
        d["filament_multi_colour"] = serde_json::json!(colors);
        d["wipe_tower_x"] = serde_json::json!(["0"]);
        d["wipe_tower_y"] = serde_json::json!(["0"]);
        serde_json::to_vec_pretty(&d).map_err(|e| e.to_string())
    }
}
#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}
impl Rect {
    fn right(self) -> f32 {
        self.x + self.w
    }
    fn top(self) -> f32 {
        self.y + self.h
    }
    fn intersects(self, b: Self) -> bool {
        self.x < b.right() - 0.0001
            && b.x < self.right() - 0.0001
            && self.y < b.top() - 0.0001
            && b.y < self.top() - 0.0001
    }
    fn contains(self, b: Self) -> bool {
        self.x <= b.x && self.y <= b.y && self.right() >= b.right() && self.top() >= b.top()
    }
}
fn subtract(free: &mut Vec<Rect>, used: Rect) {
    let mut next = Vec::new();
    for &r in free.iter() {
        if !r.intersects(used) {
            next.push(r);
            continue;
        }
        if used.x > r.x {
            next.push(Rect {
                w: used.x - r.x,
                ..r
            });
        }
        if used.right() < r.right() {
            next.push(Rect {
                x: used.right(),
                w: r.right() - used.right(),
                ..r
            });
        }
        if used.y > r.y {
            next.push(Rect {
                h: used.y - r.y,
                ..r
            });
        }
        if used.top() < r.top() {
            next.push(Rect {
                y: used.top(),
                h: r.top() - used.top(),
                ..r
            });
        }
    }
    *free = next
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            !next
                .iter()
                .enumerate()
                .any(|(j, b)| i != &j && b.contains(**r) && (!r.contains(*b) || j < *i))
        })
        .map(|(_, r)| *r)
        .collect();
}
fn empty_plate(c: &Config) -> Vec<Rect> {
    let mut free = vec![Rect {
        x: c.margin - c.gap / 2.,
        y: c.margin - c.gap / 2.,
        w: 256. - 2. * c.margin + c.gap,
        h: 256. - 2. * c.margin + c.gap,
    }];
    // P1S front-left keep-out; add the requested clearance around it.
    subtract(
        &mut free,
        Rect {
            x: -c.gap,
            y: -c.gap,
            w: 18. + c.margin + c.gap * 1.5,
            h: 28. + c.margin + c.gap * 1.5,
        },
    );
    free
}
#[derive(Clone, Debug)]
pub struct Placement {
    pub input: usize,
    pub copy: u32,
    pub plate: usize,
    pub x: f32,
    pub y: f32,
    pub rotated: bool,
    pub width: f32,
    pub depth: f32,
}
pub fn arrange(parts: &[(String, Vec3, u32)], c: &Config) -> Result<Vec<Placement>, String> {
    c.validate()?;
    let mut jobs = Vec::new();
    for (i, (name, size, qty)) in parts.iter().enumerate() {
        if !size.is_finite()
            || size.x <= 0.
            || size.y <= 0.
            || size.z < 0.
            || size.z > 250.001
            || *qty == 0
            || *qty > 100
        {
            return Err(format!(
                "{name}：尺寸或份数无效，P1S 参考配置最大打印高度为 250 mm"
            ));
        }
        for copy in 0..*qty {
            jobs.push((i, copy));
        }
    }
    if jobs.len() > 1000 {
        return Err("单次导出最多 1000 份零件".into());
    }
    jobs.sort_by(|a, b| {
        (parts[b.0].1.x * parts[b.0].1.y).total_cmp(&(parts[a.0].1.x * parts[a.0].1.y))
    });
    let mut plates = vec![empty_plate(c)];
    let mut placed = Vec::new();
    for (input, copy) in jobs {
        let raw_size = parts[input].1;
        let size = Vec3::new(
            raw_size.x + 2. * c.brim,
            raw_size.y + 2. * c.brim,
            raw_size.z,
        );
        let mut candidate = None;
        loop {
            for (plate, free) in plates.iter().enumerate() {
                for r in free {
                    for rotated in [false, true] {
                        if rotated && !c.rotate {
                            continue;
                        }
                        let (w, h) = if rotated {
                            (size.y + c.gap, size.x + c.gap)
                        } else {
                            (size.x + c.gap, size.y + c.gap)
                        };
                        if w > r.w + 0.0001 || h > r.h + 0.0001 {
                            continue;
                        }
                        let score = (r.w - w).min(r.h - h);
                        if candidate.as_ref().is_none_or(|(old, _, _, _)| score < *old) {
                            candidate = Some((
                                score,
                                plate,
                                Rect {
                                    x: r.x,
                                    y: r.y,
                                    w,
                                    h,
                                },
                                rotated,
                            ));
                        }
                    }
                }
                if candidate.is_some() {
                    break;
                }
            }
            if candidate.is_some() {
                break;
            }
            if plates.last().unwrap().len() == empty_plate(c).len()
                && placed
                    .iter()
                    .all(|p: &Placement| p.plate != plates.len() - 1)
            {
                return Err(format!(
                    "{}：{:.1} × {:.1} mm，无法放入 P1S 可用区域；请使用分体版本或调整边距",
                    parts[input].0, size.x, size.y
                ));
            }
            if !c.split {
                return Err("当前零件无法放入单个打印盘，请开启自动分盘".into());
            }
            if plates.len() >= 36 {
                return Err("导出超过 36 个打印盘，请减少零件份数".into());
            }
            plates.push(empty_plate(c));
        }
        let (_, plate, r, rotated) = candidate.unwrap();
        subtract(&mut plates[plate], r);
        placed.push(Placement {
            input,
            copy,
            plate,
            x: r.x + r.w / 2.,
            y: r.y + r.h / 2.,
            rotated,
            width: r.w - c.gap,
            depth: r.h - c.gap,
        });
    }
    if c.center {
        for plate in 0..plates.len() {
            let group: Vec<_> = placed.iter().filter(|p| p.plate == plate).collect();
            if group.is_empty() {
                continue;
            }
            let min_x = group
                .iter()
                .map(|p| p.x - p.width / 2.)
                .fold(f32::INFINITY, f32::min);
            let max_x = group
                .iter()
                .map(|p| p.x + p.width / 2.)
                .fold(f32::NEG_INFINITY, f32::max);
            let min_y = group
                .iter()
                .map(|p| p.y - p.depth / 2.)
                .fold(f32::INFINITY, f32::min);
            let max_y = group
                .iter()
                .map(|p| p.y + p.depth / 2.)
                .fold(f32::NEG_INFINITY, f32::max);
            let dx = 128. - (min_x + max_x) / 2.;
            let dy = 128. - (min_y + max_y) / 2.;
            let keepout = Rect {
                x: 0.,
                y: 0.,
                w: 18. + c.margin + c.gap,
                h: 28. + c.margin + c.gap,
            };
            let safe = group.iter().all(|p| {
                let r = Rect {
                    x: p.x + dx - p.width / 2.,
                    y: p.y + dy - p.depth / 2.,
                    w: p.width,
                    h: p.depth,
                };
                r.x >= c.margin - 0.0001
                    && r.y >= c.margin - 0.0001
                    && r.right() <= 256. - c.margin + 0.0001
                    && r.top() <= 256. - c.margin + 0.0001
                    && !r.intersects(keepout)
            });
            // Rigid translation preserves spacing and brim; keep valid packing if blocked.
            if safe {
                for p in placed.iter_mut().filter(|p| p.plate == plate) {
                    p.x += dx;
                    p.y += dy;
                }
            }
        }
    }
    // Stable source/copy order, independent of packing order.
    placed.sort_by_key(|p| (p.input, p.copy));
    Ok(placed)
}
/// Each filament/color slot owns its own plates, even when two RGB values match.
/// Keeping slots separate preserves their potentially different material settings.
#[cfg(test)]
pub fn arrange_colored(
    parts: &[(String, Vec3, u32)],
    paths: &[String],
    c: &Config,
) -> Result<Vec<Placement>, String> {
    let materials: Vec<_> = paths.iter().map(|p| vec![c.slot(p)]).collect();
    arrange_materials(parts, &materials, c)
}
/// Physical print groups never share plates, even with identical material sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlateGroup {
    Standard,
    RemovableLabel,
}
/// A multi-material object is indivisible; group plates by the complete material set.
#[cfg(test)]
pub fn arrange_materials(
    parts: &[(String, Vec3, u32)],
    materials: &[Vec<u32>],
    c: &Config,
) -> Result<Vec<Placement>, String> {
    arrange_grouped_materials(
        parts,
        materials,
        &vec![PlateGroup::Standard; parts.len()],
        c,
    )
}
/// Keep detachable labels on dedicated plates, then split each group by material set.
pub fn arrange_grouped_materials(
    parts: &[(String, Vec3, u32)],
    materials: &[Vec<u32>],
    plate_groups: &[PlateGroup],
    c: &Config,
) -> Result<Vec<Placement>, String> {
    c.validate()?;
    if parts.len() != materials.len() || parts.len() != plate_groups.len() {
        return Err("颜色配置与零件数量不一致".into());
    }
    if parts.iter().map(|p| u64::from(p.2)).sum::<u64>() > 1000 {
        return Err("单次导出最多 1000 份零件".into());
    }
    let mut all = Vec::new();
    let mut offset = 0;
    let mut groups = std::collections::BTreeMap::<(PlateGroup, Vec<u32>), Vec<usize>>::new();
    for (i, slots) in materials.iter().enumerate() {
        let mut key = slots.clone();
        key.sort_unstable();
        key.dedup();
        if key.is_empty() || key.iter().any(|s| !(1..=6).contains(s)) {
            return Err("无效耗材组合".into());
        }
        groups.entry((plate_groups[i], key)).or_default().push(i);
    }
    for ((_plate_group, slots), ids) in groups {
        let group: Vec<_> = ids.iter().map(|&i| parts[i].clone()).collect();
        let mut layout = arrange(&group, c).map_err(|e| format!("耗材组合 {slots:?}：{e}"))?;
        let count = layout.iter().map(|p| p.plate + 1).max().unwrap_or(0);
        for p in &mut layout {
            p.input = ids[p.input];
            p.plate += offset;
        }
        all.extend(layout);
        offset += count;
        if offset > 36 {
            return Err("分色后超过 36 个打印盘，请减少份数".into());
        }
    }
    all.sort_by_key(|p| (p.input, p.copy));
    Ok(all)
}
pub fn plate_origin(plate: usize, count: usize) -> [f32; 2] {
    let columns = (count as f32).sqrt().ceil() as usize;
    [
        (plate % columns) as f32 * 307.2,
        -((plate / columns) as f32) * 307.2,
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_have_dedicated_plates_even_with_identical_materials() {
        use PlateGroup::{RemovableLabel, Standard};
        let c = Config {
            center: true,
            ..Default::default()
        };
        let parts = vec![
            ("bin".into(), Vec3::new(42., 42., 60.), 1),
            ("label A".into(), Vec3::new(36., 11., 1.4), 2),
            ("label B".into(), Vec3::new(36., 11., 1.4), 1),
            ("other tall part".into(), Vec3::new(42., 42., 80.), 1),
        ];
        for slots in [vec![1], vec![1, 2]] {
            let materials = vec![slots; 4];
            let layout = arrange_grouped_materials(
                &parts,
                &materials,
                &[Standard, RemovableLabel, RemovableLabel, Standard],
                &c,
            )
            .unwrap();
            assert_eq!(layout.len(), 5);
            let label_plate = layout.iter().find(|p| p.input == 1).unwrap().plate;
            let standard_plate = layout.iter().find(|p| p.input == 0).unwrap().plate;
            assert_ne!(label_plate, standard_plate);
            for p in &layout {
                assert_eq!(
                    p.plate,
                    if p.input == 1 || p.input == 2 {
                        label_plate
                    } else {
                        standard_plate
                    }
                );
            }
        }
        assert!(arrange_grouped_materials(&parts, &vec![vec![1]; 4], &[Standard], &c).is_err());
        let parts = vec![
            ("bin".into(), Vec3::new(230., 200., 60.), 1),
            ("label overflow".into(), Vec3::new(230., 200., 1.2), 2),
        ];
        let out =
            arrange_grouped_materials(&parts, &[vec![1], vec![1]], &[Standard, RemovableLabel], &c)
                .unwrap();
        assert_eq!(out.len(), 3);
        assert_ne!(out[0].plate, out[1].plate);
        assert_ne!(out[0].plate, out[2].plate);
        assert_ne!(out[1].plate, out[2].plate);
    }
    #[test]
    fn material_sets_keep_text_with_body_and_separate_single_color() {
        let c = Config::default();
        let parts = vec![("a".into(), Vec3::splat(20.), 1); 4];
        let out =
            arrange_materials(&parts, &[vec![1, 2], vec![2, 1], vec![1], vec![2]], &c).unwrap();
        assert_eq!(out[0].plate, out[1].plate);
        assert_ne!(out[0].plate, out[2].plate);
        assert_ne!(out[0].plate, out[3].plate);
        let mut c = c;
        c.text_filaments.insert("label".into(), 3);
        let round: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(round.text_slot("label"), 3);
        assert_eq!(round.text_slot("plain"), 1);
        c.text_filaments.insert("bad".into(), 7);
        assert!(c.validate().is_err());
    }
    #[test]
    fn centering_preserves_spacing_and_avoids_keepout() {
        let mut c = Config {
            center: true,
            brim: 2.,
            ..Default::default()
        };
        let one = arrange(&[("small".into(), Vec3::new(20., 30., 10.), 1)], &c).unwrap();
        assert!((one[0].x - 128.).abs() < 0.001 && (one[0].y - 128.).abs() < 0.001);
        let parts = vec![("small".into(), Vec3::new(20., 30., 10.), 3)];
        let centered = arrange(&parts, &c).unwrap();
        c.center = false;
        let original = arrange(&parts, &c).unwrap();
        let dx = centered[0].x - original[0].x;
        let dy = centered[0].y - original[0].y;
        for (a, b) in centered.iter().zip(&original) {
            assert!((a.x - b.x - dx).abs() < 0.001 && (a.y - b.y - dy).abs() < 0.001);
            assert_eq!(a.plate, b.plate);
            assert_eq!(a.rotated, b.rotated);
        }
        c.brim = 0.;
        c.rotate = false;
        let large = vec![("large".into(), Vec3::new(220., 200., 10.), 1)];
        let original = arrange(&large, &c).unwrap();
        c.center = true;
        let fallback = arrange(&large, &c).unwrap();
        assert_eq!(
            (fallback[0].x, fallback[0].y),
            (original[0].x, original[0].y)
        );
        let paths = vec!["a".into(), "b".into()];
        c.part_filaments.insert("b".into(), 2);
        let colored = arrange_colored(
            &[
                ("a".into(), Vec3::splat(20.), 1),
                ("b".into(), Vec3::splat(30.), 1),
            ],
            &paths,
            &c,
        )
        .unwrap();
        assert_ne!(colored[0].plate, colored[1].plate);
        assert!(
            colored
                .iter()
                .all(|p| (p.x - 128.).abs() < 0.001 && (p.y - 128.).abs() < 0.001)
        );
    }
    #[test]
    fn color_groups_never_mix_and_keep_copies_overflow_and_slot_settings() {
        let paths = vec!["a".into(), "b".into(), "c".into()];
        let parts = vec![
            ("a".into(), Vec3::new(200., 200., 20.), 2),
            ("b".into(), Vec3::new(30., 30., 10.), 2),
            ("c".into(), Vec3::new(10., 10., 10.), 1),
        ];
        let mut c = Config::default();
        c.part_filaments.insert("b".into(), 2);
        c.part_filaments.insert("c".into(), 2);
        c.palette[1] = [32, 120, 210];
        let layout = arrange_colored(&parts, &paths, &c).unwrap();
        assert_eq!(layout.len(), 5);
        let count = layout.iter().map(|p| p.plate + 1).max().unwrap();
        assert_eq!(count, 3);
        for p in &layout {
            for q in &layout {
                if p.plate == q.plate {
                    assert_eq!(c.slot(&paths[p.input]), c.slot(&paths[q.input]));
                }
            }
        }
        assert!(
            arrange_colored(
                &parts,
                &paths,
                &Config {
                    split: false,
                    ..c.clone()
                }
            )
            .is_err()
        );
        let small = vec![
            ("a".into(), Vec3::new(10., 10., 10.), 1),
            ("b".into(), Vec3::new(10., 10., 10.), 1),
        ];
        c.palette[1] = c.palette[0];
        let split = arrange_colored(
            &small,
            &paths[..2],
            &Config {
                split: false,
                ..c.clone()
            },
        )
        .unwrap();
        assert_ne!(split[0].plate, split[1].plate);
        let restored: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(restored.slot("b"), 2);
        assert_eq!(restored.color("b"), c.color("b"));
        c.palette[1] = [32, 120, 210];
        let json: serde_json::Value = serde_json::from_slice(&c.project_json().unwrap()).unwrap();
        assert_eq!(json["filament_colour"][1], "#2078D2");
        c.part_filaments.insert("bad".into(), 7);
        assert!(c.validate().is_err());
    }
    #[test]
    fn rotation_fits_narrow_free_strip_and_invalid_settings_fail() {
        let parts = vec![
            ("large".into(), Vec3::new(200., 200., 10.), 1),
            ("strip".into(), Vec3::new(100., 30., 10.), 1),
        ];
        let config = Config {
            margin: 3.,
            brim: 0.,
            split: false,
            ..Default::default()
        };
        let layout = arrange(&parts, &config).unwrap();
        assert!(layout[1].rotated);
        assert!(
            arrange(
                &parts,
                &Config {
                    rotate: false,
                    ..config
                }
            )
            .is_err()
        );
        assert!(
            Config {
                gap: f32::NAN,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn p1s_multiple_plates_clearance_keepout_and_bounds() {
        let c = Config::default();
        let result = arrange(
            &[
                ("box".into(), Vec3::new(120., 100., 40.), 7),
                ("small".into(), Vec3::new(20., 30., 5.), 8),
            ],
            &c,
        )
        .unwrap();
        assert!(result.iter().any(|p| p.plate > 0));
        assert_eq!(result.len(), 15);
        for (i, p) in result.iter().enumerate() {
            let r = Rect {
                x: p.x - p.width / 2.,
                y: p.y - p.depth / 2.,
                w: p.width,
                h: p.depth,
            };
            assert!(
                r.x >= c.margin - 0.001
                    && r.y >= c.margin - 0.001
                    && r.right() <= 256. - c.margin + 0.001
                    && r.top() <= 256. - c.margin + 0.001
            );
            assert!(!r.intersects(Rect {
                x: 0.,
                y: 0.,
                w: 18.,
                h: 28.
            }));
            for q in &result[i + 1..] {
                if p.plate == q.plate {
                    assert!(
                        !Rect {
                            x: r.x - c.gap / 2.,
                            y: r.y - c.gap / 2.,
                            w: r.w + c.gap,
                            h: r.h + c.gap
                        }
                        .intersects(Rect {
                            x: q.x - q.width / 2. - c.gap / 2.,
                            y: q.y - q.depth / 2. - c.gap / 2.,
                            w: q.width + c.gap,
                            h: q.depth + c.gap
                        })
                    );
                }
            }
        }
        assert!(arrange(&[("oversize".into(), Vec3::new(300., 10., 5.), 1)], &c).is_err());
        assert!(arrange(&[("tall".into(), Vec3::new(10., 10., 251.), 1)], &c).is_err());
        assert!(
            arrange(
                &[("box".into(), Vec3::new(200., 200., 40.), 2)],
                &Config { split: false, ..c }
            )
            .is_err()
        );
        assert_eq!(plate_origin(3, 7), [0., -307.2]);
        let wide = vec![("M4".into(), Vec3::new(254.2, 57., 188.), 1)];
        assert!(arrange(&wide, &Config::default()).is_ok());
        assert!(
            arrange(
                &wide,
                &Config {
                    margin: 3.,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
