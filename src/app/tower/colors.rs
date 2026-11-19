use super::*;
use crate::catalog::colors as catalog_colors;
use crate::print_plate::{ColorGroup, ColorTheme, Config};
use std::collections::BTreeSet;

const GROUP_LABELS: [(&str, &str); 5] = [
    ("主体", "Body"),
    ("功能件", "Functional parts"),
    ("配件", "Accessories"),
    ("分隔板", "Dividers"),
    ("Gridfinity 盒子", "Gridfinity bins"),
];
const MATERIAL: &str = "PETG Basic";

#[derive(Clone, Copy)]
enum Group {
    Bucket,
    Custom(usize),
    New,
}

pub(super) struct Edit {
    group: Group,
    name: String,
    keys: BTreeSet<String>,
    slot: Option<u32>,
}

impl Edit {
    fn apply_to(&self, theme: &mut ColorTheme) {
        if let Some(slot) = self.slot {
            theme
                .components
                .extend(self.keys.iter().cloned().map(|key| (key, slot)));
        }
        if matches!(self.group, Group::Bucket) {
            return;
        }
        for group in &mut theme.groups {
            group.components.retain(|key| !self.keys.contains(key));
        }
        let updated = ColorGroup {
            name: self.name.trim().into(),
            components: self.keys.clone(),
        };
        if let Group::Custom(index) = self.group {
            theme.groups[index] = updated;
        } else {
            theme.groups.push(updated);
        }
        theme.groups.retain(|group| !group.components.is_empty());
    }
}

struct Item {
    path: String,
    count: u32,
}

