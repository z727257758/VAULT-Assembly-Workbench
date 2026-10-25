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
#[derive(Default)]
pub struct Editor {
    drag: Option<Drag>,
    pub error: Option<String>,
}
impl Editor {
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
