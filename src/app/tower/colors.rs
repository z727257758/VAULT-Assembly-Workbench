use super::*;
use crate::catalog::colors as catalog_colors;
use crate::print_plate::{ColorGroup, ColorTheme};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Group {
    Role(&'static str),
    Custom(usize),
}

pub(super) struct Edit {
    group: Option<Group>,
    name: String,
    keys: BTreeSet<String>,
    slot: u32,
}

impl Edit {
    fn apply_to(&self, theme: &mut ColorTheme) {
        if let Some(Group::Role(role)) = self.group {
            theme.roles.insert(role.to_owned(), self.slot);
            return;
        }
        for group in &mut theme.groups {
            group.components.retain(|key| !self.keys.contains(key));
        }
        let updated = ColorGroup {
            name: self.name.trim().to_owned(),
            components: self.keys.clone(),
            slot: self.slot,
        };
        if let Some(Group::Custom(index)) = self.group {
            theme.groups[index] = updated;
        } else {
            theme.groups.push(updated);
        }
        theme.groups.retain(|group| !group.components.is_empty());
    }

    fn name_available(&self, theme: &ColorTheme) -> bool {
        if matches!(self.group, Some(Group::Role(_))) {
            return true;
        }
        let name = self.name.trim().to_lowercase();
        !name.is_empty()
            && !catalog_colors::ROLES
                .iter()
                .any(|(_, cn, en)| name == cn.to_lowercase() || name == en.to_lowercase())
            && !theme.groups.iter().enumerate().any(|(index, group)| {
                Some(Group::Custom(index)) != self.group && group.name.trim().to_lowercase() == name
            })
    }
}

struct Item {
    path: String,
    count: u32,
}

fn items(app: &App) -> BTreeMap<String, Item> {
    let mut result = BTreeMap::new();
    for part in app.tower_print_items() {
        let item = result
            .entry(catalog_colors::component(&part.path))
            .or_insert(Item {
                path: part.path,
                count: 0,
            });
        item.count += part.qty;
    }
    result
}

struct Row {
    group: Group,
    name: String,
    slot: u32,
    count: u32,
}

fn groups(theme: &ColorTheme, z: bool) -> Vec<Row> {
    catalog_colors::ROLES
        .iter()
        .map(|&(role, cn, en)| Row {
            group: Group::Role(role),
            name: t(z, cn, en).into(),
            slot: theme.role_slot(role),
            count: 0,
        })
        .chain(theme.groups.iter().enumerate().map(|(index, group)| Row {
            group: Group::Custom(index),
            name: group.name.clone(),
            slot: group.slot,
            count: 0,
        }))
        .collect()
}

fn component_group(theme: &ColorTheme, key: &str, path: &str) -> Group {
    if let Some(index) = theme
        .groups
        .iter()
        .position(|group| group.components.contains(key))
    {
        return Group::Custom(index);
    }
    let role = theme.role_for(path);
    let key = catalog_colors::ROLES
        .iter()
        .find(|(key, _, _)| *key == role)
        .map_or_else(|| catalog_colors::role(path), |&(key, _, _)| key);
    Group::Role(key)
}

fn group_rows(theme: &ColorTheme, items: &BTreeMap<String, Item>, z: bool) -> Vec<Row> {
    let mut rows = groups(theme, z);
    for (key, item) in items {
        let group = component_group(theme, key, &item.path);
        if let Some(row) = rows.iter_mut().find(|row| row.group == group) {
            row.count += item.count;
        }
    }
    rows
}

fn assign_component(theme: &mut ColorTheme, path: &str, target: Option<Group>) {
    let key = catalog_colors::component(path);
    for (index, group) in theme.groups.iter_mut().enumerate() {
        if target == Some(Group::Custom(index)) {
            group.components.insert(key.clone());
        } else {
            group.components.remove(&key);
        }
    }
    theme.groups.retain(|group| !group.components.is_empty());
    match target {
        Some(Group::Role(role)) if role != catalog_colors::role(path) => {
            theme.role_overrides.insert(key, role.into());
        }
        Some(Group::Role(_)) | None => {
            theme.role_overrides.remove(&key);
        }
        Some(Group::Custom(_)) => {}
    }
}

fn color_label([r, g, b]: [u8; 3], z: bool) -> &'static str {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max - min < 26 {
        return match max {
            0..=34 => t(z, "黑色", "Black"),
            35..=105 => t(z, "深灰", "Dark gray"),
            106..=159 => t(z, "灰色", "Gray"),
            160..=190 => t(z, "银色", "Silver"),
            191..=230 => t(z, "浅灰", "Light gray"),
            _ => t(z, "白色", "White"),
        };
    }
    // Name the displayed sRGB color, before the renderer converts it to linear light.
    let delta = f32::from(max - min);
    let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
    let sector = if r == f32::from(max) {
        (g - b) / delta
    } else if g == f32::from(max) {
        (b - r) / delta + 2.
    } else {
        (r - g) / delta + 4.
    };
    match (sector * 60.).rem_euclid(360.) as u16 {
        0..=14 | 345..=360 => t(z, "红色", "Red"),
        15..=44 => t(z, "橙色", "Orange"),
        45..=68 => t(z, "黄色", "Yellow"),
        69..=165 => t(z, "绿色", "Green"),
        166..=200 => t(z, "青色", "Cyan"),
        201..=257 => t(z, "蓝色", "Blue"),
        258..=299 => t(z, "紫色", "Purple"),
        _ => t(z, "粉色", "Pink"),
    }
}

