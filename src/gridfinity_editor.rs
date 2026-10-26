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
                for a in 0..2 {
                    lo[a] = start[a].min(end[a]);
                    b.span[a] = (start[a] - end[a]).unsigned_abs() + 1;
                }
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
struct BatchPanel {
    all: bool,
    targets: std::collections::BTreeSet<u32>,
    fields: gridfinity::BatchFields,
    message: Option<Result<usize, String>>,
}
impl Default for BatchPanel {
    fn default() -> Self {
        Self {
            all: true,
            targets: Default::default(),
            fields: Default::default(),
            message: None,
        }
    }
}
#[derive(Default)]
pub struct Editor {
    batch: BatchPanel,
    drag: Option<Drag>,
    pub error: Option<String>,
}
impl Editor {
    pub fn batch_ui(
        &mut self,
        ui: &mut egui::Ui,
        layout: &mut Layout,
        s: &Selection,
        selected: Option<u32>,
    ) {
        egui::CollapsingHeader::new("批量设置小盒参数").default_open(true).show(ui, |ui| {
            let Some(source)=selected.and_then(|id|layout.bins.iter().find(|b|b.id==id)).cloned() else {
                ui.label("先在网格中选择一个盒子，作为参数来源。"); return;
            };
            ui.label(format!("以盒子 {} 的当前参数为来源；标签文本和文字启用状态逐盒保留。",source.id));
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.batch.fields.span,"宽度 / 深度");
                ui.checkbox(&mut self.batch.fields.height,"自动高度 / 高度 U");
                ui.checkbox(&mut self.batch.fields.scoop,"取物圆弧");
                ui.checkbox(&mut self.batch.fields.tab,"标签台 / 可拆结构");
                ui.checkbox(&mut self.batch.fields.lettering,"字体 / 凹凸字 / 字号 / 刻深");
            });
            ui.small("宽深不默认同步；改变宽深会保留位置，发生重叠或越界时整次取消。文字样式包含偏移，不复制文本。");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.batch.all,true,"全部其他盒子");
                ui.radio_value(&mut self.batch.all,false,"勾选目标盒子");
            });
            self.batch.targets.retain(|id|layout.bins.iter().any(|b|b.id==*id));
            if !self.batch.all {
                ui.horizontal(|ui| {
                    if ui.small_button("全选目标").clicked() {self.batch.targets=layout.bins.iter().filter(|b|b.id!=source.id).map(|b|b.id).collect();}
                    if ui.small_button("清空目标").clicked() {self.batch.targets.clear();}
                });
                ui.horizontal_wrapped(|ui| {
                    for b in layout.bins.iter().filter(|b|b.id!=source.id) {
                        let mut checked=self.batch.targets.contains(&b.id);
                        let caption=if b.label.active() {format!("盒子 {} · {}",b.id,b.label.text.replace('\n'," / "))} else {format!("盒子 {}",b.id)};
                        if ui.checkbox(&mut checked,caption).changed() {
                            if checked {self.batch.targets.insert(b.id);} else {self.batch.targets.remove(&b.id);}
                        }
                    }
                });
            }
            let targets: Vec<_>=layout.bins.iter().filter(|b|b.id!=source.id && (self.batch.all || self.batch.targets.contains(&b.id))).map(|b|b.id).collect();
            if ui.add_enabled(!targets.is_empty() && self.batch.fields.any() && !self.active(),egui::Button::new(format!("应用到 {} 个盒子",targets.len()))).clicked() {
                self.batch.message=Some(layout.apply_batch(source.id,&targets,&self.batch.fields,s));
            }
            if let Some(message)=&self.batch.message {
                match message {
                    Ok(n)=>{ui.colored_label(Color32::LIGHT_GREEN,format!("已同步 {n} 个盒子；各盒标签文本保持原样。"));}
                    Err(e)=>{ui.colored_label(Color32::LIGHT_RED,format!("批量设置未应用：{e}"));}
                }
            }
        });
    }
    pub fn cancel(&mut self) {
        self.drag = None;
    }
    pub fn active(&self) -> bool {
        self.drag.is_some()
    }
    fn fresh(layout: &Layout) -> Result<Bin, String> {
        Ok(Bin {
            id: layout
                .bins
                .iter()
                .map(|b| b.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("盒子编号溢出")?,
            ..Default::default()
        })
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
        for (a, edge) in edges.iter_mut().enumerate() {
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
            self.cancel();
        }
        if cancel {
            self.cancel();
            self.error = None;
        }
        if !cancel
            && self.drag.is_none()
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
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::proportional(13.),
                Color32::WHITE,
            );
            if Some(b.id) == *selected {
                for p in [
                    r.left_top(),
                    r.center_top(),
                    r.right_top(),
                    r.right_center(),
                    r.right_bottom(),
                    r.center_bottom(),
                    r.left_bottom(),
                    r.left_center(),
                ] {
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
                self.cancel();
            }
        } else if released {
            self.cancel();
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
        }
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
            egui::RichText::new(self.error.as_deref().unwrap_or(if self.active() {
                "绿色可放置，红色冲突；松开确认，Esc / 右键取消。"
            } else {
                "空白处拖框添加；拖动盒体移动，拖动选中盒子的边缘或角点调整尺寸。"
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
    fn batch_buttons_apply_all_or_checked_targets_without_copying_text() {
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
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200., 1000.))),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        editor.batch_ui(ui, layout, &Selection::default(), Some(1))
                    });
                },
            )
        }
        fn text_center(out: &egui::FullOutput, text: &str) -> Pos2 {
            out.shapes
                .iter()
                .find_map(|shape| {
                    if let egui::epaint::Shape::Text(t) = &shape.shape {
                        (t.galley.text() == text)
                            .then(|| Rect::from_min_size(t.pos, t.galley.size()).center())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("missing UI control: {text}"))
        }
        fn click(
            ctx: &egui::Context,
            editor: &mut Editor,
            layout: &mut Layout,
            time: &mut f64,
            text: &str,
        ) -> egui::FullOutput {
            let out = frame(ctx, editor, layout, time, vec![]);
            let pos = text_center(&out, text);
            frame(
                ctx,
                editor,
                layout,
                time,
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
                ctx,
                editor,
                layout,
                time,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
            )
        }
        let ctx = egui::Context::default();
        let mut time = 0.;
        let mut editor = Editor::default();
        let mut layout = Layout {
            bins: (0..3)
                .map(|i| Bin {
                    id: i + 1,
                    cell: [i, 0],
                    label: crate::label::Config {
                        enabled: false,
                        text: format!("unique-{i}"),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        layout.bins[0].scoop = false;
        frame(&ctx, &mut editor, &mut layout, &mut time, vec![]);
        click(&ctx, &mut editor, &mut layout, &mut time, "应用到 2 个盒子");
        assert!(layout.bins.iter().all(|b| !b.scoop));
        assert!(matches!(editor.batch.message, Some(Ok(2))));
        layout.bins[1].scoop = true;
        layout.bins[2].scoop = true;
        click(&ctx, &mut editor, &mut layout, &mut time, "勾选目标盒子");
        click(&ctx, &mut editor, &mut layout, &mut time, "盒子 2");
        click(&ctx, &mut editor, &mut layout, &mut time, "应用到 1 个盒子");
        assert!(!layout.bins[1].scoop);
        assert!(layout.bins[2].scoop);
        for (i, b) in layout.bins.iter().enumerate() {
            assert_eq!(b.label.text, format!("unique-{i}"));
            assert!(!b.label.enabled);
        }
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
