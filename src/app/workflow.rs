//! Overall workflow, color inheritance and reversible member operations.
use super::*;
use crate::print_plate::{ColorTheme, Config, FilamentMaterial, MaterialLibrary};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ExportScope {
    #[default]
    All,
    Selected,
    Current,
}

#[derive(Clone, Serialize)]
pub(super) struct Snapshot {
    members: Vec<Member>,
    active: usize,
    colors: ColorTheme,
    label_style: Option<crate::label::StyleSettings>,
    gap: f32,
}
impl Snapshot {
    fn same_content(&self, other: &Self) -> bool {
        // Selection changes alone are not edits.
        self.colors == other.colors
            && self.label_style == other.label_style
            && self.gap == other.gap
            && serde_json::to_vec(&self.members).ok() == serde_json::to_vec(&other.members).ok()
    }
}
#[derive(Default)]
pub(super) struct History {
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
}
impl History {
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }
    fn push(&mut self, snapshot: Snapshot) {
        self.past.push(snapshot);
        if self.past.len() > 50 {
            self.past.remove(0);
        }
        self.future.clear();
    }
    fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.past.pop()?;
        self.future.push(current);
        Some(previous)
    }
    fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.future.pop()?;
        self.past.push(current);
        Some(next)
    }
}
impl App {
    pub(super) fn export_indices(&self) -> Vec<usize> {
        match self.export_scope {
            ExportScope::All => (0..self.members.len()).collect(),
            ExportScope::Selected => self
                .member_selection
                .iter()
                .copied()
                .filter(|&i| i < self.members.len())
                .collect(),
            ExportScope::Current => (!self.members.is_empty())
                .then_some(self.active_member)
                .into_iter()
                .collect(),
        }
    }
    pub(super) fn export_review_ui(&mut self, ui: &mut egui::Ui) -> u32 {
        let z = self.zh;
        ui.strong(t(z, "1 · 导出范围", "1 · Export scope"));
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.export_scope,
                ExportScope::All,
                t(z, "整个装配", "Entire assembly"),
            );
            ui.selectable_value(
                &mut self.export_scope,
                ExportScope::Selected,
                t(z, "勾选成员", "Checked members"),
            );
            ui.selectable_value(
                &mut self.export_scope,
                ExportScope::Current,
                t(z, "当前成员", "Current member"),
            );
        });
        if self.export_scope != ExportScope::Current {
            ui.small(t(
                z,
                "每个成员分别生成一个 3MF，输出到同一文件夹；不合并跨成员打印盘。",
                "One 3MF per member in one folder; plates are not merged across members.",
            ));
        }
        ui.strong(t(z, "2 · 打印清单", "2 · Print list"));
        let mut total = 0;
        let indices = self.export_indices();
        if indices.is_empty() {
            ui.label(t(
                z,
                "尚未选择成员，请返回整体页勾选。",
                "No members selected. Check members on the overall page.",
            ));
        }
        egui::ScrollArea::vertical()
            .id_salt("export-review")
            .max_height(210.)
            .show(ui, |ui| {
                for i in indices {
                    let m = &self.members[i];
                    let leaf = i == self.active_member
                        && self.active_drawer.is_some()
                        && self.export_scope == ExportScope::Current;
                    let mut items = if leaf {
                        self.current_print_member().preset.printing_bom()
                    } else if i == self.active_member && self.active_drawer.is_none() {
                        self.member_snapshot().printing_bom()
                    } else {
                        m.preset.printing_bom()
                    };
                    if !leaf {
                        match self.catalog.exterior_components(
                            &m.preset.selection,
                            &m.exterior,
                            m.preset.assembly_kind == AssemblyKind::Sustainer,
                        ) {
                            Ok(extras) => items.extend(extras.into_iter().map(|c| c.bom)),
                            Err(e) => {
                                ui.colored_label(ui.visuals().error_fg_color, e);
                            }
                        }
                    }
                    let count: u32 = items.iter().filter(|b| b.enabled).map(|b| b.qty).sum();
                    total += count;
                    let mut title = format!("{} · {} {}", m.name, count, t(z, "份", "copies"));
                    if self.export_options.format == crate::export::Format::ThreeMf {
                        let printers = if leaf {
                            vec![self.export_options.plate.printer.name()]
                        } else {
                            let mut names = vec![m.preset.printer().name()];
                            for drawer in &m.preset.drawers {
                                let name = drawer.printer().name();
                                if !names.contains(&name) {
                                    names.push(name);
                                }
                            }
                            names
                        };
                        title.push_str(" · ");
                        title.push_str(&printers.join(" / "));
                    }
                    ui.collapsing(title, |ui| {
                        for b in items.iter().filter(|b| b.enabled) {
                            ui.label(format!("{} × {}", b.name(), b.qty));
                        }
                        let omitted = items.iter().filter(|b| !b.enabled).count();
                        if omitted > 0 {
                            ui.label(format!(
                                "{} {}",
                                omitted,
                                t(z, "项未勾选，不会导出", "unchecked items will not export")
                            ));
                        }
                    });
                }
            });
        ui.label(format!(
            "{} {} {}",
            t(z, "合计", "Total"),
            total,
            t(
                z,
                "份；预览显隐不影响导出。",
                "copies; preview visibility does not affect export."
            )
        ));
        ui.separator();
        ui.strong(t(z, "3 · 格式与打印设置", "3 · Format and print settings"));
        total
    }

    pub(super) fn workflow_snapshot(&self) -> Snapshot {
        Snapshot {
            members: self.members.clone(),
            active: self.active_member,
            colors: self.global_colors.clone(),
            label_style: self.project_label_style,
            gap: self.combination_gap,
        }
    }
    pub(super) fn push_workflow_snapshot(&mut self, snapshot: Snapshot) {
        self.workflow_history.push(snapshot);
    }
    pub(super) fn record_workflow(&mut self) {
        self.commit_member();
        self.workflow_history.push(self.workflow_snapshot());
    }
    pub(super) fn finish_member_edit(&mut self) {
        self.commit_member();
        if let Some(before) = self.member_edit_baseline.take()
            && !before.same_content(&self.workflow_snapshot())
        {
            self.workflow_history.push(before);
        }
    }
    fn restore_workflow(&mut self, state: Snapshot, ctx: &egui::Context) {
        self.tower.clear_reorder();
        self.members = state.members;
        self.global_colors = state.colors;
        self.project_label_style = state.label_style;
        self.combination_gap = state.gap;
        self.restore_member(state.active.min(self.members.len().saturating_sub(1)));
        self.combination_view = true;
        self.combination_move_source = None;
        self.combination_spec = None;
        self.member_edit_baseline = None;
        self.member_selection.clear();
        self.assembly(ctx);
    }
    pub(super) fn undo_workflow(&mut self, ctx: &egui::Context) {
        self.commit_member();
        if let Some(previous) = self.workflow_history.undo(self.workflow_snapshot()) {
            self.restore_workflow(previous, ctx);
        }
    }
    pub(super) fn redo_workflow(&mut self, ctx: &egui::Context) {
        self.commit_member();
        if let Some(next) = self.workflow_history.redo(self.workflow_snapshot()) {
            self.restore_workflow(next, ctx);
        }
    }
    pub(super) fn apply_global_theme(&mut self, mut theme: ColorTheme) -> Result<(), String> {
        theme.validate()?;
        theme.trim_unused_slots();
        if theme != self.global_colors {
            self.record_workflow();
            self.global_colors = theme;
            self.sync_global_colors();
        }
        Ok(())
    }

    pub(super) fn apply_project_label_style(
        &mut self,
        style: crate::label::StyleSettings,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        style.validate()?;
        if self.members.is_empty() || self.export_rx.is_some() {
            return Err(t(
                self.zh,
                "当前项目暂不可修改标签样式",
                "Project label style is unavailable right now",
            )
            .into());
        }
        if self.project_label_style == Some(style) {
            return Ok(());
        }
        self.finish_member_edit();
        self.workflow_history.push(self.workflow_snapshot());
        self.project_label_style = Some(style);
        if self.inline_edit.open {
            self.member_edit_baseline = Some(self.workflow_snapshot());
        }
        if self.combination_view || self.scene.assembly {
            self.assembly(ctx);
        } else if let Some(part) = self.scene.parts.first()
            && crate::label::is_label(&part.pose.path)
        {
            self.single(part.pose.path.clone(), ctx);
        }
        Ok(())
    }
    pub(super) fn workflow_undo_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let ready = self.export_rx.is_none() && self.combination_spec.is_none();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready && !self.workflow_history.past.is_empty(),
                    egui::Button::new(t(self.zh, "撤销", "Undo")),
                )
                .clicked()
            {
                self.undo_workflow(ctx);
            }
            if ui
                .add_enabled(
                    ready && !self.workflow_history.future.is_empty(),
                    egui::Button::new(t(self.zh, "重做", "Redo")),
                )
                .clicked()
            {
                self.redo_workflow(ctx);
            }
        });
    }
    pub(super) fn sync_global_colors(&mut self) {
        for m in &mut self.members {
            for child in &mut m.preset.drawers {
                if let Some(c) = &mut child.print_config
                    && c.follow_global_colors
                {
                    self.global_colors.apply(c);
                }
            }
            if let Some(c) = &mut m.preset.print_config
                && c.follow_global_colors
            {
                self.global_colors.apply(c);
            }
        }
        if let Some(c) = self
            .members
            .get(self.active_member)
            .and_then(|m| match self.active_drawer {
                Some(drawer) => m.preset.drawers.get(drawer),
                None => Some(&m.preset),
            })
            .and_then(|p| p.print_config.as_ref())
        {
            self.export_options.plate = c.clone();
        }
    }
    pub(super) fn member_color_follow_ui(&mut self, ui: &mut egui::Ui) {
        let z = self.zh;
        let mut follow = self.export_options.plate.follow_global_colors;
        ui.label(if follow {
            t(z, "配色：跟随整体", "Colors: follow overall")
        } else {
            t(z, "配色：自定义", "Colors: custom")
        });
        if ui
            .add_enabled(
                self.export_rx.is_none(),
                egui::Checkbox::new(&mut follow, t(z, "跟随整体配色", "Follow overall colors")),
            )
            .changed()
        {
            if self.combination_view {
                self.record_workflow();
            }
            self.export_options.plate.follow_global_colors = follow;
            if follow {
                self.global_colors.apply(&mut self.export_options.plate);
            }
            self.commit_member();
        }
    }
    pub(super) fn batch_colors(&mut self, follow: bool) {
        if self.member_selection.is_empty() {
            return;
        }
        self.record_workflow();
        let source = self.export_options.plate.clone();
        for &i in &self.member_selection {
            let Some(m) = self.members.get_mut(i) else {
                continue;
            };
            let c = m.preset.print_config.get_or_insert_with(Config::default);
            if follow {
                self.global_colors.apply(c);
            } else {
                copy_member_colors(&source, c);
            }
            c.follow_global_colors = follow;
        }
        if let Some(c) = self.members[self.active_member]
            .preset
            .print_config
            .as_ref()
        {
            self.export_options.plate = c.clone();
        }
    }
}
/// Copy by semantic role, retaining exact-source overrides when paths also match.
/// Print parameters, text contents and quantities are intentionally independent.
fn copy_member_colors(source: &Config, target: &mut Config) {
    target.palette = source.palette;
    target.materials = source.materials;
    target.extra_filaments.clone_from(&source.extra_filaments);
    target.filament = source.filament;
    target.role_filaments = source.role_filaments.clone();
    target.component_filaments = source.component_filaments.clone();
    target.component_text_filaments = source.component_text_filaments.clone();
    target.part_filaments = source.part_filaments.clone();
    target.text_filaments = source.text_filaments.clone();
    target.preview_colors = true;
}