fn group_button(ui: &mut egui::Ui, row: &Row, theme: &ColorTheme, z: bool) -> egui::Response {
    let slot_index = (row.slot - 1) as usize;
    let rgb = theme.palette[slot_index];
    let material = theme.materials[slot_index].name();
    let weak = ui.visuals().weak_text_color();
    let ink = ui.visuals().text_color();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let painter = ui.painter();
    let count = painter.layout_no_wrap(
        format!("{} {}", row.count, t(z, "件", "pcs")),
        small.clone(),
        weak,
    );
    let color = painter.layout_no_wrap(color_label(rgb, z).into(), body.clone(), weak);
    let filament = painter.layout_no_wrap(material.into(), small, weak);
    let width = ui.available_width();
    let left_pad = 38.;
    let right_pad = 12.;
    let meta_width = color.size().x + 8. + filament.size().x;
    let label_width = width - left_pad - right_pad - count.size().x - meta_width - 20.;
    let compact = label_width < if z { 59. } else { 84. };
    let height = if compact { 48. } else { 42. };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let visuals = ui.visuals();
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        10.,
        if response.hovered() {
            visuals.faint_bg_color
        } else {
            visuals.panel_fill
        },
        visuals.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    painter.circle(
        egui::pos2(rect.left() + 19., rect.center().y),
        9.5,
        Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
        visuals.widgets.noninteractive.bg_stroke,
    );
    let left = rect.left() + left_pad;
    let right = rect.right() - right_pad;
    let (label_width, label_y, meta_y) = if compact {
        (
            right - left - count.size().x - 7.,
            rect.center().y - 8.,
            rect.center().y + 10.,
        )
    } else {
        (label_width, rect.center().y, rect.center().y)
    };
    let mut label = egui::text::LayoutJob::simple(row.name.clone(), body, ink, label_width.max(1.));
    label.wrap.max_rows = 1;
    label.wrap.break_anywhere = true;
    let label = painter.layout_job(label);
    let count_x = left + label.size().x + 7.;
    painter.galley(egui::pos2(left, label_y - label.size().y * 0.5), label, ink);
    painter.galley(
        egui::pos2(count_x, label_y - count.size().y * 0.5),
        count,
        weak,
    );
    let color_x = if compact { left } else { right - meta_width };
    let material_x = color_x + color.size().x + 8.;
    painter.galley(
        egui::pos2(color_x, meta_y - color.size().y * 0.5),
        color,
        weak,
    );
    painter.galley(
        egui::pos2(material_x, meta_y - filament.size().y * 0.5),
        filament,
        weak,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &row.name));
    response
}

pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {
    let z = app.zh;
    let rows = group_rows(&app.global_colors, &items(app), z);
    let mut palette = Vec::new();
    for row in rows.iter().filter(|row| row.count > 0) {
        let rgb = app.global_colors.palette[(row.slot - 1) as usize];
        if !palette.contains(&rgb) {
            palette.push(rgb);
        }
    }
    if !layers::group(ui, "colors", t(z, "颜色", "Colors"), false, |ui| {
        layers::palette(ui, &palette, palette.len().min(5));
    }) {
        return;
    }
    let can_create = app.global_colors.groups.len() < 64 && rows.iter().any(|row| row.count > 0);
    for row in rows {
        if ui
            .push_id(row.group, |ui| {
                group_button(ui, &row, &app.global_colors, z)
            })
            .inner
            .clicked()
        {
            app.tower.color_edit = Some(Edit {
                group: Some(row.group),
                name: row.name,
                keys: match row.group {
                    Group::Role(_) => BTreeSet::new(),
                    Group::Custom(index) => app.global_colors.groups[index].components.clone(),
                },
                slot: row.slot,
            });
        }
        ui.add_space(4.);
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                can_create,
                egui::Button::new(t(z, "＋ 新建分组", "＋ New group")),
            )
            .clicked()
        {
            app.tower.color_edit = Some(Edit {
                group: None,
                name: String::new(),
                keys: BTreeSet::new(),
                slot: app.global_colors.filament,
            });
        }
        if ui.button(t(z, "整体配色…", "Overall colors…")).clicked() {
            app.open_global_colors();
        }
    });
    ui.add_space(6.);
}

fn group_choices(ui: &mut egui::Ui, theme: &mut ColorTheme, path: &str, z: bool) -> bool {
    let key = catalog_colors::component(path);
    let current = component_group(theme, &key, path);
    for row in groups(theme, z) {
        let index = (row.slot - 1) as usize;
        let rgb = theme.palette[index];
        let clicked = ui
            .horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::Vec2::splat(12.), egui::Sense::hover());
                ui.painter().circle(
                    dot.center(),
                    5.,
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                ui.selectable_label(row.group == current, &row.name)
                    .on_hover_text(format!(
                        "{} · {}",
                        color_label(rgb, z),
                        theme.materials[index].name()
                    ))
                    .clicked()
            })
            .inner;
        if clicked {
            let changed = row.group != current;
            if changed {
                assign_component(theme, path, Some(row.group));
            }
            ui.close();
            return changed;
        }
    }
    ui.separator();
    let manual = matches!(current, Group::Custom(_)) || theme.role_overrides.contains_key(&key);
    if ui
        .add_enabled(
            manual,
            egui::Button::new(t(z, "恢复自动归类", "Restore automatic group")),
        )
        .clicked()
    {
        assign_component(theme, path, None);
        ui.close();
        return true;
    }
    false
}

pub(in crate::app) fn component_group_picker(
    ui: &mut egui::Ui,
    theme: &mut ColorTheme,
    path: &str,
    z: bool,
) {
    let current = component_group(theme, &catalog_colors::component(path), path);
    let name = groups(theme, z)
        .into_iter()
        .find(|row| row.group == current)
        .map(|row| row.name)
        .unwrap_or_default();
    egui::ComboBox::from_label(t(z, "颜色分组", "Color group"))
        .selected_text(name)
        .show_ui(ui, |ui| {
            group_choices(ui, theme, path, z);
        });
}

