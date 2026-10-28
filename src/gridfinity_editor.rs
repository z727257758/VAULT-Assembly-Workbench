//! Transactional pointer gestures for the Gridfinity layout canvas.
use crate::{
    catalog::Selection,
    gridfinity::{self, Bin, Layout},
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
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
    fn rect(&self, rect: Rect, cell: f32) -> Rect {
        Rect::from_min_size(
            rect.min + Vec2::new(self.cell[0] as f32 * cell, self.cell[1] as f32 * cell),
            Vec2::new(
                self.bin.span[0] as f32 * cell,
                self.bin.span[1] as f32 * cell,
            ),
        )
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
                lo[0] = start[0].min(end[0]);
                lo[1] = start[1];
                b.span[0] = (start[0] - end[0]).unsigned_abs() + 1;
                // Depth belongs to the layout's shared configuration.
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
    ) {
        let bin = self.preview_bin(layout, selected);
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
        ui.strong("所有小盒统一配置");
        ui.small("修改立即作用于全部小盒，新增小盒自动沿用；位置、宽度和标签文本逐盒保留。");
        ui.add_enabled_ui(!self.active(), |ui| {
            ui.horizontal(|ui| {
                ui.label("统一深度（格）");
                ui.add(egui::DragValue::new(&mut settings.depth_cells).range(1..=grid[1]));
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
                ui.horizontal(|ui| {
                    ui.radio_value(&mut settings.style, crate::label::Style::Raised, "凸字");
                    ui.radio_value(&mut settings.style, crate::label::Style::Engraved, "凹字");
                    ui.radio_value(&mut settings.style, crate::label::Style::Flat, "平面分色");
                });
                ui.horizontal_wrapped(|ui| {
                    for (font, name) in [
                        (crate::label::Font::Chinese, "微软雅黑"),
                        (crate::label::Font::ChineseBold, "微软雅黑粗体"),
                        (crate::label::Font::Arial, "Arial"),
                        (crate::label::Font::ArialBold, "Arial Bold"),
                    ] {
                        ui.selectable_value(&mut settings.font, font, name);
                    }
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
                ui.horizontal(|ui| {
                    ui.label("文字偏移 X / Y mm");
                    for offset in &mut settings.offset {
                        ui.add(egui::DragValue::new(offset).speed(0.1).range(-10. ..=10.));
                    }
                });
            }
        });
        if settings != before {
            self.shared_error = layout.set_shared(settings, s).err();
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
    fn edges(r: Rect, p: Pos2) -> [i8; 2] {
        let mut edges = [0; 2];
        for (a, edge) in edges.iter_mut().enumerate().take(1) {
            let v = if a == 0 { p.x } else { p.y };
            let lo = if a == 0 { r.left() } else { r.top() };
            let hi = if a == 0 { r.right() } else { r.bottom() };
            if (v - lo).abs() <= 8. {
                *edge = -1;
            } else if (v - hi).abs() <= 8. {
                *edge = 1;
            }
        }
        edges
    }
    fn cursor(edges: [i8; 2]) -> egui::CursorIcon {
        match edges {
            [0, 0] => egui::CursorIcon::Grab,
            [0, _] => egui::CursorIcon::ResizeVertical,
            [_, 0] => egui::CursorIcon::ResizeHorizontal,
            [a, b] if a == b => egui::CursorIcon::ResizeNwSe,
            _ => egui::CursorIcon::ResizeNeSw,
        }
    }
    pub fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        layout: &mut Layout,
        s: &Selection,
        selected: &mut Option<u32>,
    ) -> Rect {
        let grid = gridfinity::grid_for(s, &layout.baseplate).unwrap_or([1, 1]);
        let cell = (ui.available_width() / grid[0] as f32).min(58.);
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
        if self.drag.as_ref().is_some_and(|d| {
            d.base != *layout
                || d.specification.width != s.width
                || d.specification.depth != s.depth
                || d.specification.height != s.height
        }) {
            self.drag = None;
        }
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
            && let Some(p) = press.filter(|p| rect.contains(*p))
        {
            let at = Self::point(rect, cell, p);
            // Selected edge handles take precedence over a neighboring bin.
            let chosen = layout.bins.iter().find(|b| {
                Some(b.id) == *selected
                    && Self::bin_rect(b, rect, cell).expand(4.).contains(p)
                    && Self::edges(Self::bin_rect(b, rect, cell), p) != [0, 0]
            });
            let hit = chosen.or_else(|| {
                layout
                    .bins
                    .iter()
                    .find(|b| Self::bin_rect(b, rect, cell).contains(p))
            });
            let entry = if let Some(bin) = hit {
                let edge = if Some(bin.id) == *selected {
                    Self::edges(Self::bin_rect(bin, rect, cell), p)
                } else {
                    [0, 0]
                };
                Ok((
                    bin.clone(),
                    if edge == [0, 0] {
                        Gesture::Move
                    } else {
                        Gesture::Resize(edge)
                    },
                ))
            } else {
                Self::fresh(layout).map(|b| (b, Gesture::Draw(at.map(|v| v.floor() as i32))))
            };
            match entry {
                Ok((bin, gesture)) => {
                    self.drag = Some(Drag {
                        base: layout.clone(),
                        specification: s.clone(),
                        bin,
                        start: at,
                        gesture,
                    });
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        let painter = ui.painter_at(rect);
        for y in 0..grid[1] {
            for x in 0..grid[0] {
                let r = Rect::from_min_size(
                    rect.min + Vec2::new(x as f32 * cell, y as f32 * cell),
                    Vec2::splat(cell),
                );
                painter.rect_stroke(
                    r,
                    2.,
                    Stroke::new(1., Color32::DARK_GRAY),
                    egui::StrokeKind::Inside,
                );
            }
        }
        for b in &layout.bins {
            let r = Self::bin_rect(b, rect, cell);
            let fill = if Some(b.id) == *selected {
                Color32::from_rgb(54, 117, 139)
            } else {
                Color32::from_rgb(72, 79, 91)
            };
            painter.rect_filled(r.shrink(3.), 4., fill);
            let name = if b.label.active() {
                b.label.text.replace('\n', " / ")
            } else {
                format!("盒子 {}", b.id)
            };
            if self.inline.as_ref().is_none_or(|edit| edit.id != b.id) {
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    name,
                    egui::FontId::proportional(13.),
                    Color32::WHITE,
                );
            }
            if Some(b.id) == *selected {
                for p in [r.left_center(), r.right_center()] {
                    painter.rect_filled(
                        Rect::from_center_size(p, Vec2::splat(7.)),
                        1.,
                        Color32::LIGHT_BLUE,
                    );
                }
            }
        }
        if let (Some(drag), Some(p)) = (&self.drag, pointer) {
            let candidate = drag.candidate(Self::point(rect, cell, p));
            let next = if rect.contains(p) {
                candidate.layout(&drag.base, s)
            } else {
                Err("请在底板网格内释放，已保留原布局".into())
            };
            let color = if next.is_ok() {
                Color32::from_rgb(80, 220, 145)
            } else {
                Color32::LIGHT_RED
            };
            let r = candidate.rect(rect, cell).shrink(2.);
            painter.rect_filled(r, 3., color.gamma_multiply(0.25));
            painter.rect_stroke(r, 3., Stroke::new(2.5, color), egui::StrokeKind::Inside);
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                format!("{} × {}", candidate.bin.span[0], candidate.bin.span[1]),
                egui::FontId::proportional(16.),
                color,
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if released {
                match next {
                    Ok(next) => {
                        *selected = Some(candidate.bin.id);
                        *layout = next;
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
                self.drag = None;
            }
        } else if released {
            self.drag = None;
        }
        if response.clicked()
            && !cancel
            && let Some(p) = pointer
        {
            *selected = layout
                .bins
                .iter()
                .find(|b| Self::bin_rect(b, rect, cell).contains(p))
                .map(|b| b.id);
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
        let f2 = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2));
        let keyboard_on_canvas = ui.memory(|m| m.focused().is_none_or(|id| id == response.id));
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
            let hit = layout
                .bins
                .iter()
                .find(|b| Self::bin_rect(b, rect, cell).contains(p));
            ui.ctx()
                .set_cursor_icon(hit.map_or(egui::CursorIcon::Crosshair, |b| {
                    Self::cursor(if Some(b.id) == *selected {
                        Self::edges(Self::bin_rect(b, rect, cell), p)
                    } else {
                        [0, 0]
                    })
                }));
        }
        // Constant-height feedback prevents the canvas and controls shifting during gestures.
        ui.label(
            egui::RichText::new(self.error.as_deref().unwrap_or(if self.inline.is_some() {
                "Enter 确认，Shift+Enter 换行；点击其他位置确认，Esc 取消。"
            } else if self.drag.is_some() {
                "绿色可放置，红色冲突；松开确认，Esc / 右键取消。"
            } else {
                "双击或选中后输入标签；空白拖框确定宽度，拖左右边改宽，深度由统一配置决定。"
            }))
            .small()
            .color(if self.error.is_some() {
                Color32::LIGHT_RED
            } else {
                Color32::GRAY
            }),
        );
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 500.))),
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
            self.canvas.min + Vec2::new(x * 58., y * 58.)
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
        assert_eq!(h.layout.bins[0].cell, [0, 2]);
        assert_eq!(h.layout.bins[0].span, [3, 1]);
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
        let enlarged = corner + Vec2::splat(58.);
        h.press(corner);
        h.move_to(enlarged);
        h.release(enlarged);
        assert_eq!(h.layout.bins[0].span, [2, 1]);
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
    fn shared_depth_is_inherited_by_canvas_draw_and_not_individually_resized() {
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
        assert_eq!(h.layout.bins[0].span, [2, 2]);
        assert_eq!(h.layout.bins[0].cell, [0, 0]);
        let edge = h.pos(2., 1.) - Vec2::new(2., 0.);
        h.press(edge);
        h.move_to(edge + Vec2::new(58., 50.));
        h.release(edge + Vec2::new(58., 50.));
        assert_eq!(h.layout.bins[0].span, [3, 2]);
        assert_eq!(h.layout.shared.depth_cells, 2);
        let clone = h.layout.bins[0].clone();
        assert_eq!(gridfinity::SharedConfig::from_bin(&clone), h.layout.shared);
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
        let drop = edge + Vec2::new(116., 0.);
        h.press(edge);
        h.move_to(drop);
        h.release(drop);
        assert_eq!(h.layout, before);
        assert!(h.editor.error.is_some());
        let left = h.pos(0., 0.5) + Vec2::new(2., 0.);
        let drop = left + Vec2::new(58., 0.);
        h.press(left);
        h.move_to(drop);
        h.release(drop);
        assert_eq!(h.layout.bins[0].span, [1, 1]);
        assert_eq!(h.layout.bins[0].cell, [0, 0]);
    }
}
