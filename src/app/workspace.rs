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

    pub(super) fn workspace_toolbar(&mut self, ctx: &egui::Context) {
        let z = self.zh;
        egui::TopBottomPanel::top("workspace-toolbar")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let tools_width = if z { 240. } else { 300. };
                    let project_width =
                        (ui.available_width() - tools_width - ui.spacing().item_spacing.x)
                            .max(240.);
                    ui.allocate_ui_with_layout(
                        egui::vec2(project_width, 34.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(RichText::new("VAULT").size(22.).strong());
                            ui.separator();
                            self.tower_project_header(ui, ctx);
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t(z, "设置", "Settings")).clicked() {
                            self.settings_page = Some(settings_panel::Page::Appearance);
                        }
                        ui.menu_button(t(z, "资料", "Resources"), |ui| {
                            if ui
                                .selectable_label(self.tab == 1, t(z, "零件库", "Parts library"))
                                .clicked()
                            {
                                self.tab = 1;
                                ui.close();
                            }
                            if ui
                                .selectable_label(self.tab == 2, t(z, "装配说明", "Manuals"))
                                .clicked()
                            {
                                self.tab = 2;
                                ui.close();
                            }
                        });
                        let label = if self.catalog.root.as_os_str().is_empty() {
                            t(z, "打开模型库…", "Open library…").to_owned()
                        } else {
                            format!(
                                "{} · {} STL",
                                t(z, "模型库", "Library"),
                                self.catalog.stl_count()
                            )
                        };
                        if ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new(RichText::new(label).small()).frame(false),
                            )
                            .on_hover_text(self.last_model_root.display().to_string())
                            .clicked()
                        {
                            self.choose_library(ctx);
                        }
                        if self.busy {
                            ui.spinner();
                        }
                    });
                });
            });
    }

    pub(super) fn workspace_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        let files = if self.combination_view {
            self.tower_file_count()
        } else {
            self.bom
                .iter()
                .filter(|item| {
                    item.enabled
                        && (self.active_drawer.is_none()
                            || !crate::catalog::tower::housing(&item.path))
                })
                .count()
        };
        if !self.combination_view {
            if ui
                .button(t(z, "← 返回整体配置", "← Back to project"))
                .clicked()
            {
                self.show_combination(ctx);
                self.tower.print_list = false;
            }
            let context = self.editor_context();
            ui.add(egui::Label::new(RichText::new(&context).strong()).truncate())
                .on_hover_text(&context);
            ui.add_space(4.);
        }
        ui.horizontal_wrapped(|ui| {
            for (tab, print, title) in [
                (
                    0,
                    false,
                    if self.combination_view {
                        t(z, "搭建", "Build")
                    } else {
                        t(z, "编辑内容", "Edit")
                    },
                ),
                (0, true, t(z, "打印清单", "Print list")),
                (1, false, t(z, "零件库", "Parts library")),
            ] {
                let selected = self.tab == tab && (tab != 0 || self.tower.print_list == print);
                let title = if print && files > 0 {
                    format!("{title}  {files}")
                } else {
                    title.into()
                };
                let text = RichText::new(title).strong().color(if selected {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().weak_text_color()
                });
                let response = ui.add(egui::Button::new(text).frame(false));
                if selected {
                    let y = response.rect.bottom();
                    ui.painter().line_segment(
                        [
                            egui::pos2(response.rect.left() + 5., y),
                            egui::pos2(response.rect.right() - 5., y),
                        ],
                        egui::Stroke::new(2., ui.visuals().text_color()),
                    );
                }
                if response.clicked() {
                    self.tab = tab;
                    if tab == 0 {
                        self.tower.print_list = print;
                    }
                }
            }
        });
        if self.tab == 0 && !self.tower.print_list && !self.combination_view {
            ui.horizontal(|ui| {
                ui.small(t(z, "内部收纳与标牌", "Inserts & labels"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !self.bom.is_empty(),
                            egui::Button::new(t(z, "配色…", "Colors…")).frame(false),
                        )
                        .clicked()
                    {
                        self.colors_open = true;
                    }
                });
            });
        } else if self.tab == 2 {
            ui.strong(t(z, "装配说明", "Manuals"));
        }
        ui.separator();
    }

    pub(super) fn editor_context(&self) -> String {
        let Some(member) = self.members.get(self.active_member) else {
            return self.scene.title.clone();
        };
        if let Some(drawer) = self.active_drawer {
            format!(
                "{} · {} {} ({})",
                member.name,
                t(self.zh, "抽屉", "Drawer"),
                drawer + 1,
                self.selection.height
            )
        } else {
            member.name.clone()
        }
    }

    pub(super) fn member_footer_panel(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::bottom("member-summary")
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin {
                        top: 8,
                        ..Default::default()
                    }),
            )
            .show_inside(ui, |ui| {
                let z = self.zh;
                ui.separator();
                let items: Vec<_> = self
                    .bom
                    .iter()
                    .filter(|b| {
                        b.enabled
                            && (self.active_drawer.is_none()
                                || !crate::catalog::tower::housing(&b.path))
                    })
                    .collect();
                let files = items.len();
                let copies: u32 = items.iter().map(|b| b.qty).sum();
                ui.small(format!(
                    "{} · {files} {} / {copies} {}",
                    t(z, "当前内容", "Current contents"),
                    t(z, "种 STL", "STL types"),
                    t(z, "份", "copies")
                ));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(if self.tower.print_list {
                            t(z, "查看打印板", "Preview plates")
                        } else {
                            t(z, "打印清单 →", "Print list →")
                        })
                        .clicked()
                    {
                        self.tab = 0;
                        if self.tower.print_list {
                            self.preview_modes.mode = preview_modes::Mode::Plates;
                        } else {
                            self.tower.print_list = true;
                        }
                    }
                    if ui
                        .add_enabled(
                            files > 0,
                            egui::Button::new(
                                RichText::new(t(z, "导出当前模块…", "Export module…"))
                                    .color(Color32::WHITE),
                            )
                            .fill(appearance::BRAND),
                        )
                        .clicked()
                    {
                        self.commit_member();
                        self.export_scope = workflow::ExportScope::Current;
                        self.export_open = true;
                    }
                });
                if self.active_drawer.is_some() {
                    ui.small(t(
                        z,
                        "模块导出包含共用外壳及同模块的其他抽屉。",
                        "Module export includes the shared housing and its other drawers.",
                    ));
                }
                if self.busy || self.preview.has_pending() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.small(t(z, "预览更新中…", "Updating preview…"));
                    });
                } else if !self.status.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&self.status).small()).truncate())
                        .on_hover_text(&self.status);
                }
            });
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
                                match self.preview_modes.mode {
                                    preview_modes::Mode::Plates => {
                                        t(z, "打印板预览", "Print plate preview")
                                    }
                                    preview_modes::Mode::Layers => {
                                        t(z, "分层预览", "Layer preview")
                                    }
                                    _ => t(z, "三维装配预览", "3D assembly preview"),
                                }
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
        let tools = egui::Area::new(egui::Id::new("stage-view-tools"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(
                rect.right_top() + egui::vec2(-16., if rect.width() < 580. { 94. } else { 16. }),
            )
            .show(ctx, |ui| {
                appearance::floating_frame(ui).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.menu_button(t(z, "视图", "View"), |ui| {
                            if ui.button(t(z, "立体", "Orbit")).clicked() {
                                let view = Camera::default();
                                self.preview_set_view(view.yaw, view.pitch);
                                ui.close();
                            }
                            for face in orientation::Face::ALL {
                                if ui.button(face.label(z)).clicked() {
                                    let (yaw, pitch) = face.view();
                                    self.preview_set_view(yaw, pitch);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui
                                .button(t(z, "3D 视图设置…", "3D view settings…"))
                                .clicked()
                            {
                                self.settings_page = Some(settings_panel::Page::View);
                                ui.close();
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
                            self.preview_mode_fit();
                        }
                        if self.preview_modes.mode != preview_modes::Mode::Plates {
                            ui.toggle_value(&mut self.tower.show_parts, t(z, "零件", "Parts"))
                                .on_hover_text(t(
                                    z,
                                    "打开 / 关闭零件与属性浮窗",
                                    "Show / hide the part tree and properties",
                                ));
                        }
                    });
                });
            });
        if !self.dimensions && !self.scene.parts.is_empty() {
            egui::Area::new(egui::Id::new("stage-orientation"))
                .order(egui::Order::Middle)
                .pivot(egui::Align2::RIGHT_TOP)
                .fixed_pos(egui::pos2(
                    rect.right() - 16.,
                    tools.response.rect.bottom() + 6.,
                ))
                .show(ctx, |ui| {
                    ui.horizontal_top(|ui| {
                        if let Some(face) = orientation::cube(ui, self.preview_camera(), z) {
                            let (yaw, pitch) = face.view();
                            self.preview_set_view(yaw, pitch);
                        }
                        if orientation::home(ui, z) {
                            self.preview_reset_view();
                        }
                    });
                });
        }
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
