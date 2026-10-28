//! P1S print-bed packing and Bambu project settings. No assembly transforms.
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
pub const BIN_COLOR_KEY: &str = "generated/gridfinity/@bin-color";
pub const LABEL_COLOR_KEY: &str = "generated/gridfinity/@label-color";
fn generated_color_type(path: &str) -> Option<(&'static str, u32)> {
    let file = path
        .strip_prefix(crate::gridfinity::PREFIX)?
        .strip_suffix(".stl")?;
    let (key, id) = if let Some(id) = file.strip_prefix("Gridfinity-") {
        (BIN_COLOR_KEY, id)
    } else {
        (LABEL_COLOR_KEY, file.strip_prefix("Cullenect-")?)
    };
    let id = id.parse::<u32>().ok()?;
    (id > 0).then_some((key, id))
}
/// Generated bins and removable labels each share an assignment across all IDs.
pub fn color_key(path: &str) -> &str {
    generated_color_type(path).map_or(path, |(key, _)| key)
}
fn assignment(map: &std::collections::BTreeMap<String, u32>, path: &str) -> Option<u32> {
    let key = color_key(path);
    map.get(key).copied().or_else(|| {
        if key == path {
            return None;
        }
        // Older presets stored one assignment per ID. Use the lowest configured
        // ID consistently until the next edit consolidates the whole type.
        map.iter()
            .filter_map(|(p, slot)| {
                generated_color_type(p)
                    .filter(|(family, _)| *family == key)
                    .map(|(_, id)| (id, *slot))
            })
            .min_by_key(|(id, _)| *id)
            .map(|(_, slot)| slot)
    })
}
fn set_assignment(map: &mut std::collections::BTreeMap<String, u32>, path: &str, slot: u32) {
    let key = color_key(path).to_owned();
    map.retain(|p, _| color_key(p) != key);
    if slot != 0 {
        map.insert(key, slot);
    }
}
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
    pub module_clip_supports: bool,
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
            module_clip_supports: true,
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
    fn brim_extent(&self) -> Result<f32, String> {
        if self.brim == 0. {
            return Ok(0.);
        }
        #[derive(Deserialize)]
        struct Brim {
            brim_object_gap: String,
        }
        let preset: Brim = serde_json::from_str(include_str!("../config/export/p1s-0.4.json"))
            .map_err(|e| e.to_string())?;
        let gap: f32 = preset
            .brim_object_gap
            .parse()
            .map_err(|_| "无效 Brim 间隙")?;
        if !gap.is_finite() || gap < 0. {
            return Err("无效 Brim 间隙".into());
        }
        Ok(self.brim + gap)
    }
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
    pub fn part_assignment(&self, path: &str) -> u32 {
        assignment(&self.part_filaments, path).unwrap_or(0)
    }
    pub fn text_assignment(&self, path: &str) -> u32 {
        assignment(&self.text_filaments, path).unwrap_or(0)
    }
    pub fn set_part_slot(&mut self, path: &str, slot: u32) {
        set_assignment(&mut self.part_filaments, path, slot);
    }
    pub fn set_text_slot(&mut self, path: &str, slot: u32) {
        set_assignment(&mut self.text_filaments, path, slot);
    }
    pub fn slot(&self, path: &str) -> u32 {
        assignment(&self.part_filaments, path)
            .unwrap_or(self.filament)
            .clamp(1, 6)
    }
    pub fn text_slot(&self, path: &str) -> u32 {
        assignment(&self.text_filaments, path)
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
    fn corners(self) -> [Vec2; 4] {
        [
            Vec2::new(self.x, self.y),
            Vec2::new(self.right(), self.y),
            Vec2::new(self.right(), self.top()),
            Vec2::new(self.x, self.top()),
        ]
    }
    fn expanded(self, padding: f32) -> Self {
        Self {
            x: self.x - padding,
            y: self.y - padding,
            w: self.w + 2. * padding,
            h: self.h + 2. * padding,
        }
    }
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
// Bambu's preview checks the convex hull of ALL extrusion paths against the
// front-left exclusion, even when every individual object avoids that corner.
fn extend_hull(hull: &[Vec2], rect: Rect) -> Vec<Vec2> {
    let mut points = hull.to_vec();
    points.extend(rect.corners());
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    let mut result = Vec::<Vec2>::new();
    for p in &points {
        while result.len() >= 2 {
            let n = result.len();
            if (result[n - 1] - result[n - 2]).perp_dot(*p - result[n - 1]) > 0. {
                break;
            }
            result.pop();
        }
        result.push(*p);
    }
    let lower = result.len();
    for p in points.iter().rev().skip(1) {
        while result.len() > lower {
            let n = result.len();
            if (result[n - 1] - result[n - 2]).perp_dot(*p - result[n - 1]) > 0. {
                break;
            }
            result.pop();
        }
        result.push(*p);
    }
    result.pop();
    result
}
fn hull_intersects(hull: &[Vec2], rect: Rect) -> bool {
    if hull.is_empty() {
        return false;
    }
    let corners = rect.corners();
    let edges = hull.iter().zip(hull.iter().cycle().skip(1)).map(|(a, b)| {
        let d = *b - *a;
        Vec2::new(-d.y, d.x).normalize_or_zero()
    });
    for axis in [Vec2::X, Vec2::Y].into_iter().chain(edges) {
        if axis == Vec2::ZERO {
            continue;
        }
        let range = |points: &[Vec2]| {
            points
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                    let v = p.dot(axis);
                    (lo.min(v), hi.max(v))
                })
        };
        let (a, b) = range(hull);
        let (c, d) = range(&corners);
        if b <= c + 0.0001 || d <= a + 0.0001 {
            return false;
        }
    }
    true
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
// Bambu subtracts this offset when writing G-code. Read the same embedded
// machine profile that project_json exports; never silently change calibration.
fn nozzle_offset() -> Result<Vec2, String> {
    #[derive(Deserialize)]
    struct Machine {
        extruder_offset: Vec<String>,
    }
    let machine: Machine = serde_json::from_str(include_str!("../config/export/p1s-0.4.json"))
        .map_err(|e| e.to_string())?;
    let [offset] = machine.extruder_offset.as_slice() else {
        return Err("P1S 摆盘只支持单喷嘴偏移配置".into());
    };
    let (x, y) = offset.split_once('x').ok_or("无效喷嘴偏移")?;
    let offset = Vec2::new(
        x.parse().map_err(|_| "无效喷嘴 X 偏移")?,
        y.parse().map_err(|_| "无效喷嘴 Y 偏移")?,
    );
    if !offset.is_finite() {
        return Err("无效喷嘴偏移".into());
    }
    Ok(offset)
}
fn usable_bed(c: &Config, offset: Vec2) -> Rect {
    // Intersection of model-space bed and machine-space bed translated back
    // into model coordinates. Both must contain the complete print footprint.
    let min = offset.max(Vec2::ZERO) + Vec2::splat(c.margin);
    let max = Vec2::splat(256. - c.margin) + offset.min(Vec2::ZERO);
    Rect {
        x: min.x,
        y: min.y,
        w: max.x - min.x,
        h: max.y - min.y,
    }
}
fn hull_keepout(c: &Config, offset: Vec2) -> Rect {
    // Union of the forbidden corner in model and nozzle coordinates, with
    // margin. Part gap applies to individual parts, not to this virtual hull.
    Rect {
        x: offset.x.min(0.),
        y: offset.y.min(0.),
        w: 18. + offset.x.abs() + c.margin,
        h: 28. + offset.y.abs() + c.margin,
    }
}
// Actual extrusion segments in the embedded P1S machine_start_gcode:
// X18..240 at Y1 and Y1.5, plus the return at X239.5..240 up to Y11.
// Keep the L shape so the rest of the front strip remains available.
const PRIME_LINE: [Rect; 2] = [
    Rect {
        x: 18.,
        y: 1.,
        w: 222.,
        h: 0.5,
    },
    Rect {
        x: 239.5,
        y: 1.,
        w: 0.5,
        h: 10.,
    },
];
const PRIME_LINE_CLEARANCE: f32 = 2.;
fn exclusion_zones(c: &Config, offset: Vec2) -> Vec<Rect> {
    // Protect each footprint in both model and G-code coordinates. The start
    // code contains literal machine positions, not per-object transforms.
    let both_frames = |r: Rect| Rect {
        x: r.x + offset.x.min(0.),
        y: r.y + offset.y.min(0.),
        w: r.w + offset.x.abs(),
        h: r.h + offset.y.abs(),
    };
    let mut zones = vec![Rect {
        x: offset.x.min(0.),
        y: offset.y.min(0.),
        w: 18. + offset.x.abs() + c.margin + c.gap,
        h: 28. + offset.y.abs() + c.margin + c.gap,
    }];
    zones.extend(
        PRIME_LINE.map(|r| both_frames(r).expanded(PRIME_LINE_CLEARANCE + c.margin + c.gap)),
    );
    zones
}
fn empty_plate(c: &Config, offset: Vec2) -> Vec<Rect> {
    let bed = usable_bed(c, offset);
    let mut free = vec![Rect {
        x: bed.x - c.gap / 2.,
        y: bed.y - c.gap / 2.,
        w: bed.w + c.gap,
        h: bed.h + c.gap,
    }];
    for zone in exclusion_zones(c, offset) {
        // Packed rectangles already include half the part gap on every side.
        // Contract obstacles by that amount to match the physical-footprint
        // checks used after centering, without counting the gap twice.
        subtract(&mut free, zone.expanded(-c.gap / 2.));
    }
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
    let offset = nozzle_offset()?;
    let bed = usable_bed(c, offset);
    if bed.w <= 0. || bed.h <= 0. {
        return Err("喷嘴偏移与边距超出可用打印区域".into());
    }
    let mut plates = vec![empty_plate(c, offset)];
    let mut hulls = vec![Vec::<Vec2>::new()];
    let brim = c.brim_extent()?;
    let mut placed = Vec::new();
    for (input, copy) in jobs {
        let raw_size = parts[input].1;
        let size = Vec3::new(raw_size.x + 2. * brim, raw_size.y + 2. * brim, raw_size.z);
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
                            let candidate_rect = Rect {
                                x: r.x,
                                y: r.y,
                                w,
                                h,
                            };
                            let hull =
                                extend_hull(&hulls[plate], candidate_rect.expanded(-c.gap / 2.));
                            if hull_intersects(&hull, hull_keepout(c, offset)) {
                                continue;
                            }
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
            if plates.last().unwrap().len() == empty_plate(c, offset).len()
                && placed
                    .iter()
                    .all(|p: &Placement| p.plate != plates.len() - 1)
            {
                return Err(format!(
                    "{}：{:.1} × {:.1} mm，无法放入避开擦料线并计入喷嘴偏移的 P1S 可用区域；请使用分体版本或调整边距",
                    parts[input].0, size.x, size.y
                ));
            }
            if !c.split {
                return Err("当前零件无法放入单个打印盘，请开启自动分盘".into());
            }
            if plates.len() >= 36 {
                return Err("导出超过 36 个打印盘，请减少零件份数".into());
            }
            plates.push(empty_plate(c, offset));
            hulls.push(Vec::new());
        }
        let (_, plate, r, rotated) = candidate.unwrap();
        subtract(&mut plates[plate], r);
        hulls[plate] = extend_hull(&hulls[plate], r.expanded(-c.gap / 2.));
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
        for (plate, hull) in hulls.iter().enumerate() {
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
            let exclusions = exclusion_zones(c, offset);
            let safe = group.iter().all(|p| {
                let r = Rect {
                    x: p.x + dx - p.width / 2.,
                    y: p.y + dy - p.depth / 2.,
                    w: p.width,
                    h: p.depth,
                };
                r.x >= bed.x - 0.0001
                    && r.y >= bed.y - 0.0001
                    && r.right() <= bed.right() + 0.0001
                    && r.top() <= bed.top() + 0.0001
                    && exclusions.iter().all(|zone| !r.intersects(*zone))
            });
            // Rigid translation preserves spacing and brim; keep valid packing if blocked.
            let shifted_hull: Vec<_> = hull.iter().map(|v| *v + Vec2::new(dx, dy)).collect();
            if safe && !hull_intersects(&shifted_hull, hull_keepout(c, offset)) {
                for p in placed.iter_mut().filter(|p| p.plate == plate) {
                    p.x += dx;
                    p.y += dy;
                }
            }
        }
    }
    // Stable source/copy order, independent of packing order.
    placed.sort_by_key(|p| (p.input, p.copy));
    // Recompute from the final placements, including any centering. Fail closed
    // if later changes to the packer violate the slicer's whole-plate rule.
    for plate in 0..plates.len() {
        let mut hull = Vec::new();
        for p in placed.iter().filter(|p| p.plate == plate) {
            let r = Rect {
                x: p.x - p.width / 2.,
                y: p.y - p.depth / 2.,
                w: p.width,
                h: p.depth,
            };
            if !bed.expanded(0.001).contains(r)
                || exclusion_zones(c, offset)
                    .iter()
                    .any(|zone| r.intersects(*zone))
            {
                return Err(format!(
                    "第 {} 盘 {}：最终打印占地越过边界或避让区域",
                    plate + 1,
                    parts[p.input].0
                ));
            }
            hull = extend_hull(&hull, r);
        }
        if hull_intersects(&hull, hull_keepout(c, offset)) {
            return Err(format!("第 {} 盘：整盘轮廓凸包进入左前禁放区", plate + 1));
        }
    }
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
    fn individually_clear_parts_can_fail_bambu_whole_plate_hull() {
        // The user's plate 4: the baseplate is left of the cap. Neither part
        // touches the corner, but their connecting convex hull does.
        let base = Rect {
            x: 0.5,
            y: 36.5,
            w: 176.,
            h: 214.699,
        };
        let cap = Rect {
            x: 24.5,
            y: 12.,
            w: 37.799,
            h: 13.172,
        };
        let forbidden = Rect {
            x: 0.,
            y: 0.,
            w: 18.,
            h: 30.,
        };
        assert!(!base.intersects(forbidden) && !cap.intersects(forbidden));
        assert!(hull_intersects(
            &extend_hull(&extend_hull(&[], base), cap),
            forbidden
        ));
        let moved_cap = Rect { x: 182.5, ..cap };
        assert!(!hull_intersects(
            &extend_hull(&extend_hull(&[], base), moved_cap),
            forbidden
        ));
        let parts = vec![
            ("baseplate".into(), Vec3::new(214.699, 176., 4.), 2),
            ("cap".into(), Vec3::new(37.799, 13.172, 11.673), 2),
            ("bin".into(), Vec3::new(41.5, 83.5, 31.55), 4),
            ("facade".into(), Vec3::new(9.84, 9.84, 3.8), 6),
        ];
        for center in [false, true] {
            for rotate in [false, true] {
                for brim in [0., 2.] {
                    let c = Config {
                        center,
                        rotate,
                        brim,
                        ..Default::default()
                    };
                    let placements = arrange(&parts, &c).unwrap();
                    assert_machine_safe(&placements, &c);
                }
            }
        }
    }
    #[test]
    fn brim_gap_is_part_of_print_footprint() {
        let c = Config {
            brim: 2.,
            ..Default::default()
        };
        let placements = arrange(&[("brimmed".into(), Vec3::new(10., 10., 1.), 1)], &c).unwrap();
        assert!((placements[0].width - 14.2).abs() < 0.001);
        assert_eq!(Config::default().brim_extent().unwrap(), 0.);
    }
    fn assert_machine_safe(layout: &[Placement], c: &Config) {
        let offset = nozzle_offset().unwrap();
        assert_eq!(offset, Vec2::new(0., 2.));
        for p in layout {
            for origin in [Vec2::ZERO, offset] {
                let r = Rect {
                    x: p.x - p.width / 2. - origin.x,
                    y: p.y - p.depth / 2. - origin.y,
                    w: p.width,
                    h: p.depth,
                };
                assert!(
                    r.x >= c.margin - 0.001
                        && r.y >= c.margin - 0.001
                        && r.right() <= 256. - c.margin + 0.001
                        && r.top() <= 256. - c.margin + 0.001,
                    "Unsafe model/machine footprint: {p:?}, offset {origin:?}"
                );
                assert!(!r.intersects(Rect {
                    x: 0.,
                    y: 0.,
                    w: 18.,
                    h: 28.
                }));
                // Independent machine-coordinate check includes the real purge
                // segments and fixed clearance, including model Brim in r.
                for line in PRIME_LINE {
                    assert!(
                        !r.intersects(line.expanded(2.)),
                        "Footprint overlaps start purge line: {p:?}, frame {origin:?}"
                    );
                }
            }
        }
        for plate in 0..layout.iter().map(|p| p.plate + 1).max().unwrap_or(0) {
            let mut hull = Vec::new();
            for p in layout.iter().filter(|p| p.plate == plate) {
                hull = extend_hull(
                    &hull,
                    Rect {
                        x: p.x - p.width / 2.,
                        y: p.y - p.depth / 2.,
                        w: p.width,
                        h: p.depth,
                    },
                );
            }
            assert!(
                !hull_intersects(&hull, hull_keepout(c, offset)),
                "Unsafe plate hull: {plate}"
            );
        }
    }
    #[test]
    fn purge_exclusions_follow_embedded_start_extrusions() {
        let profile: serde_json::Value =
            serde_json::from_str(include_str!("../config/export/p1s-0.4.json")).unwrap();
        let start = profile["machine_start_gcode"]
            .as_str()
            .unwrap()
            .split(";===== nozzle load line")
            .nth(1)
            .unwrap()
            .split("M400")
            .next()
            .unwrap();
        let mut position: Option<Vec2> = None;
        let mut count = 0;
        for line in start
            .lines()
            .filter(|l| l.starts_with("G0 ") || l.starts_with("G1 "))
        {
            let mut end = position.unwrap_or(Vec2::splat(f32::NAN));
            let mut extrusion = 0.;
            for token in line.split(';').next().unwrap().split_whitespace().skip(1) {
                if let Ok(value) = token[1..].parse::<f32>() {
                    match token.as_bytes()[0] {
                        b'X' => end.x = value,
                        b'Y' => end.y = value,
                        b'E' => extrusion = value,
                        _ => {}
                    }
                }
            }
            if let Some(previous) = position
                && extrusion > 0.
                && previous != end
            {
                let min = previous.min(end);
                let size = (end - previous).abs();
                let path = Rect {
                    x: min.x,
                    y: min.y,
                    w: size.x,
                    h: size.y,
                };
                assert!(
                    PRIME_LINE.iter().any(|zone| zone.contains(path)),
                    "Start extrusion changed; update purge exclusions: {line}"
                );
                count += 1;
            }
            assert!(end.is_finite());
            position = Some(end);
        }
        assert_eq!(count, 4);
    }
    #[test]
    fn every_plate_avoids_purge_with_rotation_brim_and_centering() {
        let parts = vec![
            ("large".into(), Vec3::new(180., 180., 20.), 5),
            ("labels".into(), Vec3::new(36., 11., 1.2), 30),
            ("wide".into(), Vec3::new(120., 24., 5.), 5),
        ];
        for center in [false, true] {
            for rotate in [false, true] {
                for gap in [0., 6., 30.] {
                    for brim in [0., 2.] {
                        let c = Config {
                            center,
                            rotate,
                            gap,
                            brim,
                            margin: 0.,
                            ..Default::default()
                        };
                        let layout = arrange(&parts, &c).unwrap();
                        assert!(layout.iter().any(|p| p.plate > 0));
                        assert_machine_safe(&layout, &c);
                    }
                }
            }
        }
    }
    #[test]
    fn centering_cannot_move_a_wide_part_onto_the_purge_return() {
        let parts = vec![("wide".into(), Vec3::new(207., 218., 10.), 1)];
        let c = Config {
            rotate: false,
            ..Default::default()
        };
        let original = arrange(&parts, &c).unwrap();
        let centered = arrange(
            &parts,
            &Config {
                center: true,
                ..c.clone()
            },
        )
        .unwrap();
        // Centering gives X=24.5..231.5 and Y=19..237: clear of the front-left
        // keep-out, but intersects the buffered right-hand purge return.
        assert_eq!(
            (original[0].x, original[0].y),
            (centered[0].x, centered[0].y)
        );
        assert_machine_safe(&centered, &c);
    }
    #[test]
    fn m4_packing_reserves_nozzle_offset_before_rotation_and_centering() {
        let parts = vec![
            (
                "M4 Module 1F".into(),
                Vec3::new(254.173, 57.009, 188.101),
                1,
            ),
            ("M4 Baseplate".into(), Vec3::new(214.699, 176., 4.), 1),
            ("Facade Cap".into(), Vec3::new(9.84, 9.84, 3.8), 3),
            ("Label".into(), Vec3::new(47.857, 20.627, 2.), 1),
            ("Key Cap".into(), Vec3::new(37.799, 13.172, 11.673), 1),
        ];
        for center in [false, true] {
            for margin in [0., 0.5] {
                let c = Config {
                    center,
                    margin,
                    ..Default::default()
                };
                let layout = arrange(&parts, &c).unwrap();
                assert_machine_safe(&layout, &c);
                assert!(
                    !layout[0].rotated,
                    "254 mm module cannot fit along offset Y axis"
                );
                assert!(layout.iter().any(|p| p.plate > 0));
                assert!(arrange(&parts, &Config { split: false, ..c }).is_err());
            }
        }
        let c = Config {
            rotate: false,
            center: true,
            brim: 2.,
            margin: 0.5,
            ..Default::default()
        };
        let layout = arrange(&[("brimmed".into(), Vec3::new(90., 100., 10.), 8)], &c).unwrap();
        assert_machine_safe(&layout, &c);
    }
    #[test]
    fn nozzle_envelope_handles_both_offset_signs() {
        let c = Config::default();
        let area = usable_bed(&c, Vec2::new(-3., 2.));
        assert_eq!(
            (area.x, area.y, area.right(), area.top()),
            (0.5, 2.5, 252.5, 255.5)
        );
    }
    /// Optional external regression fixture: dimensions only, no model library copy.
    #[test]
    fn repack_external_plate_fixture() {
        let Ok(path) = std::env::var("VAULT_PACKING_FIXTURE") else {
            return;
        };
        #[derive(Deserialize)]
        struct Fixture {
            config: Config,
            plates: Vec<Vec<(String, Vec3, u32)>>,
        }
        let fixture: Fixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let mut result = Vec::new();
        let mut offset = 0;
        for (old_plate, parts) in fixture.plates.iter().enumerate() {
            let placements = arrange(parts, &fixture.config).unwrap();
            assert_machine_safe(&placements, &fixture.config);
            let count = placements.iter().map(|p| p.plate + 1).max().unwrap_or(0);
            for p in placements {
                result.push(serde_json::json!({"old_plate":old_plate,"input":p.input,
                    "plate":p.plate+offset,"x":p.x,"y":p.y,"rotated":p.rotated}));
            }
            offset += count;
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("output/packing-boundary");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            out.join("fixed-layout.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn generated_type_colors_follow_future_ids_and_keep_text_separate() {
        let a = crate::gridfinity::Bin::default();
        let b = crate::gridfinity::Bin {
            id: 2,
            ..Default::default()
        };
        let future = crate::gridfinity::Bin {
            id: 99,
            ..Default::default()
        };
        let mut c = Config::default();
        c.set_part_slot(&a.path(), 3);
        c.set_part_slot(&a.label_path(), 4);
        c.set_text_slot(&b.label_path(), 2);
        for bin in [&a, &b, &future] {
            assert_eq!(c.slot(&bin.path()), 3);
            assert_eq!(c.slot(&bin.label_path()), 4);
            assert_eq!(c.text_slot(&bin.label_path()), 2);
            assert_eq!(c.text_slot(&bin.path()), 3);
            assert_eq!(c.color(&bin.label_path()), c.slot_color(4));
        }
        c.set_part_slot(crate::label::PATH, 6);
        let round: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(round.slot(&future.path()), 3);
        assert_eq!(round.text_slot(&future.label_path()), 2);
        assert_eq!(round.slot(crate::label::PATH), 6);
        c.set_part_slot(&b.label_path(), 0);
        assert_eq!(c.slot(&a.label_path()), c.filament);
        assert_eq!(c.text_slot(&a.label_path()), 2);
        c.set_text_slot(&future.label_path(), 0);
        assert_eq!(c.text_slot(&a.label_path()), c.filament);
        assert_eq!(c.slot(&a.path()), 3);
    }
    #[test]
    fn old_generated_assignments_converge_by_id_and_reset_without_resurrection() {
        let mut c = Config::default();
        let first = "generated/gridfinity/Cullenect-2.stl";
        let later = "generated/gridfinity/Cullenect-10.stl";
        c.part_filaments.insert(later.into(), 5);
        c.part_filaments.insert(first.into(), 3);
        c.part_filaments.insert("other.stl".into(), 6);
        assert_eq!(c.part_assignment(first), 3);
        assert_eq!(c.part_assignment(later), 3);
        c.set_part_slot(later, 4);
        assert_eq!(c.slot(first), 4);
        assert!(!c.part_filaments.contains_key(first));
        assert!(!c.part_filaments.contains_key(later));
        assert_eq!(c.part_filaments.get(LABEL_COLOR_KEY), Some(&4));
        c.set_part_slot(first, 0);
        assert_eq!(c.slot(later), c.filament);
        assert_eq!(c.slot("other.stl"), 6);
        assert_eq!(
            color_key("library/Gridfinity-2.stl"),
            "library/Gridfinity-2.stl"
        );
        assert_eq!(
            color_key("generated/gridfinity/Cullenect-custom.stl"),
            "generated/gridfinity/Cullenect-custom.stl"
        );
    }
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
            ("large".into(), Vec3::new(200., 213., 10.), 1),
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
        assert_machine_safe(&result, &c);
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
