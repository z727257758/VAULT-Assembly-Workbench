//! Compact label-face previews using real outlines prepared off the UI thread.
use crate::{
    gridfinity::Bin,
    label::{Config, Style},
    print_plate,
};
use eframe::egui::{self, Color32, Rect, Stroke, Vec2};
use manifold_rust::{linalg::Vec2 as D2, manifold::Manifold, types::Error};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Glyphs {
    points: Vec<[f32; 2]>,
    size: [f32; 2],
    fits: bool,
}
fn glyphs(config: &Config) -> Result<Arc<Glyphs>, String> {
    if !config.active() {
        return Ok(Arc::new(Glyphs {
            fits: true,
            ..Default::default()
        }));
    }
    config.validate()?;
    let polygons = crate::label::text_polygons(config)?;
    let fits = polygons
        .iter()
        .flatten()
        .all(|p| p.x.abs() <= 17. && p.y.abs() <= 5.);
    let solid = Manifold::extrude(&polygons, 1., 0, 0., D2::new(1., 1.));
    if solid.status() != Error::NoError {
        return Err(format!("文字预览生成失败：{:?}", solid.status()));
    }
    let raw = solid.get_mesh_gl64(0);
    let mut points = Vec::new();
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for tri in raw.tri_verts.chunks_exact(3) {
        let xyz: Vec<_> = tri
            .iter()
            .map(|i| {
                let i = *i as usize * raw.num_prop as usize;
                [
                    raw.vert_properties[i] as f32,
                    raw.vert_properties[i + 1] as f32,
                    raw.vert_properties[i + 2] as f32,
                ]
            })
            .collect();
        if xyz.iter().all(|p| p[2] > 0.5) {
            for p in xyz {
                // Undo the mounting rotation for an upright front-face preview.
                let p = [-p[0], p[1]];
                for a in 0..2 {
                    lo[a] = lo[a].min(p[a]);
                    hi[a] = hi[a].max(p[a]);
                }
                points.push(p);
            }
        }
    }
    Ok(Arc::new(Glyphs {
        points,
        size: [hi[0] - lo[0], hi[1] - lo[1]],
        fits,
    }))
}
#[derive(Default)]
struct GlyphState {
    wanted: Option<Config>,
    changed_at: f64,
    running: bool,
    result: Option<Result<Arc<Glyphs>, String>>,
}
#[derive(Default)]
pub struct Preview {
    glyph: Arc<Mutex<GlyphState>>,
}
impl Preview {
    fn request_glyphs(
        &self,
        ui: &egui::Ui,
        config: &Config,
    ) -> Option<Result<Arc<Glyphs>, String>> {
        let mut state = self.glyph.lock().unwrap();
        let now = ui.input(|i| i.time);
        if state.wanted.as_ref() != Some(config) {
            state.wanted = Some(config.clone());
            state.changed_at = now;
            state.result = None;
        }
        if let Some(result) = &state.result {
            return Some(result.clone());
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(40));
        if state.running || now - state.changed_at < 0.12 {
            return None;
        }
        state.running = true;
        let shared = self.glyph.clone();
        let config = config.clone();
        let ctx = ui.ctx().clone();
        drop(state);
        std::thread::spawn(move || {
            let result = glyphs(&config);
            let mut state = shared.lock().unwrap();
            state.running = false;
            if state.wanted.as_ref() == Some(&config) {
                state.result = Some(result);
            }
            ctx.request_repaint();
        });
        None
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        bin: Option<&Bin>,
        colors: &print_plate::Config,
        draft_text: bool,
    ) {
        ui.strong("所选标牌预览");
        let Some(bin) = bin else {
            ui.label("在左侧选择一个盒子。");
            self.glyph.lock().unwrap().wanted = None;
            return;
        };
        ui.small(format!(
            "盒子 {}{}",
            bin.id,
            if draft_text { " · 正在输入" } else { "" }
        ));
        if !bin.tab {
            ui.small("统一配置已关闭标签台。");
            return;
        }
        let size = if bin.detachable() {
            [36., 11.]
        } else {
            [38., 15.85]
        };
        let width = (ui.available_width() - 8.).clamp(40., 210.);
        let scale = width / size[0];
        let (area, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), size[1] * scale + 12.),
            egui::Sense::hover(),
        );
        let plate = Rect::from_center_size(area.center(), Vec2::new(width, size[1] * scale));
        let path = if bin.detachable() {
            bin.label_path()
        } else {
            bin.path()
        };
        let base = colors.color(&path);
        let ink = colors.slot_color(colors.text_slot(&path));
        let rgba = |c: [f32; 4]| {
            Color32::from_rgb(
                (c[0] * 255.) as u8,
                (c[1] * 255.) as u8,
                (c[2] * 255.) as u8,
            )
        };
        let painter = ui.painter().with_clip_rect(plate.intersect(ui.clip_rect()));
        painter.rect_filled(plate, 2., rgba(base));
        let safe = Rect::from_center_size(plate.center(), Vec2::new(34., 10.) * scale);
        match self.request_glyphs(ui, &bin.label) {
            Some(Ok(glyph)) => {
                painter.rect_stroke(
                    safe,
                    0.,
                    Stroke::new(
                        0.7,
                        if glyph.fits {
                            Color32::from_white_alpha(55)
                        } else {
                            Color32::LIGHT_RED
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                let relief = (bin.label.depth * scale).clamp(0.6, 2.5);
                let offset = if bin.label.style == Style::Raised {
                    Vec2::splat(relief)
                } else {
                    Vec2::new(-relief, -relief)
                };
                let shadow = if bin.label.style == Style::Raised {
                    rgba(base).gamma_multiply(0.5)
                } else {
                    Color32::from_white_alpha(90)
                };
                for (offset, color) in [(offset, shadow), (Vec2::ZERO, rgba(ink))] {
                    let mut shape = egui::Mesh::default();
                    for p in &glyph.points {
                        let i = shape.vertices.len() as u32;
                        shape.colored_vertex(
                            plate.center() + Vec2::new(p[0], p[1]) * scale + offset,
                            color,
                        );
                        shape.indices.push(i);
                    }
                    painter.add(shape);
                }
                if !glyph.fits {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "文字超出 34 × 10 mm 安全区，请调整统一字号或偏移。",
                    );
                }
                if bin.label.active() {
                    ui.small(format!(
                        "文字轮廓 {:.1} × {:.1} mm",
                        glyph.size[0], glyph.size[1]
                    ));
                } else {
                    ui.small("空白标牌（未启用文字或文本为空）");
                }
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            None => {
                ui.small("正在更新实际字体轮廓…");
            }
        }
        painter.rect_stroke(
            plate,
            2.,
            Stroke::new(1., Color32::GRAY),
            egui::StrokeKind::Inside,
        );
        let font = match bin.label.font {
            crate::label::Font::Chinese => "微软雅黑",
            crate::label::Font::ChineseBold => "微软雅黑粗体",
            crate::label::Font::Arial => "Arial",
            crate::label::Font::ArialBold => "Arial Bold",
        };
        ui.small(format!(
            "{} × {} mm · {font} · 字号 {} mm · {} {} mm",
            size[0],
            size[1],
            bin.label.size,
            if bin.label.style == Style::Raised {
                "凸字"
            } else {
                "凹字"
            },
            bin.label.depth
        ));
        ui.small("细框为文字安全区；配色跟随同类型设置。");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_font_preview_keeps_holes_and_checks_printable_bounds() {
        let c = Config {
            enabled: true,
            text: "O".into(),
            font: crate::label::Font::ArialBold,
            size: 6.,
            ..Default::default()
        };
        let g = glyphs(&c).unwrap();
        assert!(g.fits);
        assert!(g.size[0] > 0. && g.size[1] > 0.);
        // The center of capital O must remain a hole after face triangulation.
        let contains_center = g.points.chunks_exact(3).any(|t| {
            let cross = |a: [f32; 2], b: [f32; 2]| a[0] * b[1] - a[1] * b[0];
            let signs = [cross(t[0], t[1]), cross(t[1], t[2]), cross(t[2], t[0])];
            signs.iter().all(|v| *v >= 0.) || signs.iter().all(|v| *v <= 0.)
        });
        assert!(!contains_center);
        let shifted = Config {
            offset: [10., 0.],
            text: "MMMMM".into(),
            size: 4.,
            ..c
        };
        assert!(glyphs(&shifted).is_err() || !glyphs(&shifted).unwrap().fits);
    }
}
