//! Shared layer surfaces: children receive input before the card background.
use super::*;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Copy,
    Delete,
    Up,
    Down,
}

pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    enabled: bool,
    label: &str,
) -> egui::Response {
    // Keep disabled action regions above the card's background sense as well.
    let response = ui.add(
        crate::ui_style::quiet_button("")
            .frame(enabled)
            .min_size(egui::vec2(28., 28.)),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let ink = if enabled {
        ui.style().interact(&response).fg_stroke.color
    } else {
        ui.visuals().weak_text_color()
    };
    let stroke = egui::Stroke::new(1.3, ink);
    let center = response.rect.center();
    let painter = ui.painter();
    match icon {
        Icon::Copy => {
            for delta in [egui::vec2(2., -2.), egui::vec2(-2., 2.)] {
                painter.rect_stroke(
                    egui::Rect::from_center_size(center + delta, egui::vec2(10., 11.)),
                    0.,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Delete => {
            painter.line_segment(
                [center + egui::vec2(-7., -5.), center + egui::vec2(7., -5.)],
                stroke,
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    center + egui::vec2(-5., -5.),
                    center + egui::vec2(5., 7.),
                ),
                1.,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [center + egui::vec2(-2., -8.), center + egui::vec2(2., -8.)],
                stroke,
            );
            for x in [-2., 2.] {
                painter.line_segment(
                    [center + egui::vec2(x, -2.), center + egui::vec2(x, 4.)],
                    stroke,
                );
            }
        }
        Icon::Up | Icon::Down => {
            let direction = if matches!(icon, Icon::Up) { -1. } else { 1. };
            painter.line_segment(
                [center + egui::vec2(0., -7.), center + egui::vec2(0., 7.)],
                stroke,
            );
            let tip = center + egui::vec2(0., direction * 7.);
            for x in [-5., 5.] {
                painter.line_segment([tip, center + egui::vec2(x, direction * 2.)], stroke);
            }
        }
    }
    response.on_hover_text(label)
}

pub(super) fn surface<R>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    expanded: bool,
    empty: bool,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let response = ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(id)
            .sense(egui::Sense::click()),
        |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            let mut frame = crate::ui_style::card_frame(ui, expanded);
            let stroke = frame.stroke;
            if empty && !expanded {
                frame.stroke = egui::Stroke::NONE;
            }
            let content = frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                contents(ui)
            });
            if empty && !expanded {
                let rect = content.response.rect.shrink(stroke.width * 0.5);
                let radius = f32::from(frame.corner_radius.nw).min(rect.height() * 0.5);
                let mut path = Vec::new();
                for (center, start) in [
                    (
                        rect.right_top() + egui::vec2(-radius, radius),
                        -std::f32::consts::FRAC_PI_2,
                    ),
                    (rect.right_bottom() - egui::vec2(radius, radius), 0.),
                    (
                        rect.left_bottom() + egui::vec2(radius, -radius),
                        std::f32::consts::FRAC_PI_2,
                    ),
                    (
                        rect.left_top() + egui::vec2(radius, radius),
                        std::f32::consts::PI,
                    ),
                ] {
                    for step in 0..=8 {
                        let angle = start + step as f32 / 8. * std::f32::consts::FRAC_PI_2;
                        path.push(center + egui::vec2(angle.cos(), angle.sin()) * radius);
                    }
                }
                path.push(path[0]);
                ui.painter()
                    .extend(egui::Shape::dashed_line(&path, stroke, 6., 5.));
            }
            content.inner
        },
    );
    egui::InnerResponse::new(
        response.inner,
        response
            .response
            .on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

fn background_open(response: &egui::Response, header: egui::Rect, expanded: bool) -> Option<bool> {
    response.clicked().then(|| {
        // Blank content opens/keeps the layer. Its header and keyboard activation toggle it.
        !expanded
            || response
                .interact_pointer_pos()
                .is_some_and(|p| !header.contains(p))
    })
}

pub(super) struct Header<'a> {
    pub level: u32,
    pub kind: &'a str,
    pub size: &'a str,
    pub height: &'a str,
    pub height_mm: f32,
    pub can_delete: bool,
    pub zh: bool,
}
pub(super) struct LayerResponse {
    pub response: egui::Response,
    pub open: Option<bool>,
    pub action: Option<Action>,
}
pub(super) fn layer(
    ui: &mut egui::Ui,
    header: Header<'_>,
    expanded: bool,
    body: impl FnOnce(&mut egui::Ui),
) -> LayerResponse {
    let mut header_rect = egui::Rect::NOTHING;
    let mut drag_rect = egui::Rect::NOTHING;
    let mut action = None;
    let response = surface(ui, ("layer-card", header.level), expanded, false, |ui| {
        let row = ui.horizontal(|ui| {
            ui.set_min_height(32.);
            drag_rect = ui
                .dnd_drag_source(
                    egui::Id::new(("tower-layer", header.level)),
                    header.level,
                    |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(16., 28.), egui::Sense::hover());
                        for y in [-5., 0., 5.] {
                            for x in [-2.5, 2.5] {
                                ui.painter().circle_filled(
                                    rect.center() + egui::vec2(x, y),
                                    1.,
                                    ui.visuals().weak_text_color(),
                                );
                            }
                        }
                    },
                )
                .response
                .on_hover_text(t(header.zh, "拖动排序", "Drag to reorder"))
                .rect;
            ui.label(RichText::new(format!("{:02}", header.level + 1)).weak());
            ui.strong(header.kind);
            ui.add(egui::Label::new(RichText::new(header.size).weak()).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if expanded {
                    if icon_button(
                        ui,
                        Icon::Delete,
                        header.can_delete,
                        t(header.zh, "删除这一层", "Delete layer"),
                    )
                    .clicked()
                        && header.can_delete
                    {
                        action = Some(Action::Delete(header.level));
                    }
                    if icon_button(
                        ui,
                        Icon::Copy,
                        true,
                        t(header.zh, "复制这一层", "Duplicate layer"),
                    )
                    .clicked()
                    {
                        action = Some(Action::Duplicate(header.level));
                    }
                } else {
                    ui.label(RichText::new(format!("{:.0} mm", header.height_mm)).weak());
                    ui.label(header.height);
                }
            });
        });
        header_rect = row.response.rect;
        if expanded {
            crate::ui_style::inset_frame(ui).show(ui, body);
        }
    });
    response.response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::CollapsingHeader,
            ui.is_enabled(),
            expanded,
            format!("{} {} {}", header.level + 1, header.kind, header.size),
        )
    });
    let on_handle = response
        .response
        .interact_pointer_pos()
        .is_some_and(|p| drag_rect.contains(p));
    let open = if on_handle {
        None
    } else {
        background_open(&response.response, header_rect, expanded)
    };
    LayerResponse {
        response: response.response,
        open,
        action,
    }
}

