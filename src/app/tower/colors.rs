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
    BuiltIn(usize),
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
        if matches!(self.group, Group::BuiltIn(_)) {
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

fn group_slots(
    keys: &BTreeSet<String>,
    items: &BTreeMap<String, Item>,
    config: &Config,
) -> BTreeSet<u32> {
    keys.iter()
        .filter_map(|key| items.get(key))
        .map(|item| config.slot(&item.path))
        .collect()
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
    let groups: Vec<_> = builtin_keys
        .into_iter()
        .enumerate()
        .filter_map(|(i, keys)| {
            let (cn, en) = GROUP_LABELS[i];
            (!keys.is_empty()).then_some((Group::BuiltIn(i), t(z, cn, en).to_owned(), keys))
        })
        .chain(custom.iter().enumerate().map(|(i, group)| {
            (
                Group::Custom(i),
                group.name.clone(),
                group.components.clone(),
            )
        }))
        .collect();

    let mut palette = Vec::new();
    for (_, _, keys) in &groups {
        for slot in keys
            .iter()
            .filter_map(|key| items.get(key))
            .map(|item| config.slot(&item.path))
        {
            let rgb = config.palette[(slot - 1) as usize];
            if !palette.contains(&rgb) {
                palette.push(rgb);
            }
        }
    }
    if !layers::group(ui, "colors", t(z, "颜色", "Colors"), false, |ui| {
        layers::palette(ui, &palette, palette.len().min(4));
    }) {
        return;
    }
    for (group, title, keys) in groups {
        let count = keys
            .iter()
            .filter_map(|key| items.get(key))
            .map(|item| item.count)
            .sum::<u32>();
        let slots = group_slots(&keys, &items, &config);
        let slot = (slots.len() == 1).then(|| *slots.first().unwrap());
        let rgb = slot.map(|slot| config.palette[(slot - 1) as usize]);
        let id = match group {
            Group::BuiltIn(i) => ("built-in-color", i),
            Group::Custom(i) => ("custom-color", i),
            Group::New => unreachable!(),
        };
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
                let colors: Vec<_> = slots
                    .iter()
                    .map(|slot| config.palette[(*slot - 1) as usize])
                    .collect();
                if colors.is_empty() {
                    painter.circle(dot, radius, Color32::from_gray(140), stroke);
                } else {
                    let step = 2. * radius / colors.len() as f32;
                    for (index, [r, g, b]) in colors.into_iter().enumerate() {
                        let left = dot.x - radius + step * index as f32;
                        let stripe =
                            egui::Rect::from_x_y_ranges(left..=left + step, rect.y_range());
                        painter.with_clip_rect(stripe).circle_filled(
                            dot,
                            radius,
                            Color32::from_rgb(r, g, b),
                        );
                    }
                    painter.circle_stroke(dot, radius, stroke);
                }
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
                let color_name = match rgb {
                    Some(rgb) => color_label(rgb, z),
                    None if count > 0 => t(z, "混合", "Mixed"),
                    None => "—",
                };
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
                slot,
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

pub(super) fn dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut edit) = app.tower.color_edit.take() else {
        return;
    };
    let z = app.zh;
    let mut theme = app.global_colors.clone();
    let modifying_membership = !matches!(edit.group, Group::BuiltIn(_));
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
