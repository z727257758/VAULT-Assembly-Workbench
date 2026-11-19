//! Transactional pointer gestures for the Gridfinity layout canvas.
use crate::{
    catalog::Selection,
    gridfinity::{self, Bin, Layout},
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

mod gestures;
mod keyboard;
pub use gestures::{DragPreview, GridView};

#[derive(Clone)]
enum Gesture {
    Draw([i32; 2]),
    Stamp,
    Move,
    Resize([i8; 2]),
}
#[derive(Clone)]
struct Drag {
    base: Layout,
    specification: Selection,
    view: GridView,
    preview: Option<DragPreview>,
    bin: Bin,
    start: [f32; 2],
    gesture: Gesture,
}
#[derive(Clone)]
struct Candidate {
    bin: Bin,
    cell: [i32; 2],
}
impl Candidate {
    fn layout(&self, base: &Layout, s: &Selection) -> Result<Layout, String> {
        if self.cell.iter().any(|v| *v < 0) {
            return Err("盒子超出底板网格".into());
        }
        let mut bin = self.bin.clone();
        bin.cell = self.cell.map(|v| v as u32);
        let mut next = base.clone();
        next.bins.retain(|b| b.id != bin.id);
        next.bins.push(bin);
        // Keep stable ordering for saved layouts and the print list.
        next.bins.sort_by_key(|b| b.id);
        next.validate(s)?;
        Ok(next)
    }
}
impl Drag {
    fn candidate(&self, p: [f32; 2]) -> Candidate {
        let mut b = self.bin.clone();
        let mut lo = b.cell.map(|v| v as i32);
        let delta = [
            (p[0] - self.start[0]).round() as i32,
            (p[1] - self.start[1]).round() as i32,
        ];
        match self.gesture {
            Gesture::Draw(start) => {
                let end = p.map(|v| v.floor() as i32);
                lo = std::array::from_fn(|axis| start[axis].min(end[axis]));
                b.span = std::array::from_fn(|axis| start[axis].abs_diff(end[axis]) + 1);
            }
            Gesture::Stamp => {
                lo = p.map(|v| v.floor() as i32);
            }
            Gesture::Move => {
                for a in 0..2 {
                    lo[a] += delta[a];
                }
            }
            Gesture::Resize(edges) => {
                for a in 0..2 {
                    let mut end = lo[a] + b.span[a] as i32;
                    if edges[a] < 0 {
                        lo[a] = (lo[a] + delta[a]).min(end - 1);
                    } else if edges[a] > 0 {
                        end = (end + delta[a]).max(lo[a] + 1);
                    }
                    b.span[a] = (end - lo[a]) as u32;
                }
            }
        }
        Candidate { bin: b, cell: lo }
    }
}
struct InlineLabel {
    id: u32,
    text: String,
    focus: bool,
    composing: bool,
}
#[derive(Default)]
pub struct Editor {
    preview: crate::label_preview::Preview,
    inline: Option<InlineLabel>,
    shared_error: Option<String>,
    drag: Option<Drag>,
    pub error: Option<String>,
}
impl Editor {
    fn preview_bin(&self, layout: &Layout, selected: Option<u32>) -> Option<Bin> {
        let mut bin = layout.bins.iter().find(|b| Some(b.id) == selected)?.clone();
        if let Some(edit) = self.inline.as_ref().filter(|edit| edit.id == bin.id) {
            bin.label.text = edit.text.clone();
            bin.label.enabled = !edit.text.trim().is_empty();
            if bin.label.enabled {
                bin.tab = true;
                bin.label.size = bin.label.size.max(bin.label.min_size());
                if bin.detachable() {
                    bin.label.depth = bin.label.depth.min(0.4);
                }
            }
        }
        Some(bin)
    }
    pub fn selected_preview(
        &mut self,
        ui: &mut egui::Ui,
        layout: &Layout,
        selected: Option<u32>,
        colors: &crate::print_plate::Config,
        font: Option<crate::label::Font>,
    ) {
        let mut bin = self.preview_bin(layout, selected);
        if let Some(bin) = &mut bin {
            bin.label = bin.label.with_font(font);
        }
        self.preview.show(
            ui,
            bin.as_ref(),
            colors,
            self.inline.as_ref().is_some_and(|e| Some(e.id) == selected),
        );
    }
    pub fn shared_ui(&mut self, ui: &mut egui::Ui, layout: &mut Layout, s: &Selection) {
        let mut settings = layout.shared.clone();
        let before = settings.clone();
        let grid = gridfinity::grid_for(s, &layout.baseplate).unwrap_or([1, 1]);
        let max = gridfinity::max_units(s).unwrap_or(3);
        let mut apply_depth = false;
        ui.add_enabled_ui(!self.active(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("统一深度（格）");
                ui.add(egui::DragValue::new(&mut settings.depth_cells).range(1..=grid[1]));
                if layout
                    .bins
                    .iter()
                    .any(|bin| bin.span[1] != settings.depth_cells)
                {
                    apply_depth = ui.small_button("应用到全部").clicked();
                }
                ui.checkbox(&mut settings.auto_height, "自动高度");
                if !settings.auto_height {
                    ui.label("高度 U");
                    ui.add(egui::DragValue::new(&mut settings.height_units).range(3..=max));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut settings.scoop, "取物圆弧");
                ui.checkbox(&mut settings.tab, "标签台");
                if settings.tab
                    && ui
                        .checkbox(&mut settings.detachable_label, "Cullenect 可拆标牌")
                        .changed()
                    && settings.detachable_label
                {
                    settings.depth = 0.2;
                }
            });
            if settings.tab {
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut settings.style, crate::label::Style::Raised, "凸字");
                    ui.radio_value(&mut settings.style, crate::label::Style::Engraved, "凹字");
                    ui.radio_value(&mut settings.style, crate::label::Style::Flat, "平面分色");
                });
                let minimum = layout
                    .bins
                    .iter()
                    .filter(|b| b.label.active())
                    .map(|b| b.label.min_size())
                    .fold(4., f32::max);
                ui.add(egui::Slider::new(&mut settings.size, minimum..=12.).text("统一字号 mm"));
                ui.add(
                    egui::Slider::new(
                        &mut settings.depth,
                        0.2..=if settings.detachable_label { 0.4 } else { 0.8 },
                    )
                    .text(if settings.style == crate::label::Style::Flat {
                        "颜色层厚 mm"
                    } else {
                        "统一字高 / 刻深 mm"
                    }),
                );
                if settings.style == crate::label::Style::Flat {
                    ui.small("文字与底板齐平；选择不同的文字耗材槽，导出 3MF 保留分色。");
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("文字偏移 X / Y mm");
                    for offset in &mut settings.offset {
                        ui.add(egui::DragValue::new(offset).speed(0.1).range(-10. ..=10.));
                    }
                });
            }
        });
        if settings != before {
            self.shared_error = layout.set_shared(settings, s).err();
        } else if apply_depth {
            self.shared_error = layout.set_uniform_depth(settings.depth_cells, s).err();
        }
        if let Some(error) = &self.shared_error {
            ui.colored_label(Color32::LIGHT_RED, format!("统一配置未修改：{error}"));
        }
    }
    pub fn cancel(&mut self) {
        self.drag = None;
        self.inline = None;
    }
    pub fn active(&self) -> bool {
        self.drag.is_some() || self.inline.is_some()
    }
    pub fn begin_label(&mut self, layout: &Layout, id: u32) {
        if let Some(bin) = layout.bins.iter().find(|b| b.id == id) {
            self.drag = None;
            self.inline = Some(InlineLabel {
                id,
                text: bin.label.text.clone(),
                focus: true,
                composing: false,
            });
            self.error = None;
        }
    }
    fn commit_label(&mut self, layout: &mut Layout, s: &Selection) -> Result<(), String> {
        let edit = self.inline.as_ref().ok_or("没有正在编辑的标签")?;
        let mut next = layout.clone();
        let b = next
            .bins
            .iter_mut()
            .find(|b| b.id == edit.id)
            .ok_or("盒子已移除")?;
        b.label.text = edit.text.clone();
        b.label.enabled = !edit.text.trim().is_empty();
        if b.label.enabled {
            next.shared.tab = true;
            next.shared.size = next.shared.size.max(b.label.min_size());
            if next.shared.detachable_label {
                next.shared.depth = next.shared.depth.min(0.4);
            }
        }
        next.sync_shared();
        next.validate(s)?;
        *layout = next;
        self.inline = None;
        self.error = None;
        Ok(())
    }
    fn inline_ui(
        &mut self,
        ui: &mut egui::Ui,
        layout: &mut Layout,
        s: &Selection,
        selected: &mut Option<u32>,
        rect: Rect,
        cell: f32,
    ) {
        let Some(edit) = &mut self.inline else {
            return;
        };
        let Some(bin) = layout.bins.iter().find(|b| b.id == edit.id) else {
            self.inline = None;
            return;
        };
        let id = ui.id().with(("gridfinity-inline-label", edit.id));
        let newly_focused = edit.focus;
        if edit.focus {
            ui.memory_mut(|m| m.request_focus(id));
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(edit.text.chars().count()),
                )));
            state.store(ui.ctx(), id);
            edit.focus = false;
        }
        let ime_frame = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Ime(_))));
        ui.input(|i| {
            for event in &i.events {
                if let egui::Event::Ime(event) = event {
                    match event {
                        egui::ImeEvent::Preedit(text) => edit.composing = !text.is_empty(),
                        egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled => {
                            edit.composing = false
                        }
                        egui::ImeEvent::Enabled => {}
                    }
                }
            }
        });
        let plain_enter = ui.input(|i| i.events.iter().any(|event| matches!(event, egui::Event::Key {key:egui::Key::Enter,pressed:true,modifiers,..} if !modifiers.shift)));
        let confirm = !ime_frame
            && !edit.composing
            && plain_enter
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let r = Self::bin_rect(bin, rect, cell).shrink(5.);
        let area = Rect::from_center_size(r.center(), Vec2::new(r.width(), r.height().min(44.)));
        let mut edit_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        edit_ui.set_clip_rect(ui.clip_rect().intersect(area.expand(2.)));
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _width: f32| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    text.as_str().to_owned(),
                    egui::FontId::proportional(13.),
                    ui.visuals().text_color(),
                )
            })
        };
        let response = edit_ui.put(
            area,
            egui::TextEdit::multiline(&mut edit.text)
                .id(id)
                .font(egui::FontId::proportional(13.))
                .horizontal_align(egui::Align::Center)
                .vertical_align(egui::Align::Center)
                .desired_rows(2)
                .desired_width(area.width())
                .char_limit(48)
                .clip_text(true)
                .layouter(&mut layouter)
                .hint_text("标签文本")
                .return_key(egui::KeyboardShortcut::new(
                    egui::Modifiers::SHIFT,
                    egui::Key::Enter,
                )),
        );
        if newly_focused {
            // The canvas owned the double-click before the overlay existed. Reassert
            // focus after registration and select the full old text for replacement.
            response.request_focus();
            if !ui.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Ime(_)
                    )
                })
            }) {
                let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(edit.text.chars().count()),
                    )));
                state.store(ui.ctx(), id);
            }
        }
        let outside = ui.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.contains(p))
        });
        if confirm || response.lost_focus() || outside {
            let bin_id = edit.id;
            if let Err(e) = self.commit_label(layout, s) {
                self.error = Some(e);
                *selected = Some(bin_id);
                response.request_focus();
            }
        }
    }
    fn fresh(layout: &Layout) -> Result<Bin, String> {
        let mut bin = Bin {
            id: layout
                .bins
                .iter()
                .map(|b| b.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("盒子编号溢出")?,
            ..Default::default()
        };
        layout.shared.apply(&mut bin);
        Ok(bin)
    }
    pub fn start_stamp(&mut self, layout: &Layout, s: &Selection) {
        match Self::fresh(layout) {
            Ok(bin) => {
                self.drag = Some(Drag {
                    base: layout.clone(),
                    specification: s.clone(),
                    view: GridView::Plan,
                    preview: None,
                    bin,
                    start: [0.; 2],
                    gesture: Gesture::Stamp,
                });
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn point(rect: Rect, cell: f32, p: Pos2) -> [f32; 2] {
        [(p.x - rect.min.x) / cell, (p.y - rect.min.y) / cell]
    }
    fn bin_rect(b: &Bin, rect: Rect, cell: f32) -> Rect {
        Rect::from_min_size(
            rect.min + Vec2::new(b.cell[0] as f32 * cell, b.cell[1] as f32 * cell),
            Vec2::new(b.span[0] as f32 * cell, b.span[1] as f32 * cell),
        )
    }
    pub fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        layout: &mut Layout,
        s: &Selection,
        selected: &mut Option<u32>,
    ) -> Rect {
        let grid = gridfinity::grid_for(s, &layout.baseplate).unwrap_or([1, 1]);
        let cell = ui.available_width().max(1.) / grid[0] as f32;
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(cell * grid[0] as f32, cell * grid[1] as f32),
            egui::Sense::click_and_drag(),
        );
        let (pointer, press, released, cancel) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.press_origin(),
                i.pointer.button_released(egui::PointerButton::Primary),
                i.key_pressed(egui::Key::Escape)
                    || i.pointer.button_pressed(egui::PointerButton::Secondary)
                    || !i.focused,
            )
        });
        if cancel {
            self.drag = None;
            if ui.input(|i| {
                i.key_pressed(egui::Key::Escape)
                    || i.pointer.button_pressed(egui::PointerButton::Secondary)
            }) {
                self.inline = None;
            }
            self.error = None;
        }
        if self
            .inline
            .as_ref()
            .is_some_and(|edit| !layout.bins.iter().any(|b| b.id == edit.id))
        {
            self.inline = None;
        }
        if !cancel
            && self.drag.is_none()
            && self.inline.is_none()
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = press
        {
            self.begin_grid_drag(
                GridView::Plan,
                layout,
                s,
                selected,
                Self::point(rect, cell, p),
                8. / cell,
            );
        }
        self.update_grid_drag(
            GridView::Plan,
            layout,
            s,
            selected,
            pointer
                .filter(|p| ui.clip_rect().contains(*p))
                .map(|p| Self::point(rect, cell, p)),
            released,
        );
        let painter = ui.painter_at(rect);
        let visuals = ui.visuals();
        let accent = visuals.hyperlink_color;
        let (grid_line, bin_fill, bin_front) = if visuals.dark_mode {
            (
                Color32::from_gray(75),
                Color32::from_gray(99),
                Color32::from_gray(77),
            )
        } else {
            (
                Color32::from_gray(191),
                Color32::from_gray(180),
                Color32::from_gray(149),
            )
        };
        painter.rect_filled(rect, 5., visuals.extreme_bg_color);
        let grid_stroke = Stroke::new(1., grid_line);
        painter.rect_stroke(rect, 5., grid_stroke, egui::StrokeKind::Inside);
        for column in 1..grid[0] {
            let x = rect.left() + column as f32 * cell;
            painter.vline(x, rect.y_range(), grid_stroke);
        }
        for row in 1..grid[1] {
            let y = rect.top() + row as f32 * cell;
            painter.hline(rect.x_range(), y, grid_stroke);
        }
        for b in &layout.bins {
            let r = Self::bin_rect(b, rect, cell);
            let is_selected = Some(b.id) == *selected;
            let (fill, front) = if is_selected {
                (visuals.selection.bg_fill, accent)
            } else {
                (bin_fill, bin_front)
            };
            let body = r.shrink(2.);
            let front_height = (cell * 0.09).clamp(2., 7.).min(body.height().max(0.));
            let front_top = body.bottom() - front_height;
            painter.rect_filled(body, 5., fill);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(body.left(), front_top), body.max),
                egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 5,
                    se: 5,
                },
                front,
            );
            painter.rect_stroke(
                body,
                5.,
                if is_selected {
                    Stroke::new(2., accent)
                } else {
                    Stroke::new(1., bin_front)
                },
                egui::StrokeKind::Inside,
            );
            if self.inline.as_ref().is_none_or(|edit| edit.id != b.id) {
                let text_area =
                    Rect::from_min_max(body.min, Pos2::new(body.right(), front_top)).shrink(3.);
                let text = painter.with_clip_rect(painter.clip_rect().intersect(text_area));
                let center = text_area.center();
                let show_label =
                    b.label.active() && text_area.width() >= 32. && text_area.height() >= 30.;
                let offset = if show_label { 8. } else { 0. };
                if let Ok(units) = gridfinity::units(b, s) {
                    text.text(
                        center - Vec2::new(0., offset),
                        egui::Align2::CENTER_CENTER,
                        format!("{units}U"),
                        egui::FontId::proportional((text_area.width() * 0.48).clamp(9., 14.)),
                        visuals.text_color(),
                    );
                }
                if show_label {
                    text.text(
                        center + Vec2::new(0., offset),
                        egui::Align2::CENTER_CENTER,
                        b.label.text.replace('\n', " / "),
                        egui::FontId::proportional(12.),
                        visuals.text_color(),
                    );
                }
            }
            if is_selected {
                for p in [
                    r.left_center(),
                    r.right_center(),
                    r.center_top(),
                    r.center_bottom(),
                ] {
                    painter.rect_filled(Rect::from_center_size(p, Vec2::splat(7.)), 1., accent);
                }
            }
        }
        if let Some(candidate) = self.drag_preview() {
            let color = if candidate.valid {
                accent
            } else {
                visuals.error_fg_color
            };
            let r = Rect::from_min_size(
                rect.min + Vec2::new(candidate.cell[0] as f32, candidate.cell[1] as f32) * cell,
                Vec2::new(candidate.span[0] as f32, candidate.span[1] as f32) * cell,
            )
            .shrink(2.);
            painter.rect_filled(r, 3., color.gamma_multiply(0.25));
            painter.rect_stroke(r, 3., Stroke::new(2.5, color), egui::StrokeKind::Inside);
            if r.width() >= 40. && r.height() >= 42. {
                painter
                    .with_clip_rect(painter.clip_rect().intersect(r.shrink(3.)))
                    .text(
                        r.min + Vec2::splat(5.),
                        egui::Align2::LEFT_TOP,
                        format!("{} × {}", candidate.span[0], candidate.span[1]),
                        egui::FontId::proportional(11.),
                        color,
                    );
            }
            if response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
        if response.clicked()
            && !cancel
            && let Some(p) = pointer
        {
            *selected = Self::hit_bin(layout, Self::point(rect, cell, p));
            self.error = None;
            response.request_focus();
        }
        let double_click = response.double_clicked()
            && pointer.is_some_and(|p| {
                layout
                    .bins
                    .iter()
                    .any(|b| Self::bin_rect(b, rect, cell).shrink(8.).contains(p))
            });
        let keyboard_on_canvas = ui.memory(|m| m.focused().is_none_or(|id| id == response.id));
        self.grid_keyboard(&response, layout, s, selected);
        let f2 = keyboard_on_canvas
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2));
        let typing = keyboard_on_canvas
            && ui.input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Text(_) | egui::Event::Paste(_)))
            });
        if !cancel
            && self.inline.is_none()
            && self.drag.is_none()
            && (double_click || f2 || typing)
            && let Some(id) = *selected
        {
            self.begin_label(layout, id);
        }
        self.inline_ui(ui, layout, s, selected, rect, cell);
        if !self.active()
            && response.hovered()
            && let Some(p) = pointer
        {
            let at = Self::point(rect, cell, p);
            let resize = layout
                .bins
                .iter()
                .find(|bin| Some(bin.id) == *selected)
                .and_then(|bin| Self::resize_cursor(Self::resize_edges(bin, at, 8. / cell)));
            let cursor = resize.unwrap_or_else(|| {
                if Self::hit_bin(layout, at).is_some() {
                    egui::CursorIcon::Grab
                } else {
                    egui::CursorIcon::Crosshair
                }
            });
            ui.ctx().set_cursor_icon(cursor);
        }
        ui.vertical_centered(|ui| {
            ui.weak("前");
        });
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        } else if self.inline.is_some() {
            ui.small("Enter 确认，Shift+Enter 换行，Esc 取消。");
        }
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_shows_actual_bin_height_and_clips_text_to_each_box() {
        for (panel_width, dark, auto_height, show_name) in [
            (285., false, true, true),
            (285., true, false, true),
            (90., false, true, false),
        ] {
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let s = Selection::default();
            let mut layout = Layout::default();
            layout.shared.auto_height = auto_height;
            layout.shared.height_units = 3;
            layout.add(&s, None).unwrap();
            let name = "M3 × 10 mounting screws";
            layout.bins[0].label.text = name.into();
            layout.bins[0].label.enabled = true;
            let mut editor = Editor::default();
            let mut box_rect = Rect::NOTHING;
            let frame = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(panel_width, 480.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let board = editor.canvas(ui, &mut layout, &s, &mut None);
                        box_rect = Editor::bin_rect(&layout.bins[0], board, board.width() / 3.);
                    });
                },
            );
            let height = if auto_height { "4U" } else { "3U" };
            let text_clip = |needle: &str| {
                frame.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::Shape::Text(t) if t.galley.text() == needle => Some(shape.clip_rect),
                    _ => None,
                })
            };
            assert!(box_rect.contains_rect(text_clip(height).unwrap()));
            assert_eq!(text_clip(name).is_some(), show_name);
            if let Some(clip) = text_clip(name) {
                assert!(box_rect.contains_rect(clip));
            }
        }
    }
    #[test]
    fn selected_preview_tracks_inline_draft_without_changing_layout() {
        let mut layout = Layout::default();
        let s = Selection::default();
        layout.add(&s, None).unwrap();
        layout.add(&s, None).unwrap();
        layout.shared.size = 4.;
        layout.sync_shared();
        layout.bins[0].label.text = "M3".into();
        layout.bins[0].label.enabled = true;
        let saved = layout.clone();
        let mut editor = Editor::default();
        editor.begin_label(&layout, 2);
        editor.inline.as_mut().unwrap().text = "电阻".into();
        let preview = editor.preview_bin(&layout, Some(2)).unwrap();
        assert_eq!(preview.label.text, "电阻");
        assert!(preview.label.active());
        assert_eq!(preview.label.size, 5.);
        assert_eq!(
            editor.preview_bin(&layout, Some(1)).unwrap().label.text,
            "M3"
        );
        assert_eq!(layout, saved);
        editor.cancel();
        assert_eq!(
            editor.preview_bin(&layout, Some(2)).unwrap(),
            layout.bins[1]
        );
        assert!(editor.preview_bin(&layout, None).is_none());
    }
    #[test]
    fn shared_controls_update_all_bins_and_stamped_bins_inherit() {
        fn frame(
            ctx: &egui::Context,
            editor: &mut Editor,
            layout: &mut Layout,
            time: &mut f64,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            *time += 0.1;
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 1000.))),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        editor.shared_ui(ui, layout, &Selection::default())
                    });
                },
            )
        }
        let ctx = egui::Context::default();
        let mut editor = Editor::default();
        let mut layout = Layout::default();
        let mut time = 0.;
        layout.add(&Selection::default(), None).unwrap();
        layout.add(&Selection::default(), None).unwrap();
        layout.bins[0].label.text = "one".into();
        layout.bins[1].label.text = "two".into();
        for text in ["取物圆弧", "凹字"] {
            let out = frame(&ctx, &mut editor, &mut layout, &mut time, vec![]);
            let pos = out
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::epaint::Shape::Text(t) = &shape.shape {
                        (t.galley.text() == text)
                            .then(|| Rect::from_min_size(t.pos, t.galley.size()).center())
                    } else {
                        None
                    }
                })
                .unwrap();
            frame(
                &ctx,
                &mut editor,
                &mut layout,
                &mut time,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
            );
            frame(
                &ctx,
                &mut editor,
                &mut layout,
                &mut time,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
            );
        }
        assert!(
            layout
                .bins
                .iter()
                .all(|b| !b.scoop && b.label.style == crate::label::Style::Engraved)
        );
        assert_eq!(layout.bins[0].label.text, "one");
        assert_eq!(layout.bins[1].label.text, "two");
        let fresh = Editor::fresh(&layout).unwrap();
        assert_eq!(gridfinity::SharedConfig::from_bin(&fresh), layout.shared);
    }
    struct Harness {
        ctx: egui::Context,
        editor: Editor,
        layout: Layout,
        selected: Option<u32>,
        time: f64,
        canvas: Rect,
        button: Rect,
    }
    impl Harness {
        fn new() -> Self {
            let mut h = Self {
                ctx: Default::default(),
                editor: Default::default(),
                layout: Default::default(),
                selected: None,
                time: 0.,
                canvas: Rect::NOTHING,
                button: Rect::NOTHING,
            };
            h.frame(vec![]);
            h.frame(vec![]);
            h
        }
        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.016;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(284., 500.))),
                time: Some(self.time),
                events,
                focused: true,
                ..Default::default()
            };
            let _ = self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let add = ui
                        .add(egui::Button::new("Add / drag").sense(egui::Sense::click_and_drag()));
                    self.button = add.rect;
                    if add.drag_started_by(egui::PointerButton::Primary) {
                        self.editor.start_stamp(&self.layout, &Selection::default());
                    }
                    self.canvas = self.editor.canvas(
                        ui,
                        &mut self.layout,
                        &Selection::default(),
                        &mut self.selected,
                    );
                });
            });
        }
        fn pos(&self, x: f32, y: f32) -> Pos2 {
            self.canvas.min + Vec2::new(x, y) * self.cell_width()
        }
        fn cell_width(&self) -> f32 {
            let grid = gridfinity::grid_for(&Selection::default(), &self.layout.baseplate).unwrap();
            self.canvas.width() / grid[0] as f32
        }
        fn press(&mut self, p: Pos2) {
            self.frame(vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
        }
        fn move_to(&mut self, p: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(p)]);
        }
        fn release(&mut self, p: Pos2) {
            self.frame(vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ]);
        }
    }
    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }
    #[test]
    fn inline_double_click_buffers_input_then_commits_only_one_label() {
        let mut h = Harness::new();
        h.layout.bins = vec![
            Bin {
                span: [1, 2],
                label: crate::label::Config {
                    enabled: true,
                    text: "Old text".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            Bin {
                id: 2,
                cell: [1, 0],
                span: [1, 2],
                ..Default::default()
            },
        ];
        h.layout.adopt_shared();
        let before = h.layout.clone();
        h.frame(vec![]);
        let center = h.pos(0.5, 1.);
        for _ in 0..2 {
            h.press(center);
            h.release(center);
        }
        assert!(h.editor.inline.is_some());
        h.frame(vec![egui::Event::Text("M3*14".into())]);
        assert_eq!(
            h.layout, before,
            "typing must not generate intermediate models"
        );
        assert_eq!(h.editor.inline.as_ref().unwrap().text, "M3*14");
        h.frame(vec![key(egui::Key::Enter, Default::default())]);
        assert!(h.editor.inline.is_none());
        assert_eq!(h.layout.bins[0].label.text, "M3*14");
        assert!(h.layout.bins[0].label.active());
        assert_eq!(h.layout.bins[0].cell, before.bins[0].cell);
        assert_eq!(h.layout.bins[0].span, before.bins[0].span);
        assert_eq!(h.layout.bins[1], before.bins[1]);
    }
    #[test]
    fn focused_grid_shortcuts_validate_edits_and_leave_inline_text_alone() {
        use egui::{Key, Modifiers};
        let mut h = Harness::new();
        let s = Selection::default();
        h.layout.add(&s, None).unwrap();
        h.layout.add(&s, None).unwrap();
        h.frame(vec![]);
        let center = h.pos(0.5, 0.5);
        h.press(center);
        h.release(center);
        h.frame(vec![]);
        h.frame(vec![key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(h.selected, Some(2));
        h.frame(vec![key(Key::ArrowDown, Modifiers::NONE)]);
        assert_eq!(h.layout.bins[1].cell, [1, 1]);
        h.frame(vec![key(Key::ArrowRight, Modifiers::SHIFT)]);
        assert_eq!(h.layout.bins[1].span, [2, 1]);
        h.frame(vec![key(Key::ArrowDown, Modifiers::SHIFT)]);
        assert_eq!(h.layout.bins[1].span, [2, 2]);
        h.frame(vec![key(Key::ArrowUp, Modifiers::SHIFT)]);
        assert_eq!(h.layout.bins[1].span, [2, 1]);
        h.frame(vec![key(Key::D, Modifiers::CTRL)]);
        assert_eq!(h.selected, Some(3));
        assert_eq!(h.layout.bins.len(), 3);
        let before = h.layout.clone();
        h.frame(vec![key(Key::ArrowLeft, Modifiers::NONE)]);
        assert_eq!(h.layout, before);
        assert!(h.editor.error.is_some());
        h.frame(vec![key(Key::Delete, Modifiers::NONE)]);
        assert_eq!(h.layout.bins.len(), 2);
        h.frame(vec![key(Key::F2, Modifiers::NONE)]);
        assert!(h.editor.inline.is_some());
        let before_text = h.layout.clone();
        h.frame(vec![
            key(Key::Delete, Modifiers::NONE),
            key(Key::ArrowLeft, Modifiers::NONE),
        ]);
        assert_eq!(h.layout, before_text);
        assert!(h.editor.inline.is_some());
    }
    #[test]
    fn inline_type_to_replace_cancel_click_away_and_blank() {
        let mut h = Harness::new();
        h.layout.bins = vec![Bin {
            label: crate::label::Config {
                enabled: true,
                text: "Old".into(),
                ..Default::default()
            },
            ..Default::default()
        }];
        h.frame(vec![]);
        let center = h.pos(0.5, 0.5);
        h.press(center);
        h.release(center);
        h.frame(vec![egui::Event::Text("New".into())]);
        assert_eq!(h.editor.inline.as_ref().unwrap().text, "New");
        h.frame(vec![key(egui::Key::Escape, Default::default())]);
        assert_eq!(h.layout.bins[0].label.text, "Old");
        h.frame(vec![key(egui::Key::F2, Default::default())]);
        h.frame(vec![egui::Event::Text("Other".into())]);
        let outside = h.pos(2.5, 2.5);
        h.press(outside);
        h.release(outside);
        assert_eq!(h.layout.bins[0].label.text, "Other");
        assert!(h.editor.inline.is_none());
        h.selected = Some(1);
        h.frame(vec![key(egui::Key::F2, Default::default())]);
        h.frame(vec![key(egui::Key::Backspace, Default::default())]);
        h.frame(vec![key(egui::Key::Enter, Default::default())]);
        assert_eq!(h.layout.bins[0].label.text, "");
        assert!(!h.layout.bins[0].label.enabled);
        assert!(h.layout.bins[0].tab);
        assert_eq!(
            h.layout.bins.len(),
            1,
            "outside click while editing must not add bins"
        );
    }
    #[test]
    fn inline_ime_commit_does_not_submit_and_invalid_lines_keep_draft() {
        let mut h = Harness::new();
        h.layout.bins = vec![Bin {
            detachable_label: true,
            ..Default::default()
        }];
        h.layout.adopt_shared();
        h.selected = Some(1);
        h.frame(vec![key(egui::Key::F2, Default::default())]);
        h.frame(vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("luo".into())),
        ]);
        h.frame(vec![
            egui::Event::Ime(egui::ImeEvent::Commit("螺丝".into())),
            key(egui::Key::Enter, Default::default()),
        ]);
        assert!(
            h.editor.inline.is_some(),
            "IME candidate confirmation must not submit"
        );
        assert_eq!(h.layout.bins[0].label.text, "");
        h.frame(vec![key(egui::Key::Enter, Default::default())]);
        assert_eq!(h.layout.bins[0].label.text, "螺丝");
        assert_eq!(h.layout.bins[0].label.size, 5.);
        assert!(h.layout.bins[0].label.depth <= 0.4);
        h.frame(vec![key(egui::Key::F2, Default::default())]);
        h.frame(vec![egui::Event::Text("M3".into())]);
        h.frame(vec![key(egui::Key::Enter, egui::Modifiers::SHIFT)]);
        h.frame(vec![egui::Event::Text("14".into())]);
        h.frame(vec![key(egui::Key::Enter, Default::default())]);
        assert_eq!(h.layout.bins[0].label.text, "M3\n14");
        let before = h.layout.clone();
        h.frame(vec![key(egui::Key::F2, Default::default())]);
        h.frame(vec![egui::Event::Paste("A\nB\nC".into())]);
        h.frame(vec![key(egui::Key::Enter, Default::default())]);
        assert_eq!(h.layout, before);
        assert!(h.editor.error.is_some());
        assert!(h.editor.inline.is_some());
        h.frame(vec![key(egui::Key::Escape, Default::default())]);
        assert_eq!(h.layout, before);
    }
    #[test]
    fn actual_pointer_draw_reverse_rectangle_and_stamp() {
        let mut h = Harness::new();
        let initial_rect = h.canvas;
        let start = h.pos(2.5, 2.5);
        let end = h.pos(0.5, 1.5);
        h.press(start);
        h.move_to(end);
        assert!(h.editor.active());
        assert!(
            h.layout.bins.is_empty(),
            "drag must not regenerate intermediate geometry"
        );
        h.release(end);
        assert_eq!(h.layout.bins.len(), 1);
        assert_eq!(h.layout.bins[0].cell, [0, 1]);
        assert_eq!(h.layout.bins[0].span, [3, 2]);
        assert_eq!(h.canvas, initial_rect);
        let button = h.button.center();
        let drop = h.pos(1.5, 0.5);
        h.press(button);
        h.move_to(drop);
        assert!(h.editor.active());
        assert_eq!(h.layout.bins.len(), 1);
        h.release(drop);
        assert_eq!(h.layout.bins.len(), 2);
        assert_eq!(h.layout.bins[1].cell, [1, 0]);
        assert_eq!(h.layout.bins[1].span, [1, 1]);
        let before = h.layout.clone();
        h.press(button);
        h.move_to(drop);
        h.release(drop);
        assert_eq!(h.layout, before);
        assert!(h.editor.error.is_some());
    }
    #[test]
    fn pointer_drag_creates_the_full_rectangle_in_each_direction() {
        for (start, end) in [
            ([0.5, 0.5], [1.5, 2.5]),
            ([1.5, 0.5], [0.5, 2.5]),
            ([0.5, 2.5], [1.5, 0.5]),
            ([1.5, 2.5], [0.5, 0.5]),
        ] {
            let mut h = Harness::new();
            let from = h.pos(start[0], start[1]);
            let to = h.pos(end[0], end[1]);
            h.press(from);
            h.move_to(to);
            assert_eq!(
                h.editor.drag_preview(),
                Some(&DragPreview {
                    id: 1,
                    cell: [0, 0],
                    span: [2, 3],
                    valid: true
                })
            );
            assert!(h.layout.bins.is_empty());
            h.release(to);
            assert_eq!(h.layout.bins.len(), 1);
            assert_eq!(h.layout.bins[0].cell, [0, 0]);
            assert_eq!(h.layout.bins[0].span, [2, 3]);
            assert_eq!(h.selected, Some(1));
        }
    }

    #[test]
    fn occupied_cell_inside_drawn_rectangle_or_outside_release_preserves_layout() {
        let mut h = Harness::new();
        h.layout.bins = vec![Bin {
            cell: [1, 1],
            ..Default::default()
        }];
        let saved = h.layout.clone();
        let start = h.pos(0.5, 0.5);
        let end = h.pos(2.5, 2.5);
        h.press(start);
        h.move_to(end);
        assert_eq!(
            h.editor.drag_preview(),
            Some(&DragPreview {
                id: 2,
                cell: [0, 0],
                span: [3, 3],
                valid: false
            })
        );
        h.release(end);
        assert!(h.editor.error.is_some());
        assert_eq!(h.layout, saved);

        let outside = h.pos(0.5, 3.12);
        h.press(start);
        h.move_to(outside);
        assert!(!h.editor.drag_preview().unwrap().valid);
        h.release(outside);
        assert!(h.editor.error.is_some());
        assert_eq!(h.layout, saved);
        assert!(h.selected.is_none());
    }
    #[test]
    fn actual_pointer_move_resize_cancel_and_preserve_label() {
        let mut h = Harness::new();
        let b = Bin {
            label: crate::label::Config {
                text: "Keep text".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        h.layout.bins.push(b.clone());
        h.selected = Some(b.id);
        h.frame(vec![]);
        let start = h.pos(0.7, 0.6);
        let end = h.pos(1.7, 1.6);
        h.press(start);
        h.move_to(end);
        assert_eq!(h.layout.bins[0].cell, [0, 0]);
        h.release(end);
        assert_eq!(h.layout.bins[0].cell, [1, 1]);
        assert_eq!(h.layout.bins[0].label, b.label);
        let corner = h.pos(2., 2.) - Vec2::splat(2.);
        let enlarged = corner + Vec2::splat(h.cell_width());
        h.press(corner);
        h.move_to(enlarged);
        h.release(enlarged);
        assert_eq!(h.layout.bins[0].span, [2, 2]);
        let before = h.layout.clone();
        let start = h.pos(1.5, 1.5);
        let outside = h.canvas.min - Vec2::splat(30.);
        h.press(start);
        h.move_to(outside);
        h.release(outside);
        assert_eq!(h.layout, before);
        h.press(start);
        h.move_to(h.pos(0.5, 0.5));
        h.frame(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]);
        h.release(h.pos(0.5, 0.5));
        assert_eq!(h.layout, before);
    }
    #[test]
    fn drawn_depth_survives_shared_sync_and_side_resize_while_stamp_uses_default_depth() {
        let mut h = Harness::new();
        h.layout
            .set_shared(
                gridfinity::SharedConfig {
                    depth_cells: 2,
                    ..h.layout.shared.clone()
                },
                &Selection::default(),
            )
            .unwrap();
        let start = h.pos(0.5, 0.5);
        let end = h.pos(1.5, 2.5);
        h.press(start);
        h.move_to(end);
        h.release(end);
        assert_eq!(h.layout.bins[0].span, [2, 3]);
        assert_eq!(h.layout.bins[0].cell, [0, 0]);
        h.layout.sync_shared();
        assert_eq!(h.layout.bins[0].span, [2, 3]);
        let left = h.pos(0.03, 1.5);
        let smaller = h.pos(1.03, 1.5);
        h.press(left);
        h.move_to(smaller);
        h.release(smaller);
        assert_eq!(h.layout.bins[0].span, [1, 3]);
        let stamp = h.button.center();
        let socket = h.pos(0.5, 0.5);
        h.press(stamp);
        h.move_to(socket);
        h.release(socket);
        assert_eq!(h.layout.bins[1].span, [1, 2]);
        assert_eq!(h.layout.shared.depth_cells, 2);
    }
    #[test]
    fn actual_pointer_resize_conflict_and_single_cell_minimum() {
        let mut h = Harness::new();
        h.layout.bins = vec![
            Bin::default(),
            Bin {
                id: 2,
                cell: [2, 0],
                ..Default::default()
            },
        ];
        h.selected = Some(1);
        h.frame(vec![]);
        let before = h.layout.clone();
        let edge = h.pos(1., 0.5) - Vec2::new(2., 0.);
        let drop = edge + Vec2::new(2. * h.cell_width(), 0.);
        h.press(edge);
        h.move_to(drop);
        h.release(drop);
        assert_eq!(h.layout, before);
        assert!(h.editor.error.is_some());
        let left = h.pos(0., 0.5) + Vec2::new(2., 0.);
        let drop = left + Vec2::new(h.cell_width(), 0.);
        h.press(left);
        h.move_to(drop);
        h.release(drop);
        assert_eq!(h.layout.bins[0].span, [1, 1]);
        assert_eq!(h.layout.bins[0].cell, [0, 0]);
    }
}