pub(super) fn group(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    default_open: bool,
    summary: impl FnOnce(&mut egui::Ui),
) -> bool {
    let id = ui.make_persistent_id(("tower-group", id));
    let open = ui
        .data(|data| data.get_temp::<bool>(id))
        .unwrap_or(default_open);
    let response = ui
        .scope_builder(
            egui::UiBuilder::new()
                .id_salt(id)
                .sense(egui::Sense::click()),
            |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                ui.horizontal(|ui| {
                    ui.set_min_height(39.);
                    ui.strong(title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(14., 14.), egui::Sense::hover());
                        let center = rect.center();
                        let offset = if open { 2.5 } else { -2.5 };
                        ui.painter().line(
                            vec![
                                center + egui::vec2(-4.5, offset),
                                center - egui::vec2(0., offset),
                                center + egui::vec2(4.5, offset),
                            ],
                            egui::Stroke::new(1.2, ui.visuals().weak_text_color()),
                        );
                        summary(ui);
                    });
                });
            },
        )
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, title)
    });
    if response.clicked() {
        ui.data_mut(|data| data.insert_temp(id, !open));
        !open
    } else {
        open
    }
}

pub(super) fn set_group(ui: &egui::Ui, id: &str, open: bool) {
    let id = ui.make_persistent_id(("tower-group", id));
    ui.data_mut(|data| data.insert_temp(id, open));
}

pub(super) fn choice_row<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
    value: &mut T,
    choices: &[(T, &str)],
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(width.min(ui.available_width()), 32.),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| crate::ui_style::segmented_choices(ui, value, choices),
            );
        });
    });
}

pub(super) fn palette(ui: &mut egui::Ui, palette: &[[u8; 3]], limit: usize) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(limit as f32 * 14., 14.), egui::Sense::hover());
    for (i, &[r, g, b]) in palette.iter().take(limit).enumerate() {
        let center = rect.left_center() + egui::vec2(7. + i as f32 * 14., 0.);
        ui.painter().circle(
            center,
            4.8,
            Color32::from_rgb(r, g, b),
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
    }
}

pub(super) fn section(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    summary: &str,
    empty: bool,
    expanded: bool,
    body: impl FnOnce(&mut egui::Ui),
) -> egui::InnerResponse<Option<bool>> {
    let mut header = egui::Rect::NOTHING;
    let response = surface(ui, id, expanded, empty, |ui| {
        header = ui
            .horizontal(|ui| {
                ui.set_min_height(32.);
                ui.label(RichText::new(if id == "tower-roofs" { "↑" } else { "↓" }).weak());
                ui.add(
                    egui::Label::new(if empty {
                        RichText::new(title).weak()
                    } else {
                        RichText::new(title).strong()
                    })
                    .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(RichText::new(summary).weak());
                });
            })
            .response
            .rect;
        if expanded {
            crate::ui_style::inset_frame(ui).show(ui, body);
        }
    });
    response.response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::CollapsingHeader,
            ui.is_enabled(),
            expanded,
            title,
        )
    });
    let open = background_open(&response.response, header, expanded);
    egui::InnerResponse::new(open, response.response)
}

pub(super) fn reveal_card(ui: &egui::Ui, mut rect: egui::Rect) {
    let visible_height = (ui.clip_rect().height() - 20.).max(40.);
    rect.max.y = rect.max.y.min(rect.min.y + visible_height);
    ui.scroll_to_rect(rect, None);
}