pub(super) fn preview_menu(app: &mut App, ui: &mut egui::Ui) {
    let z = app.zh;
    let part = app.selected.and_then(|index| app.scene.parts.get(index));
    let ready =
        part.is_some() && !app.busy && !app.preview.has_pending() && app.export_rx.is_none();
    let mut changed_theme = None;
    let mut new = None;
    let mut follow = false;
    ui.add_enabled_ui(ready, |ui| {
        ui.menu_button(t(z, "颜色分组", "Color group"), |ui| {
            let Some(part) = part else {
                return;
            };
            let path = &part.pose.path;
            let key = catalog_colors::component(path);
            let theme = &app.global_colors;
            ui.strong(workflow::color_component_name(path, z));
            let mut draft = theme.clone();
            if group_choices(ui, &mut draft, path, z) {
                changed_theme = Some(draft);
            }
            if ui
                .add_enabled(
                    theme.groups.len() < 64,
                    egui::Button::new(t(z, "＋ 新建分组…", "＋ New group…")),
                )
                .clicked()
            {
                new = Some(Edit {
                    group: None,
                    name: String::new(),
                    keys: [key].into(),
                    slot: theme.group_slot(path),
                });
                ui.close();
            }
            if !app.export_options.plate.follow_global_colors
                && ui
                    .button(t(
                        z,
                        "当前成员跟随整体配色",
                        "Use project colors for this member",
                    ))
                    .clicked()
            {
                follow = true;
                ui.close();
            }
        })
        .response
        .on_hover_text(t(
            z,
            "为同类零件设置项目配色分组",
            "Set the project color group for matching components",
        ));
    })
    .response
    .on_disabled_hover_text(t(
        z,
        "在预览中先选择一个零件",
        "Select a part in the preview first",
    ));
    if follow {
        app.record_workflow();
        app.export_options.plate.follow_global_colors = true;
        app.global_colors.apply(&mut app.export_options.plate);
        app.commit_member();
        ui.ctx().request_repaint();
    }
    if let Some(theme) = changed_theme {
        if let Err(error) = app.apply_global_theme(theme) {
            app.tower.error = error;
        }
        ui.ctx().request_repaint();
    }
    if let Some(edit) = new {
        app.tower.color_edit = Some(edit);
        ui.ctx().request_repaint();
    }
}