fn builtin(path: &str) -> usize {
    let file = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if crate::print_plate::color_key(path) == crate::print_plate::BIN_COLOR_KEY {
        4
    } else if file.contains("divider") || path.contains("/Grid Dividers/") {
        3
    } else if matches!(catalog_colors::role(path), "shell" | "insert" | "facade") {
        0
    } else if [
        "runner", "rail", "hinge", "spring", "latch", "stopper", "lock", "key", "gear",
    ]
    .iter()
    .any(|name| file.contains(name))
    {
        1
    } else {
        2
    }
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

#[derive(Default)]
struct Bucket {
    keys: BTreeSet<String>,
    count: u32,
}

fn color_buckets(
    keys: &BTreeSet<String>,
    items: &BTreeMap<String, Item>,
    config: &Config,
) -> BTreeMap<u32, Bucket> {
    let mut buckets = BTreeMap::<_, Bucket>::new();
    for key in keys {
        if let Some(item) = items.get(key) {
            let bucket = buckets.entry(config.slot(&item.path)).or_default();
            bucket.keys.insert(key.clone());
            bucket.count += item.count;
        }
    }
    buckets
}

fn color_label([r, g, b]: [u8; 3], z: bool) -> &'static str {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max - min < 26 {
        return match max {
            0..=34 => t(z, "黑色", "Black"),
            35..=105 => t(z, "深灰", "Dark gray"),
            106..=172 => t(z, "灰色", "Gray"),
            173..=230 => t(z, "浅灰", "Light gray"),
            _ => t(z, "白色", "White"),
        };
    }
    let f = egui::ecolor::Hsva::from(Color32::from_rgb(r, g, b));
    match (f.h * 360.) as u16 {
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

pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {
    let z = app.zh;
    let mut config = Config::default();
    app.global_colors.apply(&mut config);
    let items = items(app);
    let custom = &app.global_colors.groups;
    let mut builtin_keys: [BTreeSet<String>; GROUP_LABELS.len()] = Default::default();
    let assigned: BTreeSet<_> = custom.iter().flat_map(|group| &group.components).collect();
    for (key, item) in &items {
        if !assigned.contains(key) {
            builtin_keys[builtin(&item.path)].insert(key.clone());
        }
    }
    let groups = builtin_keys
        .into_iter()
        .zip(GROUP_LABELS)
        .map(|(keys, (cn, en))| (Group::Bucket, t(z, cn, en).to_owned(), keys))
        .chain(custom.iter().enumerate().map(|(i, group)| {
            (
                Group::Custom(i),
                group.name.clone(),
                group.components.clone(),
            )
        }));
    let mut rows = Vec::new();
    let mut palette = Vec::new();
    for (index, (group, title, keys)) in groups.enumerate() {
        let buckets = color_buckets(&keys, &items, &config);
        let split = buckets.len() > 1;
        for (slot, bucket) in buckets {
            let rgb = config.palette[(slot - 1) as usize];
            if !palette.contains(&rgb) {
                palette.push(rgb);
            }
            let (edit_group, edit_keys) = if split || matches!(group, Group::Bucket) {
                (Group::Bucket, bucket.keys)
            } else {
                (group, keys.clone())
            };
            rows.push((
                (index, slot),
                edit_group,
                title.clone(),
                edit_keys,
                bucket.count,
                rgb,
            ));
        }
    }
    if !layers::group(ui, "colors", t(z, "颜色", "Colors"), false, |ui| {
        layers::palette(ui, &palette, palette.len().min(4));
    }) {
        return;
    }
    for (id, group, title, keys, count, rgb) in rows {
        let slot = id.1;
        let response = ui
            .push_id(id, |ui| {
                let width = ui.available_width();
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, 44.), egui::Sense::click());
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                let visuals = ui.visuals();
                let painter = ui.painter_at(rect);
                painter.rect(
                    rect,
                    12.,
                    if response.hovered() {
                        visuals.faint_bg_color
                    } else {
                        visuals.panel_fill
                    },
                    visuals.widgets.noninteractive.bg_stroke,
                    egui::StrokeKind::Inside,
                );
                let center = rect.center();
                let dot = egui::pos2(rect.left() + 22., center.y);
                let radius = 9.5;
                let stroke = visuals.widgets.noninteractive.bg_stroke;
                painter.circle(
                    dot,
                    radius,
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    stroke,
                );
                let body = egui::TextStyle::Body.resolve(ui.style());
                let weak = visuals.weak_text_color();
                let material = painter.layout_no_wrap(
                    MATERIAL.into(),
                    egui::TextStyle::Small.resolve(ui.style()),
                    weak,
                );
                let mut right = rect.right() - 12.;
                painter.galley(
                    egui::pos2(
                        right - material.size().x,
                        center.y - material.size().y * 0.5,
                    ),
                    material.clone(),
                    weak,
                );
                right -= material.size().x + 8.;
                let color_name = color_label(rgb, z);
                let name = painter.layout_no_wrap(color_name.into(), body.clone(), weak);
                painter.galley(
                    egui::pos2(right - name.size().x, center.y - name.size().y * 0.5),
                    name.clone(),
                    weak,
                );
                right -= name.size().x + 12.;
                let count = painter.layout_no_wrap(
                    format!("{count} {}", t(z, "件", "pcs")),
                    egui::TextStyle::Small.resolve(ui.style()),
                    weak,
                );
                let left = rect.left() + 41.;
                let mut label = egui::text::LayoutJob::simple(
                    title.clone(),
                    body,
                    visuals.text_color(),
                    (right - left - count.size().x - 7.).max(12.),
                );
                label.wrap.max_rows = 1;
                label.wrap.break_anywhere = true;
                let label = painter.layout_job(label);
                painter.galley(
                    egui::pos2(left, center.y - label.size().y * 0.5),
                    label.clone(),
                    visuals.text_color(),
                );
                painter.galley(
                    egui::pos2(left + label.size().x + 7., center.y - count.size().y * 0.5),
                    count,
                    weak,
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title)
                });
                response
            })
            .inner;
        if response.clicked() {
            app.tower.color_edit = Some(Edit {
                group,
                name: title,
                keys,
                slot: Some(slot),
            });
        }
        ui.add_space(4.);
    }
    let can_create = custom.len() < 64 && !items.is_empty();
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                can_create,
                egui::Button::new(t(z, "＋ 新建分组", "＋ New group")),
            )
            .clicked()
        {
            app.tower.color_edit = Some(Edit {
                group: Group::New,
                name: String::new(),
                keys: BTreeSet::new(),
                slot: Some(config.filament),
            });
        }
        if ui.button(t(z, "整体配色…", "Overall colors…")).clicked() {
            app.open_global_colors();
        }
    });
    ui.add_space(6.);
}

// Assign a catalog component as a whole, matching the existing project color scope.
fn assign_component(theme: &mut ColorTheme, key: &str, target: Option<usize>, slot: u32) {
    for (index, group) in theme.groups.iter_mut().enumerate() {
        if Some(index) == target {
            group.components.insert(key.to_owned());
        } else {
            group.components.remove(key);
        }
    }
    theme.groups.retain(|group| !group.components.is_empty());
    theme.components.insert(key.to_owned(), slot);
}

