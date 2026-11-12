use super::*;

impl App {
    pub(super) fn choose_library(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = crate::paths::directory_hint(&self.last_model_root) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(root) = dialog.pick_folder() {
            self.scan(root, ctx);
        }
    }

    pub(super) fn workspace_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        let mut theme = self.appearance;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(24., 26.), egui::Sense::hover());
            for i in 0..3 {
                let r = egui::Rect::from_min_size(
                    rect.min + egui::vec2(1., i as f32 * 8.),
                    egui::vec2(22., 6.),
                );
                ui.painter().rect_filled(
                    r,
                    2.,
                    if i == 2 {
                        appearance::BRAND
                    } else {
                        ui.visuals().text_color()
                    },
                );
                ui.painter().line_segment(
                    [
                        r.center() - egui::vec2(3., 0.),
                        r.center() + egui::vec2(3., 0.),
                    ],
                    egui::Stroke::new(1., ui.visuals().panel_fill),
                );
            }
            ui.label(RichText::new("VAULT").size(22.).strong());
            ui.label(
                RichText::new(t(z, "配置器", "Configurator"))
                    .size(16.)
                    .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(t(z, "设置", "Settings"), |ui| {
                    ui.strong(t(z, "外观", "Appearance"));
                    ui.selectable_value(
                        &mut theme,
                        appearance::Appearance::Light,
                        t(z, "浅色", "Light"),
                    );
                    ui.selectable_value(
                        &mut theme,
                        appearance::Appearance::Dark,
                        t(z, "深色", "Dark"),
                    );
                    ui.separator();
                    if ui.button(t(z, "更换模型库…", "Change library…")).clicked() {
                        self.choose_library(ctx);
                        ui.close();
                    }
                    ui.separator();
                    ui.small(crate::app_name(z));
                    ui.small(t(
                        z,
                        "非官方辅助工具 · 本地模型只读",
                        "Unofficial companion · read-only models",
                    ));
                });
                if ui
                    .button(if z { "EN" } else { "中文" })
                    .on_hover_text(t(z, "切换语言", "Switch language"))
                    .clicked()
                {
                    self.zh = !z;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                        crate::app_name(self.zh).into(),
                    ));
                    if let Err(e) = self.save_settings() {
                        self.status = e;
                    }
                }
            });
        });
        if theme != self.appearance {
            self.appearance = theme;
            theme.apply(ctx);
            if let Err(e) = self.save_settings() {
                self.status = e;
            }
        }
        ui.add_space(6.);
        let linked = !self.catalog.root.as_os_str().is_empty();
        ui.horizontal(|ui| {
            let label = if linked {
                format!(
                    "{} · {} STL",
                    t(z, "本地文件包", "Local library"),
                    self.catalog.stl_count()
                )
            } else {
                t(z, "关联 VAULT 模型库…", "Link VAULT model library…").into()
            };
            if ui
                .add_enabled(!self.busy, egui::Button::new(label))
                .on_hover_text(self.last_model_root.display().to_string())
                .clicked()
            {
                self.choose_library(ctx);
            }
            if self.busy {
                ui.spinner();
            } else if linked {
                ui.colored_label(appearance::BRAND, t(z, "已关联", "Linked"));
            }
        });
        if !self.combination_view {
            ui.add_space(4.);
            if ui
                .button(t(z, "← 返回整体配置", "← Back to project"))
                .clicked()
            {
                self.show_combination(ctx);
            }
            if let Some(member) = self.members.get(self.active_member) {
                ui.label(RichText::new(&member.name).strong());
            }
        }
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, 0, t(z, "配置", "Configure"));
            if ui
                .add_enabled(
                    !self.bom.is_empty(),
                    egui::Button::new(t(z, "配色", "Colors")).frame(false),
                )
                .clicked()
            {
                if self.combination_view {
                    self.open_global_colors();
                } else {
                    self.colors_open = true;
                }
            }
            ui.selectable_value(&mut self.tab, 1, t(z, "零件库", "Library"));
            ui.selectable_value(&mut self.tab, 2, t(z, "说明", "Manuals"));
        });
        ui.separator();
        ui.add_space(4.);
    }

    pub(super) fn stage_controls(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let z = self.zh;
        egui::Area::new(egui::Id::new("stage-project-hud"))
            .order(egui::Order::Middle)
            .fixed_pos(rect.left_top() + egui::vec2(16., 16.))
            .show(ctx, |ui| {
                appearance::floating_frame(ui)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_max_width((rect.width() - 280.).clamp(130., 280.));
                        let name = if self.combination_view {
                            &self.tower.name
                        } else if self.scene.title.is_empty() {
                            crate::app_name(z)
                        } else {
                            &self.scene.title
                        };
                        ui.add(egui::Label::new(RichText::new(name).strong()).truncate());
                        ui.horizontal(|ui| {
                            if self.busy || self.preview.has_pending() {
                                ui.spinner();
                            }
                            ui.small(if self.busy || self.preview.has_pending() {
                                t(z, "正在更新预览…", "Updating preview…")
                            } else {
                                t(z, "三维装配预览", "3D assembly preview")
                            });
                        });
                        if !self.busy
                            && !self.preview.has_pending()
                            && self.failed_preview.is_some()
                        {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                t(z, "预览更新失败", "Preview update failed"),
                            )
                            .on_hover_text(&self.status);
                            if ui.small_button(t(z, "重试", "Retry")).clicked() {
                                self.assembly(ctx);
                            }
                        }
                        if !self.scene.assembly
                            && !self.bom.is_empty()
                            && ui
                                .small_button(t(z, "← 装配预览", "← Assembly view"))
                                .clicked()
                        {
                            self.assembly(ctx);
                        }
                    });
            });
        egui::Area::new(egui::Id::new("stage-view-tools"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(
                rect.right_top() + egui::vec2(-16., if rect.width() < 580. { 94. } else { 16. }),
            )
            .show(ctx, |ui| {
                appearance::floating_frame(ui).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.menu_button(t(z, "视图", "View"), |ui| {
                            for (label, yaw, pitch) in [
                                (t(z, "立体", "Orbit"), 0.65, 0.5),
                                (t(z, "正视", "Front"), 0., 0.),
                                (
                                    t(z, "俯视", "Top"),
                                    0.,
                                    std::f32::consts::FRAC_PI_2 - 0.0001,
                                ),
                                (
                                    t(z, "右视", "Right"),
                                    std::f32::consts::FRAC_PI_2 - 0.0001,
                                    0.,
                                ),
                            ] {
                                if ui.button(label).clicked() {
                                    self.preview_set_view(yaw, pitch);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            ui.add_enabled_ui(!self.preview_modes.special(), |ui| {
                                ui.checkbox(
                                    &mut self.dimensions,
                                    t(z, "尺寸三视图", "Dimension views"),
                                );
                            });
                            ui.checkbox(&mut self.ghost, t(z, "外壳透明", "Ghost shell"));
                            if ui
                                .checkbox(
                                    &mut self.export_options.plate.preview_colors,
                                    t(z, "打印颜色预览", "Print colors"),
                                )
                                .changed()
                                && let Err(e) = self.save_settings()
                            {
                                self.status = e;
                            }
                            if self.combination_view {
                                ui.checkbox(
                                    &mut self.tower.show_parts,
                                    t(z, "零件属性", "Part properties"),
                                );
                            }
                            if self.debug_ui
                                && ui
                                    .button(t(z, "装配关系 / 检查", "Relations / Check"))
                                    .clicked()
                            {
                                self.joints_open = true;
                                ui.close();
                            }
                            if self.debug_ui
                                && self.scene.assembly
                                && ui.button(t(z, "重排爆炸方向", "Reset explosion")).clicked()
                            {
                                match crate::explosion::reset(&mut self.scene) {
                                    Ok(()) => {
                                        self.explosion = 1.;
                                        self.camera.fit(&self.scene, 1.);
                                    }
                                    Err(error) => self.status = error,
                                }
                                ui.close();
                            }
                        });
                        if ui.button(t(z, "适配画面", "Fit view")).clicked() {
                            if self.preview_modes.special() {
                                self.preview_mode_fit();
                            } else {
                                self.camera.fit(
                                    &if self.interior_editor_open() {
                                        self.interior_scene()
                                    } else {
                                        self.scene.clone()
                                    },
                                    self.explosion,
                                );
                            }
                        }
                    });
                });
            });
    }

    pub(super) fn workspace_empty(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        let bounds = ui.max_rect();
        let card = egui::Rect::from_center_size(bounds.center(), egui::vec2(420., 240.));
        ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(22.);
                ui.label(
                    RichText::new(t(
                        z,
                        "从一个抽屉，搭出你的收纳塔",
                        "Build your storage, one layer at a time",
                    ))
                    .size(22.)
                    .strong(),
                );
                ui.add_space(8.);
                ui.label(
                    RichText::new(t(
                        z,
                        "关联本地 VAULT 模型库，即可配置、预览和导出。",
                        "Link your local VAULT library to configure, preview and export.",
                    ))
                    .weak(),
                );
                ui.add_space(16.);
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new(
                            RichText::new(t(z, "打开模型库", "Open model library"))
                                .color(Color32::WHITE),
                        )
                        .fill(appearance::BRAND)
                        .min_size(egui::vec2(160., 40.)),
                    )
                    .clicked()
                {
                    self.choose_library(ctx);
                }
                ui.add_space(12.);
                ui.small(t(
                    z,
                    "模型保留在原目录，只读使用。",
                    "Your source models stay in their original folder, read-only.",
                ));
                if self.busy {
                    ui.spinner();
                }
                if !self.status.is_empty() {
                    ui.add_space(8.);
                    ui.small(&self.status);
                }
            });
        });
    }
}
