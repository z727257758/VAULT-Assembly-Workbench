use super::*;
use crate::print_plate::{FilamentMaterial, MaterialLibrary};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Appearance,
    View,
    Labels,
    Library,
    Printer,
    Materials,
    Print,
    Export,
}

impl Page {
    fn title(self, zh: bool) -> &'static str {
        match self {
            Self::Appearance => t(zh, "外观与语言", "Appearance & language"),
            Self::View => t(zh, "3D 视图", "3D view"),
            Self::Labels => t(zh, "标签", "Labels"),
            Self::Library => t(zh, "模型库", "Model library"),
            Self::Printer => t(zh, "打印机", "Printer"),
            Self::Materials => t(zh, "材料", "Materials"),
            Self::Print => t(zh, "打印参数", "Print parameters"),
            Self::Export => t(zh, "导出", "Export"),
        }
    }
}

#[derive(Clone)]
struct MaterialEditor {
    material: FilamentMaterial,
    selected: Option<usize>,
    color: [u8; 3],
}

impl MaterialEditor {
    fn first(material: FilamentMaterial, library: &MaterialLibrary) -> Self {
        let swatch = library.swatches(material).first().copied();
        Self {
            material,
            selected: swatch.map(|_| 0),
            color: swatch.unwrap_or([255; 3]),
        }
    }
}

fn material_settings_ui(ui: &mut egui::Ui, library: &mut MaterialLibrary, zh: bool) {
    let id = ui.id().with("material-editor");
    let mut editor = ui
        .data_mut(|data| data.get_temp::<MaterialEditor>(id))
        .unwrap_or_else(|| MaterialEditor::first(FilamentMaterial::PlaBasic, library));
    let mut material = editor.material;
    egui::ComboBox::from_id_salt("settings-material")
        .selected_text(material.name())
        .width(205.)
        .show_ui(ui, |ui| {
            for option in FilamentMaterial::ALL {
                ui.selectable_value(&mut material, option, option.name());
            }
        });
    if material != editor.material
        || editor
            .selected
            .is_some_and(|i| i >= library.swatches(material).len())
    {
        editor = MaterialEditor::first(material, library);
    }
    ui.small(format!(
        "{} · {}",
        t(zh, "3MF 耗材预设", "3MF filament profile"),
        material.profile_name()
    ));
    if material == FilamentMaterial::PetgMatte {
        ui.small(t(
            zh,
            "PETG Matte 使用独立色板，导出采用 PETG Basic 的打印参数。",
            "PETG Matte has its own swatches and uses the PETG Basic print profile.",
        ));
    }
    ui.add_space(12.);
    ui.strong(t(zh, "常用颜色", "Color swatches"));
    let swatches = library.swatches(material);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4., 5.);
        for (index, &color) in swatches.iter().enumerate() {
            let (rect, response) =
                ui.allocate_exact_size(egui::Vec2::splat(27.), egui::Sense::click());
            let [r, g, b] = color;
            let border = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter()
                .circle(rect.center(), 10., Color32::from_rgb(r, g, b), border);
            if editor.selected == Some(index) || response.has_focus() || response.hovered() {
                ui.painter().circle_stroke(
                    rect.center(),
                    13.,
                    egui::Stroke::new(1.5, ui.visuals().text_color()),
                );
            }
            if response
                .on_hover_text(format!("#{r:02X}{g:02X}{b:02X}"))
                .clicked()
            {
                editor.selected = Some(index);
                editor.color = color;
            }
        }
    });
    ui.add_space(8.);
    ui.horizontal_wrapped(|ui| {
        if ui.button(t(zh, "添加颜色", "Add color")).clicked() {
            editor.selected = None;
            editor.color = [62, 124, 149];
        }
        if ui
            .add_enabled(
                editor.selected.is_some(),
                egui::Button::new(t(zh, "移除颜色", "Remove color")),
            )
            .clicked()
            && let Some(index) = editor.selected
        {
            library.swatches_mut(material).remove(index);
            editor = MaterialEditor::first(material, library);
        }
        if ui
            .button(t(zh, "恢复默认色板", "Restore swatches"))
            .clicked()
        {
            library.restore(material);
            editor = MaterialEditor::first(material, library);
        }
    });
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label(if editor.selected.is_some() {
            t(zh, "选中颜色", "Selected color")
        } else {
            t(zh, "新颜色", "New color")
        });
        ui.color_edit_button_srgb(&mut editor.color);
        let [r, g, b] = editor.color;
        ui.monospace(format!("#{r:02X}{g:02X}{b:02X}"));
        let swatches = library.swatches(material);
        let available = !swatches.contains(&editor.color);
        let label = if editor.selected.is_some() {
            t(zh, "保存颜色", "Save color")
        } else {
            t(zh, "加入色板", "Add to swatches")
        };
        if ui
            .add_enabled(available, egui::Button::new(label))
            .clicked()
        {
            let colors = library.swatches_mut(material);
            if let Some(index) = editor.selected {
                colors[index] = editor.color;
            } else {
                colors.push(editor.color);
                editor.selected = Some(colors.len() - 1);
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(id, editor));
}

