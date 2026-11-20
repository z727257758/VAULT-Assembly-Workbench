use super::*;
use crate::catalog::exterior::{Base, Board, Exterior};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq)]
enum Rear {
    Backbone,
    Wall,
}

impl Rear {
    fn available(self, member: &Member) -> bool {
        match self {
            Self::Backbone => member.preset.selection.height != "4F",
            Self::Wall => member.exterior.base == Base::None,
        }
    }
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
    if let Some((index, draft)) = cards(ui, &app.members, app.combination_gap, app.zh, action) {
        app.tower.error = app
            .apply_exterior(index, draft, ctx)
            .err()
            .unwrap_or_default();
    }
}

fn cards(
    ui: &mut egui::Ui,
    members: &[Member],
    gap: f32,
    z: bool,
    action: &mut Option<Action>,
) -> Option<(usize, Exterior)> {
    let mut change = None;
    let modules = combination::modules(members);
    let layout = crate::constraints::stacking::plan(&modules, gap);
    let mut vacant = Vec::new();
    for (index, member) in members.iter().enumerate().rev() {
        let Some(kind) = rear(&member.exterior) else {
            vacant.push((index, member));
            continue;
        };
        let mut draft = member.exterior.clone();
        let top = layout
            .as_ref()
            .is_ok_and(|plan| !plan.supports.iter().any(|support| support.contains(&index)));
        ui.push_id(("rear-modification", index), |ui| {
            crate::ui_style::card_frame(ui, true).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(32.);
                    ui.add_space(4.);
                    let accessories = t(z, "其他配件…", "Other accessories…");
                    let icon = rear_icon(ui, egui::Sense::click()).on_hover_text(accessories);
                    icon.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, accessories)
                    });
                    if icon.clicked() {
                        *action = Some(Action::Accessories(index));
                    }
                    ui.strong(t(z, "背面", "Rear"));
                    ui.label(RichText::new(rear_name(kind, z)).weak());
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
                        if members.len() > 1 {
                            let label = member_label(member, z);
                            ui.add(
                                egui::Label::new(RichText::new(&label).small().weak()).truncate(),
                            )
                            .on_hover_text(label);
                        }
                    });
                });
                crate::ui_style::inset_frame(ui)
                    .show(ui, |ui| rear_controls(ui, member, &mut draft, kind, top, z));
            });
        });
        ui.add_space(5.);
        if draft != member.exterior {
            change = Some((index, draft));
        }
    }
    if !vacant.is_empty() {
        let title = if vacant.len() == members.len() {
            t(z, "无背面", "No rear")
        } else {
            t(z, "添加背面", "Add rear")
        };
        let response = vacant_card(ui, title);
        egui::Popup::menu(&response).show(|ui| {
            for &(index, member) in &vacant {
                let mut choices = |ui: &mut egui::Ui| {
                    for kind in [Rear::Backbone, Rear::Wall] {
                        if ui
                            .add_enabled(
                                kind.available(member),
                                egui::Button::new(rear_name(kind, z)),
                            )
                            .clicked()
                        {
                            let mut draft = member.exterior.clone();
                            set_rear(&mut draft, Some(kind));
                            change = Some((index, draft));
                            ui.close_kind(egui::UiKind::Menu);
                        }
                    }
                    ui.separator();
                    if ui.button(t(z, "其他配件…", "Other accessories…")).clicked() {
                        *action = Some(Action::Accessories(index));
                        ui.close_kind(egui::UiKind::Menu);
                    }
                };
                if vacant.len() > 1 {
                    ui.menu_button(member_label(member, z), choices);
                } else {
                    if members.len() > 1 {
                        ui.small(member_label(member, z));
                    }
                    choices(ui);
                }
            }
        });
    }
    ui.add_space(6.);
    change
}

fn rear_controls(
    ui: &mut egui::Ui,
    member: &Member,
    draft: &mut Exterior,
    kind: Rear,
    top: bool,
    z: bool,
) {
    ui.spacing_mut().item_spacing.y = 2.;
    let mut selected = kind;
    ui.add_enabled_ui(
        Rear::Backbone.available(member) && Rear::Wall.available(member),
        |ui| {
            layers::choice_row(
                ui,
                t(z, "类型", "Type"),
                if z { 104. } else { 184. },
                &mut selected,
                &[
                    (Rear::Backbone, rear_name(Rear::Backbone, z)),
                    (Rear::Wall, rear_name(Rear::Wall, z)),
                ],
            );
        },
    )
    .response
    .on_disabled_hover_text(t(
        z,
        "4F 无背板安装位；带底座的成员无法挂墙。",
        "4F has no backbone mount. Wall mounting requires a member without a base.",
    ));
    if selected != kind {
        set_rear(draft, Some(selected));
    }
    if rear(draft) != Some(Rear::Backbone) {
        return;
    }
    ui.add_enabled_ui(!draft.pull_handle && !draft.stationary_handle, |ui| {
        layers::choice_row(
            ui,
            t(z, "洞洞板挂件", "Pegboard"),
            158.,
            &mut draft.board,
            &[
                (Board::None, t(z, "无", "None")),
                (Board::Ikea, "IKEA"),
                (Board::Ussa, "USSA"),
            ],
        );
    })
    .response
    .on_disabled_hover_text(t(
        z,
        "洞洞板挂件与把手共用安装位。",
        "Pegboard mounts and handles share the mounting points.",
    ));
    option_checkbox(
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
    option_checkbox(
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
        "{} {:02} / {} · {}{} {}",
        t(zh, "层", "Layer"),
        member.placement.level + 1,
        member.placement.column + 1,
        p.width,
        p.depth,
        p.height
    )
}

fn vacant_card(ui: &mut egui::Ui, title: &str) -> egui::Response {
    let response = layers::surface(ui, "add-rear", false, true, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(28.);
            rear_icon(ui, egui::Sense::hover());
            ui.label(RichText::new(title).weak());
        });
    })
    .response;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), title)
    });
    response
}

fn rear_icon(ui: &mut egui::Ui, sense: egui::Sense) -> egui::Response {
    let (slot, response) = ui.allocate_exact_size(egui::vec2(41., 26.), sense);
    let center = slot.left_center() + egui::vec2(9., 0.);
    let rect = egui::Rect::from_center_size(center, egui::vec2(13., 10.));
    let stroke = egui::Stroke::new(1., ui.visuals().weak_text_color());
    let painter = ui.painter();
    if sense.senses_click() && (response.hovered() || response.has_focus()) {
        painter.rect_filled(
            egui::Rect::from_center_size(center, egui::vec2(24., 24.)),
            5.,
            ui.visuals().widgets.hovered.weak_bg_fill,
        );
    }
    painter.rect_stroke(rect, 1., stroke, egui::StrokeKind::Inside);
    painter.hline(rect.x_range(), rect.center().y, stroke);
    painter.vline(rect.center().x - 3., rect.top()..=rect.center().y, stroke);
    painter.vline(
        rect.center().x + 3.,
        rect.center().y..=rect.bottom(),
        stroke,
    );
    if sense.senses_click() {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn option_checkbox(ui: &mut egui::Ui, label: &str, value: &mut bool, enabled: bool, hint: &str) {
    ui.add_enabled(enabled || *value, egui::Checkbox::new(value, label))
        .on_disabled_hover_text(hint);
}
