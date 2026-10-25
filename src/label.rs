//! Parametric lettering on the original Label.stl. Never changes its mounting pose.
use crate::{
    mesh::{Mesh, Vertex},
    scene::Scene,
};
use glam::Vec3;
use manifold_rust::{
    cross_section::CrossSection,
    linalg::{Vec2 as D2, Vec3 as D3},
    manifold::Manifold,
    types::{Error, MeshGL64},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::Write, path::Path, sync::Arc};

pub const PATH: &str = "01. DRAWERS/07. Universal/Label.stl";
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Style {
    #[default]
    Raised,
    Engraved,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Font {
    Chinese,
    Arial,
    #[default]
    ChineseBold,
    ArialBold,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub text: String,
    pub style: Style,
    pub font: Font,
    pub size: f32,
    pub depth: f32,
    pub offset: [f32; 2],
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            text: "VAULT".into(),
            style: Style::Raised,
            font: Font::ChineseBold,
            size: 5.,
            depth: 0.6,
            offset: [0., 0.],
        }
    }
}
impl Config {
    pub fn active(&self) -> bool {
        self.enabled && !self.text.trim().is_empty()
    }
    /// Font em size in mm; actual glyph height depends on the font.
    pub fn min_size(&self) -> f32 {
        if self
            .text
            .chars()
            .any(|c| !c.is_ascii() && !c.is_whitespace())
        {
            5.
        } else {
            4.
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.active() {
            return Ok(());
        }
        if self.text.chars().count() > 48 || self.text.lines().count() > 2 {
            return Err("标签最多两行、48 个字符".into());
        }
        if !self.size.is_finite()
            || !(self.min_size()..=12.).contains(&self.size)
            || !self.depth.is_finite()
            || !(0.2..=0.8).contains(&self.depth)
            || self.offset.iter().any(|x| !x.is_finite() || x.abs() > 10.)
        {
            return Err(format!(
                "字号需为 {}–12 mm（英文/数字最低 4 mm，中文及其他文字最低 5 mm），字高/刻深需为 0.2–0.8 mm，偏移不可超过 10 mm",
                self.min_size()
            ));
        }
        if self
            .text
            .chars()
            .any(|c| c.is_control() && !matches!(c as u32, 10 | 13))
        {
            return Err("文字含不支持的控制字符".into());
        }
        Ok(())
    }
}
struct Outline {
    paths: Vec<Vec<D2>>,
    scale: f64,
    origin: D2,
}
impl Outline {
    fn point(&self, x: f32, y: f32) -> D2 {
        D2::new(
            x as f64 * self.scale + self.origin.x,
            y as f64 * self.scale + self.origin.y,
        )
    }
    fn last(&self) -> D2 {
        *self.paths.last().unwrap().last().unwrap()
    }
    fn push(&mut self, p: D2) {
        self.paths.last_mut().unwrap().push(p);
    }
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.paths.push(vec![self.point(x, y)]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.push(self.point(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let a = self.last();
        let b = self.point(x1, y1);
        let c = self.point(x, y);
        for i in 1..=16 {
            let t = i as f64 / 16.;
            self.push(a * ((1. - t) * (1. - t)) + b * (2. * t * (1. - t)) + c * (t * t));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.last();
        let b = self.point(x1, y1);
        let c = self.point(x2, y2);
        let d = self.point(x, y);
        for i in 1..=20 {
            let t = i as f64 / 20.;
            let u = 1. - t;
            self.push(
                a * (u * u * u) + b * (3. * u * u * t) + c * (3. * u * t * t) + d * (t * t * t),
            );
        }
    }
    fn close(&mut self) {
        if let Some(path) = self.paths.last_mut() {
            if path.len() > 1
                && (path[0].x - path.last().unwrap().x).abs()
                    + (path[0].y - path.last().unwrap().y).abs()
                    < 1e-8
            {
                path.pop();
            }
            // TrueType outlines use clockwise exterior contours.
            path.reverse();
        }
    }
}
pub(crate) fn text_polygons(config: &Config) -> Result<Vec<Vec<D2>>, String> {
    let file = match config.font {
        Font::Chinese => "msyh.ttc",
        Font::Arial => "arial.ttf",
        Font::ChineseBold => "msyhbd.ttc",
        Font::ArialBold => "arialbd.ttf",
    };
    let path = crate::paths::windows_font(file).ok_or_else(|| format!("系统字体不可用：{file}"))?;
    let data = std::fs::read(&path).map_err(|e| format!("字体不可用：{}: {e}", path.display()))?;
    let face = ttf_parser::Face::parse(&data, 0).map_err(|e| format!("字体读取失败：{e:?}"))?;
    let mut out = Outline {
        paths: Vec::new(),
        scale: config.size as f64 / face.units_per_em() as f64,
        origin: D2::new(0., 0.),
    };
    for (row, line) in config.text.lines().enumerate() {
        out.origin = D2::new(0., -(row as f64) * config.size as f64 * 1.35);
        let first = out.paths.len();
        for ch in line.chars().filter(|c| *c as u32 != 13) {
            let id = face
                .glyph_index(ch)
                .ok_or_else(|| format!("当前字体不包含字符：{ch}"))?;
            let before = out.paths.len();
            face.outline_glyph(id, &mut out);
            if out.paths.len() == before && !ch.is_whitespace() {
                return Err(format!("字符没有可用轮廓：{ch}"));
            }
            out.origin.x += face.glyph_hor_advance(id).unwrap_or(0) as f64 * out.scale;
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for p in out.paths[first..].iter().flatten() {
            lo = lo.min(p.x);
            hi = hi.max(p.x);
        }
        if lo.is_finite() {
            for p in out.paths[first..].iter_mut().flatten() {
                p.x -= (lo + hi) * 0.5;
            }
        }
    }
    out.paths.retain(|p| p.len() >= 3);
    if out.paths.is_empty() {
        return Err("文字没有可生成的轮廓".into());
    }
    let lo = out
        .paths
        .iter()
        .flatten()
        .fold(D2::new(f64::INFINITY, f64::INFINITY), |a, b| {
            D2::new(a.x.min(b.x), a.y.min(b.y))
        });
    let hi = out
        .paths
        .iter()
        .flatten()
        .fold(D2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, b| {
            D2::new(a.x.max(b.x), a.y.max(b.y))
        });
    for p in out.paths.iter_mut().flatten() {
        p.y -= (lo.y + hi.y) * 0.5;
        p.x += config.offset[0] as f64;
        p.y += config.offset[1] as f64;
        if p.x.abs() > 19. || p.y.abs() > 6. {
            return Err("文字超出 38 × 12 mm 安全区域，请缩小字号或减少偏移".into());
        }
        // The complete facade is installed with its raw XY axes rotated 180 degrees.
        p.x = -p.x;
        p.y = -p.y;
    }
    Ok(CrossSection::new(out.paths).to_polygons())
}
/// Fraction lost by a circular opening: detects narrow strokes, but also sharp tips.
/// This is a planar geometry estimate, not an exact minimum width or sliced toolpath.
fn narrow_area_fraction(section: &CrossSection, width: f64) -> f64 {
    let area = section.area().abs();
    if area < 1e-9 {
        return 0.;
    }
    let opened = section.offset(-width * 0.5).offset(width * 0.5);
    ((area - opened.area().abs()) / area).clamp(0., 1.)
}

fn stroke_report(config: &Config) -> Result<String, String> {
    config.validate()?;
    let section = CrossSection::new(text_polygons(config)?);
    let fine = narrow_area_fraction(&section, 0.4) * 100.;
    let recommended = narrow_area_fraction(&section, 0.6) * 100.;
    Ok(format!(
        "笔画几何估算：0.4 mm 检查损失面积 {fine:.1}%，0.6 mm 检查损失面积 {recommended:.1}%。\n数值越高，细笔画或尖角越多；建议放大字号或选择粗体。按 0.4 mm 喷嘴参考，非精确最小笔画测量；未检查字内间隙，仍需检查切片路径。"
    ))
}

#[derive(Default)]
struct StrokeUiState {
    config: Option<Config>,
    changed_at: f64,
    running: bool,
    result: Option<Result<String, String>>,
}

/// One debounced background job per editor; stale results never replace current input.
pub(crate) fn stroke_check_ui(ui: &mut eframe::egui::Ui, config: &Config) {
    use eframe::egui;
    use std::sync::Mutex;
    let id = ui.id().with("stroke-width-check");
    let shared = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<StrokeUiState>>>(id)
            .clone()
    });
    let now = ui.input(|i| i.time);
    let mut state = shared.lock().unwrap();
    if state.config.as_ref() != Some(config) {
        state.config = Some(config.clone());
        state.changed_at = now;
        state.result = None;
    }
    if !config.active() {
        return;
    }
    if let Some(result) = &state.result {
        match result {
            Ok(report) => {
                ui.small(report);
            }
            Err(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("笔画检查失败：{error}"));
            }
        }
        return;
    }
    ui.small("笔画宽度检查中…");
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
    if state.running || now - state.changed_at < 0.4 {
        return;
    }
    state.running = true;
    let config = config.clone();
    let ctx = ui.ctx().clone();
    drop(state);
    std::thread::spawn(move || {
        let result = stroke_report(&config);
        let mut state = shared.lock().unwrap();
        state.running = false;
        if state.config.as_ref() == Some(&config) {
            state.result = Some(result);
        }
        ctx.request_repaint();
    });
}

pub(crate) fn solid(mesh: &Mesh) -> Result<Manifold, String> {
    let mut input = MeshGL64 {
        num_prop: 3,
        ..Default::default()
    };
    let mut ids = HashMap::new();
    for v in &mesh.vertices {
        let [x, y, z] = v.position;
        let p = [x, -z, y];
        let key = p.map(|v| if v == 0. { 0 } else { v.to_bits() });
        let id = *ids.entry(key).or_insert_with(|| {
            let id = (input.vert_properties.len() / 3) as u64;
            input.vert_properties.extend(p.map(f64::from));
            id
        });
        input.tri_verts.push(id);
    }
    let result = Manifold::from_mesh_gl64(&input);
    if result.status() != Error::NoError {
        return Err(format!("标签网格不是有效闭合实体：{:?}", result.status()));
    }
    Ok(result)
}
pub fn generate(source: &Arc<Mesh>, config: &Config) -> Result<Arc<Mesh>, String> {
    config.validate()?;
    if !config.active() {
        return Ok(source.clone());
    }
    if (source.size - Vec3::new(47.85742, 20.626831, 2.00004))
        .abs()
        .max_element()
        > 0.05
    {
        return Err("Label.stl 尺寸与已验证标签不同，停止生成文字".into());
    }
    let polygons = text_polygons(config)?;
    if polygons.is_empty() {
        return Err("文字轮廓为空".into());
    }
    apply_text(source, config, &polygons, [0., 0.], source.max.y as f64)
}
/// Decorate an existing mesh without recentering or changing its mounting coordinates.
pub(crate) fn apply_text(
    source: &Arc<Mesh>,
    config: &Config,
    polygons: &Vec<Vec<D2>>,
    offset: [f64; 2],
    top: f64,
) -> Result<Arc<Mesh>, String> {
    config.validate()?;
    let (z, h) = match config.style {
        Style::Raised => (top - 0.12, config.depth as f64 + 0.12),
        Style::Engraved => (top - config.depth as f64, config.depth as f64 + 0.12),
    };
    let text = Manifold::extrude(polygons, h, 0, 0., D2::new(1., 1.))
        .translate(D3::new(offset[0], offset[1], z));
    if text.status() != Error::NoError {
        return Err(format!("文字实体生成失败：{:?}", text.status()));
    }
    let base = solid(source)?;
    let result = match config.style {
        Style::Raised => base.union(&text),
        Style::Engraved => base.difference(&text),
    };
    if result.status() != Error::NoError || result.volume() <= 0. {
        return Err(format!("标签布尔运算失败：{:?}", result.status()));
    }
    let delta = result.volume() - base.volume();
    if (config.style == Style::Raised && delta <= 0.)
        || (config.style == Style::Engraved && delta >= 0.)
    {
        return Err("文字没有与标签正面正确相交".into());
    }
    let output = result.get_mesh_gl64(0);
    let mut vertices = Vec::with_capacity(output.tri_verts.len());
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for t in output.tri_verts.chunks_exact(3) {
        let points: Vec<Vec3> = t
            .iter()
            .map(|i| {
                let i = *i as usize * output.num_prop as usize;
                Vec3::new(
                    output.vert_properties[i] as f32,
                    output.vert_properties[i + 2] as f32,
                    -output.vert_properties[i + 1] as f32,
                )
            })
            .collect();
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        let center = (points[0] + points[1] + points[2]) / 3.;
        let lettering = (center.x - offset[0] as f32).abs() <= 19.01
            && (center.z + offset[1] as f32).abs() <= 6.01
            && match config.style {
                Style::Raised => {
                    center.y > top as f32 + 0.001 && center.y <= top as f32 + config.depth + 0.001
                }
                Style::Engraved => {
                    (center.y - (top as f32 - config.depth)).abs() < 0.001 && normal.y > 0.9
                }
            };
        for p in points {
            min = min.min(p);
            max = max.max(p);
            vertices.push(Vertex {
                position: p.to_array(),
                normal: normal.to_array(),
                shade: if lettering { 0.45 } else { 1. },
            });
        }
    }
    let extent = max - min;
    Ok(Arc::new(Mesh {
        vertices,
        min,
        max,
        size: Vec3::new(extent.x, extent.z, extent.y),
    }))
}
/// Split the finished solid into disjoint material volumes without changing its shape.
/// Engraving colors a 0.4 mm floor below the recess; it never fills the recess.
pub(crate) fn color_regions(
    mesh: &Arc<Mesh>,
    config: &Config,
    offset: [f64; 2],
    top: f64,
) -> Result<Vec<Arc<Mesh>>, String> {
    config.validate()?;
    let polygons = text_polygons(config)?;
    let floor = match config.style {
        Style::Raised => top,
        Style::Engraved => top - config.depth as f64,
    };
    let mask = Manifold::extrude(
        &polygons,
        match config.style {
            Style::Raised => config.depth as f64 + 0.5,
            Style::Engraved => 0.40001,
        },
        0,
        0.,
        D2::new(1., 1.),
    )
    .translate(D3::new(offset[0], offset[1], floor - 0.4));
    let whole = solid(mesh)?;
    let lettering = whole.intersection(&mask);
    let body = whole.difference(&mask);
    if lettering.status() != Error::NoError
        || body.status() != Error::NoError
        || lettering.volume() <= 0.
        || body.volume() <= 0.
    {
        return Err("文字分色实体生成失败".into());
    }
    if (lettering.volume() + body.volume() - whole.volume()).abs()
        > 0.01_f64.max(whole.volume() * 1e-6)
    {
        return Err("文字分色改变了模型体积".into());
    }
    [body, lettering]
        .iter()
        .map(|solid| {
            let data = solid.get_mesh_gl64(0);
            let mut vertices = Vec::new();
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for tri in data.tri_verts.chunks_exact(3) {
                let pts: Vec<_> = tri
                    .iter()
                    .map(|i| {
                        let k = *i as usize * data.num_prop as usize;
                        Vec3::new(
                            data.vert_properties[k] as f32,
                            data.vert_properties[k + 2] as f32,
                            -data.vert_properties[k + 1] as f32,
                        )
                    })
                    .collect();
                let normal = (pts[1] - pts[0])
                    .cross(pts[2] - pts[0])
                    .normalize_or_zero()
                    .to_array();
                for v in pts {
                    min = min.min(v);
                    max = max.max(v);
                    vertices.push(Vertex {
                        position: v.to_array(),
                        normal,
                        shade: 1.,
                    });
                }
            }
            let d = max - min;
            Ok(Arc::new(Mesh {
                vertices,
                min,
                max,
                size: Vec3::new(d.x, d.z, d.y),
            }))
        })
        .collect()
}
pub fn apply(scene: &mut Scene, config: &Config) -> Result<(), String> {
    if !config.active() {
        return Ok(());
    }
    let mut generated = None;
    for part in scene.parts.iter_mut().filter(|p| p.pose.path == PATH) {
        let mesh = if let Some(mesh) = &generated {
            Arc::clone(mesh)
        } else {
            let m = generate(&part.mesh, config)?;
            generated = Some(m.clone());
            m
        };
        part.mesh = mesh;
    }
    Ok(())
}
pub fn write_stl(mesh: &Mesh, path: &Path) -> Result<(), String> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    out.write_all(&[0; 80]).map_err(|e| e.to_string())?;
    out.write_all(&((mesh.vertices.len() / 3) as u32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    for tri in mesh.vertices.chunks_exact(3) {
        let n = tri[0].normal;
        for f in [n[0], -n[2], n[1]] {
            out.write_all(&f.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        for v in tri {
            let [x, y, z] = v.position;
            for f in [x, -z, y - mesh.min.y] {
                out.write_all(&f.to_le_bytes()).map_err(|e| e.to_string())?;
            }
        }
        out.write_all(&[0; 2]).map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stroke_opening_detects_thin_strokes_and_preserves_holes() {
        fn rectangle(w: f64, h: f64) -> CrossSection {
            CrossSection::new(vec![vec![
                D2::new(0., 0.),
                D2::new(w, 0.),
                D2::new(w, h),
                D2::new(0., h),
            ]])
        }
        assert!(narrow_area_fraction(&rectangle(0.3, 5.), 0.4) > 0.99);
        assert!(narrow_area_fraction(&rectangle(2., 5.), 0.4) < 0.05);
        let ring = CrossSection::new(vec![
            vec![
                D2::new(0., 0.),
                D2::new(5., 0.),
                D2::new(5., 5.),
                D2::new(0., 5.),
            ],
            vec![
                D2::new(0.2, 0.2),
                D2::new(0.2, 4.8),
                D2::new(4.8, 4.8),
                D2::new(4.8, 0.2),
            ],
        ]);
        assert!(narrow_area_fraction(&ring, 0.6) > 0.99);
    }

    #[test]
    fn label_limits_and_empty_text() {
        assert!(
            Config {
                enabled: true,
                depth: 1.8,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Config {
                enabled: true,
                text: "x".repeat(49),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            !Config {
                enabled: true,
                text: String::new(),
                ..Default::default()
            }
            .active()
        );
        assert!(
            text_polygons(&Config {
                enabled: true,
                text: "too wide for this label".into(),
                size: 12.,
                ..Default::default()
            })
            .is_err()
        );
    }
    #[test]
    fn two_line_layout_spacing_centering_and_limits() {
        let config = Config {
            enabled: true,
            text: "收纳\nA8".into(),
            ..Default::default()
        };
        config.validate().unwrap();
        let polygons = text_polygons(&config).unwrap();
        let mut rows: [Vec<&D2>; 2] = [Vec::new(), Vec::new()];
        for polygon in &polygons {
            let lo = polygon.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
            let hi = polygon
                .iter()
                .map(|p| p.y)
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(lo > 0. || hi < 0., "a glyph must not cross the line gap");
            rows[usize::from(lo > 0.)].extend(polygon.iter());
        }
        for row in &rows {
            assert!(!row.is_empty());
            let left = row.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
            let right = row.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
            assert!(
                (left + right).abs() < 0.001,
                "each line must be centered independently"
            );
        }
        let upper_edge = rows[0]
            .iter()
            .map(|p| p.y)
            .fold(f64::NEG_INFINITY, f64::max);
        let lower_edge = rows[1].iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        assert!(lower_edge - upper_edge > 1., "two lines need a clear gap");
        assert!(
            polygons
                .iter()
                .flatten()
                .all(|p| p.x.abs() <= 19. && p.y.abs() <= 6.)
        );
        let crlf = Config {
            text: "收纳\r\nA8".into(),
            ..config.clone()
        };
        let other = text_polygons(&crlf).unwrap();
        assert_eq!(format!("{polygons:?}"), format!("{other:?}"));
        assert!(
            Config {
                text: "一\n二\n三".into(),
                ..config.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            text_polygons(&Config {
                size: 12.,
                ..config
            })
            .is_err()
        );
    }
    #[test]
    fn real_label_boolean_and_original_pose() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let file = Path::new(&root).join(PATH);
        let bytes = std::fs::read(&file).unwrap();
        let mesh = Mesh::load(&file).unwrap();
        let volume = solid(&mesh).unwrap().volume();
        for (case, text) in [("single", "收纳 A8"), ("double", "收纳\nA8")] {
            for style in [Style::Raised, Style::Engraved] {
                let config = Config {
                    enabled: true,
                    text: text.into(),
                    style,
                    ..Default::default()
                };
                let result = generate(&mesh, &config).unwrap();
                let v = solid(&result).unwrap().volume();
                let regions = color_regions(&result, &config, [0., 0.], mesh.max.y as f64).unwrap();
                let a = solid(&regions[0]).unwrap();
                let b = solid(&regions[1]).unwrap();
                assert!(
                    (a.volume() + b.volume() - v).abs() < 0.05,
                    "volume a={} b={} original={v} style={style:?}",
                    a.volume(),
                    b.volume()
                );
                assert!(a.intersection(&b).volume().abs() < 0.001);
                let whole = solid(&result).unwrap();
                let combined = a.union(&b);
                assert!(whole.difference(&combined).volume().abs() < 0.05);
                assert!(combined.difference(&whole).volume().abs() < 0.05);
                if style == Style::Engraved {
                    assert!(regions[1].max.y <= mesh.max.y - config.depth + 0.001);
                }

                assert!(if style == Style::Raised {
                    v > volume
                } else {
                    v < volume
                });
                assert!((result.min.y - mesh.min.y).abs() < 0.001);
                assert!((result.size.x - mesh.size.x).abs() < 0.001);
                let out = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("output")
                    .join(format!("label-{case}-{style:?}.stl"));
                write_stl(&result, &out).unwrap();
                let bom = vec![crate::catalog::Bom {
                    path: PATH.into(),
                    qty: 2,
                    enabled: true,
                    group: "Mechanism".into(),
                }];
                let target = out.with_extension("3mf");
                crate::export::run_with_label(
                    Path::new(&root),
                    &bom,
                    &target,
                    crate::export::Format::ThreeMf,
                    &config,
                )
                .unwrap();
                let temp = tempfile::tempdir().unwrap();
                let stls = temp.path().join("label");
                crate::export::run_with_label(
                    Path::new(&root),
                    &bom,
                    &stls,
                    crate::export::Format::Stl,
                    &config,
                )
                .unwrap();
                for file in std::fs::read_dir(&stls).unwrap() {
                    let exported = Mesh::load(&file.unwrap().path()).unwrap();
                    assert!((solid(&exported).unwrap().volume() - v).abs() < 0.01);
                }
            }
        }
        assert_eq!(bytes, std::fs::read(file).unwrap());
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        let scene =
            Scene::load(&c.root, &c.bom(&Default::default()), "label".into(), true).unwrap();
        let p = scene.parts.iter().find(|p| p.pose.path == PATH).unwrap();
        assert!(p.pose.matrix(0.).transform_vector3(-Vec3::X).x > 0.99);
        assert!(p.pose.matrix(0.).transform_vector3(Vec3::Z).y > 0.99);
        let mut changed = scene.clone();
        apply(
            &mut changed,
            &Config {
                enabled: true,
                text: "Label".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let q = changed.parts.iter().find(|p| p.pose.path == PATH).unwrap();
        assert_eq!(p.pose.position, q.pose.position);
        assert_eq!(p.pose.rotation, q.pose.rotation);
    }
}
