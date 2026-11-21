use super::*;
use crate::print_plate::{FilamentMaterial, MaterialLibrary};

pub(super) enum HeaderAction {
    Toggle,
    Reset,
}

fn button_info(response: egui::Response, label: &str) -> egui::Response {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn chevron(painter: &egui::Painter, center: egui::Pos2, up: bool, ink: Color32) {
    let direction = if up { -1. } else { 1. };
    painter.line(
        vec![
            center + egui::vec2(-3.6, -direction * 1.8),
            center + egui::vec2(0., direction * 1.8),
            center + egui::vec2(3.6, -direction * 1.8),
        ],
        egui::Stroke::new(1.35, ink),
    );
}

pub(super) fn header(
    ui: &mut egui::Ui,
    row: &Row,
    appearance: GroupAppearance,
    material_name: &str,
    follows_base: bool,
    open: bool,
    z: bool,
) -> Option<HeaderAction> {
    let rgb = appearance.rgb;
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let ink = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let painter = ui.painter();
    let text = |s: String, font| painter.layout_no_wrap(s, font, weak);
    let count = (row.group != Group::Text).then(|| {
        text(
            format!("{} {}", row.count, t(z, "件", "pcs")),
            small.clone(),
        )
    });
    let count_width = count.as_ref().map_or(0., |count| count.size().x + 8.);
    let color = text(
        if follows_base {
            t(z, "跟随标牌底色", "Use label body color")
        } else {
            color_label(rgb, z)
        }
        .into(),
        small.clone(),
    );
    let width = ui.available_width();
    let mut layout = egui::text::LayoutJob::simple(
        if follows_base {
            String::new()
        } else {
            material_name.to_owned()
        },
        small,
        weak,
        (width - color.size().x - 62.).max(1.),
    );
    layout.wrap.max_rows = 1;
    layout.wrap.break_anywhere = true;
    let material = painter.layout_job(layout);
    let meta_width = color.size().x
        + if follows_base {
            0.
        } else {
            6. + material.size().x
        };
    let label_space = width - 31. - count_width - meta_width - 28.;
    let stacked = !open && label_space < if z { 55. } else { 82. };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, if stacked { 42. } else { 32. }),
        egui::Sense::hover(),
    );
    let reset_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - 54., rect.top()),
        egui::pos2(rect.right() - 25., rect.bottom()),
    );
    let label_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(
            if open {
                reset_rect.left()
            } else {
                rect.right()
            },
            rect.bottom(),
        ),
    );
    let toggle = button_info(
        ui.interact(label_rect, ui.id().with("toggle"), egui::Sense::click()),
        &row.name,
    );
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let left = rect.left() + 29.;
    painter.circle(
        egui::pos2(rect.left() + 13., center.y),
        9.2,
        if follows_base {
            Color32::TRANSPARENT
        } else {
            Color32::from_rgb(rgb[0], rgb[1], rgb[2])
        },
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
    let name_y = if stacked { center.y - 8. } else { center.y };
    let name_width = if open {
        reset_rect.left() - left - count_width
    } else if stacked {
        rect.right() - left - count_width - 22.
    } else {
        label_space
    };
    let mut layout = egui::text::LayoutJob::simple(row.name.clone(), body, ink, name_width.max(1.));
    layout.wrap.max_rows = 1;
    layout.wrap.break_anywhere = true;
    let name = painter.layout_job(layout);
    let count_x = left + name.size().x + 7.;
    painter.galley(egui::pos2(left, name_y - name.size().y * 0.5), name, ink);
    if let Some(count) = count {
        painter.galley(
            egui::pos2(count_x, name_y - count.size().y * 0.5),
            count,
            weak,
        );
    }
    let arrow = egui::pos2(rect.right() - 12., center.y);
    if !open {
        let x = if stacked {
            left
        } else {
            rect.right() - 26. - meta_width
        };
        let y = if stacked { center.y + 9. } else { center.y };
        let material_x = x + color.size().x + 6.;
        painter.galley(egui::pos2(x, y - color.size().y * 0.5), color, weak);
        if !follows_base {
            painter.galley(
                egui::pos2(material_x, y - material.size().y * 0.5),
                material,
                weak,
            );
        }
    }
    chevron(&painter, arrow, open, weak);
    if open {
        let reset_label = t(z, "恢复本组默认配色", "Restore this group's default");
        let reset = button_info(
            ui.interact(reset_rect, ui.id().with("reset"), egui::Sense::click()),
            reset_label,
        )
        .on_hover_text(reset_label);
        let c = reset_rect.center();
        let stroke = egui::Stroke::new(1.3, if reset.hovered() { ink } else { weak });
        let arc = (0..=22)
            .map(|n| {
                let angle = -2.48 + n as f32 / 22. * 5.5;
                c + egui::vec2(angle.cos(), angle.sin()) * 4.8
            })
            .collect();
        painter.line(arc, stroke);
        painter.line(
            vec![
                c + egui::vec2(-4.8, -6.1),
                c + egui::vec2(-4.5, -2.1),
                c + egui::vec2(-0.7, -2.5),
            ],
            stroke,
        );
        if reset.clicked() {
            return Some(HeaderAction::Reset);
        }
        let collapse_label = t(z, "收起", "Collapse");
        let collapse_rect =
            egui::Rect::from_min_max(egui::pos2(reset_rect.right(), rect.top()), rect.max);
        let collapse = button_info(
            ui.interact(
                collapse_rect,
                ui.id().with("collapse"),
                egui::Sense::click(),
            ),
            collapse_label,
        )
        .on_hover_text(collapse_label);
        if collapse.clicked() {
            return Some(HeaderAction::Toggle);
        }
    }
    toggle.clicked().then_some(HeaderAction::Toggle)
}

