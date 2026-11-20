//! Plan view of the same measured wall slots and placements used in the 3D editor.
use crate::sustainer_dividers::{Axis, Geometry, Layout, Placement};
use eframe::egui::{self, Pos2, Rect, Stroke, Vec2};

#[derive(Default)]
pub struct Editor {
    pub open: bool,
    pub selected: Option<usize>,
    pub(crate) plan_rect: Option<Rect>,
    pub(crate) error: Option<String>,
    undo: Vec<Layout>,
    redo: Vec<Layout>,
    drag: Option<(usize, Layout)>,
}

fn remember(history: &mut Vec<Layout>, layout: Layout) {
    if history.len() == 100 {
        history.remove(0);
    }
    history.push(layout);
}

impl Editor {
    pub(crate) fn close(&mut self, layout: &Layout) {
        if let Some((_, before)) = self.drag.take()
            && before != *layout
        {
            remember(&mut self.undo, before);
            self.redo.clear();
        }
        self.open = false;
        self.plan_rect = None;
        self.error = None;
    }

    pub fn reset(&mut self) {
        *self = Self {
            open: self.open,
            ..Default::default()
        };
    }

    fn replace(&mut self, layout: &mut Layout, next: Layout) -> bool {
        if *layout == next {
            return false;
        }
        remember(&mut self.undo, std::mem::replace(layout, next));
        self.redo.clear();
        self.selected = None;
        self.drag = None;
        true
    }

    pub(crate) fn toggle_slot(
        &mut self,
        layout: &mut Layout,
        g: &Geometry,
        slot: Placement,
    ) -> bool {
        if g.line(&slot).is_none()
            || (slot.axis == Axis::Depth && slot.lane != 0)
            || (!layout.items.is_empty() && layout.specification != g.specification)
        {
            return false;
        }
        let mut next = layout.clone();
        next.specification.clone_from(&g.specification);
        let selected = if let Some(i) = next.items.iter().position(|p| *p == slot) {
            next.items.remove(i);
            None
        } else {
            next.items.push(slot);
            Some(next.items.len() - 1)
        };
        self.replace(layout, next);
        self.selected = selected;
        true
    }

    fn history(&mut self, layout: &mut Layout, redo: bool) -> bool {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let Some(previous) = from.pop() else {
            return false;
        };
        remember(to, std::mem::replace(layout, previous));
        self.selected = None;
        self.drag = None;
        true
    }

    fn presets(&mut self, ui: &mut egui::Ui, layout: &mut Layout, g: &Geometry, zh: bool) -> bool {
        let mut changed = false;
        ui.menu_button(if zh { "预设" } else { "Presets" }, |ui| {
            let mut option = |title: &str, items: Vec<Placement>| {
                if ui.button(title).clicked() {
                    changed |= self.replace(
                        layout,
                        Layout {
                            specification: g.specification.clone(),
                            items,
                        },
                    );
                    ui.close();
                }
            };
            option(if zh { "留空" } else { "Empty" }, Vec::new());
            option(
                if zh { "居中横分" } else { "Center row" },
                g.slots()
                    .filter(|p| {
                        p.axis == Axis::Width && p.slot == g.rows.len().saturating_sub(1) / 2
                    })
                    .collect(),
            );
            option(
                if zh { "填满横槽" } else { "All rows" },
                g.slots().filter(|p| p.axis == Axis::Width).collect(),
            );
            if !g.columns.is_empty() {
                option(
                    if zh { "填满纵槽" } else { "All columns" },
                    g.choices(Axis::Depth, 0),
                );
                option(
                    if zh { "纵横网格" } else { "Full grid" },
                    g.slots().collect(),
                );
            }
        });
        changed
    }