impl App {
    pub(super) fn settings_panel(&mut self, ctx: &egui::Context) {
        let Some(mut page) = self.settings_page else {
            return;
        };
        let zh = self.zh;
        let previous_theme = self.appearance;
        let previous_font = self.label_font;
        let previous_export = self.export_options.clone();
        let previous_materials = self.material_library.clone();
        let mut done = false;
        let mut library = false;
        let size = ctx.content_rect().size();
        let width = (size.x - 48.).clamp(680., 820.);
        let height = (size.y - 180.).clamp(300., 480.);
        let nav_width = 172.;
        let modal = egui::Modal::new(egui::Id::new("workspace-settings"))
            .frame(
                egui::Frame::popup(&ctx.style())
                    .fill(ctx.style().visuals.window_fill)
                    .corner_radius(14.)
                    .inner_margin(18.),
            )
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.heading(t(zh, "设置", "Settings"));
                ui.separator();
                ui.horizontal_top(|ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .corner_radius(8.)
                        .inner_margin(8.)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(nav_width);
                                ui.set_height(height - 16.);
                                for (group, pages) in [
                                (
                                    t(zh, "工作区", "Workspace"),
                                    &[Page::Appearance, Page::View, Page::Labels, Page::Library][..],
                                ),
                                (t(zh, "打印", "Printing"), &[Page::Printer, Page::Materials, Page::Print, Page::Export][..]),
                            ] {
                                    ui.add_space(4.);
                                    ui.label(RichText::new(group).small().weak());
                                    for &target in pages {
                                        if ui
                                            .add_sized(
                                                [nav_width, 34.],
                                                egui::Button::selectable(page == target, target.title(zh)),
                                            )
                                            .clicked()
                                        {
                                            page = target;
                                        }
                                    }
                                    ui.add_space(8.);
                                }
                            });
                        });
                    let content_width = width - nav_width - 16. - ui.spacing().item_spacing.x;
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_width, height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(content_width);
                            ui.heading(page.title(zh));
                            ui.add_space(6.);
                            egui::ScrollArea::vertical()
                                .id_salt(page.title(false))
                                .max_height((height - 38.).max(1.))
                                .show(ui, |ui| match page {
                                    Page::Appearance => {
                                        ui.strong(t(zh, "主题", "Theme"));
                                        ui.horizontal(|ui| {
                                            ui.selectable_value(&mut self.appearance, appearance::Appearance::Light, t(zh, "浅色", "Light"));
                                            ui.selectable_value(&mut self.appearance, appearance::Appearance::Dark, t(zh, "深色", "Dark"));
                                        });
                                        ui.add_space(12.);
                                        ui.strong(t(zh, "界面语言", "Interface language"));
                                        ui.horizontal(|ui| {
                                            ui.selectable_value(&mut self.zh, true, "中文");
                                            ui.selectable_value(&mut self.zh, false, "English");
                                        });
                                    }
                                    Page::View => {
                                        ui.add_enabled_ui(!self.preview_modes.special(), |ui| {
                                            ui.checkbox(&mut self.dimensions, t(zh, "尺寸三视图", "Dimension views"));
                                        });
                                        ui.checkbox(&mut self.ghost, t(zh, "外壳透明", "Ghost shell"));
                                        ui.checkbox(&mut self.export_options.plate.preview_colors, t(zh, "打印颜色预览", "Print colors"));
                                    }
                                    Page::Labels => self.label_font_ui(ui),
                                    Page::Library => {
                                        if self.last_model_root.as_os_str().is_empty() {
                                            ui.label(t(zh, "尚未选择模型库", "No model library selected"));
                                        } else {
                                            ui.add(egui::Label::new(self.last_model_root.display().to_string()).wrap());
                                        }
                                        ui.small(t(zh, "原始模型文件以只读方式打开。", "Source models are opened read-only."));
                                        library = ui.add_enabled(!self.busy, egui::Button::new(t(zh, "选择模型库…", "Choose model library…"))).clicked();
                                    }
                                    Page::Printer => {
                                        ui.strong("Bambu Lab P1S · 0.4 mm");
                                        ui.label(t(zh, "平台 256 × 256 mm · 参考配置限高 250 mm", "256 × 256 mm bed · 250 mm profile height limit"));
                                        for material in crate::print_plate::FilamentMaterial::ALL {
                                            if (1..=self.export_options.plate.slot_count()).any(|slot| self.export_options.plate.appearance(slot).material == material) {
                                                ui.label(material.name()).on_hover_text(material.profile_name());
                                            }
                                        }
                                    }
                                    Page::Materials => material_settings_ui(ui, &mut self.material_library, zh),
                                    Page::Print => {
                                        ui.small(t(zh, "参数应用于当前成员的 3MF 和打印板预览。", "Parameters apply to the current member’s 3MF and plate preview."));
                                        ui.add_enabled_ui(self.export_rx.is_none(), |ui| {
                                            print_parameters_ui(ui, &mut self.export_options.plate, zh);
                                        });
                                    }
                                    Page::Export => {
                                        ui.strong(t(zh, "文件格式", "File format"));
                                        ui.add_enabled_ui(self.export_rx.is_none(), |ui| {
                                            for (format, label) in [
                                                (crate::export::Format::ThreeMf, t(zh, "3MF 工程（P1S 自动摆盘）", "3MF project (P1S plate layout)")),
                                                (crate::export::Format::Stl, t(zh, "STL 文件夹（每份单独文件）", "STL folder (one file per copy)")),
                                            ] {
                                                ui.radio_value(&mut self.export_options.format, format, label);
                                            }
                                        });
                                    }
                                });
                        },
                    );
                });
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    done = ui
                        .add(
                            egui::Button::new(RichText::new(t(zh, "完成", "Done")).color(Color32::WHITE))
                                .fill(appearance::BRAND),
                        )
                        .clicked();
                });
            });
        self.settings_page = (!(done || modal.should_close())).then_some(page);

        let plate_changed = self.export_options.plate != previous_export.plate;
        let font_changed = self.label_font != previous_font;
        let theme_changed = self.appearance != previous_theme;
        let language_changed = self.zh != zh;
        if plate_changed
            || font_changed
            || theme_changed
            || language_changed
            || self.material_library != previous_materials
            || self.export_options.format != previous_export.format
        {
            if let Err(error) = self.save_settings() {
                self.export_options = previous_export;
                self.material_library = previous_materials;
                self.label_font = previous_font;
                self.appearance = previous_theme;
                self.zh = zh;
                self.status = error;
            } else {
                if theme_changed {
                    self.appearance.apply(ctx);
                }
                if language_changed {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                        crate::app_name(self.zh).into(),
                    ));
                }
                if plate_changed {
                    self.commit_member();
                }
                if font_changed && self.tab == 0 && !self.catalog.root.as_os_str().is_empty() {
                    if self.scene.assembly || self.combination_view {
                        self.assembly(ctx);
                    } else if let Some(part) = self.scene.parts.first()
                        && crate::label::is_label(&part.pose.path)
                    {
                        self.single(part.pose.path.clone(), ctx);
                    }
                }
            }
        }
        if library {
            self.choose_library(ctx);
        }
    }

    fn label_font_ui(&mut self, ui: &mut egui::Ui) {
        use crate::label::Font;
        let zh = self.zh;
        ui.strong(t(zh, "标签字体", "Label font"));
        for (font, name) in [
            (None, t(zh, "沿用方案字体", "Use saved fonts")),
            (Some(Font::Chinese), t(zh, "微软雅黑", "Microsoft YaHei")),
            (
                Some(Font::ChineseBold),
                t(zh, "微软雅黑粗体", "Microsoft YaHei Bold"),
            ),
            (Some(Font::Arial), "Arial"),
            (Some(Font::ArialBold), "Arial Bold"),
        ] {
            ui.radio_value(&mut self.label_font, font, name);
        }
    }

    pub(super) fn settings_smoke(&mut self, ctx: &egui::Context) {
        assert!(
            self.smoke_start.elapsed() < Duration::from_secs(45),
            "Timed out capturing the settings modal"
        );
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.smoke_stage == 0 {
            self.appearance = appearance::Appearance::Light;
            self.appearance.apply(ctx);
            self.settings_page = Some(Page::Appearance);
            self.smoke_stage = 1;
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 12 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(screenshot) = screenshot else {
            return;
        };
        let (page, filename) = if self.smoke_stage == 1 {
            (Page::Appearance, "appearance-light.png")
        } else {
            (Page::Print, "print-dark-compact.png")
        };
        assert!(self.settings_page == Some(page));
        let folder = self.home.join("output/settings-ui");
        std::fs::create_dir_all(&folder).expect("Create settings smoke output");
        let pixels: Vec<_> = screenshot
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect();
        image::save_buffer(
            folder.join(filename),
            &pixels,
            screenshot.size[0] as u32,
            screenshot.size[1] as u32,
            image::ColorType::Rgba8,
        )
        .expect("Save settings screenshot");
        if self.smoke_stage == 1 {
            self.appearance = appearance::Appearance::Dark;
            self.appearance.apply(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1050., 700.)));
            self.settings_page = Some(Page::Print);
            self.smoke_stage = 2;
            self.smoke_frame = 0;
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn print_parameters_ui(ui: &mut egui::Ui, p: &mut crate::print_plate::Config, zh: bool) {
    ui.strong(t(zh, "摆盘", "Plate layout"));
    ui.horizontal(|ui| {
        ui.label(t(zh, "打印板", "Plate"));
        ui.selectable_value(&mut p.textured, true, "Textured PEI");
        ui.selectable_value(&mut p.textured, false, "Smooth PEI / High Temp");
    });
    ui.add(egui::Slider::new(&mut p.margin, 0.0..=30.0).text(t(
        zh,
        "平台边距 mm",
        "Bed margin mm",
    )));
    ui.add(egui::Slider::new(&mut p.gap, 0.0..=30.0).text(t(zh, "零件间距 mm", "Part spacing mm")));
    ui.add(egui::Slider::new(&mut p.brim, 0.0..=10.0).text(t(
        zh,
        "外侧 Brim mm（0 关闭）",
        "Outer brim mm (0 off)",
    )));
    ui.checkbox(
        &mut p.rotate,
        t(zh, "允许平面旋转 90°", "Allow 90° rotation on bed"),
    );
    ui.checkbox(
        &mut p.flat_text_face_down,
        t(
            zh,
            "平面分色标牌字面朝下",
            "Print flat-color labels face down",
        ),
    )
    .on_hover_text(t(
        zh,
        "影响独立标牌与可拆标牌，盒子一体标签台保持原朝向。",
        "Applies to standalone and removable labels. Integral bin shelves keep their orientation.",
    ));
    ui.checkbox(
        &mut p.split,
        t(zh, "放不下时自动增加打印盘", "Add plates on overflow"),
    );
    ui.checkbox(
        &mut p.center,
        t(zh, "每盘零件整体居中", "Center each plate’s parts"),
    )
    .on_hover_text(t(
        zh,
        "保留间距整体居中，若触及禁放区则保留原摆放。",
        "Preserve spacing and keep the original placement if centering hits the keep-out.",
    ));
    ui.separator();
    ui.strong(t(zh, "切片参数", "Slicing parameters"));
    ui.add(
        egui::Slider::new(&mut p.layer, 0.08..=0.28)
            .step_by(0.01)
            .text(t(zh, "层高 mm", "Layer height mm")),
    );
    ui.add(egui::Slider::new(&mut p.walls, 1..=10).text(t(zh, "壁数", "Wall loops")));
    ui.add(egui::Slider::new(&mut p.infill, 0..=100).text(t(zh, "螺旋填充 %", "Gyroid infill %")));
    ui.checkbox(
        &mut p.support_by_name,
        t(
            zh,
            "为 Turn On Supports 零件启用支撑",
            "Enable support for Turn On Supports parts",
        ),
    );
    ui.checkbox(
        &mut p.module_clip_supports,
        t(
            zh,
            "模块底部卡扣局部支撑",
            "Local supports under module clips",
        ),
    )
    .on_hover_text(t(
        zh,
        "为识别到的标准 S/M/L 模块卡扣写入强制支撑涂绘。",
        "Paint manual tree supports under recognized standard S/M/L module clips.",
    ));
}