fn material_picker(
    ui: &mut egui::Ui,
    library: &MaterialLibrary,
    material: &mut FilamentMaterial,
    z: bool,
) {
    let width = ui.available_width();
    egui::ComboBox::from_id_salt("material")
        .width(width)
        .height(ui.ctx().content_rect().height() * 0.65)
        .selected_text(library.name(*material))
        .truncate()
        .show_ui(ui, |ui| {
            let mut choices = library.visible_choices().peekable();
            if choices.peek().is_none() {
                ui.small(t(
                    z,
                    "请在材料设置中启用耗材。",
                    "Enable a filament in Materials settings.",
                ));
            }
            let mut section = "";
            for option in choices {
                let name = library.name(option);
                let imported = library.preset(option).is_some();
                let group = if imported {
                    t(z, "导入的预设", "Imported presets")
                } else {
                    library.family(option)
                };
                if section != group {
                    section = group;
                    ui.label(RichText::new(section).small().weak());
                }
                let selected = *material == option;
                let row_width = ui.available_width();
                let (rect, row) =
                    ui.allocate_exact_size(egui::vec2(row_width, 27.), egui::Sense::click());
                let mut row = button_info(row, name);
                if imported {
                    row = row.on_hover_text(name);
                } else if option == FilamentMaterial::PetgMatte {
                    row = row.on_hover_text(t(
                        z,
                        "使用 PETG Basic 打印预设",
                        "Uses the PETG Basic print profile",
                    ));
                }
                let visuals = ui.style().interact_selectable(&row, selected);
                let ink = visuals.fg_stroke.color;
                if selected || row.hovered() || row.has_focus() {
                    ui.painter().rect(
                        rect,
                        visuals.corner_radius,
                        visuals.weak_bg_fill,
                        visuals.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                let colors: Vec<_> = library.visible_swatches(option).collect();
                let n = colors.len().min(5);
                let mut layout = egui::text::LayoutJob::simple(
                    name.to_owned(),
                    egui::TextStyle::Body.resolve(ui.style()),
                    ink,
                    (row_width - 32. - n as f32 * 9.).max(1.),
                );
                layout.wrap.max_rows = 1;
                layout.wrap.break_anywhere = true;
                let name = ui.painter().layout_job(layout);
                let name_right = rect.left() + 7. + name.size().x;
                ui.painter().galley(
                    rect.left_center() + egui::vec2(7., -0.5 * name.size().y),
                    name,
                    ink,
                );
                if selected {
                    let c = egui::pos2(name_right + 10., rect.center().y);
                    ui.painter().line(
                        vec![
                            c + egui::vec2(-3., 0.),
                            c + egui::vec2(-0.7, 2.5),
                            c + egui::vec2(4., -3.),
                        ],
                        egui::Stroke::new(1.5, ink),
                    );
                }
                for i in 0..n {
                    let [r, g, b] = colors[i * colors.len() / n];
                    let center = egui::pos2(
                        rect.right() - 10. - (n - 1 - i) as f32 * 9.,
                        rect.center().y,
                    );
                    ui.painter().circle(
                        center,
                        6.,
                        Color32::from_rgb(r, g, b),
                        egui::Stroke::new(0.8, ui.visuals().panel_fill),
                    );
                }
                if row.clicked() {
                    *material = option;
                    ui.close();
                }
            }
        });
}

fn custom_color(ui: &mut egui::Ui, size: egui::Vec2, rgb: &mut [u8; 3], z: bool) {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let label = t(z, "自定义颜色", "Custom color");
    let response = button_info(response, label).on_hover_text(label);
    let r = (size.x * 0.36).clamp(5., 9.5);
    let c = rect.center();
    for i in 0..24 {
        let angle = |n: usize| (n as f32 / 24.) * std::f32::consts::TAU;
        let point = |a: f32, radius: f32| c + egui::vec2(a.cos(), a.sin()) * radius;
        let color = Color32::from(egui::ecolor::Hsva::new(i as f32 / 24., 0.85, 1., 1.));
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                point(angle(i), r),
                point(angle(i + 1), r),
                point(angle(i + 1), r - 3.8),
                point(angle(i), r - 3.8),
            ],
            color,
            egui::Stroke::NONE,
        ));
    }
    let id = response.id.with("rgb");
    if response.clicked() {
        ui.data_mut(|d| d.insert_temp(id, *rgb));
    }
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let mut draft = ui.data(|d| d.get_temp::<[u8; 3]>(id)).unwrap_or(*rgb);
            let mut color = Color32::from_rgb(draft[0], draft[1], draft[2]);
            if egui::color_picker::color_picker_color32(
                ui,
                &mut color,
                egui::color_picker::Alpha::Opaque,
            ) {
                draft = [color.r(), color.g(), color.b()];
            }
            ui.data_mut(|d| d.insert_temp(id, draft));
            if ui
                .add(crate::ui_style::primary_button(t(
                    z,
                    "应用颜色",
                    "Apply color",
                )))
                .clicked()
            {
                *rgb = draft;
                ui.close();
            }
        });
}

