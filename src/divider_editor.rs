//! Native egui top-view editor; mutations go through layout validation/history.
use crate::{
    catalog::{Catalog, Selection},
    dividers::{self, Grid, Layout, PITCH, Placement},
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
#[derive(Default)]
pub struct Editor {
    pub open: bool,
    selected: Option<usize>,
    path: String,
    vertical: bool,
    height: usize,
    add_mode: bool,
    undo: Vec<Layout>,
    redo: Vec<Layout>,
    drag: Option<Drag>,
    pub(crate) plan_add_rect: Option<Rect>,
    pub error: String,
}
struct Drag {
    index: usize,
    origin: Pos2,
    offset: Vec2,
}

fn t(zh: bool, cn: &'static str, en: &'static str) -> &'static str {
    if zh { cn } else { en }
}

impl Editor {
    pub(crate) fn close(&mut self) {
        self.open = false;
        self.cancel_gesture();
        self.plan_add_rect = None;
    }

    pub(crate) fn cancel_gesture(&mut self) {
        self.drag = None;
        self.error.clear();
        self.add_mode = false;
    }

    pub(crate) fn divider_color(selected: bool) -> Color32 {
        if selected {
            Color32::from_rgb(244, 173, 70)
        } else {
            Color32::from_rgb(66, 157, 173)
        }
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn select(&mut self, index: Option<usize>) {
        if self.selected != index {
            self.selected = index;
            self.error.clear();
            self.drag = None;
        }
    }

    pub(crate) fn placing(&self) -> bool {
        self.add_mode
    }

    pub fn reset(&mut self) {
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        self.error.clear();
        self.add_mode = false;
        self.plan_add_rect = None;
    }

    pub(crate) fn commit(
        &mut self,
        layout: &mut Layout,
        next: Layout,
        c: &Catalog,
        s: &Selection,
    ) -> bool {
        if *layout == next {
            return false;
        }
        if let Err(e) = next.validate(c, s) {
            self.error = e;
            return false;
        }
        self.undo.push(layout.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        *layout = next;
        self.error.clear();
        true
    }
    fn candidate(&self, layout: &Layout, s: &Selection) -> Layout {
        let mut l = layout.clone();
        l.enabled = true;
        l.specification = dividers::spec(s);
        l
    }
    fn placement_at(&self, g: Grid, center: [f32; 2]) -> Placement {
        let n = dividers::code(&self.path).unwrap();
        let axis = usize::from(self.vertical);
        let limits = [g.cols, g.rows];
        let mut start = center.map(|v| v.round() as i32);
        // Full-span pieces have a fixed longitudinal anchor. Clicking anywhere
        // across their span chooses only the perpendicular row/column.
        start[axis] = if n == g.full_code(self.vertical) {
            g.full_start(self.vertical)
        } else {
            (center[axis] - (n - 1) as f32 / 2.)
                .round()
                .clamp(0., (limits[axis] - n).max(0) as f32) as i32
        };
        Placement {
            path: self.path.clone(),
            start,
            vertical: self.vertical,
        }
    }
    fn add(
        &mut self,
        layout: &mut Layout,
        c: &Catalog,
        s: &Selection,
        g: Grid,
        at: Option<[f32; 2]>,
    ) -> bool {
        if self.path.is_empty() {
            return false;
        }
        let mut next = self.candidate(layout, s);
        let center = at.unwrap_or([g.cols as f32 / 2., g.rows as f32 / 2.]);
        let mut p = self.placement_at(g, center);
        let start = p.start;
        if at.is_none() {
            let mut found = None;
            // Try the center first, then remaining legal grid anchors.
            for start in std::iter::once(start)
                .chain((0..g.rows).flat_map(|y| (0..g.cols).map(move |x| [x, y])))
            {
                p.start = start;
                let mut trial = next.clone();
                trial.items.push(p.clone());
                if trial.validate(c, s).is_ok() {
                    found = Some(p.clone());
                    break;
                }
            }
            let Some(placed) = found else {
                self.error = "没有足够空间放置该隔板，请选择较短型号或移走已有隔板".into();
                return false;
            };
            p = placed;
        }
        next.items.push(p);
        let i = next.items.len() - 1;
        if self.commit(layout, next, c, s) {
            self.selected = Some(i);
            true
        } else {
            false
        }
    }
    fn history(&mut self, layout: &mut Layout, redo: bool) -> bool {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let Some(next) = from.pop() else {
            return false;
        };
        to.push(std::mem::replace(layout, next));
        self.selected = None;
        self.drag = None;
        self.error.clear();
        true
    }
    pub(crate) fn placement_candidate(
        &self,
        layout: &Layout,
        s: &Selection,
        grid: Grid,
        center: [f32; 2],
    ) -> Option<Layout> {
        dividers::code(&self.path)?;
        let mut next = self.candidate(layout, s);
        next.items.push(self.placement_at(grid, center));
        Some(next)
    }

    pub(crate) fn place(
        &mut self,
        layout: &mut Layout,
        c: &Catalog,
        s: &Selection,
        grid: Grid,
        center: [f32; 2],
    ) -> bool {
        self.add(layout, c, s, grid, Some(center))
    }

    pub(crate) fn drag_position(
        layout: &Layout,
        grid: Grid,
        index: usize,
        delta_cells: Vec2,
    ) -> Option<Layout> {
        let old = layout.items.get(index)?;
        let mut next = layout.clone();
        let mut delta = [delta_cells.x.round() as i32, delta_cells.y.round() as i32];
        if grid.full(old) {
            delta[usize::from(old.vertical)] = 0;
        }
        next.items[index].start = std::array::from_fn(|axis| old.start[axis] + delta[axis]);
        Some(next)
    }

    pub(crate) fn drag_preview(&self, layout: &Layout, grid: Grid) -> Option<(usize, Layout)> {
        let drag = self.drag.as_ref()?;
        Some((
            drag.index,
            Self::drag_position(layout, grid, drag.index, drag.offset)?,
        ))
    }

    pub(crate) fn panel(
        &mut self,
        ui: &mut egui::Ui,
        c: &Catalog,
        s: &Selection,
        layout: &mut Layout,
        zh: bool,
    ) -> Rect {
        let grid = match Grid::new(s) {
            Ok(grid) => grid,
            Err(error) => {
                self.plan_add_rect = None;
                ui.label(error);
                return Rect::NOTHING;
            }
        };
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_gesture();
        }
        let heights = ["20 mm", "34.5 mm", "82.5 mm"];
        let folders = ["20MM", "34.5MM", "82.5MM"];
        let max_height = if s.height == "1F" { 1 } else { 2 };
        if self.height > max_height {
            self.height = 0;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(t(zh, "高度", "Height"));
            egui::ComboBox::from_id_salt("divider-height")
                .width(62.)
                .selected_text(heights[self.height])
                .show_ui(ui, |ui| {
                    for (height, &name) in heights.iter().enumerate().take(max_height + 1) {
                        ui.selectable_value(&mut self.height, height, name);
                    }
                });
            ui.selectable_value(&mut self.vertical, false, t(zh, "横向", "Across"));
            ui.selectable_value(&mut self.vertical, true, t(zh, "纵向", "Lengthwise"));
        });
        let folder = folders[self.height];
        let span = grid.full_code(self.vertical);
        let slots = if self.vertical { grid.rows } else { grid.cols };
        let mut choices: Vec<_> = c
            .accessories(s)
            .into_iter()
            .filter_map(|entry| {
                let code = dividers::code(&entry.path)?;
                (entry.path.contains("/Grid Dividers/")
                    && entry.path.contains(folder)
                    && code <= slots)
                    .then_some((code, entry.path))
            })
            .collect();
        choices.sort_by_key(|(code, _)| *code);
        let full_path = choices
            .iter()
            .find(|(code, _)| *code == span)
            .map(|(_, path)| path.as_str());
        if !choices.iter().any(|(_, path)| path == &self.path) {
            self.path = full_path
                .or_else(|| choices.first().map(|(_, path)| path.as_str()))
                .unwrap_or_default()
                .into();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(t(zh, "型号", "Model"));
            egui::ComboBox::from_id_salt("divider-length")
                .width(104.)
                .selected_text(
                    self.path
                        .rsplit('/')
                        .next()
                        .filter(|name| !name.is_empty())
                        .unwrap_or(t(zh, "无可用型号", "No models")),
                )
                .show_ui(ui, |ui| {
                    for (code, path) in &choices {
                        let name = path.rsplit('/').next().unwrap_or(path);
                        ui.selectable_value(
                            &mut self.path,
                            path.clone(),
                            format!("{name} · {:.1} mm", *code as f32 * PITCH),
                        );
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            self.presets(ui, layout, c, s, full_path.map(|path| (grid, path)), zh);
            let count = layout.items.len();
            ui.weak(if zh {
                format!("{count} 块隔板")
            } else {
                format!("{count} dividers")
            });
        });
        let plan_rect = self.canvas(ui, layout, c, s, grid, zh);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.path.is_empty(),
                    crate::ui_style::primary_button(t(zh, "添加", "Add")),
                )
                .clicked()
            {
                self.add(layout, c, s, grid, None);
            }
            let place = ui
                .add_enabled(
                    !self.path.is_empty(),
                    egui::Button::selectable(self.add_mode, t(zh, "点击放置", "Click to place")),
                )
                .on_hover_text(t(
                    zh,
                    "点击底孔连续放置，按 Esc 或右键退出",
                    "Click holes to place dividers. Press Esc or right-click to exit.",
                ));
            self.plan_add_rect = Some(place.rect);
            if place.clicked() {
                self.add_mode = !self.add_mode;
                self.drag = None;
                self.error.clear();
            }
            if ui
                .add_enabled(
                    !self.undo.is_empty(),
                    egui::Button::new(t(zh, "撤销", "Undo")),
                )
                .clicked()
            {
                self.history(layout, false);
            }
            if ui
                .add_enabled(
                    !self.redo.is_empty(),
                    egui::Button::new(t(zh, "重做", "Redo")),
                )
                .clicked()
            {
                self.history(layout, true);
            }
        });
        self.properties(ui, layout, c, s, grid, zh);
        if !self.error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.error);
        }
        ui.collapsing(t(zh, "操作说明", "How to edit"), |ui| {
            ui.small(t(
                zh,
                "点击隔板选中，拖动可沿 10.5 mm 孔距移动。贯穿整宽或整深的隔板自动配端件，每块隔板配两个销。简图标注底部净距。",
                "Click a divider to select it and drag it in 10.5 mm steps. Full spans include end pieces, and each divider includes two pins. Plan labels show gaps between divider bases.",
            ));
        });
        plan_rect
    }

    fn presets(
        &mut self,
        ui: &mut egui::Ui,
        layout: &mut Layout,
        c: &Catalog,
        s: &Selection,
        full_span: Option<(Grid, &str)>,
        zh: bool,
    ) {
        ui.menu_button(t(zh, "预设", "Presets"), |ui| {
            if ui
                .add_enabled(
                    !layout.items.is_empty(),
                    egui::Button::new(t(zh, "清空", "Clear")),
                )
                .clicked()
            {
                let mut next = self.candidate(layout, s);
                next.items.clear();
                if self.commit(layout, next, c, s) {
                    self.select(None);
                }
                ui.close();
            }
            let Some((grid, path)) = full_span else {
                return;
            };
            for count in [2, 3] {
                let label = if zh {
                    format!("{count} 等分")
                } else {
                    format!("Split into {count}")
                };
                if ui.button(label).clicked() {
                    let mut next = self.candidate(layout, s);
                    let axis = usize::from(self.vertical);
                    let cross = if self.vertical { grid.cols } else { grid.rows };
                    next.items = (1..count)
                        .map(|k| {
                            let mut start = [0; 2];
                            start[axis] = grid.full_start(self.vertical);
                            start[1 - axis] =
                                ((cross - 1) as f32 * k as f32 / count as f32).round() as i32;
                            Placement {
                                path: path.to_owned(),
                                start,
                                vertical: self.vertical,
                            }
                        })
                        .collect();
                    if self.commit(layout, next, c, s) {
                        self.select(None);
                    }
                    ui.close();
                }
            }
        });
    }

    fn properties(
        &mut self,
        ui: &mut egui::Ui,
        layout: &mut Layout,
        c: &Catalog,
        s: &Selection,
        grid: Grid,
        zh: bool,
    ) {
        let Some(i) = self.selected.filter(|&i| i < layout.items.len()) else {
            return;
        };
        ui.separator();
        let divider = layout.items[i].clone();
        ui.strong(format!(
            "{} {} · {}",
            t(zh, "隔板", "Divider"),
            i + 1,
            divider.path.rsplit('/').next().unwrap_or(&divider.path)
        ));
        let mut next = layout.clone();
        let axis = usize::from(divider.vertical);
        let full = grid.full(&divider);
        let n = dividers::code(&divider.path).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            for (a, name, limit) in [
                (0, t(zh, "列", "Column"), grid.cols),
                (1, t(zh, "行", "Row"), grid.rows),
            ] {
                let mut value = divider.start[a] + 1;
                let label = ui.label(name);
                if ui
                    .add_enabled(
                        !full || a != axis,
                        egui::DragValue::new(&mut value).range(1..=limit),
                    )
                    .labelled_by(label.id)
                    .changed()
                {
                    next.items[i].start[a] = value - 1;
                }
            }
        });
        self.commit(layout, next, c, s);
        ui.horizontal_wrapped(|ui| {
            if ui.button(t(zh, "复制", "Duplicate")).clicked() {
                self.height = match dividers::height(&divider.path) {
                    82.5 => 2,
                    34.5 => 1,
                    _ => 0,
                };
                self.vertical = divider.vertical;
                self.path.clone_from(&divider.path);
                self.add(layout, c, s, grid, None);
            }
            if ui.button(t(zh, "旋转 90°", "Rotate 90°")).clicked() {
                let mut next = layout.clone();
                let mut center = divider.start.map(|value| value as f32);
                center[axis] += (n - 1) as f32 / 2.;
                let vertical = !divider.vertical;
                let new_axis = usize::from(vertical);
                let mut start = center.map(|value| value.round() as i32);
                start[new_axis] = if n == grid.full_code(vertical) {
                    grid.full_start(vertical)
                } else {
                    (center[new_axis] - (n - 1) as f32 / 2.).round() as i32
                };
                next.items[i] = Placement {
                    start,
                    vertical,
                    ..divider.clone()
                };
                self.commit(layout, next, c, s);
            }
            if ui
                .add_enabled(
                    !self.path.is_empty() && self.path != divider.path,
                    egui::Button::new(t(zh, "替换型号", "Replace model")),
                )
                .clicked()
            {
                let mut next = layout.clone();
                let p = &mut next.items[i];
                p.path.clone_from(&self.path);
                if dividers::code(&p.path) == Some(grid.full_code(p.vertical)) {
                    p.start[axis] = grid.full_start(p.vertical);
                }
                self.commit(layout, next, c, s);
            }
            if ui
                .add(crate::ui_style::danger_button(t(zh, "删除", "Delete")))
                .clicked()
            {
                let mut next = layout.clone();
                next.items.remove(i);
                if self.commit(layout, next, c, s) {
                    self.select(None);
                }
            }
        });
    }

    fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        layout: &mut Layout,
        c: &Catalog,
        s: &Selection,
        grid: Grid,
        zh: bool,
    ) -> Rect {
        let size = Vec2::new(grid.right - grid.left, grid.front - grid.rear);
        let width = ui.available_width().max(1.);
        let scale = (width / size.x).min(245. / size.y);
        let (space, response) = ui.allocate_exact_size(
            Vec2::new(width, size.y * scale + 34.),
            egui::Sense::click_and_drag(),
        );
        let rect = Rect::from_min_size(
            space.center_top() + Vec2::new(-size.x * scale / 2., 16.),
            size * scale,
        );
        let floor_origin = Vec2::new(grid.left, grid.rear);
        let screen = |x: f32, y: f32| rect.min + (Vec2::new(x, y) - floor_origin) * scale;
        let cell = |pointer: Pos2| {
            let p = (pointer - rect.min) / scale + floor_origin;
            let first = grid.point([0., 0.]);
            [(p.x - first[0]) / PITCH, (p.y - first[1]) / PITCH]
        };
        let placement_rect = |p: &Placement| {
            let [left, rear, right, front] = grid.bounds(p);
            Rect::from_min_max(screen(left, rear), screen(right, front))
        };
        let hit = |layout: &Layout, pos: Pos2| {
            rect.contains(pos)
                .then(|| {
                    layout
                        .items
                        .iter()
                        .rposition(|p| placement_rect(p).contains(pos))
                })
                .flatten()
        };

        if self.add_mode && response.secondary_clicked() {
            self.cancel_gesture();
        }
        if !self.add_mode && response.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin());
            let index = origin.and_then(|pos| hit(layout, pos));
            self.select(index);
            if let Some((index, origin)) = index.zip(origin) {
                self.drag = Some(Drag {
                    index,
                    origin,
                    offset: Vec2::ZERO,
                });
            }
        }
        if let Some(drag) = &mut self.drag
            && let Some(pos) = response.interact_pointer_pos()
        {
            drag.offset = (pos - drag.origin) / (scale * PITCH);
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(pointer) = response.interact_pointer_pos()
        {
            if self.add_mode {
                if rect.contains(pointer) {
                    self.place(layout, c, s, grid, cell(pointer));
                }
            } else {
                self.select(hit(layout, pointer));
            }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) {
            if let Some((_, next)) = self.drag_preview(layout, grid) {
                self.error.clear();
                self.commit(layout, next, c, s);
            }
            self.drag = None;
        }

        let paint = ui.painter_at(rect);
        paint.rect_filled(rect, 5., Color32::from_rgb(32, 43, 52));
        paint.rect_stroke(
            rect,
            5.,
            Stroke::new(1., Color32::GRAY),
            egui::StrokeKind::Inside,
        );
        let caption = ui.visuals().weak_text_color();
        let font = egui::FontId::proportional(11.);
        ui.painter().text(
            rect.center_top() - Vec2::new(0., 8.),
            egui::Align2::CENTER_CENTER,
            format!("{} · {:.1} mm", t(zh, "后部", "Rear"), size.x),
            font.clone(),
            caption,
        );
        ui.painter().text(
            rect.center_bottom() + Vec2::new(0., 9.),
            egui::Align2::CENTER_CENTER,
            format!("{} · {:.1} mm", t(zh, "前部", "Front"), size.y),
            font,
            caption,
        );
        for y in 0..grid.rows {
            for x in 0..grid.cols {
                let [x, y] = grid.point([x as f32, y as f32]);
                paint.circle_filled(screen(x, y), 1.35, Color32::from_gray(110));
            }
        }
        for (i, p) in layout.items.iter().enumerate() {
            let color = Self::divider_color(self.selected == Some(i));
            let part = placement_rect(p);
            paint.rect_filled(part, 2., color.gamma_multiply(0.28));
            paint.rect_stroke(part, 2., Stroke::new(1.5, color), egui::StrokeKind::Inside);
            let [x, y] = grid.center(p);
            let half = dividers::code(&p.path).unwrap_or_default() as f32 * PITCH / 2.;
            let line = if p.vertical {
                [screen(x, y - half), screen(x, y + half)]
            } else {
                [screen(x - half, y), screen(x + half, y)]
            };
            paint.line_segment(line, Stroke::new(2.5, color));
        }
        if !layout.items.is_empty()
            && layout
                .items
                .iter()
                .all(|p| grid.full(p) && p.vertical == layout.items[0].vertical)
        {
            Self::paint_clearances(&paint, grid, layout, screen);
        }

        let preview = if self.add_mode {
            response
                .hover_pos()
                .filter(|&pos| rect.contains(pos))
                .and_then(|pos| self.placement_candidate(layout, s, grid, cell(pos)))
                .map(|next| (next.items.len() - 1, next))
        } else {
            self.drag_preview(layout, grid)
        };
        if let Some((i, next)) = preview {
            let valid = next.validate(c, s);
            let color = if valid.is_ok() {
                Color32::LIGHT_GREEN
            } else {
                Color32::LIGHT_RED
            };
            let part = placement_rect(&next.items[i]);
            paint.rect_filled(part, 2., color.gamma_multiply(0.24));
            paint.rect_stroke(part, 2., Stroke::new(2., color), egui::StrokeKind::Inside);
            if let Err(reason) = valid {
                response.on_hover_text(reason);
            }
        }
        rect
    }

    fn paint_clearances(
        paint: &egui::Painter,
        grid: Grid,
        layout: &Layout,
        screen: impl Fn(f32, f32) -> Pos2,
    ) {
        let vertical = layout.items[0].vertical;
        let axis = usize::from(!vertical);
        let mut spans: Vec<_> = layout
            .items
            .iter()
            .map(|p| {
                let b = grid.bounds(p);
                (b[axis], b[axis + 2])
            })
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut from, end) = if vertical {
            (grid.left, grid.right)
        } else {
            (grid.rear, grid.front)
        };
        for (lo, hi) in spans.into_iter().chain(std::iter::once((end, end))) {
            let gap = lo - from;
            let center = (from + lo) / 2.;
            let point = if vertical {
                screen(center, grid.rear + 8.)
            } else {
                screen(grid.left + 15., center)
            };
            paint.text(
                point,
                egui::Align2::CENTER_CENTER,
                format!("{gap:.1}"),
                egui::FontId::proportional(10.),
                Color32::LIGHT_GREEN,
            );
            from = hi;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn actual_pointer_clicks_match_preview_without_moving_canvas() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let c = Catalog::scan(root.into()).unwrap();
        let s = Selection::default();
        let g = Grid::new(&s).unwrap();
        let ctx = egui::Context::default();
        let mut style = (*ctx.style()).clone();
        style.animation_time = 0.;
        ctx.set_style(style);
        let mut editor = Editor {
            open: true,
            add_mode: true,
            path: "01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/10-S, x3.stl".into(),
            ..Default::default()
        };
        let mut layout = Layout::default();
        let mut time = 0.;
        let mut frame = |events: Vec<egui::Event>, editor: &mut Editor, layout: &mut Layout| {
            time += 1. / 60.;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500., 1100.))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        editor.panel(ui, &c, &s, layout, true);
                    });
                },
            );
            output
                .shapes
                .into_iter()
                .find_map(|shape| match shape.shape {
                    egui::Shape::Rect(r) if r.fill == Color32::from_rgb(32, 43, 52) => Some(r.rect),
                    _ => None,
                })
        };
        let mut canvas = frame(vec![], &mut editor, &mut layout);
        for _ in 0..5 {
            canvas = frame(vec![], &mut editor, &mut layout);
        }
        let canvas = canvas.expect("editor canvas after sizing frames");
        for (x, y, count) in [(0., 2., 1), (11., 6., 2), (5., 6., 2)] {
            let raw = g.point([x, y]);
            let p = canvas.min
                + Vec2::new(
                    (raw[0] - g.left) / (g.right - g.left) * canvas.width(),
                    (raw[1] - g.rear) / (g.front - g.rear) * canvas.height(),
                );
            frame(vec![egui::Event::PointerMoved(p)], &mut editor, &mut layout);
            for pressed in [true, false] {
                frame(
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                    &mut editor,
                    &mut layout,
                );
            }
            let after = frame(vec![], &mut editor, &mut layout).expect("visible canvas");
            assert_eq!(layout.items.len(), count);
            assert!(
                (after.min - canvas.min).length() < 0.1,
                "canvas must not jump after selecting the new divider"
            );
            assert!((after.size() - canvas.size()).length() < 0.1);
        }
        assert_eq!(layout.items[0].start, [1, 2]);
        assert_eq!(layout.items[1].start, [1, 6]);

        editor.cancel_gesture();
        let point = |row| {
            let raw = g.point([5., row]);
            canvas.min
                + Vec2::new(
                    (raw[0] - g.left) / (g.right - g.left) * canvas.width(),
                    (raw[1] - g.rear) / (g.front - g.rear) * canvas.height(),
                )
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let previous = layout.clone();
        let undo_count = editor.undo.len();
        let start = point(2.);
        for (row, valid) in [(4., false), (3., true)] {
            let end = point(row);
            frame(
                vec![egui::Event::PointerMoved(start), button(start, true)],
                &mut editor,
                &mut layout,
            );
            frame(
                vec![egui::Event::PointerMoved(end)],
                &mut editor,
                &mut layout,
            );
            assert_eq!(layout, previous, "drag previews must not change the layout");
            frame(vec![button(end, false)], &mut editor, &mut layout);
            if valid {
                assert_eq!(layout.items[0].start, [1, 3]);
                assert_eq!(editor.undo.len(), undo_count + 1);
            } else {
                assert_eq!(layout, previous);
                assert_eq!(editor.undo.len(), undo_count);
            }
        }
    }
    #[test]
    fn click_anchors_full_spans_and_rounds_short_centers_once() {
        let g = Grid::new(&Selection::default()).unwrap();
        let mut editor = Editor {
            path: "10-S, x3.stl".into(),
            ..Default::default()
        };
        for x in [-0.4, 0., 2., 5.5, 9., 11.4] {
            assert_eq!(editor.placement_at(g, [x, 5.2]).start, [1, 5]);
        }
        editor.vertical = true;
        for y in [0., 3., 6., 11.] {
            assert_eq!(editor.placement_at(g, [6.2, y]).start, [6, 1]);
        }
        editor.vertical = false;
        editor.path = "6.stl".into();
        assert_eq!(editor.placement_at(g, [4.1, 5.]).start, [2, 5]);
        assert_eq!(editor.placement_at(g, [4.9, 5.]).start, [2, 5]);
        assert_eq!(editor.placement_at(g, [0., 5.]).start, [0, 5]);
        assert_eq!(editor.placement_at(g, [11., 5.]).start, [6, 5]);
        editor.path = "5.stl".into();
        assert_eq!(editor.placement_at(g, [4.1, 5.]).start, [2, 5]);
    }
    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn repeated_clicks_keep_the_row_and_reject_overlap_transactionally() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let c = Catalog::scan(root.into()).unwrap();
        let s = Selection::default();
        let g = Grid::new(&s).unwrap();
        let mut editor = Editor {
            path: "01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/10-S, x3.stl".into(),
            ..Default::default()
        };
        let mut layout = Layout::default();
        assert!(editor.add(&mut layout, &c, &s, g, Some([0., 2.])));
        assert!(editor.add(&mut layout, &c, &s, g, Some([11., 6.])));
        assert_eq!(layout.items[0].start, [1, 2]);
        assert_eq!(layout.items[1].start, [1, 6]);
        let before = layout.clone();
        let history = editor.undo.len();
        assert!(!editor.add(&mut layout, &c, &s, g, Some([5., 6.])));
        assert_eq!(layout, before);
        assert_eq!(editor.undo.len(), history);
    }
    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn edits_are_transactional_and_history_branches() {
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let c = Catalog::scan(root.into()).unwrap();
        let s = Selection::default();
        let g = Grid::new(&s).unwrap();
        let mut editor = Editor {
            path: "01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/10-S, x3.stl".into(),
            ..Default::default()
        };
        let mut layout = Layout::default();
        assert!(editor.add(&mut layout, &c, &s, g, None));
        assert_eq!(layout.items[0].start, [1, 6]);
        let first = layout.clone();
        assert!(editor.add(&mut layout, &c, &s, g, None));
        let second = layout.clone();
        assert_eq!(layout.items.len(), 2);
        let mut invalid = layout.clone();
        invalid.items[0].start = [-1, 0];
        assert!(!editor.commit(&mut layout, invalid, &c, &s));
        assert_eq!(layout, second);
        assert!(editor.history(&mut layout, false));
        assert_eq!(layout, first);
        assert!(editor.history(&mut layout, true));
        assert_eq!(layout, second);
        assert!(editor.history(&mut layout, false));
        let mut moved = layout.clone();
        moved.items[0].start[1] += 1;
        assert!(editor.commit(&mut layout, moved, &c, &s));
        assert!(editor.redo.is_empty());
        assert!(!editor.history(&mut layout, true));
    }
}
