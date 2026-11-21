use super::*;
use crate::label::StyleSettings;
use crate::ui_style;

pub(super) struct Editor {
    selected: usize,
    name: String,
    label_draft: Option<StyleSettings>,
    confirm_delete: bool,
    error: Option<String>,
}

impl Editor {
    pub(super) fn new(state: &State) -> Self {
        let mut editor = Self {
            selected: 0,
            name: String::new(),
            label_draft: None,
            confirm_delete: false,
            error: None,
        };
        editor.select(state, state.current);
        editor
    }

    fn select(&mut self, state: &State, index: usize) {
        self.selected = index.min(state.projects.len().saturating_sub(1));
        self.name = state
            .projects
            .get(self.selected)
            .map_or_else(String::new, |p| p.name.clone());
        self.label_draft = None;
        self.confirm_delete = false;
        self.error = None;
    }
}

#[derive(Clone, Copy)]
enum Action {
    Open,
    Rename,
    Label(StyleSettings),
    Save,
    Delete,
}

impl App {
    fn project_label_form(
        ui: &mut egui::Ui,
        zh: bool,
        current: Option<crate::label::StyleSettings>,
        stored_draft: &mut Option<crate::label::StyleSettings>,
        fallback: crate::label::StyleSettings,
        enabled: bool,
    ) -> Option<crate::label::StyleSettings> {
        use crate::label::{Font, Style};

        let mut draft = stored_draft.or(current).unwrap_or(fallback);
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
            "统一此项目的抽屉、箱子和 Gridfinity 标签样式。各标签保留自己的文字。",
            "Set the style for drawer, box, and Gridfinity labels in this project. Each label keeps its text.",
        ));
        if enabled && current.is_none() {
            ui.small(t(
                zh,
                "现有标签继续使用已保存的样式，直到应用统一设置。",
                "Existing labels use their saved styles until you apply the project settings.",
            ));
        }
        ui.add_space(6.);
        let apply = ui.add_enabled_ui(enabled, |ui| {
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
            ui.add_enabled(
                current != Some(draft),
                ui_style::primary_button(t(zh, "应用到项目", "Apply to project")),
            ).clicked()
        }).inner;
        *stored_draft = (enabled && current != Some(draft)).then_some(draft);
        apply.then_some(draft)
    }

    pub(in crate::app) fn project_settings_panel(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.tower.project_settings.take() else {
            return;
        };
        let zh = self.zh;
        let ready = self.tower.loaded
            && !self.tower.pending_restore
            && !self.busy
            && !self.preview.has_pending()
            && self.export_rx.is_none();
        let mut action = None;
        let mut done = false;
        let modal = egui::Modal::new(egui::Id::new("project-settings"))
            .frame(ui_style::dialog_frame(&ctx.style()))
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 48.).clamp(520., 800.));
                ui.heading(t(zh, "项目设置", "Project settings"));
                ui.separator();
                let height = (ctx.content_rect().height() - 150.).clamp(280., 510.);
                ui.horizontal_top(|ui| {
                    ui_style::inset_frame(ui).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(192.);
                            ui.set_height(height);
                            ui.strong(t(zh, "本地项目", "Local projects"));
                            egui::ScrollArea::vertical()
                                .max_height(height - 36.)
                                .show(ui, |ui| {
                                    for (index, project) in self.tower.projects.iter().enumerate() {
                                        let current = if index == self.tower.current
                                            && !self.members.is_empty()
                                        {
                                            t(zh, " · 当前", " · Current")
                                        } else {
                                            ""
                                        };
                                        let label = format!(
                                            "{}\n{}{}",
                                            project.name,
                                            project.summary(zh),
                                            current,
                                        );
                                        if ui
                                            .add_sized(
                                                [ui.available_width(), 48.],
                                                ui_style::choice_button(
                                                    editor.selected == index,
                                                    label,
                                                )
                                                .wrap(),
                                            )
                                            .on_hover_text(&project.name)
                                            .clicked()
                                        {
                                            editor.select(&self.tower, index);
                                        }
                                    }
                                });
                        });
                    });
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        let Some(project) = self.tower.projects.get(editor.selected) else {
                            ui.label(t(zh, "暂无本地项目。", "No local projects."));
                            return;
                        };
                        egui::ScrollArea::vertical()
                            .max_height(height)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    let already_open = editor.selected == self.tower.current
                                        && !self.members.is_empty();
                                    if ui
                                        .add_enabled(
                                            ready
                                                && !already_open
                                                && !self.catalog.root.as_os_str().is_empty(),
                                            egui::Button::new(t(zh, "打开项目", "Open project")),
                                        )
                                        .clicked()
                                    {
                                        action = Some(Action::Open);
                                    }
                                    if ui.button(t(zh, "保存为文件…", "Save to file…")).clicked()
                                    {
                                        action = Some(Action::Save);
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label(t(zh, "名称", "Name"));
                                    ui.add(
                                        egui::TextEdit::singleline(&mut editor.name)
                                            .char_limit(80)
                                            .desired_width(220.),
                                    );
                                    let name = editor.name.trim();
                                    if ui
                                        .add_enabled(
                                            ready && !name.is_empty() && name != project.name,
                                            egui::Button::new(t(zh, "保存名称", "Save name")),
                                        )
                                        .clicked()
                                    {
                                        action = Some(Action::Rename);
                                    }
                                });
                                ui.separator();
                                ui.strong(t(zh, "项目标签", "Project labels"));
                                let mut fallback =
                                    StyleSettings::from_config(&project.saved.preset.label);
                                if let Some(font) = self.label_font {
                                    fallback.font = font;
                                }
                                if let Some(style) = Self::project_label_form(
                                    ui,
                                    zh,
                                    project.saved.label_style,
                                    &mut editor.label_draft,
                                    fallback,
                                    ready,
                                ) {
                                    action = Some(Action::Label(style));
                                }
                            });
                    });
                });
                ui.separator();
                if let Some(error) = &editor.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            ready && editor.selected < self.tower.projects.len(),
                            ui_style::danger_button(t(zh, "删除项目…", "Delete project…")),
                        )
                        .clicked()
                    {
                        editor.confirm_delete = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        done = ui.button(t(zh, "完成", "Done")).clicked();
                    });
                });
            });
        if editor.confirm_delete {
            if let Some(project) = self.tower.projects.get(editor.selected) {
                let confirm = egui::Modal::new(egui::Id::new("delete-local-project"))
                    .frame(ui_style::dialog_frame(&ctx.style()))
                    .show(ctx, |ui| {
                        ui.set_width(360.);
                        ui.heading(t(zh, "删除项目", "Delete project"));
                        ui.label(format!(
                            "{}「{}」",
                            t(zh, "将删除本地项目", "Delete local project "),
                            project.name
                        ));
                        ui.label(t(
                            zh,
                            "原始模型与已另存的方案文件会保留。",
                            "Source models and saved copies will be kept.",
                        ));
                        ui.horizontal(|ui| {
                            if ui.button(t(zh, "取消", "Cancel")).clicked() {
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    ready,
                                    ui_style::danger_button(t(zh, "确认删除", "Delete")),
                                )
                                .clicked()
                            {
                                action = Some(Action::Delete);
                                ui.close();
                            }
                        });
                    });
                editor.confirm_delete = !confirm.should_close();
            } else {
                editor.confirm_delete = false;
            }
        } else if done || modal.should_close() {
            return;
        }

        if let Some(action) = action {
            let index = editor.selected;
            let current = index == self.tower.current && !self.members.is_empty();
            let result = match action {
                Action::Open => {
                    self.tower_capture();
                    self.tower_apply_project(self.tower.projects[index].clone(), ctx)
                        .map(|()| self.tower.current = index)
                }
                Action::Rename => {
                    let name = editor.name.trim().to_owned();
                    let project = &mut self.tower.projects[index];
                    project.name.clone_from(&name);
                    project.saved.project_name.clone_from(&name);
                    if current {
                        self.tower.name = name;
                    }
                    Ok(())
                }
                Action::Label(style) if current => self
                    .apply_project_label_style(style, ctx)
                    .map(|()| self.tower_capture()),
                Action::Label(style) => style
                    .validate()
                    .map(|()| self.tower.projects[index].saved.label_style = Some(style)),
                Action::Save => {
                    self.tower_capture();
                    self.save_project_file(&self.tower.projects[index].saved.clone());
                    Ok(())
                }
                Action::Delete => self.delete_local_project(index, ctx),
            };
            if let Err(error) = result {
                editor.error = Some(error);
            } else {
                editor.error = None;
                match action {
                    Action::Open => return,
                    Action::Rename => editor.name = self.tower.projects[index].name.clone(),
                    Action::Label(_) => editor.label_draft = None,
                    Action::Delete => editor.select(&self.tower, index),
                    Action::Save => {}
                }
            }
            ctx.request_repaint();
        }
        self.tower.project_settings = Some(editor);
    }

    pub(super) fn delete_local_project(
        &mut self,
        index: usize,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        self.tower_capture();
        let mut next = Projects {
            current: self.tower.current,
            items: self.tower.projects.clone(),
        };
        let removing_current = next.remove(index)?;
        let replacement = if removing_current {
            next.items
                .get(next.current)
                .cloned()
                .map(|mut project| {
                    project
                        .saved
                        .prepare_restore(&self.catalog, &self.export_options.plate)?;
                    Ok::<_, String>(project)
                })
                .transpose()?
        } else {
            None
        };
        let bytes = toml::to_string(&next)
            .map_err(|error| error.to_string())?
            .into_bytes();
        if let Some(write) = self.tower.write.take() {
            let _ = write.recv();
        }
        persist_projects(&self.home.join("state"), &bytes)?;
        self.tower.key = bytes.clone();
        self.tower.saved_key = bytes;
        self.tower.due = None;
        self.tower.current = next.current;
        self.tower.projects = next.items;
        self.tower.empty_saved = self.tower.projects.is_empty();
        if let Some(project) = replacement {
            self.tower_restore_project(project, ctx);
        } else if self.tower.empty_saved {
            self.clear_local_workspace();
        }
        Ok(())
    }

    fn clear_local_workspace(&mut self) {
        self.close_member_editors();
        self.preview.cancel();
        self.failed_preview = None;
        self.animation = Default::default();
        self.members.clear();
        self.bom.clear();
        self.active_member = 0;
        self.active_drawer = None;
        self.selected = None;
        self.scene = Scene::default();
        self.scene_key.clear();
        self.preview_modes = Default::default();
        self.combination_spec = None;
        self.combination_move_source = None;
        self.combination_view_state = None;
        self.combination_view = true;
        self.member_selection.clear();
        self.workflow_history.clear();
        self.member_edit_baseline = None;
        self.project_label_style = None;
        self.tower.name = default_name();
        self.tower.rename_draft = None;
        self.tower.color_edit = None;
        self.tower.open_color = None;
        self.tower.print_list = false;
        self.tab = 0;
    }
}

impl Projects {
    pub(super) fn remove(&mut self, index: usize) -> Result<bool, String> {
        if index >= self.items.len() {
            return Err("Project is unavailable".into());
        }
        self.items.remove(index);
        let removing_current = self.current == index;
        self.current = self
            .current
            .saturating_sub(usize::from(index < self.current))
            .min(self.items.len().saturating_sub(1));
        Ok(removing_current)
    }
}
