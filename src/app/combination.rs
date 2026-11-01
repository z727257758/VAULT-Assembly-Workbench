use super::*;
use crate::scene::Placement;

pub(super) struct SpecEdit {
    target: Option<usize>,
    anchor: usize,
    preset: Preset,
    above: bool,
    pattern: u8,
    clear_inserts: bool,
    error: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Member {
    pub name: String,
    pub placement: Placement,
    pub preset: Preset,
}
#[derive(Serialize, Deserialize)]
pub(super) struct CombinationPreset {
    #[serde(flatten)]
    pub preset: Preset,
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub active_member: usize,
    #[serde(default)]
    pub combination_gap: f32,
}
fn modules(members: &[Member]) -> Vec<crate::constraints::stacking::Module<'_>> {
    members
        .iter()
        .map(|m| crate::constraints::stacking::Module {
            name: &m.name,
            selection: &m.preset.selection,
            placement: m.placement,
            is_box: m.preset.assembly_kind == AssemblyKind::Sustainer,
        })
        .collect()
}
fn validate_layout(members: &[Member], gap: f32) -> Result<(), String> {
    crate::constraints::stacking::plan(&modules(members), gap).map(|_| ())
}
impl Preset {
    fn validate_member(&self, catalog: &Catalog) -> Result<(), String> {
        if !["S", "M", "L", "XL"].contains(&self.selection.width.as_str())
            || !(3..=7).contains(&self.selection.depth)
            || !["1F", "2F", "4F"].contains(&self.selection.height.as_str())
            || self.selection.width == "XL" && self.selection.height != "1F"
            || !self.sustainer_angle.is_finite()
            || !(0. ..=110.).contains(&self.sustainer_angle)
            || self.bom.iter().any(|b| {
                b.qty == 0
                    || b.qty > 100
                    || (!b.path.starts_with(crate::gridfinity::PREFIX)
                        && catalog.resolve(&b.path).is_none())
            })
        {
            return Err(
                "成员规格、文件或份数无效 / Invalid member specification, source or quantity"
                    .into(),
            );
        }
        if self.assembly_kind == AssemblyKind::Sustainer
            && (!self.gridfinity.bins.is_empty()
                || self.divider_layout.enabled
                || !self.divider_layout.items.is_empty())
            || self.assembly_kind == AssemblyKind::Drawer
                && !self.sustainer_dividers.items.is_empty()
        {
            return Err("成员内部配置不兼容 / Incompatible member inserts".into());
        }
        self.label.validate()?;
        self.gridfinity.validate(&self.selection)?;
        self.divider_layout.validate(catalog, &self.selection)?;
        self.sustainer_dividers
            .validate(catalog, &self.selection, self.sustainer_options)?;
        if let Some(config) = &self.print_config {
            config.validate()?;
        }
        Ok(())
    }
}
impl CombinationPreset {
    pub fn validate(&self, catalog: &Catalog) -> Result<(), String> {
        if self.members.len() > 64
            || !self.combination_gap.is_finite()
            || !(0. ..=100.).contains(&self.combination_gap)
            || (!self.members.is_empty() && self.active_member >= self.members.len())
        {
            return Err("组合数量、间距或当前成员无效 / Invalid combination".into());
        }
        self.preset.validate_member(catalog)?;
        let mut slots = std::collections::HashSet::new();
        for member in &self.members {
            member.placement.validate()?;
            if member.name.trim().is_empty()
                || member.name.chars().count() > 80
                || !slots.insert((member.placement.column, member.placement.level))
            {
                return Err(
                    "成员名称为空、过长或列层重复 / Invalid member name or duplicate position"
                        .into(),
                );
            }
            member
                .preset
                .validate_member(catalog)
                .map_err(|e| format!("{}: {e}", member.name))?;
        }
        validate_layout(&self.members, self.combination_gap)
    }
}