pub(super) fn dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut edit) = app.tower.color_edit.take() else {
        return;
    };
    let z = app.zh;
    let theme = &app.global_colors;
    let modifying_membership = !matches!(edit.group, Some(Group::Role(_)));
    let items = modifying_membership.then(|| items(app));
    let mut apply = false;
    let mut cancel = false;
    let mut remove = false;
    let modal = egui::Modal::new(egui::Id::new("tower-color-group")).show(ctx, |ui| {
        ui.set_width((ctx.content_rect().width() - 48.).clamp(150., 380.));
        ui.heading(if edit.group.is_none() {
            t(z, "新建分组", "New color group")
        } else {
            &edit.name
        });
        if modifying_membership {
            ui.add(
                egui::TextEdit::singleline(&mut edit.name)
                    .hint_text(t(z, "分组名称", "Group name"))
                    .char_limit(48)
                    .desired_width(f32::INFINITY),
            );
            if !edit.name.trim().is_empty() && !edit.name_available(theme) {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t(z, "已有同名分组", "This group name is already in use"),
                );
            }
        }
        let index = (edit.slot - 1) as usize;
        ui.horizontal_wrapped(|ui| {
            ui.strong(t(z, "耗材槽", "Filament slot"));
            ui.label(format!(
                "{} · {}",
                color_label(theme.palette[index], z),
                theme.materials[index].name()
            ));
        });
        ui.horizontal_wrapped(|ui| {
            for (index, &[r, g, b]) in theme.palette.iter().enumerate() {
                let slot = (index + 1) as u32;
                let rgb = Color32::from_rgb(r, g, b);
                let linear = egui::Rgba::from(rgb);
                let luminance = 0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b();
                let ink = if luminance > 0.179 {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
                let selected = edit.slot == slot;
                let response = ui.add(
                    egui::Button::new(RichText::new(slot.to_string()).color(ink))
                        .min_size(egui::vec2(36., 30.))
                        .selected(selected)
                        .fill(rgb)
                        .stroke(if selected {
                            egui::Stroke::new(2., ui.visuals().selection.stroke.color)
                        } else {
                            ui.visuals().widgets.noninteractive.bg_stroke
                        }),
                );
                if response
                    .on_hover_text(format!(
                        "{} · {} · #{r:02X}{g:02X}{b:02X}",
                        color_label([r, g, b], z),
                        theme.materials[index].name()
                    ))
                    .clicked()
                {
                    edit.slot = slot;
                }
            }
        });
        if let Some(items) = &items {
            ui.separator();
            ui.strong(t(z, "零件", "Parts"));
            ui.small(t(
                z,
                "勾选移入本组，取消勾选后回到用途分组。",
                "Check parts to move them here. Unchecked parts return to their role groups.",
            ));
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() * 0.38).min(280.))
                .show(ui, |ui| {
                    for (key, item) in items {
                        let mut selected = edit.keys.contains(key);
                        ui.horizontal(|ui| {
                            let caption = workflow::color_component_name(&item.path, z);
                            if ui
                                .checkbox(&mut selected, caption)
                                .on_hover_text(&item.path)
                                .changed()
                            {
                                if selected {
                                    edit.keys.insert(key.clone());
                                } else {
                                    edit.keys.remove(key);
                                }
                            }
                            ui.weak(format!("×{}", item.count));
                        });
                    }
                });
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let valid = (edit.group.is_some() || !edit.keys.is_empty())
                && edit.name_available(theme)
                && app.export_rx.is_none();
            apply = ui
                .add_enabled(valid, egui::Button::new(t(z, "应用", "Apply")))
                .clicked();
            cancel = ui.button(t(z, "取消", "Cancel")).clicked();
            if matches!(edit.group, Some(Group::Custom(_))) {
                remove = ui
                    .add_enabled(
                        app.export_rx.is_none(),
                        egui::Button::new(t(z, "解散分组", "Remove group")),
                    )
                    .clicked();
            }
        });
    });
    if cancel || modal.should_close() {
        return;
    }
    if apply || remove {
        let mut theme = theme.clone();
        if remove {
            edit.keys.clear();
        }
        edit.apply_to(&mut theme);
        match app.apply_global_theme(theme) {
            Ok(()) => return,
            Err(error) => app.tower.error = error,
        }
    }
    app.tower.color_edit = Some(edit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_plate::Config;

    const GUIDE: &str = "01. DRAWERS/07. Universal/Runners/x3 Runner Guide.stl";
    const SLIDE: &str = "01. DRAWERS/07. Universal/Runners/x3 Runner Slide.stl";
    const BIN: &str = "generated/gridfinity/Gridfinity-12.stl";

    fn fixture() -> BTreeMap<String, Item> {
        [(GUIDE, 2), (SLIDE, 4), (BIN, 3)]
            .into_iter()
            .map(|(path, count)| {
                (
                    catalog_colors::component(path),
                    Item {
                        path: path.into(),
                        count,
                    },
                )
            })
            .collect()
    }

    fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
        rows.iter().find(|row| row.name == name).unwrap()
    }

    fn new_custom(theme: &mut ColorTheme, name: &str, paths: &[&str], slot: u32) {
        Edit {
            group: None,
            name: name.into(),
            keys: paths
                .iter()
                .map(|path| catalog_colors::component(path))
                .collect(),
            slot,
        }
        .apply_to(theme);
    }

    #[test]
    fn default_swatches_display_the_five_reference_color_names() {
        let theme = ColorTheme::default();
        let labels = groups(&theme, true)
            .into_iter()
            .map(|group| {
                let rgb = theme.palette[(group.slot - 1) as usize];
                (color_label(rgb, true), color_label(rgb, false))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                ("银色", "Silver"),
                ("橙色", "Orange"),
                ("深灰", "Dark gray"),
                ("黑色", "Black"),
                ("灰色", "Gray")
            ]
        );
    }

    #[test]
    fn five_role_rows_count_copies_once_including_empty_roles() {
        let mut theme = ColorTheme::default();
        let items = fixture();
        let rows = group_rows(&theme, &items, true);
        assert_eq!(rows.len(), 5);
        assert_eq!(row(&rows, "功能件").count, 6);
        assert_eq!(row(&rows, "分隔板").count, 0);
        assert_eq!(rows.iter().map(|row| row.count).sum::<u32>(), 9);
        let role = Group::Role("functional");
        Edit {
            group: Some(role),
            name: "功能件".into(),
            keys: BTreeSet::new(),
            slot: 4,
        }
        .apply_to(&mut theme);

        new_custom(&mut theme, "活动件", &[SLIDE], 5);
        let rows = group_rows(&theme, &items, true);
        assert_eq!(rows.len(), 6);
        assert_eq!(row(&rows, "功能件").count, 2);
        assert_eq!(row(&rows, "功能件").slot, 4);
        assert_eq!(row(&rows, "活动件").count, 4);
        assert_eq!(rows.iter().map(|row| row.count).sum::<u32>(), 9);
        let mut config = Config::default();
        theme.apply(&mut config);
        assert_eq!(config.slot(GUIDE), 4);
        assert_eq!(
            config.slot("01. DRAWERS/07. Universal/Runners/x7 Runner Slide.stl"),
            5
        );
    }

    #[test]
    fn preview_reassignment_custom_release_and_automatic_reset_survive_roundtrip() {
        let mut theme = ColorTheme::default();
        assign_component(&mut theme, GUIDE, Some(Group::Role("accessory")));
        new_custom(&mut theme, "备用", &[GUIDE, SLIDE], 5);
        new_custom(&mut theme, "抽屉侧件", &[SLIDE, BIN], 2);
        let items = fixture();
        let rows = group_rows(&theme, &items, true);
        assert_eq!(row(&rows, "备用").count, 2);
        assert_eq!(row(&rows, "抽屉侧件").count, 7);
        assert_eq!(row(&rows, "配件").count, 0);

        // Releasing a component restores the manual role it had before the custom group.
        let edit = Edit {
            group: Some(Group::Custom(0)),
            name: "备用".into(),
            keys: BTreeSet::new(),
            slot: 3,
        };
        edit.apply_to(&mut theme);
        assert_eq!(theme.groups.len(), 1);
        assert_eq!(row(&group_rows(&theme, &items, true), "配件").count, 2);
        assign_component(&mut theme, BIN, Some(Group::Role("divider")));
        let mut saved: ColorTheme = toml::from_str(&toml::to_string(&theme).unwrap()).unwrap();
        saved.validate().unwrap();
        let rows = group_rows(&saved, &items, true);
        assert_eq!(row(&rows, "分隔板").count, 3);
        assert_eq!(row(&rows, "抽屉侧件").count, 4);
        assign_component(&mut saved, GUIDE, None);
        assign_component(&mut saved, SLIDE, None);
        assert!(saved.groups.is_empty());
        assert_eq!(row(&group_rows(&saved, &items, true), "功能件").count, 6);
    }

    #[test]
    fn custom_names_cannot_duplicate_roles_or_other_groups() {
        let mut theme = ColorTheme::default();
        new_custom(&mut theme, "备用", &[GUIDE], 2);
        let mut edit = Edit {
            group: None,
            name: " 功能件 ".into(),
            keys: BTreeSet::new(),
            slot: 3,
        };
        assert!(!edit.name_available(&theme));
        edit.name = "备用".into();
        assert!(!edit.name_available(&theme));
        edit.group = Some(Group::Custom(0));
        assert!(edit.name_available(&theme));
    }
}
