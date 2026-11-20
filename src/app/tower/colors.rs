use super::*;
use crate::catalog::colors as catalog_colors;
use crate::print_plate::{ColorGroup, ColorTheme, GroupAppearance, PaletteGroup as Group};
mod card;
use std::collections::BTreeSet;

pub(super) struct Edit {
    original: Option<String>,
    name: String,
    keys: BTreeSet<String>,
    slot: u32,
}

impl Edit {
    fn apply_to(&self, theme: &mut ColorTheme) {
        let index = match &self.original {
            Some(name) => {
                let Some(index) = theme.groups.iter().position(|group| &group.name == name) else {
                    return;
                };
                Some(index)
            }
            None => None,
        };
        for group in &mut theme.groups {
            group.components.retain(|key| !self.keys.contains(key));
        }
        if let Some(index) = index {
            let group = &mut theme.groups[index];
            group.name = self.name.trim().into();
            group.components.clone_from(&self.keys);
        } else {
            theme.groups.push(ColorGroup {
                name: self.name.trim().into(),
                components: self.keys.clone(),
                slot: self.slot,
                initial_appearance: None,
            });
        }
        theme.groups.retain(|group| !group.components.is_empty());
    }

    fn name_available(&self, theme: &ColorTheme) -> bool {
        let name = self.name.trim().to_lowercase();
        !name.is_empty()
            && !catalog_colors::ROLES
                .iter()
                .any(|(_, cn, en)| name == cn.to_lowercase() || name == en.to_lowercase())
            && !theme.groups.iter().any(|group| {
                Some(&group.name) != self.original.as_ref()
                    && group.name.trim().to_lowercase() == name
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
    count: u32,
}

fn groups(theme: &ColorTheme, z: bool) -> Vec<Row> {
    catalog_colors::ROLES
        .iter()
        .map(|&(role, cn, en)| Row {
            group: Group::Role(role),
            name: t(z, cn, en).into(),
            count: 0,
        })
        .chain(theme.groups.iter().enumerate().map(|(index, group)| Row {
            group: Group::Custom(index),
            name: group.name.clone(),
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

fn edit_members(
    ui: &mut egui::Ui,
    edit: &mut Edit,
    items: &BTreeMap<String, Item>,
    theme: &ColorTheme,
    z: bool,
) -> (bool, bool) {
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
    egui::ScrollArea::vertical()
        .id_salt("group-parts")
        .max_height(150.)
        .show(ui, |ui| {
            for (key, item) in items {
                let mut checked = edit.keys.contains(key);
                if ui
                    .checkbox(
                        &mut checked,
                        format!(
                            "{}  ×{}",
                            workflow::color_component_name(&item.path, z),
                            item.count
                        ),
                    )
                    .on_hover_text(&item.path)
                    .changed()
                {
                    if checked {
                        edit.keys.insert(key.clone());
                    } else {
                        edit.keys.remove(key);
                    }
                }
            }
        });
    ui.horizontal_wrapped(|ui| {
        let valid =
            (!edit.keys.is_empty() || edit.original.is_some()) && edit.name_available(theme);
        let apply = ui
            .add_enabled(valid, egui::Button::new(t(z, "保存分组", "Save group")))
            .clicked();
        let cancel = ui.button(t(z, "取消", "Cancel")).clicked();
        let remove =
            edit.original.is_some() && ui.button(t(z, "解散分组", "Remove group")).clicked();
        if remove {
            edit.keys.clear();
        }
        (apply || remove, cancel)
    })
    .inner
}

pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {
    let z = app.zh;
    let items = items(app);
    let rows = group_rows(&app.global_colors, &items, z);
    let mut palette = Vec::new();
    for row in rows.iter().filter(|row| row.count > 0) {
        let rgb = app.global_colors.group_appearance(row.group).rgb;
        if !palette.contains(&rgb) {
            palette.push(rgb);
        }
    }
    let reveal = std::mem::take(&mut app.tower.reveal_colors);
    if reveal {
        layers::set_group(ui, "colors", true);
    }
    let start = ui.cursor().min;
    let expanded = layers::group(ui, "colors", t(z, "颜色", "Colors"), false, |ui| {
        layers::palette(ui, &palette, palette.len().min(5));
    });
    if !expanded {
        return;
    }
    let busy = app.export_rx.is_some();
    let mut draft = app.global_colors.clone();
    let mut edit = app.tower.color_edit.take();
    let mut save = false;
    let mut cancel = false;
    for row in rows {
        let open = app.tower.open_color.as_ref() == Some(&row.name);
        let result = ui
            .push_id(("group-card", &row.name), |ui| {
                card::frame(ui, open)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let appearance = draft.group_appearance(row.group);
                        match card::header(ui, &row, appearance, open, z) {
                            Some(card::HeaderAction::Toggle) => {
                                app.tower.open_color = (!open).then(|| row.name.clone());
                                edit = None;
                            }
                            Some(card::HeaderAction::Reset) if !busy => {
                                if let Err(error) = draft.reset_group_appearance(row.group) {
                                    app.tower.error = error;
                                }
                            }
                            _ => {}
                        }
                        if open {
                            ui.add_enabled_ui(!busy, |ui| {
                                let mut next = appearance;
                                card::appearance_ui(ui, &app.material_library, &mut next, z);
                                if next != appearance
                                    && let Err(error) = draft.set_group_appearance(row.group, next)
                                {
                                    app.tower.error = error;
                                }
                                if let Group::Custom(index) = row.group {
                                    if let Some(current) = edit.as_mut().filter(|current| {
                                        current.original.as_ref() == Some(&row.name)
                                    }) {
                                        (save, cancel) =
                                            edit_members(ui, current, &items, &draft, z);
                                    } else if ui
                                        .small_button(t(z, "编辑分组", "Edit group"))
                                        .clicked()
                                    {
                                        edit = Some(Edit {
                                            original: Some(row.name.clone()),
                                            name: row.name.clone(),
                                            keys: draft.groups[index].components.clone(),
                                            slot: draft.groups[index].slot,
                                        });
                                    }
                                }
                            });
                        }
                    })
                    .response
            })
            .inner;
        if reveal && open {
            result.scroll_to_me(Some(egui::Align::Min));
        }
        ui.add_space(3.);
    }
    let editing_new = edit.as_ref().is_some_and(|edit| edit.original.is_none());
    if editing_new {
        let response = card::frame(ui, true)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.strong(t(z, "新建分组", "New group"));
                ui.add_enabled_ui(!busy, |ui| {
                    if let Some(edit) = &mut edit {
                        (save, cancel) = edit_members(ui, edit, &items, &draft, z);
                    }
                });
            })
            .response;
        if reveal {
            response.scroll_to_me(Some(egui::Align::Min));
        }
    } else {
        let can_create = !busy && draft.groups.len() < 64 && !items.is_empty();
        if ui
            .add_enabled(
                can_create,
                egui::Button::new(t(z, "＋ 新建分组", "＋ New group")),
            )
            .clicked()
        {
            app.tower.open_color = None;
            app.tower.reveal_colors = true;
            edit = Some(Edit {
                original: None,
                name: String::new(),
                keys: BTreeSet::new(),
                slot: draft.filament,
            });
            ui.ctx().request_repaint();
        }
    }
    if reveal && !editing_new && app.tower.open_color.is_none() {
        ui.scroll_to_rect(
            egui::Rect::from_min_size(start, egui::Vec2::splat(1.)),
            Some(egui::Align::Min),
        );
    }
    if save && let Some(current) = &edit {
        current.apply_to(&mut draft);
    }
    if draft != app.global_colors {
        match app.apply_global_theme(draft) {
            Ok(()) => {
                if save && let Some(current) = &edit {
                    app.tower.open_color =
                        (!current.keys.is_empty()).then(|| current.name.trim().to_owned());
                    edit = None;
                }
                ui.ctx().request_repaint();
            }
            Err(error) => app.tower.error = error,
        }
    } else if save {
        edit = None;
    }
    if cancel {
        edit = None;
    }
    app.tower.color_edit = edit;
    ui.add_space(6.);
}

fn group_choices(ui: &mut egui::Ui, theme: &mut ColorTheme, path: &str, z: bool) -> bool {
    let key = catalog_colors::component(path);
    let current = component_group(theme, &key, path);
    for row in groups(theme, z) {
        let appearance = theme.group_appearance(row.group);
        let rgb = appearance.rgb;
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
                        appearance.material.name()
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

fn text_color_menu(ui: &mut egui::Ui, theme: &mut ColorTheme, path: &str, z: bool) -> bool {
    let key = catalog_colors::component(path);
    let selected = theme.component_text.get(&key).copied();
    let mut changed = false;
    ui.menu_button(t(z, "文字颜色", "Text color"), |ui| {
        if ui
            .selectable_label(
                selected.is_none(),
                if theme.roles.contains_key("text") {
                    t(z, "跟随项目文字颜色", "Use project text color")
                } else {
                    t(z, "跟随分组颜色", "Use group color")
                },
            )
            .clicked()
        {
            theme.component_text.remove(&key);
            changed = selected.is_some();
            ui.close();
        }
        for slot in 1..=theme.slot_count() {
            let appearance = theme.appearance(slot);
            let [r, g, b] = appearance.rgb;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14., 14.), egui::Sense::hover());
                ui.painter().circle(
                    rect.center(),
                    5.,
                    Color32::from_rgb(r, g, b),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                if ui
                    .selectable_label(
                        selected == Some(slot),
                        format!(
                            "{slot} · {} · {}",
                            color_label([r, g, b], z),
                            appearance.material.name()
                        ),
                    )
                    .clicked()
                {
                    theme.component_text.insert(key.clone(), slot);
                    changed = selected != Some(slot);
                    ui.close();
                }
            });
        }
    });
    changed
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
            let mut changed = group_choices(ui, &mut draft, path, z);
            if crate::label::is_label(path) || path.starts_with(crate::gridfinity::PREFIX) {
                changed |= text_color_menu(ui, &mut draft, path, z);
            }
            if changed {
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
                    original: None,
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
        app.reveal_tower_colors(ui.ctx());
        app.tower.open_color = None;
        app.tower.color_edit = Some(edit);
        ui.ctx().request_repaint();
    }
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
            original: None,
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
                let rgb = theme.group_appearance(group.group).rgb;
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
        theme.roles.insert("functional".into(), 4);

        new_custom(&mut theme, "活动件", &[SLIDE], 5);
        let rows = group_rows(&theme, &items, true);
        assert_eq!(rows.len(), 6);
        assert_eq!(row(&rows, "功能件").count, 2);
        assert_eq!(theme.role_slot("functional"), 4);
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
            original: Some("备用".into()),
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
    fn inline_membership_edit_keeps_current_appearance_and_reset_color() {
        let mut theme = ColorTheme::default();
        new_custom(&mut theme, "卡扣", &[GUIDE], 2);
        let group = Group::Custom(0);
        let initial = theme.group_appearance(group);
        let custom = GroupAppearance {
            rgb: [11, 33, 72],
            ..initial
        };
        theme.set_group_appearance(group, custom).unwrap();
        Edit {
            original: Some("卡扣".into()),
            name: "铰链".into(),
            keys: [catalog_colors::component(BIN)].into(),
            slot: 1,
        }
        .apply_to(&mut theme);
        assert_eq!(theme.group_appearance(group), custom);
        theme.reset_group_appearance(group).unwrap();
        assert_eq!(theme.group_appearance(group), initial);
        assert_eq!(theme.groups[0].name, "铰链");
        assert_eq!(
            theme.groups[0].components,
            [catalog_colors::component(BIN)].into()
        );
    }

    #[test]
    fn custom_names_cannot_duplicate_roles_or_other_groups() {
        let mut theme = ColorTheme::default();
        new_custom(&mut theme, "备用", &[GUIDE], 2);
        let mut edit = Edit {
            original: None,
            name: " 功能件 ".into(),
            keys: BTreeSet::new(),
            slot: 3,
        };
        assert!(!edit.name_available(&theme));
        edit.name = "备用".into();
        assert!(!edit.name_available(&theme));
        edit.original = Some("备用".into());
        assert!(edit.name_available(&theme));
    }
}