pub(super) fn preview_menu(app: &mut App, ui: &mut egui::Ui) {
    let z = app.zh;
    let part = app.selected.and_then(|index| app.scene.parts.get(index));
    let ready =
        part.is_some() && !app.busy && !app.preview.has_pending() && app.export_rx.is_none();
    let mut assign = None;
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
            let mut config = Config::default();
            theme.apply(&mut config);
            let active = config.slot(path);
            let current = theme
                .groups
                .iter()
                .position(|group| group.components.contains(&key));
            let (cn, en) = GROUP_LABELS[builtin(path)];
            let default = t(z, cn, en);
            ui.strong(workflow::color_component_name(path, z));
            let choice = |ui: &mut egui::Ui, title: &str, slot: u32, selected: bool| {
                let rgb = config.palette[(slot - 1) as usize];
                let [r, g, b] = rgb;
                let (dot, _) = ui.allocate_exact_size(egui::Vec2::splat(12.), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 5., Color32::from_rgb(r, g, b));
                ui.selectable_label(selected, format!("{title} · {}", color_label(rgb, z)))
                    .on_hover_text(config.color_hex(slot))
                    .clicked()
            };
            ui.menu_button(t(z, "按用途分组", "Group by role"), |ui| {
                for slot in 1..=6 {
                    ui.horizontal(|ui| {
                        if choice(ui, default, slot, current.is_none() && active == slot) {
                            assign = Some((key.clone(), None, slot));
                            ui.close();
                        }
                    });
                }
            });
            let project_items = items(app);
            for (index, group) in theme.groups.iter().enumerate() {
                let buckets = color_buckets(&group.components, &project_items, &config);
                for &slot in buckets.keys() {
                    ui.horizontal(|ui| {
                        if choice(
                            ui,
                            &group.name,
                            slot,
                            current == Some(index) && active == slot,
                        ) {
                            assign = Some((key.clone(), Some(index), slot));
                            ui.close();
                        }
                    });
                }
            }
            ui.separator();
            if ui
                .add_enabled(
                    theme.groups.len() < 64,
                    egui::Button::new(t(z, "＋ 新建分组…", "＋ New group…")),
                )
                .clicked()
            {
                new = Some(Edit {
                    group: Group::New,
                    name: String::new(),
                    keys: [key].into(),
                    slot: Some(active),
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
    if let Some((key, group, slot)) = assign {
        let mut theme = app.global_colors.clone();
        assign_component(&mut theme, &key, group, slot);
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
    let mut theme = app.global_colors.clone();
    let modifying_membership = !matches!(edit.group, Group::Bucket);
    let items = modifying_membership.then(|| items(app));
    let mut apply = false;
    let mut cancel = false;
    let mut remove = false;
    let modal = egui::Modal::new(egui::Id::new("tower-color-group")).show(ctx, |ui| {
        ui.set_width(380.);
        ui.heading(if matches!(edit.group, Group::New) {
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
        }
        ui.horizontal(|ui| {
            ui.strong(t(z, "耗材槽", "Filament slot"));
            ui.label(RichText::new(MATERIAL).weak());
        });
        ui.horizontal_wrapped(|ui| {
            for (index, &rgb) in theme.palette.iter().enumerate() {
                let slot = (index + 1) as u32;
                let rgb = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                let ink =
                    if rgb.r() as f32 * 0.2126 + rgb.g() as f32 * 0.7152 + rgb.b() as f32 * 0.0722
                        > 130.
                    {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    };
                let response = ui.add(
                    egui::Button::new(RichText::new(slot.to_string()).color(ink))
                        .min_size(egui::vec2(36., 30.))
                        .selected(edit.slot == Some(slot))
                        .fill(rgb)
                        .stroke(if edit.slot == Some(slot) {
                            ui.visuals().widgets.active.fg_stroke
                        } else {
                            ui.visuals().widgets.noninteractive.bg_stroke
                        }),
                );
                if response
                    .on_hover_text(format!(
                        "{} · #{:02X}{:02X}{:02X}",
                        color_label([rgb.r(), rgb.g(), rgb.b()], z),
                        rgb.r(),
                        rgb.g(),
                        rgb.b()
                    ))
                    .clicked()
                {
                    edit.slot = Some(slot);
                }
            }
        });
        if let Some(items) = &items {
            ui.separator();
            ui.strong(t(z, "零件", "Parts"));
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() * 0.38).min(280.))
                .show(ui, |ui| {
                    for (key, item) in items {
                        let mut selected = edit.keys.contains(key);
                        if ui
                            .checkbox(&mut selected, workflow::color_component_name(&item.path, z))
                            .on_hover_text(&item.path)
                            .changed()
                        {
                            if selected {
                                edit.keys.insert(key.clone());
                            } else {
                                edit.keys.remove(key);
                            }
                        }
                    }
                });
        }
        ui.separator();
        ui.horizontal(|ui| {
            let valid =
                !edit.keys.is_empty() && !edit.name.trim().is_empty() && app.export_rx.is_none();
            apply = ui
                .add_enabled(valid, egui::Button::new(t(z, "应用", "Apply")))
                .clicked();
            cancel = ui.button(t(z, "取消", "Cancel")).clicked();
            if matches!(edit.group, Group::Custom(_)) {
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
    if apply {
        edit.apply_to(&mut theme);
    } else if remove && let Group::Custom(i) = edit.group {
        theme.groups.remove(i);
    }
    if apply || remove {
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

    #[test]
    fn color_rows_partition_quantities_and_preview_assignment_keeps_other_colors() {
        let mut theme = ColorTheme::default();
        let path_a = "Catalog/Runners/Runner Guide.stl";
        let path_b = "Catalog/Runners/Runner Slide.stl";
        let a = catalog_colors::component(path_a);
        let b = catalog_colors::component(path_b);
        let keys: BTreeSet<_> = [a.clone(), b.clone()].into();
        let items = [
            (
                a.clone(),
                Item {
                    path: path_a.into(),
                    count: 2,
                },
            ),
            (
                b.clone(),
                Item {
                    path: path_b.into(),
                    count: 4,
                },
            ),
        ]
        .into();
        theme.groups.push(ColorGroup {
            name: "Hardware".into(),
            components: keys.clone(),
        });
        theme.components.extend([(a.clone(), 1), (b.clone(), 2)]);
        let mut config = Config::default();
        theme.apply(&mut config);
        let before = color_buckets(&keys, &items, &config);
        assert_eq!(before[&1].count, 2);
        assert_eq!(before[&2].count, 4);

        assign_component(&mut theme, &a, None, 3);
        theme.apply(&mut config);
        assert_eq!(theme.groups[0].components, [b.clone()].into());
        assert_eq!(config.slot(path_a), 3);
        assert_eq!(config.slot(path_b), 2);
        assign_component(&mut theme, &a, Some(0), 2);
        theme.apply(&mut config);
        let after = color_buckets(&keys, &items, &config);
        assert_eq!(after.len(), 1);
        assert_eq!(after[&2].count, 6);
        assert_eq!(theme.groups[0].components, keys);
    }

    #[test]
    fn regrouping_moves_components_and_preserves_color_assignment_in_saved_projects() {
        let path = "generated/gridfinity/Gridfinity-12.stl";
        let key = catalog_colors::component(path);
        let mut theme: ColorTheme = toml::from_str("filament = 1").unwrap();
        assert!(theme.groups.is_empty());
        for (name, slot) in [("Small bins", 2), ("Travel bins", 3)] {
            Edit {
                group: Group::New,
                name: name.into(),
                keys: [key.clone()].into(),
                slot: Some(slot),
            }
            .apply_to(&mut theme);
        }
        let saved: ColorTheme = toml::from_str(&toml::to_string(&theme).unwrap()).unwrap();
        assert_eq!(saved.groups.len(), 1);
        assert_eq!(saved.groups[0].name, "Travel bins");
        saved.validate().unwrap();
        let mut config = Config::default();
        saved.apply(&mut config);
        assert_eq!(config.slot("generated/gridfinity/Gridfinity-24.stl"), 3);
        assert_eq!(builtin(path), 4);
        assert_eq!(builtin("Catalog/Grid Dividers/A.stl"), 3);
        assert_eq!(builtin("Catalog/Runners/Runner Slide.stl"), 1);
    }
}
