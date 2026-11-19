use super::*;
use crate::catalog::exterior::{Base, Board, Exterior};

#[derive(Clone, Copy, PartialEq)]
enum Rear {
    Backbone,
    Wall,
}

fn rear(exterior: &Exterior) -> Option<Rear> {
    if exterior.wall_mount {
        Some(Rear::Wall)
    } else if exterior.backbone {
        Some(Rear::Backbone)
    } else {
        None
    }
}

fn set_rear(exterior: &mut Exterior, rear: Option<Rear>) {
    if rear != Some(Rear::Backbone) {
        exterior.board = Board::None;
        exterior.pull_handle = false;
        exterior.pull_extension = 0.;
        exterior.stationary_handle = false;
    }
    exterior.backbone = rear == Some(Rear::Backbone);
    exterior.wall_mount = rear == Some(Rear::Wall);
}

pub(super) fn show(
    app: &mut App,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    action: &mut Option<Action>,
) {
    let z = app.zh;
    let mut change = None;
    let members = &app.members;
    let modules = combination::modules(members);
    let layout = crate::constraints::stacking::plan(&modules, app.combination_gap);
    let configured = members
        .iter()
        .filter(|member| rear(&member.exterior).is_some())
        .count();
    for (index, member) in members.iter().enumerate().rev() {
        let Some(kind) = rear(&member.exterior) else {
            continue;
        };
        let mut draft = member.exterior.clone();
        let top = layout
            .as_ref()
            .is_ok_and(|plan| !plan.supports.iter().any(|support| support.contains(&index)));
        let allow_backbone = member.preset.selection.height != "4F";
        let allow_wall = member.exterior.base == Base::None;
        let label = member_label(member, z);
        layers::surface(ui, ("rear-modification", index), true, false, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(28.);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(22., 18.), egui::Sense::hover());
                paint_back(ui, rect.center());
                ui.strong(t(z, "背面", "Rear"));
                ui.label(RichText::new(rear_name(kind, z)).weak());
                if members.len() > 1 {
                    ui.small(&label);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if layers::icon_button(
                        ui,
                        layers::Icon::Delete,
                        true,
                        t(z, "移除背面改装", "Remove rear modification"),
                    )
                    .clicked()
                    {
                        set_rear(&mut draft, None);
                    }
                });
            });
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(9.)
                .inner_margin(7.)
                .show(ui, |ui| {
                    let mut selected = kind;
                    control_row(ui, t(z, "类型", "Type"), if z { 100. } else { 150. }, 36., |ui| {
                        ui.add_enabled_ui(allow_backbone && allow_wall, |ui| {
                            segmented(ui, &mut selected, &[
                                (Rear::Backbone, rear_name(Rear::Backbone, z)),
                                (Rear::Wall, rear_name(Rear::Wall, z)),
                            ]);
                        }).response.on_disabled_hover_text(t(z, "4F 无背板安装位；带底座的成员无法挂墙。", "4F has no backbone mount. Wall mounting requires a member without a base."));
                    });
                    if selected != kind {
                        set_rear(&mut draft, Some(selected));
                    }
                    if rear(&draft) != Some(Rear::Backbone) {
                        return;
                    }
                    control_row(ui, t(z, "洞洞板挂件", "Pegboard"), 144., 36., |ui| {
                        ui.add_enabled_ui(!draft.pull_handle && !draft.stationary_handle, |ui| {
                            segmented(ui, &mut draft.board, &[(Board::None, t(z, "无", "None")), (Board::Ikea, "IKEA"), (Board::Ussa, "USSA")]);
                        }).response.on_disabled_hover_text(t(z, "洞洞板挂件与把手共用安装位。", "Pegboard mounts and handles share the mounting points."));
                    });
                    switch(
                        ui,
                        t(z, "拉杆", "Pull handle"),
                        &mut draft.pull_handle,
                        top && draft.board == Board::None
                            && !draft.stationary_handle
                            && draft.roof != crate::catalog::exterior::Roof::UnderTable,
                        t(
                            z,
                            "拉杆需要顶部安装位，且不能与其他背面配件共用。",
                            "The pull handle requires a top member and a free rear mount.",
                        ),
                    );
                    if !draft.pull_handle {
                        draft.pull_extension = 0.;
                    }
                    switch(
                        ui,
                        t(z, "固定把手", "Fixed handle"),
                        &mut draft.stationary_handle,
                        !draft.pull_handle && draft.board == Board::None,
                        t(
                            z,
                            "固定把手需要空闲的背面安装位。",
                            "The fixed handle requires a free rear mount.",
                        ),
                    );
                });
        });
        ui.add_space(5.);
        if draft != member.exterior {
            change = Some((index, draft));
        }
    }
    ui.horizontal_wrapped(|ui| {
        if configured < members.len() {
            ui.menu_button(t(z, "＋ 背面改装", "＋ Rear modification"), |ui| {
                for (index, member) in members
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, member)| rear(&member.exterior).is_none())
                {
                    ui.menu_button(member_label(member, z), |ui| {
                        for (kind, enabled) in [
                            (Rear::Backbone, member.preset.selection.height != "4F"),
                            (Rear::Wall, member.exterior.base == Base::None),
                        ] {
                            if ui
                                .add_enabled(enabled, egui::Button::new(rear_name(kind, z)))
                                .clicked()
                            {
                                let mut draft = member.exterior.clone();
                                set_rear(&mut draft, Some(kind));
                                change = Some((index, draft));
                                ui.close_kind(egui::UiKind::Menu);
                            }
                        }
                    });
                }
            });
        }
        ui.menu_button(t(z, "其他配件…", "Other accessories…"), |ui| {
            for (index, member) in members.iter().enumerate().rev() {
                if ui.button(member_label(member, z)).clicked() {
                    *action = Some(Action::Accessories(index));
                    ui.close();
                }
            }
        });
    });
    ui.add_space(6.);
    if let Some((index, draft)) = change {
        app.tower.error = app
            .apply_exterior(index, draft, ctx)
            .err()
            .unwrap_or_default();
    }
}