    // Partial fills add to the current layout. This also works for a sealed case
    // whose separate bays take different divider parts in the same numbered row.
    fn fill_bay(&mut self, ui: &mut egui::Ui, layout: &mut Layout, g: &Geometry, zh: bool) -> bool {
        let mut changed = false;
        ui.menu_button(if zh { "补齐槽位" } else { "Fill slots" }, |ui| {
            let mut fill = |axis, lane, title: String| {
                let free: Vec<_> = g
                    .choices(axis, lane)
                    .into_iter()
                    .filter(|p| !layout.items.contains(p))
                    .collect();
                if ui
                    .add_enabled(!free.is_empty(), egui::Button::new(title))
                    .clicked()
                {
                    let mut next = layout.clone();
                    next.specification.clone_from(&g.specification);
                    next.items.extend(free);
                    changed = self.replace(layout, next);
                    ui.close();
                }
            };
            for lane in 0..g.lanes.len() {
                let title = if g.lanes.len() == 1 {
                    if zh {
                        "补齐横槽".into()
                    } else {
                        "Fill remaining rows".into()
                    }
                } else if zh {
                    format!("补齐区域 {} 横槽", lane + 1)
                } else {
                    format!("Fill rows in bay {}", lane + 1)
                };
                fill(Axis::Width, lane, title);
            }
            if !g.columns.is_empty() {
                fill(
                    Axis::Depth,
                    0,
                    if zh {
                        "补齐纵槽".into()
                    } else {
                        "Fill remaining columns".into()
                    },
                );
            }
        });
        changed
    }

