mod materials;

use super::*;
use crate::print_plate::{BambuPreset, Printer};
use crate::ui_style;

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
            Self::Labels => t(zh, "项目标签", "Project labels"),
            Self::Library => t(zh, "模型库", "Model library"),
            Self::Printer => t(zh, "打印机", "Printer"),
            Self::Materials => t(zh, "材料", "Materials"),
            Self::Print => t(zh, "打印参数", "Print parameters"),
            Self::Export => t(zh, "导出", "Export"),
        }
    }
}

#[derive(Default)]
pub(super) struct MaterialImport {
    pending: Option<Receiver<Result<Vec<BambuPreset>, String>>>,
    feedback: Option<Result<usize, String>>,
}

impl App {
    fn start_material_import(&mut self, ctx: &egui::Context) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter("Bambu Studio", &["json", "zip", "bbsflmt", "bbscfg"])
            .pick_files()
        else {
            return;
        };
        let library = self.material_library.clone();
        let (sender, receiver) = mpsc::channel();
        self.material_import.pending = Some(receiver);
        self.material_import.feedback = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(library.read_bambu_presets(&paths));
            ctx.request_repaint();
        });
    }

    fn poll_material_import(&mut self) {
        let Some(receiver) = &self.material_import.pending else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(t(
                self.zh,
                "材料预设读取中断。",
                "Filament preset import was interrupted.",
            )
            .into()),
        };
        self.material_import.pending = None;
        self.material_import.feedback = Some(result.and_then(|presets| {
            let mut next = self.material_library.clone();
            let count = next.import_presets(presets)?;
            let previous = std::mem::replace(&mut self.material_library, next);
            if let Err(error) = self.save_settings() {
                self.material_library = previous;
                return Err(error);
            }
            Ok(count)
        }));
    }

    pub(super) fn settings_panel(&mut self, ctx: &egui::Context) {
        self.poll_material_import();
        let Some(mut page) = self.settings_page else {
            self.project_label_draft = None;
            return;
        };
        let zh = self.zh;
        let previous_theme = self.appearance;
        let previous_export = self.export_options.clone();
        let previous_materials = self.material_library.clone();
        let mut done = false;
        let mut library = false;
        let mut import_materials = false;
        let mut apply_project_printer = false;
        let mut apply_project_labels = None;
        let size = ctx.content_rect().size();
        let width = (size.x - 48.).clamp(680., 820.);
        let max_height = if page == Page::Materials { 640. } else { 480. };
        let height = (size.y - 180.).clamp(300., max_height);
        let nav_width = 172.;
        let modal = egui::Modal::new(egui::Id::new("workspace-settings"))
            .frame(ui_style::dialog_frame(&ctx.style()))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.heading(t(zh, "设置", "Settings"));
                ui.separator();
                ui.horizontal_top(|ui| {
                    let navigation = ui_style::inset_frame(ui);
                    let nav_height = height - navigation.total_margin().sum().y;
                    navigation.show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(nav_width);
                            ui.set_height(nav_height);
                            for (group, pages) in [
                                (
                                    t(zh, "工作区", "Workspace"),
                                    [Page::Appearance, Page::View, Page::Labels, Page::Library],
                                ),
                                (
                                    t(zh, "打印", "Printing"),
                                    [Page::Printer, Page::Materials, Page::Print, Page::Export],
                                ),
                            ] {
                                ui.add_space(4.);
                                ui.label(RichText::new(group).small().weak());
                                for target in pages {
                                    if ui
                                        .add_sized(
                                            [nav_width, ui.spacing().interact_size.y],
                                            ui_style::choice_button(page == target, target.title(zh)),
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
                    let content_width = ui.available_width();
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
                                    Page::Labels => apply_project_labels = self.project_label_style_ui(ui),
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
                                        apply_project_printer = ui.add_enabled_ui(
                                            self.export_rx.is_none(), |ui| self.printer_settings_ui(ui)
                                        ).inner;
                                    }
                                    Page::Materials => {
                                        import_materials = materials::show(
                                            ui,
                                            &mut self.material_library,
                                            &self.material_import,
                                            self.export_options.plate.printer,
                                            zh,
                                        );
                                    }
                                    Page::Print => {
                                        if ui.link(format!("{} · {}", t(zh, "打印机", "Printer"), self.export_options.plate.printer.name())).clicked() {
                                            page = Page::Printer;
                                        }
                                        ui.small(t(zh, "参数应用于当前成员的 3MF 和打印板预览。", "Parameters apply to the current member’s 3MF and plate preview."));
                                        ui.add_enabled_ui(self.export_rx.is_none(), |ui| {
                                            print_parameters_ui(ui, &mut self.export_options.plate, zh);
                                        });
                                    }
                                    Page::Export => {
                                        ui.strong(t(zh, "文件格式", "File format"));
                                        ui.add_enabled_ui(self.export_rx.is_none(), |ui| {
                                            for (format, label) in [
                                                (crate::export::Format::ThreeMf, t(zh, "3MF 工程（按机型自动摆盘）", "3MF project (printer-specific layout)")),
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
                    done = ui.add(ui_style::primary_button(t(zh, "完成", "Done"))).clicked();
                });
            });
        self.settings_page = (!(done || modal.should_close())).then_some(page);
        if self.settings_page != Some(Page::Labels) {
            self.project_label_draft = None;
        }
        if import_materials {
            self.start_material_import(ctx);
        }

        let plate_changed = self.export_options.plate != previous_export.plate;
        let theme_changed = self.appearance != previous_theme;
        let language_changed = self.zh != zh;
        if plate_changed
            || apply_project_printer
            || theme_changed
            || language_changed
            || self.material_library != previous_materials
            || self.export_options.format != previous_export.format
        {
            if let Err(error) = self.save_settings() {
                self.export_options = previous_export;
                self.material_library = previous_materials;
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
                let printer = self.export_options.plate.printer;
                let printer_changed = previous_export.plate.printer != printer;
                if (apply_project_printer || printer_changed) && !self.members.is_empty() {
                    // Capture any pending member edits with the old printer for undo.
                    self.export_options.plate.printer = previous_export.plate.printer;
                    self.record_workflow();
                    self.export_options.plate.printer = printer;
                }
                if plate_changed || apply_project_printer {
                    self.commit_member();
                }
                if apply_project_printer {
                    for member in &mut self.members {
                        member.preset.set_printer(printer);
                    }
                } else if printer_changed
                    && self.active_drawer.is_none()
                    && let Some(member) = self.members.get_mut(self.active_member)
                {
                    member.preset.set_printer(printer);
                }
            }
        }
        if library {
            self.choose_library(ctx);
        }
        if let Some(style) = apply_project_labels {
            let result = self.apply_project_label_style(style, ctx);
            let error = egui::Id::new("project-label-style-error");
            ctx.data_mut(|data| {
                data.remove::<String>(error);
                if let Err(message) = result {
                    data.insert_temp(error, message);
                }
            });
            ctx.request_repaint();
        }
    }

    fn printer_settings_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let zh = self.zh;
        if let Some(member) = self.members.get(self.active_member) {
            let target = match self.active_drawer {
                Some(index) => format!("{} {}", t(zh, "抽屉", "Drawer"), index + 1),
                None if !member.preset.drawers.is_empty() => {
                    t(zh, "模块及其抽屉", "Module and its drawers").to_owned()
                }
                None => t(zh, "当前模块", "Current module").to_owned(),
            };
            ui.label(format!("{} · {target}", member.name));
        }
        let printer = &mut self.export_options.plate.printer;
        egui::ComboBox::from_id_salt("printer-model")
            .selected_text(printer.name())
            .width(220.)
            .show_ui(ui, |ui| {
                for model in Printer::ALL {
                    ui.selectable_value(printer, model, model.name());
                }
            });
        ui.add_space(8.);
        let [width, depth] = printer.bed_size();
        egui::Grid::new("printer-specs")
            .spacing([20., 6.])
            .show(ui, |ui| {
                for (label, value) in [
                    (
                        t(zh, "打印平台", "Print bed"),
                        format!("{width} × {depth} mm"),
                    ),
                    (
                        t(zh, "配置限高", "Profile height limit"),
                        format!("{} mm", printer.height()),
                    ),
                    (t(zh, "喷嘴直径", "Nozzle diameter"), "0.4 mm".to_owned()),
                ] {
                    ui.weak(label);
                    ui.label(value);
                    ui.end_row();
                }
            });
        if self.members.is_empty() {
            return false;
        }
        ui.add_space(10.);
        let differs = self
            .members
            .iter()
            .any(|m| !m.preset.uses_printer(*printer));
        ui.add_enabled(
            differs,
            egui::Button::new(t(
                zh,
                "整个项目使用此机型",
                "Apply printer to entire project",
            )),
        )
        .clicked()
    }

    fn project_label_style_ui(&mut self, ui: &mut egui::Ui) -> Option<crate::label::StyleSettings> {
        use crate::label::{Font, Style, StyleSettings};

        let zh = self.zh;
        let current = self.project_label_style;
        let mut draft = self
            .project_label_draft
            .or(current)
            .unwrap_or_else(|| StyleSettings::from_config(&self.label_for_project()));
        let previous = draft;
        let error = egui::Id::new("project-label-style-error");
        let fonts = [
            (Font::Chinese, t(zh, "微软雅黑", "Microsoft YaHei")),
            (
                Font::ChineseBold,
                t(zh, "微软雅黑粗体", "Microsoft YaHei Bold"),
            ),
            (Font::Arial, "Arial"),
            (Font::ArialBold, "Arial Bold"),
        ];
        ui.label(t(
            zh,
            "统一当前项目的抽屉、箱子和 Gridfinity 标签样式。各标签保留自己的文字。",
            "Set the style for drawer, box, and Gridfinity labels in this project. Each label keeps its text.",
        ));
        if self.members.is_empty() {
            ui.small(t(
                zh,
                "请先打开或新建项目。",
                "Open or create a project first.",
            ));
        } else if current.is_none() {
            ui.small(t(
                zh,
                "现有标签继续使用已保存的样式，直到应用统一设置。",
                "Existing labels use their saved styles until you apply the project settings.",
            ));
        }
        ui.add_space(6.);
        let apply = ui.add_enabled_ui(!self.members.is_empty() && self.export_rx.is_none(), |ui| {
            egui::Grid::new("project-label-properties")
                .num_columns(2)
                .spacing([12., 8.])
                .show(ui, |ui| {
                    ui.label(t(zh, "字体", "Font"));
                    let font_name = fonts.iter().find(|(font, _)| *font == draft.font)
                        .map_or("", |(_, name)| *name);
                    egui::ComboBox::from_id_salt("project-label-font")
                        .selected_text(font_name)
                        .show_ui(ui, |ui| {
                            for (font, name) in fonts {
                                ui.selectable_value(&mut draft.font, font, name);
                            }
                        });
                    ui.end_row();

                    ui.label(t(zh, "表现形式", "Lettering style"));
                    ui.horizontal_wrapped(|ui| {
                        for (style, name) in [
                            (Style::Raised, t(zh, "凸字", "Raised")),
                            (Style::Engraved, t(zh, "凹字", "Engraved")),
                            (Style::Flat, t(zh, "平面分色", "Flat color")),
                        ] {
                            ui.selectable_value(&mut draft.style, style, name);
                        }
                    });
                    ui.end_row();

                    ui.label(t(zh, "字号", "Font size"));
                    ui.add(egui::DragValue::new(&mut draft.size).range(4. ..=12.).speed(0.1).suffix(" mm"));
                    ui.end_row();

                    ui.label(if draft.style == Style::Flat {
                        t(zh, "颜色层厚", "Color depth")
                    } else {
                        t(zh, "字高 / 刻深", "Relief / depth")
                    });
                    ui.add(egui::DragValue::new(&mut draft.depth).range(0.2..=0.8).speed(0.01).suffix(" mm"));
                    ui.end_row();

                    for (offset, name) in draft.offset.iter_mut().zip([
                        t(zh, "水平偏移", "Horizontal offset"),
                        t(zh, "垂直偏移", "Vertical offset"),
                    ]) {
                        ui.label(name);
                        ui.add(egui::DragValue::new(offset).range(-10. ..=10.).speed(0.1).suffix(" mm"));
                        ui.end_row();
                    }
                });
            ui.add_space(6.);
            ui.small(t(
                zh,
                "英文和数字的最小字号为 4 mm，其他文字为 5 mm。Gridfinity 可拆标牌的字高、刻深或颜色层厚上限为 0.4 mm。",
                "The minimum font size is 4 mm for ASCII and 5 mm for other text. Detachable Gridfinity labels cap relief, engraving, and color depth at 0.4 mm.",
            ));
            if self.project_label_draft.is_none() || draft != previous {
                ui.data_mut(|data| data.remove::<String>(error));
            }
            let apply = ui.add_enabled(
                current != Some(draft),
                ui_style::primary_button(t(zh, "应用到项目", "Apply to project")),
            ).clicked();
            if let Some(message) = ui.data(|data| data.get_temp::<String>(error)) {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            apply
        }).inner;
        self.project_label_draft =
            (!self.members.is_empty() && current != Some(draft)).then_some(draft);
        apply.then_some(draft)
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
        let (page, filename) = match self.smoke_stage {
            1 => (Page::Appearance, "appearance-light.png"),
            2 => (Page::Print, "print-dark-compact.png"),
            3 => (Page::Printer, "printer-a1-mini.png"),
            4 => (Page::Materials, "materials-dark-compact.png"),
            _ => (Page::Labels, "project-labels-dark-compact.png"),
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
        } else if self.smoke_stage == 2 {
            self.export_options.plate.printer = Printer::A1Mini;
            self.settings_page = Some(Page::Printer);
            self.smoke_stage = 3;
            self.smoke_frame = 0;
        } else if self.smoke_stage == 3 {
            self.settings_page = Some(Page::Materials);
            self.smoke_stage = 4;
            self.smoke_frame = 0;
        } else if self.smoke_stage == 4 {
            self.settings_page = Some(Page::Labels);
            self.smoke_stage = 5;
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
