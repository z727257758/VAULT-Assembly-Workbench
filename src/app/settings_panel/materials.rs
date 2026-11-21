use super::{MaterialImport, t};
use crate::print_plate::{
    FilamentMaterial, GroupAppearance, MaterialLibrary, PaletteGroup, Printer,
};
use crate::ui_style;
use eframe::egui::{self, Atom, Color32, RichText};

const DEFAULT_CARD_WIDTH: f32 = 264.;

pub(super) fn show(
    ui: &mut egui::Ui,
    library: &mut MaterialLibrary,
    import: &MaterialImport,
    printer: Printer,
    zh: bool,
) -> bool {
    let pick_files = ui
        .horizontal_wrapped(|ui| {
            ui.strong(t(zh, "可选耗材", "Available filaments"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clicked = ui
                    .add_enabled(
                        import.pending.is_none(),
                        egui::Button::new(t(
                            zh,
                            "导入 Bambu Studio 预设…",
                            "Import Bambu Studio presets…",
                        )),
                    )
                    .clicked();
                if import.pending.is_some() {
                    ui.spinner();
                }
                clicked
            })
            .inner
        })
        .inner;
    match &import.feedback {
        Some(Ok(count)) => {
            ui.small(if zh {
                format!("已导入 {count} 项耗材预设。")
            } else {
                format!("Imported {count} filament presets.")
            });
        }
        Some(Err(error)) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        None => {}
    }
    ui.add_space(4.);
    let materials: Vec<_> = library.choices().collect();
    for material in materials {
        preset_row(ui, library, material, printer, zh);
        ui.separator();
    }

    ui.add_space(8.);
    ui.horizontal(|ui| {
        ui.strong(t(zh, "默认耗材", "Default filaments"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(ui_style::quiet_button(t(
                    zh,
                    "恢复初始值",
                    "Restore defaults",
                )))
                .clicked()
            {
                library.reset_default_appearances();
            }
        });
    });
    ui.small(t(
        zh,
        "用于新项目和预设新建的项目。",
        "Applies to new projects, including those made from presets.",
    ));
    for &(role, cn, en) in &crate::catalog::colors::ROLES {
        default_row(ui, library, PaletteGroup::Role(role), t(zh, cn, en), zh);
    }
    default_row(
        ui,
        library,
        PaletteGroup::Text,
        t(zh, "字体颜色", "Text color"),
        zh,
    );
    pick_files
}

fn preset_row(
    ui: &mut egui::Ui,
    library: &mut MaterialLibrary,
    material: FilamentMaterial,
    printer: Printer,
    zh: bool,
) {
    ui.push_id(material, |ui| {
        let hidden = library.is_hidden(material);
        let total = library.swatches(material).len();
        let shown = library.visible_swatches(material).count();
        let state = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id("material-row"),
            false,
        );
        let mut title_clicked = false;
        let mut header = state.show_header(ui, |ui| {
            let title_width = (ui.available_width() * 0.31).clamp(95., 180.);
            let name = library.name(material);
            let title = if hidden {
                RichText::new(name).weak()
            } else {
                RichText::new(name)
            };
            title_clicked = ui
                .allocate_ui_with_layout(
                    egui::vec2(title_width, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(title)
                                .truncate()
                                .sense(egui::Sense::click()),
                        )
                    },
                )
                .inner
                .on_hover_text(name)
                .clicked();
            ui.small(format!("{shown} / {total}")).on_hover_text(t(
                zh,
                "可见色样 / 全部色样",
                "Visible / total swatches",
            ));
            if hidden {
                ui.small(RichText::new(t(zh, "已隐藏", "Hidden")).weak());
            } else {
                let max_samples =
                    ((ui.available_width() - 105.) / 16.).floor().clamp(0., 9.) as usize;
                let colors: Vec<_> = library.visible_swatches(material).collect();
                let count = max_samples.min(colors.len());
                color_samples(ui, (0..count).map(|i| colors[i * colors.len() / count]));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(ui_style::quiet_button(if hidden {
                        t(zh, "取消隐藏", "Show material")
                    } else {
                        t(zh, "全部隐藏", "Hide all")
                    }))
                    .clicked()
                {
                    library.set_hidden(material, !hidden);
                }
            });
        });
        if title_clicked {
            header.toggle();
        }
        header.body(|ui| {
            preset_details(ui, library, material, printer, zh);
            swatch_editor(ui, library, material, zh);
        });
    });
}

fn color_samples(ui: &mut egui::Ui, colors: impl IntoIterator<Item = [u8; 3]>) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 2.;
        for rgb in colors {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(14., 18.), egui::Sense::hover());
            ui.painter()
                .circle(rect.center(), 6.5, color32(rgb), ui.visuals().window_stroke);
            response.on_hover_text(hex(rgb));
        }
    });
}