    pub(crate) fn panel(
        &mut self,
        ui: &mut egui::Ui,
        geometry: Option<&Geometry>,
        layout: &mut Layout,
        zh: bool,
    ) -> bool {
        let Some(g) = geometry else {
            self.plan_rect = None;
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(if zh {
                    "正在读取箱体槽位…"
                } else {
                    "Reading case slots…"
                });
            });
            return false;
        };
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            changed |= self.presets(ui, layout, g, zh);
            let n = layout.items.len();
            let cells = g.compartment_count(layout);
            ui.weak(if zh {
                format!("{n} 块隔板 · {cells} 格")
            } else {
                format!("{n} dividers · {cells} compartments")
            });
        });
        changed |= self.canvas(ui, layout, g, zh);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.undo.is_empty(),
                    egui::Button::new(if zh { "撤销" } else { "Undo" }),
                )
                .clicked()
            {
                changed |= self.history(layout, false);
            }
            if ui
                .add_enabled(
                    !self.redo.is_empty(),
                    egui::Button::new(if zh { "重做" } else { "Redo" }),
                )
                .clicked()
            {
                changed |= self.history(layout, true);
            }
            if g.lanes.len() > 1 || !g.columns.is_empty() {
                changed |= self.fill_bay(ui, layout, g, zh);
            }
        });
        ui.weak(if zh {
            "点击槽位装上或取下隔板。在简图中拖动隔板可换槽。"
        } else {
            "Click a slot to add or remove a divider. Drag a divider in the plan to move it."
        });
        changed
    }

    fn canvas(&mut self, ui: &mut egui::Ui, layout: &mut Layout, g: &Geometry, zh: bool) -> bool {
        let size = Vec2::new(g.bounds[2] - g.bounds[0], g.bounds[3] - g.bounds[1]);
        let width = ui.available_width().max(1.);
        let scale = (width / size.x).min(300. / size.y);
        let (space, response) = ui.allocate_exact_size(
            Vec2::new(width, size.y * scale + 22.),
            egui::Sense::click_and_drag(),
        );
        let rect = Rect::from_min_size(
            space.center_top() - Vec2::new(size.x * scale * 0.5, 0.),
            size * scale,
        );
        self.plan_rect = Some(rect);
        let screen =
            |p: [f32; 2]| rect.min + Vec2::new(p[0] - g.bounds[0], p[1] - g.bounds[1]) * scale;
        let line = |slot: &Placement| g.line(slot).map(|ends| ends.map(screen));
        let candidates: Vec<_> = g.slots().collect();
        let nearest = |pos: Pos2, axis: Option<Axis>| {
            if !rect.contains(pos) {
                return None;
            }
            candidates
                .iter()
                .filter(|p| axis.is_none_or(|a| a == p.axis))
                .filter_map(|p| {
                    let [start, end] = line(p)?;
                    if p.axis == Axis::Width && !(start.x..=end.x).contains(&pos.x) {
                        return None;
                    }
                    let segment = end - start;
                    let factor = ((pos - start).dot(segment) / segment.length_sq()).clamp(0., 1.);
                    let distance = pos.distance(start + segment * factor);
                    (distance <= 8.).then_some((p, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(slot, _)| slot.clone())
        };
        if response.drag_started_by(egui::PointerButton::Primary) {
            let source = ui
                .input(|i| i.pointer.press_origin())
                .and_then(|pos| nearest(pos, None));
            self.selected = source.and_then(|p| layout.items.iter().position(|item| *item == p));
            self.drag = self.selected.map(|index| (index, layout.clone()));
        }
        let primary = egui::PointerButton::Primary;
        let hover = response
            .hover_pos()
            .or_else(|| response.interact_pointer_pos())
            .and_then(|pos| {
                nearest(
                    pos,
                    self.drag
                        .as_ref()
                        .map(|(i, baseline)| baseline.items[*i].axis),
                )
            });
        let mut changed = false;
        if response.clicked_by(primary)
            && let Some(p) = hover.as_ref()
        {
            changed = self.toggle_slot(layout, g, p.clone());
        }
        if let Some((index, baseline)) = &self.drag {
            if ui.input(|i| i.key_pressed(egui::Key::Escape) || !i.focused) {
                changed = layout != baseline;
                layout.clone_from(baseline);
                self.drag = None;
            } else if let Some(p) = &hover
                && layout.items.get(*index).is_some_and(|item| item != p)
                && !layout.items.contains(p)
            {
                layout.items[*index] = p.clone();
                changed = true;
            }
        }
        if response.drag_stopped()
            && let Some((index, before)) = self.drag.take()
            && before != *layout
        {
            if layout.items.get(index) == hover.as_ref() {
                remember(&mut self.undo, before);
                self.redo.clear();
            } else {
                *layout = before;
                changed = true;
            }
        }
        let visuals = ui.visuals();
        let paint = ui.painter();
        paint.rect_filled(rect, 5., visuals.faint_bg_color);
        for walls in g.lanes.windows(2) {
            paint.rect_filled(
                Rect::from_min_max(
                    screen([walls[0][1], g.bounds[1]]),
                    screen([walls[1][0], g.bounds[3]]),
                ),
                1.,
                visuals.widgets.inactive.bg_stroke.color,
            );
        }
        for p in &candidates {
            let Some(segment) = line(p) else { continue };
            let index = layout.items.iter().position(|item| item == p);
            let pointed = hover.as_ref() == Some(p);
            if index.is_some() || pointed {
                let selected = index.is_some() && index == self.selected;
                let color = if pointed && index.is_some() && self.drag.is_some() && !selected {
                    visuals.error_fg_color
                } else if pointed || selected {
                    visuals.hyperlink_color
                } else {
                    visuals.text_color()
                };
                paint.line_segment(
                    segment,
                    Stroke::new(if index.is_some() { 4. } else { 2. }, color),
                );
            } else {
                paint.add(egui::Shape::dashed_line(
                    &segment,
                    Stroke::new(1., visuals.widgets.inactive.bg_stroke.color),
                    4.,
                    4.,
                ));
            }
        }
        paint.rect_stroke(rect, 5., visuals.window_stroke, egui::StrokeKind::Inside);
        paint.text(
            rect.center_bottom() + Vec2::new(0., 11.),
            egui::Align2::CENTER_CENTER,
            if zh { "前" } else { "Front" },
            egui::TextStyle::Small.resolve(ui.style()),
            visuals.weak_text_color(),
        );
        if hover.is_some() {
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_bays() -> Geometry {
        Geometry {
            specification: "test-two-bays".into(),
            bounds: [-50., -60., 50., 60.],
            rows: vec![-30., 0., 30.],
            lanes: vec![[-50., -2.], [2., 50.]],
            ..Default::default()
        }
    }

    fn row(lane: usize, slot: usize) -> Placement {
        Placement {
            axis: Axis::Width,
            slot,
            lane,
        }
    }

    #[test]
    fn both_views_toggle_the_same_layout_and_reject_unmeasured_slots() {
        let g = two_bays();
        let mut e = Editor::default();
        let mut l = Layout::default();
        assert!(e.toggle_slot(&mut l, &g, row(0, 0)));
        assert!(e.toggle_slot(&mut l, &g, row(1, 0)));
        assert!(!e.toggle_slot(&mut l, &g, row(2, 0)));
        assert!(!e.toggle_slot(&mut l, &g, row(0, 3)));
        assert!(e.toggle_slot(&mut l, &g, row(0, 0)));
        assert_eq!(l.items, [row(1, 0)]);
        assert!(e.history(&mut l, false));
        assert_eq!(l.items, [row(0, 0), row(1, 0)]);
        assert!(e.history(&mut l, true));
        assert_eq!(l.items, [row(1, 0)]);
        l.specification = "other-size".into();
        assert!(!e.toggle_slot(&mut l, &g, row(1, 0)));
    }

    #[test]
    fn actual_plan_pointer_toggles_drags_and_leaves_the_fixed_wall_alone() {
        let ctx = egui::Context::default();
        let g = two_bays();
        let mut e = Editor::default();
        let mut l = Layout::default();
        let mut time = 0.;
        let mut frame = |events, e: &mut Editor, l: &mut Layout| {
            time += 1. / 60.;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(420., 640.))),
                    events,
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        e.panel(ui, Some(&g), l, true);
                    });
                },
            );
        };
        frame(vec![], &mut e, &mut l);
        frame(vec![], &mut e, &mut l);
        let rect = e.plan_rect.unwrap();
        let point = |x: f32, y: f32| rect.min + rect.size() * Vec2::new(x, y);
        let click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let from = point(0.25, 0.25);
        let to = point(0.75, 0.75);
        for (pos, expected) in [(point(0.5, 0.25), 0), (from, 1), (from, 0), (from, 1)] {
            frame(
                vec![egui::Event::PointerMoved(pos), click(pos, true)],
                &mut e,
                &mut l,
            );
            frame(vec![click(pos, false)], &mut e, &mut l);
            assert_eq!(l.items.len(), expected);
            assert_eq!(e.plan_rect, Some(rect));
        }
        frame(
            vec![egui::Event::PointerMoved(from), click(from, true)],
            &mut e,
            &mut l,
        );
        frame(vec![egui::Event::PointerMoved(to)], &mut e, &mut l);
        frame(vec![click(to, false)], &mut e, &mut l);
        assert_eq!(l.items, [row(1, 2)]);
        assert!(e.history(&mut l, false));
        assert_eq!(l.items, [row(0, 0)]);
        assert!(e.toggle_slot(&mut l, &g, row(1, 2)));
        let original = l.clone();
        let history = e.undo.len();
        frame(
            vec![egui::Event::PointerMoved(from), click(from, true)],
            &mut e,
            &mut l,
        );
        frame(
            vec![egui::Event::PointerMoved(point(0.25, 0.5))],
            &mut e,
            &mut l,
        );
        assert_eq!(l.items[0], row(0, 1));
        frame(vec![egui::Event::PointerMoved(to)], &mut e, &mut l);
        frame(vec![click(to, false)], &mut e, &mut l);
        assert_eq!(l, original);
        assert_eq!(e.undo.len(), history);
    }
}
