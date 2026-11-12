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
        egui::Button::new("")
            .frame(false)
            .min_size(egui::vec2(28., 28.)),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let ink = if enabled {
        ui.visuals().text_color()
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
            let stroke = egui::Stroke::new(
                1.,
                if expanded {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().widgets.noninteractive.bg_stroke.color
                },
            );
            let frame = egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .corner_radius(13.)
                .inner_margin(8.)
                .stroke(if empty && !expanded {
                    egui::Stroke::NONE
                } else {
                    stroke
                });
            let content = frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                contents(ui)
            });
            if empty && !expanded {
                let rect = content.response.rect.shrink(0.5);
                let radius = 12_f32.min(rect.height() * 0.5);
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
                    ui.label(RichText::new(header.height).weak());
                }
            });
        });
        header_rect = row.response.rect;
        if expanded {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .corner_radius(9.)
                .inner_margin(8.)
                .show(ui, body);
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

pub(super) fn section(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    summary: &str,
    empty: bool,
    default_open: bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    let state_id = ui.make_persistent_id(("layer-section", id));
    let expanded = ui
        .data_mut(|d| d.get_temp::<bool>(state_id))
        .unwrap_or(default_open);
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
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .corner_radius(9.)
                .inner_margin(8.)
                .show(ui, body);
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
    if let Some(open) = background_open(&response.response, header, expanded) {
        ui.data_mut(|d| d.insert_temp(state_id, open));
    }
}