impl App {
    pub(super) fn member_snapshot(&self) -> Preset {
        Preset {
            sustainer_dividers: if self.assembly_kind == AssemblyKind::Sustainer {
                self.sustainer_dividers.clone()
            } else {
                Default::default()
            },
            print_config: Some(self.export_options.plate.clone()),
            gridfinity: if self.assembly_kind == AssemblyKind::Drawer {
                self.gridfinity.clone()
            } else {
                Default::default()
            },
            label: self.label.clone(),
            assembly_kind: self.assembly_kind,
            sustainer_options: self.sustainer_options.normalized(&self.selection),
            sustainer_angle: self.sustainer_angle,
            selection: self.selection.clone(),
            bom: self.bom.clone(),
            divider_layout: if self.assembly_kind == AssemblyKind::Drawer {
                self.divider_layout.clone()
            } else {
                Default::default()
            },
        }
    }
    pub(super) fn commit_member(&mut self) {
        if !self.members.is_empty() {
            self.members[self.active_member].preset = self.member_snapshot();
        }
    }
    fn close_member_editors(&mut self) {
        self.gridfinity_open = false;
        self.label_open = false;
        self.colors_open = false;
        self.gridfinity_editor = Default::default();
        self.gridfinity_selected = None;
        self.divider_editor = Default::default();
        self.sustainer_divider_editor = Default::default();
        self.divider_view = None;
        self.assembly_view = None;
        self.selected = None;
        self.joints_open = false;
        self.calibration = false;
    }
    pub(super) fn restore_member(&mut self, index: usize) {
        let p = self.members[index].preset.clone();
        self.active_member = index;
        self.combination_view = false;
        self.close_member_editors();
        self.selection = p.selection;
        self.bom = p.bom;
        self.assembly_kind = p.assembly_kind;
        self.sustainer_options = p.sustainer_options;
        self.sustainer_angle = p.sustainer_angle;
        self.gridfinity = p.gridfinity;
        self.label = p.label;
        self.divider_layout = p.divider_layout;
        self.sustainer_dividers = p.sustainer_dividers;
        self.export_options.plate = p.print_config.unwrap_or_default();
        self.drawer_state = None;
        self.box_state = None;
        self.other_label = Default::default();
        self.layout_cache.clear();
        self.sustainer_layout_cache.clear();
        self.scene_key.clear();
        self.explosion = 0.;
    }
    fn ensure_members(&mut self) {
        if self.members.is_empty() {
            self.members.push(Member {
                name: "成员 1".into(),
                placement: Default::default(),
                preset: self.member_snapshot(),
            });
            self.active_member = 0;
        }
    }
    pub(super) fn show_combination(&mut self, ctx: &egui::Context) {
        self.remember_combination_view();
        self.tab = 0;
        self.commit_member();
        self.ensure_members();
        if let Err(e) = (CombinationPreset {
            preset: self.member_snapshot(),
            members: self.members.clone(),
            active_member: self.active_member,
            combination_gap: self.combination_gap,
        })
        .validate(&self.catalog)
        {
            self.status = e;
            return;
        }
        self.close_member_editors();
        self.combination_view = true;
        self.explosion = 0.;
        self.assembly(ctx);
    }
    /// Switch the editor context while keeping the combined scene and camera in place.
    pub(super) fn select_member_in_combination(&mut self, index: usize, ctx: &egui::Context) {
        if index >= self.members.len() || index == self.active_member {
            return;
        }
        self.commit_member();
        let key = self.scene_key.clone();
        let explosion = self.explosion;
        self.restore_member(index);
        self.combination_view = true;
        self.scene_key = key;
        self.explosion = explosion;
        ctx.request_repaint();
    }
    pub(super) fn select_combination_part(&mut self, index: usize, ctx: &egui::Context) {
        let Some(part) = self.scene.parts.get(index) else {
            return;
        };
        let member = (part.pose.instance / MEMBER_STRIDE) as usize;
        if member >= self.members.len() || self.busy || self.preview.has_pending() {
            return;
        }
        self.select_member_in_combination(member, ctx);
        self.selected = Some(index);
        self.tab = 0;
    }
    fn remember_combination_view(&mut self) {
        if self.scene_key == "combination" {
            self.combination_view_state = Some((
                self.camera.clone(),
                self.explosion,
                self.dimensions,
                self.ghost,
            ));
        }
    }
    pub(super) fn focus_member(&mut self, ctx: &egui::Context) {
        if self.members.is_empty() {
            return;
        }
        self.remember_combination_view();
        self.tab = 0;
        self.commit_member();
        self.restore_member(self.active_member);
        self.assembly(ctx);
    }
    fn refresh_layout(&mut self, ctx: &egui::Context) {
        if !self.combination_view {
            self.combination_view = true;
            self.close_member_editors();
        }
        self.assembly(ctx);
    }
    fn relocate(&mut self, index: usize, column: u32, level: u32, ctx: &egui::Context) {
        let before: Vec<_> = self.members.iter().map(|m| m.placement).collect();
        let mut candidate = self.members.clone();
        let result = relocate_member(&mut candidate, index, column, level).and_then(|changed| {
            validate_layout(&candidate, self.combination_gap)?;
            Ok(changed)
        });
        match result {
            Ok(true) => {
                self.members = candidate;
                self.combination_layout_history.push(before);
                if self.combination_layout_history.len() > 20 {
                    self.combination_layout_history.remove(0);
                }
                if index != self.active_member {
                    self.select_member_in_combination(index, ctx);
                }
                self.refresh_layout(ctx);
            }
            Ok(false) => {}
            Err(e) => self.status = e,
        }
    }
    fn undo_layout(&mut self, ctx: &egui::Context) {
        if let Some(previous) = self.combination_layout_history.pop() {
            for (member, placement) in self.members.iter_mut().zip(previous) {
                member.placement = placement;
            }
            self.refresh_layout(ctx);
        }
    }
    fn remove_member(&mut self, ctx: &egui::Context) {
        self.remove_members(&[self.active_member], ctx);
    }
    fn remove_members(&mut self, indices: &[usize], ctx: &egui::Context) {
        if self.members.len() <= indices.len() {
            return;
        }
        self.commit_member();
        let candidate: Vec<_> = self
            .members
            .iter()
            .enumerate()
            .filter(|(i, _)| !indices.contains(i))
            .map(|(_, m)| m.clone())
            .collect();
        if let Err(e) = validate_layout(&candidate, self.combination_gap) {
            self.status = e;
            return;
        }
        self.remember_combination_view();
        let selected = self
            .active_member
            .saturating_sub(indices.iter().filter(|&&i| i < self.active_member).count())
            .min(candidate.len() - 1);
        self.members = candidate;
        self.restore_member(selected);
        self.combination_view = true;
        self.combination_layout_history.clear();
        self.combination_drag = None;
        self.assembly(ctx);
    }
    fn detach_pair(&mut self, indices: &[usize], ctx: &egui::Context) {
        self.commit_member();
        match detached_pair(&self.members, indices) {
            Ok(candidate) => {
                self.combination_layout_history
                    .push(self.members.iter().map(|m| m.placement).collect());
                if self.combination_layout_history.len() > 20 {
                    self.combination_layout_history.remove(0);
                }
                self.members = candidate;
                self.refresh_layout(ctx);
            }
            Err(e) => self.status = e,
        }
    }
    fn add_member(&mut self, kind: Option<AssemblyKind>, above: bool, _ctx: &egui::Context) {
        self.commit_member();
        self.ensure_members();
        let mut preset = self.members[self.active_member].preset.clone();
        if let Some(kind) = kind {
            preset.assembly_kind = kind;
            preset.gridfinity = Default::default();
            preset.divider_layout = Default::default();
            preset.sustainer_dividers = Default::default();
            preset.label = Default::default();
            preset.sustainer_angle = 0.;
            preset.sustainer_options = Default::default();
            preset.bom.clear();
        }
        self.combination_spec = Some(SpecEdit {
            target: None,
            anchor: self.active_member,
            preset,
            above,
            pattern: 0,
            clear_inserts: false,
            error: String::new(),
        });
    }
    pub(super) fn edit_member_size(&mut self) {
        self.commit_member();
        self.ensure_members();
        self.combination_spec = Some(SpecEdit {
            target: Some(self.active_member),
            anchor: self.active_member,
            preset: self.members[self.active_member].preset.clone(),
            above: false,
            pattern: 0,
            clear_inserts: false,
            error: String::new(),
        });
    }
    fn apply_member_spec(&mut self, draft: &SpecEdit, ctx: &egui::Context) -> Result<(), String> {
        self.commit_member();
        let mut next = self.members.clone();
        let anchor = next
            .get(draft.anchor)
            .ok_or("成员已变更，请重新打开规格窗口 / Member changed")?
            .clone();
        let mut p = draft.preset.clone();
        if draft.clear_inserts {
            p.gridfinity = Default::default();
            p.divider_layout = Default::default();
            p.sustainer_dividers = Default::default();
            p.bom.clear();
        }
        let configure = |p: &mut Preset| -> Result<(), String> {
            p.sustainer_options = p.sustainer_options.normalized(&p.selection);
            p.bom = if p.assembly_kind == AssemblyKind::Sustainer {
                self.catalog
                    .reconfigure_sustainer(&p.selection, p.sustainer_options, &p.bom)?
            } else {
                self.catalog.reconfigure(&p.selection, &p.bom)
            };
            p.validate_member(&self.catalog).map_err(|e| {
                format!(
                    "{e}；可取消修改，或勾选重置内部配置 / Cancel or reset incompatible inserts"
                )
            })
        };
        let active;
        if let Some(index) = draft.target {
            configure(&mut p)?;
            next.get_mut(index)
                .ok_or("成员已移除 / Member removed")?
                .preset = p;
            active = index;
        } else {
            if draft.pattern != 0 {
                p.selection.width = "S".into();
            }
            configure(&mut p)?;
            let column = if draft.above {
                anchor.placement.column
            } else {
                next.iter().map(|m| m.placement.column).max().unwrap_or(0) + 1
            };
            let level = if draft.above {
                anchor.placement.level + 1
            } else {
                0
            };
            active = next.len();
            let mut additions = vec![p.clone()];
            if draft.pattern != 0 {
                p.selection.width = if draft.pattern == 1 { "S" } else { "M" }.into();
                configure(&mut p)?;
                additions.push(p);
            }
            for (i, preset) in additions.into_iter().enumerate() {
                next.push(Member {
                    name: format!(
                        "{} {}",
                        if preset.assembly_kind == AssemblyKind::Sustainer {
                            t(self.zh, "盒子", "Box")
                        } else {
                            t(self.zh, "抽屉", "Drawer")
                        },
                        next.len() + 1
                    ),
                    placement: Placement {
                        column: column + i as u32,
                        level,
                        offset: [0.; 3],
                    },
                    preset,
                });
            }
            // A wide member can occupy multiple columns. Find the next valid ground slot.
            if !draft.above {
                for shift in 0..64 - column.min(63) {
                    for (i, m) in next[active..].iter_mut().enumerate() {
                        m.placement.column = column + shift + i as u32;
                    }
                    if validate_layout(&next, 0.).is_ok() {
                        break;
                    }
                }
            }
        }
        validate_layout(&next, self.combination_gap)?;
        if next.len() > 64 {
            return Err("最多 64 个成员 / Up to 64 members".into());
        }
        self.remember_combination_view();
        self.members = next;
        self.restore_member(active);
        self.combination_view = true;
        self.combination_layout_history.clear();
        self.combination_drag = None;
        self.combination_view_state = None;
        self.assembly(ctx);
        Ok(())
    }
    pub(super) fn combination_spec_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.combination_spec.take() else {
            return;
        };
        let z = self.zh;
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("combination-spec")).show(ctx, |ui| {
            ui.set_max_width(440.);
            ui.heading(if draft.target.is_some() {
                t(z, "修改成员尺寸", "Change member size")
            } else {
                t(z, "添加模块：确定规格", "Add module: choose size")
            });
            ui.separator();
            ui.label(draft.preset.assembly_kind.label(z));
            if draft.target.is_none() {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut draft.above, false, t(z, "并排添加", "Add beside"));
                    ui.radio_value(
                        &mut draft.above,
                        true,
                        t(z, "放到所选模块上方", "Above selected module"),
                    );
                });
                egui::ComboBox::from_id_salt("add-pattern")
                    .selected_text(match draft.pattern {
                        1 => "S＋S",
                        2 => "S＋M",
                        _ => t(z, "单个模块", "One module"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.pattern, 0, t(z, "单个模块", "One module"));
                        ui.selectable_value(&mut draft.pattern, 1, "S＋S");
                        ui.selectable_value(&mut draft.pattern, 2, "S＋M");
                    });
            }
            if draft.pattern != 0 {
                draft.preset.selection.width = "S".into();
            }
            ui.horizontal(|ui| {
                ui.label(t(z, "宽度", "Width"));
                ui.add_enabled_ui(draft.pattern == 0, |ui| {
                    egui::ComboBox::from_id_salt("overall-width")
                        .selected_text(&draft.preset.selection.width)
                        .show_ui(ui, |ui| {
                            for w in ["S", "M", "L", "XL"] {
                                ui.selectable_value(&mut draft.preset.selection.width, w.into(), w);
                            }
                        });
                });
            });
            ui.horizontal(|ui| {
                ui.label(t(z, "深度", "Depth"));
                egui::ComboBox::from_id_salt("overall-depth")
                    .selected_text(draft.preset.selection.depth.to_string())
                    .show_ui(ui, |ui| {
                        for d in 3..=7 {
                            ui.selectable_value(
                                &mut draft.preset.selection.depth,
                                d,
                                d.to_string(),
                            );
                        }
                    });
            });
            if draft.preset.selection.width == "XL" && draft.pattern == 0 {
                draft.preset.selection.height = "1F".into();
            }
            ui.horizontal(|ui| {
                ui.label(t(z, "高度", "Height"));
                egui::ComboBox::from_id_salt("overall-height")
                    .selected_text(&draft.preset.selection.height)
                    .show_ui(ui, |ui| {
                        for h in ["1F", "2F", "4F"] {
                            if draft.preset.selection.width != "XL"
                                || draft.pattern != 0
                                || h == "1F"
                            {
                                ui.selectable_value(
                                    &mut draft.preset.selection.height,
                                    h.into(),
                                    h,
                                );
                            }
                        }
                    });
            });
            ui.small(t(
                z,
                "堆叠：同宽同深，或 L ↔ S＋S、XL ↔ S＋M；支承顶面须齐平。",
                "Stack: matching footprint, L ↔ S+S, or XL ↔ S+M; support tops must align.",
            ));
            if draft.target.is_some()
                || !draft.preset.gridfinity.bins.is_empty()
                || !draft.preset.divider_layout.items.is_empty()
                || !draft.preset.sustainer_dividers.items.is_empty()
            {
                ui.checkbox(
                    &mut draft.clear_inserts,
                    t(
                        z,
                        "重置内部配置（保留标签和配色）",
                        "Reset inserts (keep label and colors)",
                    ),
                );
                ui.small(t(
                    z,
                    "默认保留内部配置；不兼容时会提示，不会自动删除。",
                    "Inserts are preserved unless you explicitly reset them.",
                ));
            }
            if !draft.error.is_empty() {
                ui.colored_label(Color32::from_rgb(230, 120, 100), &draft.error);
            }
            ui.horizontal(|ui| {
                apply = ui.button(t(z, "确认规格", "Confirm size")).clicked();
                cancel = ui.button(t(z, "取消", "Cancel")).clicked();
            });
        });
        if response.should_close() || cancel {
            return;
        }
        if apply {
            match self.apply_member_spec(&draft, ctx) {
                Ok(()) => return,
                Err(e) => draft.error = e,
            }
        }
        self.combination_spec = Some(draft);
    }
    pub(super) fn combination_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        if !self.combination_view && !self.members.is_empty() {
            ui.strong(format!(
                "{}：{}",
                t(z, "正在编辑", "Editing"),
                self.members[self.active_member].name
            ));
            ui.small(t(
                z,
                "修改自动保留在组合中。",
                "Changes are kept in the combination.",
            ));
            if ui
                .button(t(z, "← 返回整体装配", "← Back to overall assembly"))
                .clicked()
            {
                self.show_combination(ctx);
            }
            ui.separator();
            return;
        }
        if self.combination_spec.is_some() {
            ui.disable();
        }
        ui.strong(t(z, "整体排列", "Overall layout"));
        ui.small(t(
            z,
            "按说明书第 8 页检查堆叠规格与支承。",
            "Stacking compatibility follows manual page 8.",
        ));
        if self.members.is_empty() {
            if ui
                .add_enabled(
                    !self.bom.is_empty(),
                    egui::Button::new(t(
                        z,
                        "创建组合（保留当前装配）",
                        "Create combination from current assembly",
                    )),
                )
                .clicked()
            {
                self.show_combination(ctx);
            }
            ui.separator();
            return;
        }
        ui.small(t(
            z,
            "单击选择，双击编辑；拖动卡片移动 / 交换。",
            "Click to select, double-click to edit; drag to move / swap.",
        ));
        let action = layout_map(
            ui,
            &self.members,
            self.active_member,
            &mut self.combination_drag,
            z,
        );
        if let Some(action) = action {
            match action {
                LayoutAction::Select(index) => {
                    if self.combination_view {
                        self.select_member_in_combination(index, ctx);
                    } else {
                        self.commit_member();
                        self.restore_member(index);
                        self.assembly(ctx);
                    }
                }
                LayoutAction::Move(index, column, level) => {
                    self.relocate(index, column, level, ctx)
                }
                LayoutAction::Edit(index) => {
                    self.select_member_in_combination(index, ctx);
                    self.focus_member(ctx);
                    return;
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(self.members.len() < 64, |ui| {
                if ui.button(t(z, "＋抽屉", "+ Drawer")).clicked() {
                    self.add_member(Some(AssemblyKind::Drawer), false, ctx);
                }
                if ui.button(t(z, "＋盒子", "+ Box")).clicked() {
                    self.add_member(Some(AssemblyKind::Sustainer), false, ctx);
                }
                if ui.button(t(z, "修改尺寸…", "Change size…")).clicked() {
                    self.edit_member_size();
                }
                ui.menu_button(t(z, "复制…", "Copy…"), |ui| {
                    if ui.button(t(z, "复制到右侧", "Copy beside")).clicked() {
                        self.add_member(None, false, ctx);
                        ui.close();
                    }
                    if ui.button(t(z, "复制到上方", "Copy above")).clicked() {
                        self.add_member(None, true, ctx);
                        ui.close();
                    }
                });
            });
            if ui
                .add_enabled(
                    self.members.len() > 1,
                    egui::Button::new(t(z, "删除", "Remove")),
                )
                .clicked()
            {
                self.remove_member(ctx);
            }
            if ui
                .add_enabled(
                    !self.combination_layout_history.is_empty(),
                    egui::Button::new(t(z, "撤销移动", "Undo move")),
                )
                .clicked()
            {
                self.undo_layout(ctx);
            }
        });
        let pair = paired_members(&self.members, self.active_member);
        if pair.len() == 2 {
            ui.menu_button(
                t(z, "成对操作（2 个成员）…", "Pair actions (2 members)…"),
                |ui| {
                    if ui
                        .button(t(z, "成对移到右侧底层", "Move pair to ground at right"))
                        .clicked()
                    {
                        self.detach_pair(&pair, ctx);
                        ui.close();
                    }
                    if ui
                        .button(t(z, "删除这对成员", "Remove both members"))
                        .clicked()
                    {
                        self.remove_members(&pair, ctx);
                        ui.close();
                    }
                },
            );
        }
        let mut target = None;
        let selected = self.members[self.active_member].placement;
        ui.horizontal(|ui| {
            ui.label(t(z, "移动", "Move"));
            for (label, dx, dy) in [("←", -1, 0), ("→", 1, 0), ("↑", 0, 1), ("↓", 0, -1)] {
                let col = selected.column as i32 + dx;
                let level = selected.level as i32 + dy;
                if ui
                    .add_enabled(
                        (0..64).contains(&col) && (0..64).contains(&level),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    target = Some((col as u32, level as u32));
                }
            }
        });
        if let Some((column, level)) = target {
            self.relocate(self.active_member, column, level, ctx);
        }
        let current = self.members[self.active_member].placement;
        let mut requested = (current.column, current.level);
        egui::CollapsingHeader::new(t(z, "名称 / 列层位置", "Name / column and level")).show(
            ui,
            |ui| {
                let member = &mut self.members[self.active_member];
                ui.horizontal(|ui| {
                    ui.label(t(z, "名称", "Name"));
                    ui.add(egui::TextEdit::singleline(&mut member.name).char_limit(80));
                });
                let (mut column, mut level) =
                    (member.placement.column + 1, member.placement.level + 1);
                ui.horizontal(|ui| {
                    ui.label(t(z, "列 / 层", "Column / level"));
                    ui.add(egui::DragValue::new(&mut column).range(1..=64));
                    ui.add(egui::DragValue::new(&mut level).range(1..=64));
                });
                requested = (column - 1, level - 1);
            },
        );
        if requested
            != (
                self.members[self.active_member].placement.column,
                self.members[self.active_member].placement.level,
            )
        {
            self.relocate(self.active_member, requested.0, requested.1, ctx);
        }
        ui.separator();
    }
    pub(super) fn combination_summary(&mut self, ui: &mut egui::Ui) {
        ui.strong(t(
            self.zh,
            "组合打印清单（按成员）",
            "Combination print list (by member)",
        ));
        egui::ScrollArea::vertical()
            .max_height(120.)
            .show(ui, |ui| {
                for (i, m) in self.members.iter().enumerate() {
                    let count: u32 = m
                        .preset
                        .bom
                        .iter()
                        .filter(|b| b.enabled)
                        .map(|b| b.qty)
                        .sum();
                    ui.collapsing(
                        format!("{} · {} {}", m.name, count, t(self.zh, "份", "copies")),
                        |ui| {
                            for b in m.preset.bom.iter().filter(|b| b.enabled) {
                                ui.label(format!("{} × {}", b.name(), b.qty));
                            }
                        },
                    )
                    .header_response
                    .on_hover_text(format!("#{}", i + 1));
                }
            });
        ui.horizontal(|ui| {
            if self.busy || self.preview.has_pending() {
                ui.spinner();
            }
            ui.label(&self.status);
            if !self.busy
                && !self.preview.has_pending()
                && self.failed_preview.is_some()
                && ui.button(t(self.zh, "重试", "Retry")).clicked()
            {
                self.assembly(ui.ctx());
            }
        });
    }
    pub(super) fn combination_properties(&mut self, ui: &mut egui::Ui) {
        ui.strong(format!(
            "{}：{}",
            t(self.zh, "当前成员", "Selected member"),
            self.members[self.active_member].name
        ));
        ui.label(format!(
            "{} · {}{} / {}",
            self.assembly_kind.label(self.zh),
            self.selection.width,
            self.selection.depth,
            self.selection.height
        ));
        if ui
            .button(t(self.zh, "编辑此成员 →", "Edit this member →"))
            .clicked()
        {
            self.focus_member(ui.ctx());
            return;
        }
        ui.small(t(
            self.zh,
            "双击模型也可进入对应的单盒 / 单抽屉装配页。",
            "Double-click its model to open the box / drawer assembly page.",
        ));
        ui.horizontal_wrapped(|ui| {
            if ui.button(t(self.zh, "显示全部", "Show all")).clicked() {
                for p in &mut self.scene.parts {
                    p.visible = true;
                }
            }
            if ui
                .button(t(self.zh, "只看当前成员", "Isolate member"))
                .clicked()
            {
                for p in &mut self.scene.parts {
                    p.visible = p.pose.instance / MEMBER_STRIDE == self.active_member as u32;
                }
            }
        });
        let mut member_selected = None;
        let mut part_selected = None;
        egui::ScrollArea::vertical()
            .id_salt("combination-tree")
            .max_height(280.)
            .show(ui, |ui| {
                for (member, m) in self.members.iter().enumerate() {
                    ui.push_id(member, |ui| {
                        if ui
                            .selectable_label(member == self.active_member, &m.name)
                            .clicked()
                        {
                            member_selected = Some(member);
                        }
                        ui.collapsing(t(self.zh, "零件", "Parts"), |ui| {
                            for (index, p) in
                                self.scene.parts.iter_mut().enumerate().filter(|(_, p)| {
                                    p.pose.instance / MEMBER_STRIDE == member as u32
                                })
                            {
                                let mut local = p.clone();
                                local.pose.instance %= MEMBER_STRIDE;
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut p.visible, "");
                                    if ui
                                        .selectable_label(
                                            self.selected == Some(index),
                                            local.label(self.zh),
                                        )
                                        .clicked()
                                    {
                                        part_selected = Some(index);
                                    }
                                });
                            }
                        });
                    });
                }
            });
        if !self.busy && !self.preview.has_pending() {
            if let Some(index) = member_selected {
                self.select_member_in_combination(index, ui.ctx());
            }
            if let Some(index) = part_selected {
                self.select_combination_part(index, ui.ctx());
            }
        }
        ui.separator();
        ui.small(t(
            self.zh,
            "规格按说明书检查；安装面按网格估算，接口啮合未验证。",
            "Manual-compatible specifications; estimated mesh seating; connector engagement unverified.",
        ));
    }
    pub(super) fn paint_active_member(&self, ui: &egui::Ui, rect: egui::Rect) {
        let mut bounds = egui::Rect::NOTHING;
        for p in
            self.scene.parts.iter().filter(|p| {
                p.visible && p.pose.instance / MEMBER_STRIDE == self.active_member as u32
            })
        {
            let matrix = self.camera.matrix(rect.aspect_ratio()) * p.pose.matrix(self.explosion);
            for i in 0..8 {
                let v = glam::Vec3::new(
                    if i & 1 == 0 {
                        p.mesh.min.x
                    } else {
                        p.mesh.max.x
                    },
                    if i & 2 == 0 {
                        p.mesh.min.y
                    } else {
                        p.mesh.max.y
                    },
                    if i & 4 == 0 {
                        p.mesh.min.z
                    } else {
                        p.mesh.max.z
                    },
                );
                let ndc = matrix.project_point3(v);
                if ndc.is_finite() && (0. ..=1.).contains(&ndc.z) {
                    bounds.extend_with(egui::pos2(
                        rect.center().x + ndc.x * rect.width() * 0.5,
                        rect.center().y - ndc.y * rect.height() * 0.5,
                    ));
                }
            }
        }
        if bounds.is_finite() && bounds.intersects(rect) {
            let painter = ui.painter().with_clip_rect(rect);
            let bounds = bounds.expand(5.);
            painter.rect_stroke(
                bounds,
                5.,
                egui::Stroke::new(1.5, Color32::LIGHT_BLUE),
                egui::StrokeKind::Outside,
            );
            painter.text(
                bounds.left_top() - egui::vec2(0., 5.),
                egui::Align2::LEFT_BOTTOM,
                &self.members[self.active_member].name,
                egui::FontId::proportional(13.),
                Color32::LIGHT_BLUE,
            );
        }
    }
}
use crate::scene::MEMBER_STRIDE;

