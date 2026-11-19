use super::*;

pub(super) struct Edit<'a> {
    pub layout: &'a mut Layout,
    pub selection: &'a Selection,
    pub selected: &'a mut Option<u32>,
    pub editor: &'a mut Editor,
}

impl State {
    pub(super) fn interact(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        view: &Projection,
        edit: &mut Edit<'_>,
    ) {
        let (press, pointer, released, cancel) = ui.input(|i| {
            (
                i.pointer.press_origin(),
                i.pointer.interact_pos(),
                i.pointer.button_released(egui::PointerButton::Primary),
                !i.focused
                    || i.key_pressed(egui::Key::Escape)
                    || i.pointer.button_pressed(egui::PointerButton::Secondary),
            )
        });
        if cancel {
            self.dragging = false;
            edit.editor.cancel();
            return;
        }
        let origin = view.screen([0., 0.], 0.);
        let cell_pixels = origin
            .distance(view.screen([1., 0.], 0.))
            .min(origin.distance(view.screen([0., 1.], 0.)))
            .max(1.);
        let margin = (7. / cell_pixels).min(0.22);
        let selected_rim = self.bins.iter().find(|b| Some(b.id) == *edit.selected);
        let height = selected_rim.map(|b| b.height);
        let selected_bin = edit
            .layout
            .bins
            .iter()
            .find(|b| Some(b.id) == *edit.selected);
        let rim_edge = |p| {
            let at = view.cell_at(p, height?)?;
            let edges = Editor::resize_edges(selected_bin?, at, margin);
            (edges != [0, 0]).then_some((at, edges))
        };
        let hover_edge = pointer
            .and_then(rim_edge)
            .and_then(|(_, edges)| Editor::resize_cursor(edges));
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some((at, edge_margin)) =
                press.filter(|p| view.rect.contains(*p)).and_then(|p| {
                    rim_edge(p)
                        .map(|(at, _)| (at, margin))
                        .or_else(|| self.hit(view, p).map(|(at, _)| (at, 0.)))
                })
        {
            response.request_focus();
            edit.editor.begin_grid_drag(
                GridView::Scene,
                edit.layout,
                edit.selection,
                edit.selected,
                at,
                edge_margin,
            );
            self.dragging = true;
        }
        // Rim handles resize in their own plane. Placement uses the surface
        // under the pointer, including a free socket on the baseplate.
        let grid_point = pointer.filter(|p| view.rect.contains(*p)).and_then(|p| {
            if edit.editor.resizing(GridView::Scene) {
                view.cell_at(p, height?)
            } else {
                self.hit(view, p).map(|(at, _)| at)
            }
        });
        edit.editor.update_grid_drag(
            GridView::Scene,
            edit.layout,
            edit.selection,
            edit.selected,
            grid_point,
            released,
        );
        if released {
            self.dragging = false;
        }
        if response.clicked_by(egui::PointerButton::Primary) {
            response.request_focus();
            *edit.selected = pointer
                .and_then(|p| self.hit(view, p))
                .and_then(|(at, _)| Editor::hit_bin(edit.layout, at));
            edit.editor.error = None;
        }
        if response.hovered()
            && let Some(cursor) = hover_edge
        {
            ui.ctx().set_cursor_icon(cursor);
        } else if response.hovered()
            && let Some((at, _)) = pointer.and_then(|p| self.hit(view, p))
        {
            let hit = Editor::hit_bin(edit.layout, at);
            ui.ctx().set_cursor_icon(if hit.is_some() {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::Crosshair
            });
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        edit.editor
            .grid_keyboard(response, edit.layout, edit.selection, edit.selected);
    }

    pub(super) fn paint(
        &self,
        painter: &egui::Painter,
        view: &Projection,
        edit: &Edit<'_>,
        dark: bool,
    ) {
        let grid_line = if dark {
            Color32::from_white_alpha(88)
        } else {
            Color32::from_black_alpha(92)
        };
        for y in 0..view.grid[1] {
            for x in 0..view.grid[0] {
                let center = [x as f32 + 0.5, y as f32 + 0.5];
                if !self.bins.iter().any(|bin| bin.contains(center)) {
                    let corners = view.outline([x as f32, y as f32], [1, 1], 0.);
                    painter.add(egui::Shape::closed_line(
                        corners.to_vec(),
                        egui::Stroke::new(0.8, grid_line),
                    ));
                }
            }
        }
        let accent = Color32::from_rgb(77, 177, 238);
        if let Some(b) = self.bins.iter().find(|b| Some(b.id) == *edit.selected) {
            let top = view.outline(b.cell.map(|v| v as f32), b.span, b.height);
            painter.add(egui::Shape::closed_line(
                top.to_vec(),
                egui::Stroke::new(2., accent),
            ));
            for i in 0..4 {
                let handle = top[i].lerp(top[(i + 1) % 4], 0.5);
                painter.circle(handle, 3.5, accent, egui::Stroke::new(1., Color32::WHITE));
            }
        }
        if let Some(drag) = edit.editor.drag_preview() {
            let cell = drag.cell.map(|v| v as f32);
            let height = self
                .bins
                .iter()
                .find(|bin| bin.id == drag.id)
                .map(|b| b.height)
                .unwrap_or_else(|| {
                    let units = if edit.layout.shared.auto_height {
                        gridfinity::max_units(edit.selection).unwrap_or(3)
                    } else {
                        edit.layout.shared.height_units
                    };
                    units as f32 * 7. + 3.55
                });
            let base = view.outline(cell, drag.span, 0.);
            let top = view.outline(cell, drag.span, height);
            let color = if drag.valid {
                Color32::from_rgb(52, 181, 125)
            } else {
                Color32::from_rgb(235, 83, 83)
            };
            let stroke = egui::Stroke::new(2., color);
            painter.add(egui::Shape::closed_line(base.to_vec(), stroke));
            for (a, b) in base.into_iter().zip(top) {
                painter.line_segment([a, b], stroke);
            }
            painter.add(egui::Shape::convex_polygon(
                top.to_vec(),
                color.gamma_multiply(0.22),
                stroke,
            ));
        }
    }
}