pub(super) fn color_component_name(path: &str, z: bool) -> String {
    match crate::print_plate::color_key(path) {
        crate::print_plate::BIN_COLOR_KEY => t(
            z,
            "Gridfinity 小盒（同类型）",
            "Gridfinity bins (shared type)",
        )
        .into(),
        crate::print_plate::LABEL_COLOR_KEY => t(
            z,
            "Cullenect 可拆标牌（同类型）",
            "Cullenect labels (shared type)",
        )
        .into(),
        _ => crate::accessories::info(path, z).map_or_else(
            || {
                path.rsplit('/')
                    .next()
                    .unwrap_or(path)
                    .trim_end_matches(".stl")
                    .to_owned()
            },
            |m| m.name,
        ),
    }
}
pub(super) fn material_picker(
    ui: &mut egui::Ui,
    i: usize,
    material: &mut FilamentMaterial,
    library: &MaterialLibrary,
    z: bool,
) {
    egui::ComboBox::from_id_salt(("filament-material", i))
        .selected_text(format!("{} · {}", i + 1, library.name(*material)))
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
            let mut imported = false;
            for option in choices {
                if !imported && library.preset(option).is_some() {
                    imported = true;
                    ui.separator();
                    ui.small(t(z, "导入的预设", "Imported presets"));
                }
                ui.selectable_value(material, option, library.name(option));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        let preset = Preset {
            drawers: vec![],
            selection: Default::default(),
            bom: vec![Bom {
                path: "test.stl".into(),
                qty: 3,
                enabled: true,
                group: "test".into(),
            }],
            label: crate::label::Config {
                text: "Tools".into(),
                ..Default::default()
            },
            print_config: Some(Config::default()),
            gridfinity: Default::default(),
            assembly_kind: AssemblyKind::Drawer,
            sustainer_options: Default::default(),
            sustainer_angle: 0.,
            drawer_extension: 0.,
            divider_layout: Default::default(),
            sustainer_dividers: Default::default(),
        };
        Snapshot {
            members: vec![
                Member {
                    name: "A".into(),
                    placement: Default::default(),
                    preset: preset.clone(),
                    exterior: Default::default(),
                },
                Member {
                    name: "B".into(),
                    placement: crate::scene::Placement {
                        column: 1,
                        ..Default::default()
                    },
                    preset,
                    exterior: Default::default(),
                },
            ],
            active: 1,
            colors: ColorTheme::default(),
            label_style: None,
            gap: 0.,
        }
    }
    #[test]
    fn undo_delete_restores_member_data_and_redo_branching() {
        let original = snapshot();
        let mut removed = original.clone();
        removed.members.pop();
        removed.active = 0;
        let mut history = History::default();
        history.push(original.clone());
        let restored = history.undo(removed.clone()).unwrap();
        assert_eq!(restored.active, 1);
        assert_eq!(restored.members[1].preset.label.text, "Tools");
        assert_eq!(restored.members[1].preset.bom[0].qty, 3);
        let next = history.redo(restored).unwrap();
        assert_eq!(next.members.len(), 1);
        history.undo(next).unwrap();
        history.push(original);
        assert!(history.future.is_empty());
    }
    #[test]
    fn theme_roundtrip_preserves_custom_members_and_independent_contents() {
        let mut state = snapshot();
        state.colors.roles.insert("accessory".into(), 3);
        state.colors.role_overrides.insert(
            crate::catalog::colors::component("part.stl"),
            "divider".into(),
        );
        state
            .colors
            .component_text
            .insert(crate::catalog::colors::component(crate::label::PATH), 2);
        state.members[0]
            .preset
            .print_config
            .as_mut()
            .unwrap()
            .follow_global_colors = true;
        state.members[1]
            .preset
            .print_config
            .as_mut()
            .unwrap()
            .set_part_slot(crate::label::PATH, 5);
        for m in &mut state.members {
            let c = m.preset.print_config.as_mut().unwrap();
            if c.follow_global_colors {
                state.colors.apply(c);
            }
        }
        let project = CombinationPreset {
            project_name: super::tower::default_name(),
            colors: state.colors,
            label_style: state.label_style,
            preset: state.members[0].preset.clone(),
            members: state.members,
            active_member: 1,
            combination_gap: 0.,
        };
        let loaded: CombinationPreset =
            toml::from_str(&toml::to_string(&project).unwrap()).unwrap();
        assert_eq!(
            loaded.members[0]
                .preset
                .print_config
                .as_ref()
                .unwrap()
                .slot(crate::label::PATH),
            3
        );
        assert_eq!(
            loaded.members[1]
                .preset
                .print_config
                .as_ref()
                .unwrap()
                .slot(crate::label::PATH),
            5
        );
        assert_eq!(loaded.members[1].preset.label.text, "Tools");
        assert_eq!(
            loaded.members[0]
                .preset
                .print_config
                .as_ref()
                .unwrap()
                .slot("part.stl"),
            4
        );
        assert_eq!(
            loaded.members[0]
                .preset
                .print_config
                .as_ref()
                .unwrap()
                .text_slot(crate::label::PATH),
            2
        );
        assert_eq!(
            loaded.members[1]
                .preset
                .print_config
                .as_ref()
                .unwrap()
                .text_slot(crate::label::PATH),
            5
        );
        let mut target = Config {
            layer: 0.12,
            gap: 9.,
            ..Default::default()
        };
        copy_member_colors(
            loaded.members[0].preset.print_config.as_ref().unwrap(),
            &mut target,
        );
        assert_eq!((target.layer, target.gap), (0.12, 9.));
        assert_eq!(target.slot(crate::label::PATH), 3);
    }
}