fn color32([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn preset_details(
    ui: &mut egui::Ui,
    library: &MaterialLibrary,
    material: FilamentMaterial,
    printer: Printer,
    zh: bool,
) {
    ui.small(format!(
        "{} · {}",
        t(zh, "3MF 耗材预设", "3MF filament profile"),
        library.profile_name(material, printer)
    ));
    if let Some(preset) = library.preset(material) {
        ui.small(format!(
            "{} · {}",
            t(zh, "继承预设", "Base preset"),
            preset
                .parent
                .as_deref()
                .unwrap_or_else(|| preset.base.profile_name(printer))
        ));
        if !preset.printers.is_empty() {
            let names = preset
                .printers
                .iter()
                .map(|printer| printer.name())
                .collect::<Vec<_>>()
                .join(" / ");
            ui.small(format!(
                "{} · {names}",
                t(zh, "适用机型", "Compatible printers")
            ));
            if !preset.printers.contains(&printer) {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t(
                        zh,
                        "此预设不适用于当前机型。",
                        "This preset is unavailable for the current printer.",
                    ),
                );
            }
        }
        ui.horizontal_wrapped(|ui| {
            for (key, title, unit) in [
                ("nozzle_temperature", t(zh, "喷嘴", "Nozzle"), " °C"),
                (
                    "textured_plate_temp",
                    t(zh, "纹理 PEI", "Textured PEI"),
                    " °C",
                ),
                ("filament_flow_ratio", t(zh, "流量比", "Flow ratio"), ""),
                (
                    "filament_max_volumetric_speed",
                    t(zh, "最大体积流量", "Max flow"),
                    " mm³/s",
                ),
            ] {
                if let Some(value) = preset.options.get(key) {
                    ui.small(format!("{title} {value}{unit}"));
                }
            }
        });
    }
    if material == FilamentMaterial::PetgMatte {
        ui.small(t(
            zh,
            "PETG Matte 使用独立色板，导出采用 PETG Basic 的打印参数。",
            "PETG Matte has its own swatches and uses the PETG Basic print profile.",
        ));
    }
}

#[derive(Clone)]
struct SwatchEditor {
    selected: Option<usize>,
    color: [u8; 3],
}

impl SwatchEditor {
    fn first(colors: &[[u8; 3]]) -> Self {
        Self {
            selected: (!colors.is_empty()).then_some(0),
            color: colors.first().copied().unwrap_or([255; 3]),
        }
    }
}

fn swatch_editor(
    ui: &mut egui::Ui,
    library: &mut MaterialLibrary,
    material: FilamentMaterial,
    zh: bool,
) {
    let id = ui.make_persistent_id("material-swatch-editor");
    let swatches = library.swatches(material);
    let mut editor = ui
        .data_mut(|data| data.get_temp::<SwatchEditor>(id))
        .filter(|editor| editor.selected.is_none_or(|index| index < swatches.len()))
        .unwrap_or_else(|| SwatchEditor::first(swatches));

    ui.add_space(6.);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4., 5.);
        for (index, &rgb) in swatches.iter().enumerate() {
            let hidden = library.is_swatch_hidden(material, rgb);
            let response = ui_style::swatch(ui, color32(rgb), editor.selected == Some(index), "");
            let tooltip = if hidden {
                format!("{} · {}", hex(rgb), t(zh, "已隐藏", "Hidden"))
            } else {
                hex(rgb)
            };
            if hidden {
                let center = response.rect.center();
                let [r, g, b] = rgb.map(u16::from);
                let ink = if r + g + b > 380 {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
                ui.painter().line_segment(
                    [center + egui::vec2(-7., 7.), center + egui::vec2(7., -7.)],
                    egui::Stroke::new(1.6, ink),
                );
            }
            if response.on_hover_text(tooltip).clicked() {
                editor.selected = Some(index);
                editor.color = rgb;
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(t(zh, "添加颜色", "Add color")).clicked() {
            editor.selected = None;
            editor.color = [62, 124, 149];
        }
        if let Some(index) = editor.selected {
            let rgb = library.swatches(material)[index];
            let hidden = library.is_swatch_hidden(material, rgb);
            if ui
                .small_button(if hidden {
                    t(zh, "显示颜色", "Show color")
                } else {
                    t(zh, "隐藏颜色", "Hide color")
                })
                .clicked()
            {
                library.set_swatch_hidden(material, rgb, !hidden);
            }
            if ui
                .add(ui_style::danger_button(t(zh, "移除颜色", "Remove color")))
                .clicked()
            {
                library.set_swatch_hidden(material, rgb, false);
                library.swatches_mut(material).remove(index);
                editor = SwatchEditor::first(library.swatches(material));
            }
        }
        if ui
            .small_button(t(zh, "恢复默认色板", "Restore swatches"))
            .clicked()
        {
            library.restore(material);
            editor = SwatchEditor::first(library.swatches(material));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(if editor.selected.is_some() {
            t(zh, "选中颜色", "Selected color")
        } else {
            t(zh, "新颜色", "New color")
        });
        ui.color_edit_button_srgb(&mut editor.color);
        ui.monospace(hex(editor.color));
        let available = !library.swatches(material).contains(&editor.color);
        let label = if editor.selected.is_some() {
            t(zh, "保存颜色", "Save color")
        } else {
            t(zh, "加入色板", "Add to swatches")
        };
        if ui
            .add_enabled(available, egui::Button::new(label))
            .clicked()
        {
            if let Some(index) = editor.selected {
                let previous = library.swatches(material)[index];
                let hidden = library.is_swatch_hidden(material, previous);
                library.set_swatch_hidden(material, previous, false);
                library.swatches_mut(material)[index] = editor.color;
                library.set_swatch_hidden(material, editor.color, hidden);
            } else {
                let colors = library.swatches_mut(material);
                colors.push(editor.color);
                editor.selected = Some(colors.len() - 1);
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(id, editor));
}

fn default_row(
    ui: &mut egui::Ui,
    library: &mut MaterialLibrary,
    group: PaletteGroup,
    title: &str,
    zh: bool,
) {
    let Some(mut appearance) = library.default_appearance(group) else {
        return;
    };
    ui.push_id(group, |ui| {
        ui.horizontal(|ui| {
            ui.label(title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let width = ui.available_width().clamp(80., 280.);
                let selection = format!(
                    "{}  ·  {}",
                    hex(appearance.rgb),
                    library.name(appearance.material)
                );
                let arrow = egui::Id::new("arrow");
                let button = egui::Button::new(selection)
                    .right_text(Atom::custom(arrow, [16., 12.]))
                    .min_size(egui::vec2(width, ui.spacing().interact_size.y))
                    .truncate()
                    .atom_ui(ui);
                if let Some(rect) = button.rect(arrow) {
                    let center = rect.center();
                    ui.painter().line(
                        vec![
                            center + egui::vec2(-4., -2.),
                            center + egui::vec2(0., 2.),
                            center + egui::vec2(4., -2.),
                        ],
                        egui::Stroke::new(1.3, ui.style().interact(&button.response).text_color()),
                    );
                }
                default_popup(ui, &button.response, library, &mut appearance, zh);
                color_samples(ui, [appearance.rgb]);
            });
        });
    });
    if library.default_appearance(group) != Some(appearance) {
        library.set_default_appearance(group, appearance);
    }
}

fn default_popup(
    ui: &mut egui::Ui,
    button: &egui::Response,
    library: &MaterialLibrary,
    appearance: &mut GroupAppearance,
    zh: bool,
) {
    let id = egui::Popup::default_response_id(button);
    let was_open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false))
        && ui.ctx().read_response(id).is_some();
    // The nested ComboBox uses egui's single memory-backed popup slot.
    let popup = egui::Popup::from_response(button)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(4.)
        .width(DEFAULT_CARD_WIDTH + 16.)
        .open(was_open ^ button.clicked())
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .show(|ui| default_card(ui, library, appearance, zh));
    let open = popup.is_some_and(|popup| {
        let ctx = &popup.response.ctx;
        if popup.response.should_close() {
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            return false;
        }
        // The dropdown is a separate layer. Its clicks belong to this card as well.
        !was_open
            || !popup.response.clicked_elsewhere()
            || popup.inner.is_some_and(|layer| {
                ctx.input(|input| input.pointer.interact_pos())
                    .and_then(|pos| ctx.layer_id_at(pos))
                    == Some(layer)
            })
    });
    ui.data_mut(|data| data.insert_temp(id, open));
}

fn default_card(
    ui: &mut egui::Ui,
    library: &MaterialLibrary,
    appearance: &mut GroupAppearance,
    zh: bool,
) -> Option<egui::LayerId> {
    ui.set_width(DEFAULT_CARD_WIDTH);
    ui.small(t(zh, "材料", "Material"));
    let card_layer = ui.layer_id();
    let dropdown = egui::ComboBox::from_id_salt("material")
        .selected_text(library.name(appearance.material))
        .width(ui.available_width())
        .height(230.)
        .truncate()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let layer = ui.layer_id();
            ui.ctx().set_sublayer(card_layer, layer);
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                ui.close();
            }
            let mut materials = library.visible_choices().peekable();
            if materials.peek().is_none() {
                ui.small(t(
                    zh,
                    "请先显示至少一种耗材。",
                    "Show a filament to make it available here.",
                ));
            }
            for material in materials {
                let name = library.name(material);
                if ui
                    .add(ui_style::choice_button(appearance.material == material, name).truncate())
                    .on_hover_text(name)
                    .clicked()
                {
                    appearance.material = material;
                    ui.close();
                }
            }
            layer
        });

    ui.add_space(4.);
    ui.small(t(zh, "颜色", "Color"));
    let mut swatches = library.visible_swatches(appearance.material).peekable();
    if swatches.peek().is_none() {
        ui.small(t(zh, "此耗材没有可用色样。", "No available swatches."));
        return dropdown.inner;
    }
    egui::ScrollArea::vertical()
        .max_height(180.)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4., 4.);
                for rgb in swatches {
                    if ui_style::swatch(ui, color32(rgb), appearance.rgb == rgb, "")
                        .on_hover_text(hex(rgb))
                        .clicked()
                    {
                        appearance.rgb = rgb;
                        ui.close();
                    }
                }
            });
        });
    dropdown.inner
}