pub(super) fn appearance_ui(
    ui: &mut egui::Ui,
    library: &MaterialLibrary,
    appearance: &mut GroupAppearance,
    z: bool,
) {
    crate::ui_style::inset_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        material_picker(ui, library, &mut appearance.material, z);
        ui.separator();
        let mut swatches: Vec<_> = library.visible_swatches(appearance.material).collect();
        // The open card still shows the project's assigned color.
        if !swatches.contains(&appearance.rgb) {
            swatches.push(appearance.rgb);
        }
        let width = ui.available_width();
        let size = egui::Vec2::splat(crate::ui_style::SWATCH_SIZE);
        let columns = ((width / size.x).floor() as usize).clamp(1, 10);
        let gap = (width - size.x * columns as f32) / columns.saturating_sub(1).max(1) as f32;
        ui.spacing_mut().item_spacing = egui::vec2(gap.max(0.), 1.);
        let cells = (swatches.len() + 1).max(columns);
        let mut colors = swatches.into_iter();
        for start in (0..cells).step_by(columns) {
            ui.horizontal(|ui| {
                for index in start..(start + columns).min(cells) {
                    if index == columns - 1 {
                        custom_color(ui, size, &mut appearance.rgb, z);
                    } else if let Some(rgb @ [r, g, b]) = colors.next() {
                        let name = appearance
                            .material
                            .aliz_swatch_label(rgb, z)
                            .unwrap_or_else(|| {
                                format!("{} · #{r:02X}{g:02X}{b:02X}", color_label(rgb, z))
                            });
                        if crate::ui_style::swatch(
                            ui,
                            Color32::from_rgb(r, g, b),
                            appearance.rgb == rgb,
                            "",
                        )
                        .on_hover_text(name)
                        .clicked()
                        {
                            appearance.rgb = rgb;
                        }
                    } else {
                        ui.allocate_space(size);
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_rgb_drag_is_committed_only_by_apply() {
        let ctx = egui::Context::default();
        let original = [20, 80, 100];
        let mut rgb = original;
        let mut paints = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640., 600.),
                )),
                events,
                ..Default::default()
            };
            let mut ring = egui::Pos2::ZERO;
            let painted = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ring = ui.next_widget_position() + egui::vec2(15., 13.);
                    custom_color(ui, egui::vec2(30., 26.), &mut rgb, false);
                });
            });
            let texts: Vec<_> = painted
                .shapes
                .iter()
                .filter_map(|shape| {
                    let egui::Shape::Text(text) = &shape.shape else {
                        return None;
                    };
                    Some((
                        text.galley.job.text.clone(),
                        text.pos + text.galley.rect.center().to_vec2(),
                    ))
                })
                .collect();
            (rgb, ring, texts)
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let (_, ring, _) = paints(vec![]);
        paints(vec![egui::Event::PointerMoved(ring), button(ring, true)]);
        paints(vec![button(ring, false)]);
        let (_, _, texts) = paints(vec![]);
        let text_at = |texts: &[(String, egui::Pos2)], value: &str| {
            texts
                .iter()
                .find(|(text, _)| text == value)
                .map(|(_, pos)| *pos)
                .unwrap_or_else(|| panic!("Missing {value} in {texts:?}"))
        };
        let red = text_at(&texts, "R 20");
        paints(vec![egui::Event::PointerMoved(red), button(red, true)]);
        let moved = red + egui::vec2(40., 0.);
        paints(vec![egui::Event::PointerMoved(moved)]);
        let (draft_frame, _, texts) = paints(vec![button(moved, false)]);
        assert_eq!(draft_frame, original);
        assert!(
            texts
                .iter()
                .any(|(text, _)| text.starts_with("R ") && text != "R 20")
        );
        let apply = text_at(&texts, "Apply color");
        paints(vec![egui::Event::PointerMoved(apply), button(apply, true)]);
        let (applied, _, _) = paints(vec![button(apply, false)]);
        assert_ne!(applied[0], original[0]);
        assert_eq!(&applied[1..], &original[1..]);
    }
}