fn rear_name(kind: Rear, zh: bool) -> &'static str {
    match kind {
        Rear::Backbone => t(zh, "背板", "Backbone"),
        Rear::Wall => t(zh, "挂墙", "Wall mount"),
    }
}

fn member_label(member: &Member, zh: bool) -> String {
    let p = &member.preset.selection;
    format!(
        "{} {:02} · {}{} {}",
        t(zh, "层", "Layer"),
        member.placement.level + 1,
        p.width,
        p.depth,
        p.height
    )
}

fn paint_back(ui: &egui::Ui, center: egui::Pos2) {
    let rect = egui::Rect::from_center_size(center, egui::vec2(13., 10.));
    let stroke = egui::Stroke::new(1., ui.visuals().weak_text_color());
    let painter = ui.painter();
    painter.rect_stroke(rect, 1., stroke, egui::StrokeKind::Inside);
    painter.hline(rect.x_range(), rect.center().y, stroke);
    painter.vline(rect.center().x - 3., rect.top()..=rect.center().y, stroke);
    painter.vline(
        rect.center().x + 3.,
        rect.center().y..=rect.bottom(),
        stroke,
    );
}

fn control_row(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
    height: f32,
    controls: impl FnOnce(&mut egui::Ui),
) {
    let (row, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter().text(
        row.left_center(),
        egui::Align2::LEFT_CENTER,
        label,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().weak_text_color(),
    );
    let right = egui::Rect::from_min_max(
        egui::pos2((row.right() - width).max(row.left()), row.top()),
        row.right_bottom(),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(label)
            .max_rect(right)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    controls(&mut child);
}

fn segmented<T: PartialEq + Clone>(ui: &mut egui::Ui, value: &mut T, choices: &[(T, &str)]) {
    let track = ui.available_rect_before_wrap().shrink2(egui::vec2(0., 1.));
    let visuals = ui.visuals();
    ui.painter().rect(
        track,
        8.,
        Color32::from_gray(if visuals.dark_mode { 52 } else { 233 }),
        visuals.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let content = track.shrink(3.);
    let segment = content.width() / choices.len() as f32;
    for (index, (choice, label)) in choices.iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            content.min + egui::vec2(segment * index as f32, 0.),
            egui::vec2(segment, content.height()),
        )
        .shrink2(egui::vec2(0.7, 0.));
        let selected = value == choice;
        let response = ui.interact(rect, ui.make_persistent_id(index), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                *label,
            )
        });
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect(
                rect,
                6.,
                ui.visuals().panel_fill,
                ui.visuals().widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let color = if ui.is_enabled() && selected {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::TextStyle::Body.resolve(ui.style()),
            color,
        );
        if response.clicked() {
            *value = choice.clone();
        }
    }
}

fn switch(ui: &mut egui::Ui, label: &str, value: &mut bool, enabled: bool, hint: &str) {
    control_row(ui, label, 36., 30., |ui| {
        ui.add_enabled_ui(enabled || *value, |ui| {
            let response = ui.add(
                egui::Button::new("")
                    .frame(false)
                    .min_size(egui::vec2(36., 20.)),
            );
            if response.clicked() {
                *value = !*value;
            }
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    ui.is_enabled(),
                    *value,
                    label,
                )
            });
            let rect = egui::Rect::from_center_size(response.rect.center(), egui::vec2(36., 20.));
            let fill = if *value {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals()
                    .widgets
                    .inactive
                    .fg_stroke
                    .color
                    .gamma_multiply(0.5)
            };
            let knob = egui::pos2(
                if *value {
                    rect.right() - 10.
                } else {
                    rect.left() + 10.
                },
                rect.center().y,
            );
            ui.painter().rect_filled(rect, 10., fill);
            ui.painter()
                .circle_filled(knob, 8., ui.visuals().panel_fill);
        })
        .response
        .on_disabled_hover_text(hint);
    });
}