pub(super) fn load(
    root: &Path,
    home: &Path,
    members: &[Member],
    gap: f32,
    cancel: &crate::preview::Cancel,
) -> Result<Scene, String> {
    let descriptors = modules(members);
    let layout = crate::constraints::stacking::plan(&descriptors, gap)?;
    let rules = crate::display::Rules::load(&home.join("config/display-rules.toml"))?;
    let mut scenes = Vec::new();
    for member in members {
        crate::preview::check(Some(cancel))?;
        let p = &member.preset;
        let mut items = p.bom.clone();
        let catalog = Catalog {
            root: root.to_path_buf(),
            entries: items
                .iter()
                .map(|b| crate::catalog::Entry {
                    path: b.path.clone(),
                    name: b.name().into(),
                    category: 2,
                })
                .collect(),
        };
        if p.assembly_kind == AssemblyKind::Sustainer {
            items = catalog.sustainer_bom(&p.selection, p.sustainer_options)?;
            p.sustainer_dividers.sync_bom(
                &mut items,
                &catalog,
                &p.selection,
                p.sustainer_options,
            )?;
        }
        let originals: Vec<_> = items
            .iter()
            .filter(|b| !b.path.starts_with(crate::gridfinity::PREFIX))
            .cloned()
            .collect();
        let mut scene = Scene::load_with_rules(root, &originals, member.name.clone(), true, &rules)
            .map_err(|e| format!("{}: {e}", member.name))?;
        if p.assembly_kind == AssemblyKind::Sustainer {
            crate::sustainer_dividers::apply(
                &mut scene,
                &p.sustainer_dividers,
                &catalog,
                &p.selection,
                p.sustainer_options,
            )?;
        } else {
            crate::dividers::apply(&mut scene, &p.divider_layout, &p.selection)?;
        }
        crate::label::apply(&mut scene, &p.label)?;
        crate::gridfinity::apply_preview(
            &mut scene,
            home,
            &p.gridfinity,
            &p.selection,
            &items,
            false,
            Some(cancel),
        )?;
        let key = if p.assembly_kind == AssemblyKind::Sustainer {
            p.sustainer_options.key(&p.selection)
        } else {
            format!(
                "{}{}-{}",
                p.selection.width, p.selection.depth, p.selection.height
            )
        };
        load_scene_definition(&mut scene, home, &key)?;
        scenes.push((scene, member.placement, p.sustainer_angle));
    }
    crate::preview::check(Some(cancel))?;
    let mut closed: Vec<_> = scenes.iter().map(|(s, _, _)| s.clone()).collect();
    for scene in &mut closed {
        if crate::sustainer::lid_angle(scene).is_some() {
            crate::sustainer::set_lid_angle(scene, 0.)?;
        }
    }
    let shifts = crate::constraints::stacking::shifts(&layout, &descriptors, &closed)?;
    for (i, (_, _, angle)) in scenes.iter_mut().enumerate() {
        if layout.supports.iter().any(|below| below.contains(&i)) {
            *angle = 0.;
        }
    }
    Scene::combine_positioned(scenes, gap, Some(shifts))
}

