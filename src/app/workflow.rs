//! Overall workflow, color inheritance and reversible member operations.
use super::*;
use crate::print_plate::{ColorTheme, Config};

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
    gap: f32,
}
impl Snapshot {
    fn same_content(&self, other: &Self) -> bool {
        // Selection changes alone are not edits.
        self.colors == other.colors
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
                    let mut items = if i == self.active_member {
                        self.bom.clone()
                    } else {
                        m.preset.bom.clone()
                    };
                    match self.catalog.exterior_components(
                        &m.preset.selection,
                        &m.exterior,
                        m.preset.assembly_kind == AssemblyKind::Sustainer,
                    ) {
                        Ok(extras) => items.extend(extras.into_iter().map(|c| c.bom)),
                        Err(e) => {
                            ui.colored_label(Color32::LIGHT_RED, e);
                        }
                    }
                    let count: u32 = items.iter().filter(|b| b.enabled).map(|b| b.qty).sum();
                    total += count;
                    ui.collapsing(
                        format!("{} · {} {}", m.name, count, t(z, "份", "copies")),
                        |ui| {
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
                        },
                    );
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
        self.members = state.members;
        self.global_colors = state.colors;
        self.combination_gap = state.gap;
        self.restore_member(state.active.min(self.members.len().saturating_sub(1)));
        self.combination_view = true;
        self.combination_move_source = None;
        self.combination_spec = None;
        self.global_colors_draft = None;
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
    pub(super) fn apply_global_theme(&mut self, theme: ColorTheme) -> Result<(), String> {
        theme.validate()?;
        if theme != self.global_colors {
            self.record_workflow();
            self.global_colors = theme;
            self.sync_global_colors();
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
            if let Some(c) = &mut m.preset.print_config
                && c.follow_global_colors
            {
                self.global_colors.apply(c);
            }
        }
        if let Some(c) = self
            .members
            .get(self.active_member)
            .and_then(|m| m.preset.print_config.as_ref())
        {
            self.export_options.plate = c.clone();
        }
    }
    pub(super) fn open_global_colors(&mut self) {
        self.commit_member();
        self.global_colors_draft = Some(self.global_colors.clone());
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
    pub(super) fn global_colors_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.global_colors_draft.take() else {
            return;
        };
        let z = self.zh;
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("overall-colors")).show(ctx,|ui| {
            ui.set_width(470.);
            ui.heading(t(z,"整体配色","Overall colors"));
            ui.label(t(z,"按用途跨规格同步；仅更新跟随整体的成员，新成员自动继承。","Shared by role across sizes. Updates following members; new members inherit."));
            egui::Grid::new("overall-palette").num_columns(2).show(ui,|ui| {
                for i in 0..6 { ui.label(format!("Bambu PETG Basic {}", i+1)); ui.color_edit_button_srgb(&mut draft.palette[i]); ui.end_row(); }
            });
            let following=self.members.iter().filter(|m|m.preset.print_config.as_ref().is_some_and(|c|c.follow_global_colors)).count();
            ui.label(format!("{} {} / {}",t(z,"跟随整体的成员","Following members"),following,self.members.len()));
            ui.separator();
            egui::Grid::new("overall-role-colors").num_columns(2).show(ui,|ui| {
                ui.label(t(z,"默认颜色","Default color"));
                slot_picker(ui,"global-default", &mut draft.filament,&draft.palette,false,z);
                ui.end_row();
                for (key,cn,en) in crate::catalog::colors::ROLES {
                    ui.label(t(z,cn,en));
                    let mut slot = draft.roles.get(key).copied().unwrap_or(0);
                    slot_picker(ui,key,&mut slot,&draft.palette,true,z);
                    if slot == 0 { draft.roles.remove(key); } else { draft.roles.insert(key.into(),slot); }
                    ui.end_row();
                }
            });
            ui.small(t(z,"成员的标签文字、内部布局、份数和打印参数保持各自配置。自定义成员可在属性中恢复跟随。","Labels, layouts, quantities and print settings remain independent. Custom members can resume following in Properties."));
            ui.horizontal(|ui| {
                apply = ui.add_enabled(self.export_rx.is_none(),egui::Button::new(t(z,"应用整体配色","Apply overall colors"))).clicked();
                cancel = ui.button(t(z,"取消","Cancel")).clicked();
            });
        });
        if cancel || response.should_close() {
            return;
        }
        if apply {
            if let Err(e) = self.apply_global_theme(draft) {
                self.status = e;
            }
            ctx.request_repaint();
        } else {
            self.global_colors_draft = Some(draft);
        }
    }
    pub(super) fn batch_member_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        ui.horizontal(|ui| {
            if ui.small_button(t(z, "全选", "Select all")).clicked() {
                self.member_selection = (0..self.members.len()).collect();
            }
            if ui
                .small_button(t(z, "清除选择", "Clear selection"))
                .clicked()
            {
                self.member_selection.clear();
            }
            ui.label(format!(
                "{} {}",
                self.member_selection.len(),
                t(z, "个已勾选", "selected")
            ));
        });
        if self.member_selection.is_empty() {
            return;
        }
        ui.add_enabled_ui(self.export_rx.is_none(), |ui| {
            if ui
                .button(t(
                    z,
                    "勾选成员跟随整体配色",
                    "Selected: follow overall colors",
                ))
                .clicked()
            {
                self.batch_colors(true);
            }
            if ui
                .button(t(
                    z,
                    "当前配色 → 勾选成员（一次性）",
                    "Copy current colors to selected (once)",
                ))
                .clicked()
            {
                self.batch_colors(false);
            }
            ui.small(t(
                z,
                "一次性复制保留用途配色；单独零件覆盖仅匹配相同源文件。",
                "One-time copy keeps role colors; per-part overrides match identical sources only.",
            ));
            if ui
                .add_enabled(
                    self.member_selection.len() < self.members.len(),
                    egui::Button::new(t(z, "删除勾选成员", "Delete selected members")),
                )
                .clicked()
            {
                let indices: Vec<_> = self.member_selection.iter().copied().collect();
                self.remove_members(&indices, ctx);
            }
        });
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
fn slot_picker(
    ui: &mut egui::Ui,
    id: &str,
    slot: &mut u32,
    palette: &[[u8; 3]; 6],
    default: bool,
    z: bool,
) {
    let label = |s: u32| {
        if s == 0 {
            if id == "text" {
                t(z, "跟随标签底片", "Match label body")
            } else {
                t(z, "跟随默认颜色", "Default color")
            }
            .to_owned()
        } else {
            let [r, g, b] = palette[(s.clamp(1, 6) - 1) as usize];
            format!("{s} · #{r:02X}{g:02X}{b:02X}")
        }
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*slot))
        .show_ui(ui, |ui| {
            for s in u32::from(!default)..=6 {
                ui.selectable_value(slot, s, label(s));
            }
        });
}
/// Copy by semantic role, retaining exact-source overrides when paths also match.
/// Print parameters, text contents and quantities are intentionally independent.
fn copy_member_colors(source: &Config, target: &mut Config) {
    target.palette = source.palette;
    target.filament = source.filament;
    target.role_filaments = source.role_filaments.clone();
    target.part_filaments = source.part_filaments.clone();
    target.text_filaments = source.text_filaments.clone();
    target.preview_colors = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        let preset = Preset {
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
        state.colors.roles.insert("label".into(), 3);
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
            colors: state.colors,
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