impl App {
    pub(super) fn export_combination(&mut self, ctx: &egui::Context) {
        self.commit_member();
        let saved = CombinationPreset {
            preset: self.member_snapshot(),
            members: self.members.clone(),
            active_member: self.active_member,
            combination_gap: self.combination_gap,
        };
        if let Err(e) = saved.validate(&self.catalog) {
            self.status = e;
            return;
        }
        let Some(folder) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let target = folder.join(format!(
            "VAULT-combination-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        if let Err(e) = self.check_destination(&target) {
            self.status = e;
            return;
        }
        let root = self.catalog.root.clone();
        let home = self.home.clone();
        let format = self.export_options.format;
        let (tx, rx) = mpsc::channel();
        self.export_rx = Some(rx);
        self.export_last = None;
        self.status = t(self.zh, "正在后台导出组合…", "Exporting combination…").into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(export_members(&root, &home, &target, format, &saved));
            ctx.request_repaint();
        });
    }
}
fn export_members(
    root: &Path,
    home: &Path,
    target: &Path,
    format: crate::export::Format,
    saved: &CombinationPreset,
) -> Result<PathBuf, String> {
    let library = root.canonicalize().map_err(|e| e.to_string())?;
    let parent = target
        .parent()
        .ok_or("Invalid destination")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if parent.starts_with(&library) || target.exists() {
        return Err("目标已存在或位于模型库内 / Existing destination or model library".into());
    }
    let total: u32 = saved
        .members
        .iter()
        .flat_map(|m| &m.preset.bom)
        .filter(|b| b.enabled)
        .map(|b| b.qty)
        .sum();
    if total == 0 || total > 1000 {
        return Err("组合单次导出需为 1–1000 份 / Export requires 1–1000 copies".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".vault-combination-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    for (index, member) in saved.members.iter().enumerate() {
        let p = &member.preset;
        if !p.bom.iter().any(|b| b.enabled) {
            continue;
        }
        let name = format!(
            "{:02}-{}{}-{}",
            index + 1,
            p.selection.width,
            p.selection.depth,
            p.selection.height
        );
        let destination = staging
            .path()
            .join(if format == crate::export::Format::ThreeMf {
                format!("{name}.3mf")
            } else {
                name
            });
        crate::export::run_configured(
            root,
            &p.bom,
            &destination,
            &crate::export::Options {
                format,
                plate: p.print_config.clone().unwrap_or_default(),
            },
            &p.label,
            Some((home, &p.gridfinity, &p.selection)),
        )
        .map_err(|e| format!("{}: {e}", member.name))?;
    }
    std::fs::write(
        staging.path().join("combination.toml"),
        toml::to_string_pretty(saved).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(staging.path(), target).map_err(|e| e.to_string())?;
    Ok(target.to_path_buf())
}

impl App {
    pub(super) fn setup_combination_smoke(&mut self) {
        let base = self.member_snapshot();
        self.members.clear();
        for i in 0..3 {
            let mut p = base.clone();
            p.assembly_kind = if i == 1 {
                AssemblyKind::Sustainer
            } else {
                AssemblyKind::Drawer
            };
            p.selection = Selection {
                width: if i == 1 { "M" } else { "S" }.into(),
                depth: if i == 1 { 4 } else { 3 },
                height: if i == 1 { "2F" } else { "1F" }.into(),
                hidden_key: false,
            };
            p.bom = if i == 1 {
                self.catalog
                    .sustainer_bom(&p.selection, p.sustainer_options)
                    .unwrap()
            } else {
                self.catalog.bom(&p.selection)
            };
            p.label.enabled = true;
            p.label.text = format!("BOX {}", i + 1);
            p.label.style = crate::label::Style::Raised;
            let config = p.print_config.as_mut().unwrap();
            config.preview_colors = true;
            config.palette = [
                [74, 104, 120],
                [234, 151, 73],
                [78, 156, 148],
                [168, 184, 198],
                [136, 109, 173],
                [235, 235, 232],
            ];
            config.set_part_slot(p.assembly_kind.label_path(), i + 2);
            config.set_text_slot(p.assembly_kind.label_path(), 6);
            self.members.push(Member {
                name: format!("成员 {}", i + 1),
                placement: Placement {
                    column: if i == 1 { 1 } else { 0 },
                    level: if i == 2 { 1 } else { 0 },
                    offset: [0.; 3],
                },
                preset: p,
            });
        }
        self.restore_member(0);
        self.combination_view = true;
        self.explosion = 0.;
        self.smoke_stage = 0;
    }
    pub(super) fn combination_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        let folder = self.home.join("output");
        if self.smoke_start.elapsed() > Duration::from_secs(180)
            || (!self.busy && !self.preview.has_pending() && self.failed_preview.is_some())
        {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("combination-smoke-error.txt"), &self.status);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 && self.smoke_stage == 0 {
            self.camera.fit(&self.scene, 0.);
            self.camera.yaw = 0.5;
            self.camera.zoom = 1.08;
        }
        if self.smoke_frame >= 10 && !self.screenshot_requested {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let screenshot = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        let Some(image) = screenshot else {
            return;
        };
        let result = (|| -> Result<(), String> {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                folder.join(format!("combination-smoke-{}.png", self.smoke_stage)),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            if self.combination_view {
                let ids: std::collections::HashSet<_> = self
                    .scene
                    .parts
                    .iter()
                    .map(crate::constraints::PartId::of)
                    .collect();
                if ids.len() != self.scene.parts.len() || self.scene.calibrated {
                    return Err("Invalid combined identity or calibration state".into());
                }
                for i in 0..self.members.len() as u32 {
                    if !self
                        .scene
                        .parts
                        .iter()
                        .any(|p| p.pose.instance / MEMBER_STRIDE == i)
                    {
                        return Err("Missing member in rendered scene".into());
                    }
                }
            }
            if [2, 4].contains(&self.smoke_stage) {
                let expected = &self
                    .combination_view_state
                    .as_ref()
                    .ok_or("Missing saved overall view")?
                    .0;
                if (self.camera.yaw - expected.yaw).abs() > 0.001
                    || (self.camera.zoom - expected.zoom).abs() > 0.001
                    || (self.camera.center - expected.center).length() > 0.001
                {
                    return Err("Overall camera was not restored".into());
                }
            }
            if (4..=8).contains(&self.smoke_stage)
                && (self.members[0].preset.selection.depth != 3
                    || self.members[2].preset.selection.depth != 3
                    || self.members[2].preset.label.text != "EDITED")
            {
                return Err("Member edits leaked or were lost".into());
            }
            if self.smoke_stage == 5
                && (
                    self.members[2].placement.column,
                    self.members[2].placement.level,
                ) != (0, 1)
            {
                return Err("Invalid move was not rejected".into());
            }
            if self.smoke_stage == 6
                && (
                    self.members[2].placement.column,
                    self.members[2].placement.level,
                ) != (0, 1)
            {
                return Err("Undo failed".into());
            }
            if self.smoke_stage == 7 && self.members.len() != 4
                || self.smoke_stage == 8 && self.members.len() != 3
            {
                return Err("Add/remove failed".into());
            }
            if [10, 12, 13].contains(&self.smoke_stage) {
                validate_layout(&self.members, self.combination_gap)?;
                if self.members.len() != 3 || self.members[0].preset.sustainer_angle != 65. {
                    return Err("Pair addition or saved lid state failed".into());
                }
                if let Some(angle) = crate::sustainer::lid_angle(&self.scene)
                    && angle.abs() > 0.01
                {
                    return Err("Supporting lid is open".into());
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::write(folder.join("combination-smoke-error.txt"), e);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.smoke_stage += 1;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
        match self.smoke_stage {
            1 => {
                self.select_member_in_combination(1, ctx);
                self.focus_member(ctx);
                self.sustainer_angle = 65.;
                self.assembly(ctx);
            }
            2 => self.show_combination(ctx),
            3 => {
                self.select_member_in_combination(2, ctx);
                self.focus_member(ctx);
                self.label.text = "EDITED".into();
                self.bom = self.catalog.reconfigure(&self.selection, &self.bom);
                self.assembly(ctx);
            }
            4 => self.show_combination(ctx),
            5 => self.relocate(2, 1, 0, ctx),
            6 => {
                self.edit_member_size();
                let mut draft = self.combination_spec.take().unwrap();
                draft.preset.selection.depth = 4;
                if self.apply_member_spec(&draft, ctx).is_ok() {
                    let _ = std::fs::write(
                        folder.join("combination-smoke-error.txt"),
                        "Incompatible resize accepted",
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                draft.preset.selection.depth = 3;
                draft.preset.selection.height = "2F".into();
                if let Err(e) = self.apply_member_spec(&draft, ctx) {
                    let _ = std::fs::write(folder.join("combination-smoke-error.txt"), e);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            7 => {
                self.add_member(Some(AssemblyKind::Sustainer), false, ctx);
                let draft = self.combination_spec.take().unwrap();
                if let Err(e) = self.apply_member_spec(&draft, ctx) {
                    let _ = std::fs::write(folder.join("combination-smoke-error.txt"), e);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            8 => self.remove_member(ctx),
            9 | 11 => {
                let mut p = self.members[0].preset.clone();
                p.assembly_kind = AssemblyKind::Sustainer;
                p.selection.width = if self.smoke_stage == 9 { "L" } else { "XL" }.into();
                p.selection.height = "1F".into();
                p.selection.depth = 3;
                p.sustainer_angle = 65.;
                p.bom = self
                    .catalog
                    .sustainer_bom(&p.selection, p.sustainer_options)
                    .unwrap();
                self.members = vec![Member {
                    name: "支承盒子".into(),
                    placement: Default::default(),
                    preset: p,
                }];
                self.restore_member(0);
                self.combination_view_state = None;
                self.combination_view = true;
                self.combination_layout_history.clear();
                self.assembly(ctx);
            }
            10 | 12 => {
                self.add_member(Some(AssemblyKind::Sustainer), true, ctx);
                let mut draft = self.combination_spec.take().unwrap();
                draft.pattern = if self.smoke_stage == 10 { 1 } else { 2 };
                if let Err(e) = self.apply_member_spec(&draft, ctx) {
                    let _ = std::fs::write(folder.join("combination-smoke-error.txt"), e);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            13 => self.edit_member_size(),
            _ => {
                let _ = std::fs::write(
                    folder.join("combination-smoke-result.txt"),
                    format!(
                        "PASS: two pages; single-member label editing; overall size change; incompatible resize and unsupported move rejected; saved overall camera; confirmed add/remove; L/S+S and XL/S+M groups; supporting lid closure; size dialog. GPU: {}. Parts: {}",
                        self.renderer.state.adapter.get_info().name,
                        self.scene.parts.len()
                    ),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

fn paired_members(members: &[Member], index: usize) -> Vec<usize> {
    let Ok(layout) = crate::constraints::stacking::plan(&modules(members), 0.) else {
        return vec![];
    };
    let Some(support) = layout.supports.get(index) else {
        return vec![];
    };
    if support.len() == 1 {
        let group: Vec<_> = layout
            .supports
            .iter()
            .enumerate()
            .filter(|(i, below)| {
                members[*i].placement.level == members[index].placement.level && *below == support
            })
            .map(|(i, _)| i)
            .collect();
        if group.len() == 2 {
            return group;
        }
    }
    layout
        .supports
        .iter()
        .find(|below| below.len() == 2 && below.contains(&index))
        .cloned()
        .unwrap_or_default()
}
fn detached_pair(members: &[Member], indices: &[usize]) -> Result<Vec<Member>, String> {
    if indices.len() != 2 || indices.iter().any(|&i| i >= members.len()) {
        return Err("无效配对 / Invalid pair".into());
    }
    let mut next = members.to_vec();
    let column = members
        .iter()
        .map(|m| m.placement.column)
        .max()
        .unwrap_or(0)
        + 1;
    for shift in 0..64 - column.min(63) {
        for (slot, &i) in indices.iter().enumerate() {
            next[i].placement.column = column + shift + slot as u32;
            next[i].placement.level = 0;
        }
        if validate_layout(&next, 0.).is_ok() {
            return Ok(next);
        }
    }
    Err(
        "无法整体移到右侧底层；请先移开上层支承对象或腾出位置 / Cannot detach this supporting pair"
            .into(),
    )
}

/// Only layout coordinates change; each member keeps its own configuration and fine offset.
fn relocate_member(
    members: &mut [Member],
    index: usize,
    column: u32,
    level: u32,
) -> Result<bool, String> {
    let member = members.get(index).ok_or("Unknown member")?;
    Placement {
        column,
        level,
        offset: member.placement.offset,
    }
    .validate()?;
    let old = (member.placement.column, member.placement.level);
    if old == (column, level) {
        return Ok(false);
    }
    if let Some(other) = members
        .iter()
        .position(|m| (m.placement.column, m.placement.level) == (column, level))
    {
        members[other].placement.column = old.0;
        members[other].placement.level = old.1;
    }
    members[index].placement.column = column;
    members[index].placement.level = level;
    Ok(true)
}
#[derive(Debug, PartialEq, Eq)]
enum LayoutAction {
    Select(usize),
    Edit(usize),
    Move(usize, u32, u32),
}
fn layout_map(
    ui: &mut egui::Ui,
    members: &[Member],
    active: usize,
    dragged: &mut Option<usize>,
    zh: bool,
) -> Option<LayoutAction> {
    let mut columns: std::collections::BTreeSet<_> =
        members.iter().map(|m| m.placement.column).collect();
    let mut levels: std::collections::BTreeSet<_> =
        members.iter().map(|m| m.placement.level).collect();
    if let Some(&column) = columns.last()
        && column < 63
    {
        columns.insert(column + 1);
    }
    if let Some(&level) = levels.last()
        && level < 63
    {
        levels.insert(level + 1);
    }
    let position = members
        .get(active)
        .map(|m| (active, m.placement.column, m.placement.level, members.len()));
    let key = ui.make_persistent_id("combination-map-reveal");
    let reveal = ui.data_mut(|data| {
        let previous = data
            .get_temp::<Option<(usize, u32, u32, usize)>>(key)
            .flatten();
        data.insert_temp(key, position);
        previous != position
    });
    let mut action = None;
    let released = ui.input(|i| i.pointer.any_released());
    if ui.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.secondary_clicked()) {
        *dragged = None;
    }
    egui::ScrollArea::both()
        .id_salt("combination-layout-map")
        .max_height(240.)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("combination-layout-cells")
                .spacing([4., 4.])
                .show(ui, |ui| {
                    ui.label("");
                    for col in &columns {
                        ui.small(format!("{} {}", t(zh, "列", "Column"), col + 1));
                    }
                    ui.end_row();
                    for &level in levels.iter().rev() {
                        ui.small(format!("{} {}", t(zh, "层", "Level"), level + 1));
                        for &column in &columns {
                            let member = members.iter().position(|m| {
                                m.placement.column == column && m.placement.level == level
                            });
                            let text = member
                                .map(|index| {
                                    let m = &members[index];
                                    format!(
                                        "{}\n{}{} / {}",
                                        m.name,
                                        m.preset.selection.width,
                                        m.preset.selection.depth,
                                        m.preset.selection.height
                                    )
                                })
                                .unwrap_or_else(|| t(zh, "空位", "Empty").into());
                            let fill = if member == Some(active) {
                                Color32::from_rgb(35, 87, 115)
                            } else if member.is_some() {
                                Color32::from_rgb(40, 48, 59)
                            } else {
                                Color32::from_rgb(25, 30, 37)
                            };
                            let response = ui.add_sized(
                                [96., 52.],
                                egui::Button::new(text)
                                    .fill(fill)
                                    .sense(egui::Sense::click_and_drag()),
                            );
                            if reveal && member == Some(active) && dragged.is_none() {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            if let Some(index) = member {
                                if response.double_clicked() {
                                    action = Some(LayoutAction::Edit(index));
                                } else if response.clicked() {
                                    action = Some(LayoutAction::Select(index));
                                }
                                if response.drag_started() {
                                    *dragged = Some(index);
                                }
                            }
                            let hovered = ui.input(|i| {
                                i.pointer.interact_pos().is_some_and(|p| {
                                    response.rect.contains(p) && ui.clip_rect().contains(p)
                                })
                            });
                            if hovered && let Some(index) = *dragged {
                                ui.painter().rect_stroke(
                                    response.rect,
                                    3.,
                                    egui::Stroke::new(2., Color32::LIGHT_GREEN),
                                    egui::StrokeKind::Inside,
                                );
                                if released {
                                    action = Some(LayoutAction::Move(index, column, level));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    if released {
        *dragged = None;
    }
    ui.small(t(
        zh,
        "排列示意，非尺寸比例；宽模块可跨列，堆叠必须连续。",
        "Not to scale; wide modules span columns; stack levels must be continuous.",
    ));
    action
}
pub(super) fn retain_visibility(scene: &mut Scene, old: &Scene) {
    let visible: HashMap<_, _> = old
        .parts
        .iter()
        .map(|p| (crate::constraints::PartId::of(p), p.visible))
        .collect();
    for part in &mut scene.parts {
        if let Some(&value) = visible.get(&crate::constraints::PartId::of(part)) {
            part.visible = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preset() -> Preset {
        Preset {
            sustainer_dividers: Default::default(),
            print_config: Some(Default::default()),
            gridfinity: Default::default(),
            label: Default::default(),
            assembly_kind: AssemblyKind::Drawer,
            sustainer_options: Default::default(),
            sustainer_angle: 0.,
            selection: Default::default(),
            bom: vec![],
            divider_layout: Default::default(),
        }
    }

    fn members() -> Vec<Member> {
        ["Alpha", "Beta"]
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let mut p = preset();
                p.label.text = name.into();
                Member {
                    name: name.into(),
                    placement: Placement {
                        column: i as u32,
                        level: 0,
                        offset: [i as f32, 0., 0.],
                    },
                    preset: p,
                }
            })
            .collect()
    }
    #[test]
    fn moving_and_swapping_preserve_member_configuration_and_fail_atomically() {
        let mut members = members();
        assert!(relocate_member(&mut members, 0, 1, 0).unwrap());
        assert_eq!(members[0].placement.column, 1);
        assert_eq!(members[1].placement.column, 0);
        assert_eq!(members[0].preset.label.text, "Alpha");
        assert_eq!(members[1].preset.label.text, "Beta");
        assert_eq!(members[1].placement.offset, [1., 0., 0.]);
        let before = toml::to_string(&members[0]).unwrap();
        assert!(relocate_member(&mut members, 0, 64, 0).is_err());
        assert_eq!(toml::to_string(&members[0]).unwrap(), before);
        assert!(relocate_member(&mut members, 0, 0, 1).unwrap());
        assert_eq!(members[0].placement.level, 1);
        assert!(!relocate_member(&mut members, 0, 0, 1).unwrap());
    }
    #[test]
    fn layout_map_pointer_selection_drag_swap_and_outside_cancel() {
        let ctx = egui::Context::default();
        let members = members();
        let mut dragged = None;
        let mut time = 0.;
        let mut frame = |events: Vec<egui::Event>| {
            time += 0.05;
            let mut action = None;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 400.),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        action = layout_map(ui, &members, 0, &mut dragged, false);
                    });
                },
            );
            let mut centers = HashMap::new();
            for clipped in output.shapes {
                if let egui::Shape::Text(text) = clipped.shape {
                    let value = text.galley.text();
                    for name in ["Alpha", "Beta"] {
                        if value.starts_with(name) {
                            centers.insert(
                                name,
                                egui::Rect::from_min_size(text.pos, text.galley.size()).center(),
                            );
                        }
                    }
                }
            }
            (action, centers)
        };
        for _ in 0..3 {
            frame(vec![]);
        }
        let (_, centers) = frame(vec![]);
        let a = centers["Alpha"];
        let b = centers["Beta"];
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(b), button(b, true)]);
        assert_eq!(
            frame(vec![button(b, false)]).0,
            Some(LayoutAction::Select(1))
        );
        frame(vec![egui::Event::PointerMoved(a), button(a, true)]);
        frame(vec![egui::Event::PointerMoved(b)]);
        assert_eq!(
            frame(vec![button(b, false)]).0,
            Some(LayoutAction::Move(0, 1, 0))
        );
        let outside = egui::pos2(550., 350.);
        frame(vec![egui::Event::PointerMoved(a), button(a, true)]);
        frame(vec![egui::Event::PointerMoved(outside)]);
        assert_eq!(frame(vec![button(outside, false)]).0, None);
        // A fresh click after the canceled drag is a selection, not a stale drop.
        for _ in 0..10 {
            frame(vec![]);
        }
        frame(vec![egui::Event::PointerMoved(b), button(b, true)]);
        assert_eq!(
            frame(vec![button(b, false)]).0,
            Some(LayoutAction::Select(1))
        );
        frame(vec![button(b, true)]);
        assert_eq!(frame(vec![button(b, false)]).0, Some(LayoutAction::Edit(1)));
    }
    #[test]
    fn supported_pairs_can_be_detached_and_removed_without_losing_configuration() {
        let mut base = preset();
        base.selection.width = "L".into();
        let mut a = preset();
        a.label.text = "A".into();
        let mut b = preset();
        b.label.text = "B".into();
        let members = vec![
            Member {
                name: "base".into(),
                placement: Default::default(),
                preset: base,
            },
            Member {
                name: "A".into(),
                placement: Placement {
                    level: 1,
                    ..Default::default()
                },
                preset: a,
            },
            Member {
                name: "B".into(),
                placement: Placement {
                    column: 1,
                    level: 1,
                    offset: [0.; 3],
                },
                preset: b,
            },
        ];
        validate_layout(&members, 0.).unwrap();
        assert_eq!(paired_members(&members, 1), vec![1, 2]);
        let detached = detached_pair(&members, &[1, 2]).unwrap();
        assert_eq!(detached[1].placement.level, 0);
        assert_eq!(detached[2].placement.level, 0);
        assert_eq!(detached[1].preset.label.text, "A");
        assert_eq!(detached[2].preset.label.text, "B");
        validate_layout(&members[..1], 0.).unwrap();
        assert!(validate_layout(&members[..2], 0.).is_err());
    }
    #[test]
    fn old_presets_load_and_combination_roundtrip_keeps_independent_state() {
        let old = toml::to_string(&preset()).unwrap();
        let legacy: CombinationPreset = toml::from_str(&old).unwrap();
        assert!(legacy.members.is_empty());
        let mut a = preset();
        a.label.text = "A".into();
        a.print_config
            .as_mut()
            .unwrap()
            .set_part_slot(crate::label::PATH, 2);
        let mut b = a.clone();
        b.label.text = "B".into();
        b.selection.width = "M".into();
        b.selection.depth = 4;
        b.print_config
            .as_mut()
            .unwrap()
            .set_part_slot(crate::label::PATH, 3);
        let project = CombinationPreset {
            preset: a.clone(),
            members: vec![
                Member {
                    name: "A".into(),
                    placement: Default::default(),
                    preset: a,
                },
                Member {
                    name: "B".into(),
                    placement: Placement {
                        column: 1,
                        level: 0,
                        offset: [0.; 3],
                    },
                    preset: b,
                },
            ],
            active_member: 1,
            combination_gap: 0.,
        };
        let restored: CombinationPreset =
            toml::from_str(&toml::to_string(&project).unwrap()).unwrap();
        restored.validate(&Catalog::default()).unwrap();
        assert_eq!(restored.members[0].preset.label.text, "A");
        assert_eq!(restored.members[1].preset.label.text, "B");
        assert_eq!(restored.members[1].preset.selection.width, "M");
        assert_eq!(restored.members[1].placement.offset, [0.; 3]);
        assert_eq!(
            restored.members[0]
                .preset
                .print_config
                .as_ref()
                .unwrap()
                .slot(crate::label::PATH),
            2
        );
        assert_eq!(
            restored.members[1]
                .preset
                .print_config
                .as_ref()
                .unwrap()
                .slot(crate::label::PATH),
            3
        );
        assert_eq!(restored.active_member, 1);
        let mut invalid = restored;
        invalid.members[1].placement.offset = [1., 2., 3.];
        assert!(invalid.validate(&Catalog::default()).is_err());
        invalid.members[1].placement = invalid.members[0].placement;
        assert!(invalid.validate(&Catalog::default()).is_err());
    }
    #[test]
    fn combination_export_keeps_duplicate_sources_and_counts_separate() {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let output = home.join("output");
        std::fs::create_dir_all(&output).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("combination-test-")
            .tempdir_in(output)
            .unwrap();
        let library = temp.path().join("library");
        std::fs::create_dir(&library).unwrap();
        let source = "solid test\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid test\n";
        std::fs::write(library.join("test.stl"), source).unwrap();
        let mut p = preset();
        p.bom = vec![Bom {
            path: "test.stl".into(),
            qty: 2,
            enabled: true,
            group: "test".into(),
        }];
        let mut q = p.clone();
        q.bom[0].qty = 3;
        let saved = CombinationPreset {
            preset: p.clone(),
            members: vec![
                Member {
                    name: "A".into(),
                    placement: Default::default(),
                    preset: p,
                },
                Member {
                    name: "B".into(),
                    placement: Placement {
                        level: 1,
                        ..Default::default()
                    },
                    preset: q,
                },
            ],
            active_member: 0,
            combination_gap: 0.,
        };
        let target = temp.path().join("export");
        export_members(&library, &home, &target, crate::export::Format::Stl, &saved).unwrap();
        assert_eq!(
            std::fs::read_dir(target.join("01-S3-1F")).unwrap().count(),
            2
        );
        assert_eq!(
            std::fs::read_dir(target.join("02-S3-1F")).unwrap().count(),
            3
        );
        assert!(target.join("combination.toml").is_file());
        assert_eq!(
            std::fs::read_to_string(library.join("test.stl")).unwrap(),
            source
        );
        assert!(
            export_members(
                &library,
                &home,
                &library.join("bad"),
                crate::export::Format::Stl,
                &saved
            )
            .is_err()
        );
        assert!(!library.join("bad").exists());
    }
}
