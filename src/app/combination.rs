pub(super) mod exterior;
#[cfg(test)]
mod regression;
mod spatial;
use super::*;
use crate::scene::Placement;
use spatial::{Side, position_relative};

pub(super) struct SpecEdit {
    target: Option<usize>,
    anchor: usize,
    preset: Preset,
    exterior: crate::catalog::exterior::Exterior,
    side: Side,
    pattern: u8,
    clear_inserts: bool,
    error: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Member {
    pub name: String,
    pub placement: Placement,
    pub preset: Preset,
    #[serde(default)]
    pub exterior: crate::catalog::exterior::Exterior,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct CombinationPreset {
    #[serde(default)]
    pub colors: crate::print_plate::ColorTheme,
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
    let descriptors = modules(members);
    let layout = crate::constraints::stacking::plan(&descriptors, gap)?;
    crate::constraints::exterior::validate_layout(
        &descriptors,
        &layout,
        &members
            .iter()
            .map(|m| m.exterior.clone())
            .collect::<Vec<_>>(),
    )
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
        self.colors.validate()?;
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
                    "成员名称为空、过长或放置位置重复 / Invalid member name or duplicate position"
                        .into(),
                );
            }
            catalog.exterior_components(
                &member.preset.selection,
                &member.exterior,
                member.preset.assembly_kind == AssemblyKind::Sustainer,
            )?;
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
        self.exterior_edit = None;
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
    pub(super) fn ensure_members(&mut self) {
        if self.members.is_empty() {
            self.members.push(Member {
                exterior: Default::default(),
                name: format!("{} 1", self.assembly_kind.label(self.zh)),
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
            colors: self.global_colors.clone(),
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
        self.finish_member_edit();
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
        if self.member_edit_baseline.is_none() {
            self.member_edit_baseline = Some(self.workflow_snapshot());
        }
        self.assembly(ctx);
    }
    fn remove_member(&mut self, ctx: &egui::Context) {
        self.remove_members(&[self.active_member], ctx);
    }
    pub(super) fn remove_members(&mut self, indices: &[usize], ctx: &egui::Context) {
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
        self.record_workflow();
        self.members = candidate;
        self.restore_member(selected);
        self.combination_view = true;
        self.member_selection.clear();
        self.combination_move_source = None;
        self.assembly(ctx);
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
            let c = preset.print_config.get_or_insert_with(Default::default);
            self.global_colors.apply(c);
            c.follow_global_colors = true;
        }
        self.combination_spec = Some(SpecEdit {
            exterior: if kind.is_none() {
                self.members[self.active_member].exterior.clone()
            } else {
                Default::default()
            },
            target: None,
            anchor: self.active_member,
            preset,
            side: if above { Side::Above } else { Side::Right },
            pattern: 0,
            clear_inserts: false,
            error: String::new(),
        });
    }
    pub(super) fn edit_member_size(&mut self) {
        self.commit_member();
        self.ensure_members();
        self.combination_spec = Some(SpecEdit {
            exterior: self.members[self.active_member].exterior.clone(),
            target: Some(self.active_member),
            anchor: self.active_member,
            preset: self.members[self.active_member].preset.clone(),
            side: Side::Right,
            pattern: 0,
            clear_inserts: false,
            error: String::new(),
        });
    }
    fn apply_member_spec(&mut self, draft: &SpecEdit, ctx: &egui::Context) -> Result<(), String> {
        self.commit_member();
        let mut next = self.members.clone();
        let _anchor = next
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
            active = next.len();
            let mut additions = vec![p.clone()];
            if draft.pattern != 0 {
                p.selection.width = if draft.pattern == 1 { "S" } else { "M" }.into();
                configure(&mut p)?;
                additions.push(p);
            }
            for (i, preset) in additions.into_iter().enumerate() {
                next.push(Member {
                    exterior: draft.exterior.clone(),
                    name: unique_member_name(
                        &next,
                        &format!(
                            "{} {}",
                            if preset.assembly_kind == AssemblyKind::Sustainer {
                                t(self.zh, "盒子", "Box")
                            } else {
                                t(self.zh, "抽屉", "Drawer")
                            },
                            next.len() + 1
                        ),
                    ),
                    placement: Placement {
                        column: i as u32,
                        level: 0,
                        offset: [0.; 3],
                    },
                    preset,
                });
            }
            next = position_relative(
                &next,
                &(active..next.len()).collect::<Vec<_>>(),
                draft.anchor,
                draft.side,
            )?;
        }
        validate_layout(&next, self.combination_gap)?;
        for m in &next {
            self.catalog.exterior_components(
                &m.preset.selection,
                &m.exterior,
                m.preset.assembly_kind == AssemblyKind::Sustainer,
            )?;
        }
        if next.len() > 64 {
            return Err("最多 64 个成员 / Up to 64 members".into());
        }
        self.remember_combination_view();
        self.record_workflow();
        self.members = next;
        self.restore_member(active);
        self.combination_view = true;
        self.member_selection.clear();
        self.combination_move_source = None;
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
                ui.label(format!(
                    "{}：{}",
                    t(z, "参照对象", "Relative to"),
                    self.members
                        .get(draft.anchor)
                        .map(|m| m.name.as_str())
                        .unwrap_or("?")
                ));
                ui.horizontal(|ui| {
                    for side in [Side::Left, Side::Above, Side::Right] {
                        ui.radio_value(&mut draft.side, side, side.label(z));
                    }
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
            self.member_color_follow_ui(ui);
            ui.separator();
            return;
        }
        if self.combination_spec.is_some() {
            ui.disable();
        }
        ui.strong(t(z, "三维装配", "3D assembly"));
        if self.members.is_empty() {
            if ui
                .add_enabled(
                    !self.bom.is_empty(),
                    egui::Button::new(t(z, "开始整体装配", "Start overall assembly")),
                )
                .clicked()
            {
                self.show_combination(ctx);
            }
            return;
        }
        ui.label(t(
            z,
            "在预览中点击盒子或抽屉，通过模型旁的按钮添加或修改。",
            "Select a box or drawer in the preview, then use its on-model controls.",
        ));
        ui.small(t(z,"左键拖动旋转，右键拖动平移；移动对象时点击目标，再选择对齐方向。","Drag to orbit; right-drag to pan. To move a member, pick its target and choose a direction."));
        ui.separator();
        ui.strong(t(z, "组合中的对象", "Objects"));
        let mut selected = None;
        for (i, m) in self.members.iter().enumerate() {
            ui.horizontal(|ui| {
                let mut checked = self.member_selection.contains(&i);
                if ui
                    .checkbox(&mut checked, "")
                    .on_hover_text(t(z, "批量选择", "Select for batch actions"))
                    .changed()
                {
                    if checked {
                        self.member_selection.insert(i);
                    } else {
                        self.member_selection.remove(&i);
                    }
                }
                if ui
                    .selectable_label(
                        i == self.active_member,
                        format!(
                            "{} · {}{} / {}",
                            m.name,
                            m.preset.selection.width,
                            m.preset.selection.depth,
                            m.preset.selection.height
                        ),
                    )
                    .clicked()
                {
                    selected = Some(i);
                }
            });
        }
        if let Some(i) = selected {
            self.select_member_in_combination(i, ctx);
        }
        ui.separator();
        ui.label(t(z, "名称", "Name"));
        let mut name = self.members[self.active_member].name.clone();
        if ui
            .add(egui::TextEdit::singleline(&mut name).char_limit(80))
            .changed()
        {
            self.record_workflow();
            self.members[self.active_member].name = name;
        }
        self.workflow_undo_ui(ui, ctx);
        self.batch_member_actions(ui, ctx);
        if ui
            .button(t(z, "安装 / 修改配件…", "Install / edit accessories…"))
            .clicked()
        {
            self.open_exterior();
        }
        self.combination_dimensions(ui);
        ui.small(t(
            z,
            "堆叠规格按原说明书检查；接口啮合尚未实物验证。",
            "Stacking specifications follow the manual; physical engagement is unverified.",
        ));
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
                    let extras = self
                        .catalog
                        .exterior_components(
                            &m.preset.selection,
                            &m.exterior,
                            m.preset.assembly_kind == AssemblyKind::Sustainer,
                        )
                        .unwrap_or_default();
                    let count: u32 = extras.iter().map(|c| c.bom.qty).sum::<u32>()
                        + m.preset
                            .bom
                            .iter()
                            .filter(|b| b.enabled)
                            .map(|b| b.qty)
                            .sum::<u32>();
                    ui.collapsing(
                        format!("{} · {} {}", m.name, count, t(self.zh, "份", "copies")),
                        |ui| {
                            for b in m
                                .preset
                                .bom
                                .iter()
                                .chain(extras.iter().map(|c| &c.bom))
                                .filter(|b| b.enabled)
                            {
                                ui.label(format!("{} × {}", b.name(), b.qty));
                            }
                            for h in m.exterior.hardware(&m.preset.selection) {
                                ui.label(h.line());
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
        self.member_color_follow_ui(ui);
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
        let Some(&(lo, hi)) = self.scene.member_bounds.get(self.active_member) else {
            return;
        };
        if self.explosion > 0.001 {
            return;
        }
        let matrix = self.camera.matrix(rect.aspect_ratio());
        // Project the eight world-space corners and connect the twelve box edges.
        // Keeping their depth in the projection makes the frame follow the model
        // when the camera rotates instead of enclosing it in a screen rectangle.
        let corners: [Option<egui::Pos2>; 8] = std::array::from_fn(|i| {
            let p = glam::Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let v = matrix.project_point3(p);
            (v.is_finite() && (0. ..=1.).contains(&v.z)).then(|| {
                egui::pos2(
                    rect.center().x + v.x * rect.width() * 0.5,
                    rect.center().y - v.y * rect.height() * 0.5,
                )
            })
        });
        let painter = ui.painter().with_clip_rect(rect);
        let stroke = egui::Stroke::new(1.5, Color32::LIGHT_BLUE);
        for i in 0..8 {
            for axis in [1, 2, 4] {
                if i & axis == 0
                    && let (Some(a), Some(b)) = (corners[i], corners[i | axis])
                {
                    painter.line_segment([a, b], stroke);
                }
            }
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
    validate_layout(members, gap)?;
    let descriptors = modules(members);
    let layout = crate::constraints::stacking::plan(&descriptors, gap)?;
    let exterior_catalog = if members.iter().any(|m| !m.exterior.is_empty()) {
        Some(Catalog::scan(root.to_path_buf())?)
    } else {
        None
    };
    let rules = crate::display::Rules::load(&home.join("config/display-rules.toml"))?;
    let mut loader = crate::scene::LoadCache::new(root, &rules);
    let mut geometry = crate::scene::MeshPool::default();
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
        let mut scene = Scene::load_cached(&mut loader, &originals, member.name.clone(), true)
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
        geometry.share(&mut scene)?;
        scenes.push((scene, member.placement, p.sustainer_angle));
    }
    crate::preview::check(Some(cancel))?;
    let mut closed: Vec<_> = scenes.iter().map(|(s, _, _)| s.clone()).collect();
    for scene in &mut closed {
        if crate::sustainer::lid_angle(scene).is_some() {
            crate::sustainer::set_lid_angle(scene, 0.)?;
        }
    }
    let mut shifts = crate::constraints::stacking::shifts(&layout, &descriptors, &closed)?;
    let mut lift = vec![0f32; members.len()];
    for &i in &layout.order {
        if let Some(catalog) = &exterior_catalog {
            let member = &members[i];
            let components = catalog.exterior_components(
                &member.preset.selection,
                &member.exterior,
                member.preset.assembly_kind == AssemblyKind::Sustainer,
            )?;
            let d = crate::constraints::stacking::datum(&closed[i])?;
            let tower_height = d.top + shifts[i].y;
            let parts = closed[i].append_exterior(&mut loader, &components)?;
            crate::constraints::exterior::attach(
                &mut closed[i],
                &member.preset.selection,
                &member.exterior,
                &parts,
                tower_height,
            )?;
            if let Some(&below) = layout.supports[i].first() {
                lift[i] = lift[below];
                if layout.supports[i]
                    .iter()
                    .any(|&j| (lift[j] - lift[i]).abs() > 0.25)
                {
                    return Err(
                        "底座使两侧支承高度不同 / Base heights differ across supports".into(),
                    );
                }
            } else if member.exterior.base != crate::catalog::exterior::Base::None {
                lift[i] = (d.bottom - closed[i].closed_extent()?.0.y).max(0.);
            }
            geometry.share(&mut closed[i])?;
            shifts[i].y += lift[i];
            scenes[i].0 = closed[i].clone();
        }
    }
    if exterior_catalog.is_some() {
        crate::constraints::exterior::space_groups(&layout, &closed, &mut shifts)?;
    }
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
            colors: self.global_colors.clone(),
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
        let indices = self.export_indices();
        if indices.is_empty() {
            self.status = "请选择导出成员 / Select members to export".into();
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
            let _ = tx.send(export_members(
                &root,
                &home,
                &target,
                format,
                &saved,
                Some(&indices),
            ));
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
    selection: Option<&[usize]>,
) -> Result<PathBuf, String> {
    if selection.is_some_and(|ids| ids.is_empty() || ids.iter().any(|&i| i >= saved.members.len()))
    {
        return Err("无效的导出成员 / Invalid export members".into());
    }
    let included = |i: usize| selection.is_none_or(|ids| ids.contains(&i));
    let library = root.canonicalize().map_err(|e| e.to_string())?;
    let parent = target
        .parent()
        .ok_or("Invalid destination")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if parent.starts_with(&library) || target.exists() {
        return Err("目标已存在或位于模型库内 / Existing destination or model library".into());
    }
    let catalog = Catalog::scan(root.to_path_buf())?;
    let extras: Vec<_> = saved
        .members
        .iter()
        .map(|m| {
            catalog.exterior_components(
                &m.preset.selection,
                &m.exterior,
                m.preset.assembly_kind == AssemblyKind::Sustainer,
            )
        })
        .collect::<Result<_, _>>()?;
    let total: u32 = saved
        .members
        .iter()
        .enumerate()
        .filter(|(i, _)| included(*i))
        .map(|(i, m)| {
            extras[i].iter().map(|c| c.bom.qty).sum::<u32>()
                + m.preset
                    .bom
                    .iter()
                    .filter(|b| b.enabled)
                    .map(|b| b.qty)
                    .sum::<u32>()
        })
        .sum();
    if total == 0 || total > 1000 {
        return Err("组合单次导出需为 1–1000 份 / Export requires 1–1000 copies".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".vault-combination-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    for (index, member) in saved.members.iter().enumerate() {
        if !included(index) {
            continue;
        }
        let p = &member.preset;
        let mut items = p.bom.clone();
        items.extend(extras[index].iter().map(|c| c.bom.clone()));
        if !items.iter().any(|b| b.enabled) {
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
            &items,
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
    let manifest: Vec<_> = saved.members.iter().enumerate().filter(|(i,_)|included(*i)).map(|(i,m)|
        serde_json::json!({"member_index":i+1,"name":m.name,"specification":format!("{}{}-{}",m.preset.selection.width,m.preset.selection.depth,m.preset.selection.height)})
    ).collect();
    std::fs::write(
        staging.path().join("exported-members.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut hardware = String::from(
        "整体配件五金清单 / Exterior hardware\n尺寸只包含预览零件；采购件按手册核对。\n",
    );
    for (i, m) in saved.members.iter().enumerate() {
        if !included(i) {
            continue;
        }
        hardware.push_str(&format!("\n{}\n", m.name));
        for h in m.exterior.hardware(&m.preset.selection) {
            hardware.push_str(&format!("{}\n", h.line()));
        }
    }
    std::fs::write(staging.path().join("hardware.txt"), hardware).map_err(|e| e.to_string())?;
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
                exterior: Default::default(),
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
        if self.global_colors_draft.is_some() {
            if let Some(error) = self.global_color_preview.error() {
                let _ = std::fs::write(folder.join("combination-smoke-error.txt"), error);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            if !self.global_color_preview.ready() {
                return;
            }
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
            if [10, 12, 13, 15].contains(&self.smoke_stage) {
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
            if [17, 19, 21].contains(&self.smoke_stage) && self.members.len() != 3 {
                return Err("Workflow undo/redo member count failed".into());
            }
            if [18, 20].contains(&self.smoke_stage) && self.members.len() != 1 {
                return Err("Workflow delete history failed".into());
            }
            if [27, 29].contains(&self.smoke_stage)
                && (self.members.len() != 4
                    || self.members[0].name == self.members[3].name
                    || self.members[0].preset.label.text != self.members[3].preset.label.text)
            {
                return Err("Copy data or unique name failed".into());
            }
            if self.smoke_stage == 28 && self.members.len() != 3 {
                return Err("Copy undo failed".into());
            }
            if self.smoke_stage == 24 {
                let c = self.members[1].preset.print_config.as_ref().unwrap();
                if c.slot(crate::label::PATH) != 2 || !c.follow_global_colors {
                    return Err("Overall theme did not reach following member".into());
                }
            }
            if self.smoke_stage == 26 {
                let a = self.members[0].preset.print_config.as_ref().unwrap();
                let b = self.members[1].preset.print_config.as_ref().unwrap();
                if a.follow_global_colors
                    || a.slot(crate::label::PATH) != 5
                    || b.slot(crate::label::PATH) != 3
                {
                    return Err("Custom color override leaked or was overwritten".into());
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
            5 => self.apply_spatial_move(2, 1, Side::Above, ctx),
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
                self.select_member_in_combination(1, ctx);
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
                    exterior: Default::default(),
                    name: "支承盒子".into(),
                    placement: Default::default(),
                    preset: p,
                }];
                self.restore_member(0);
                self.combination_view_state = None;
                self.combination_view = true;
                self.member_selection.clear();
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
            14 => {
                self.combination_spec = None;
                self.apply_spatial_move(1, 0, Side::Ground, ctx);
            }
            15 => self.undo_workflow(ctx),
            16 => self.remove_members(&[1, 2], ctx),
            17 => self.undo_workflow(ctx),
            18 => self.remove_members(&[1, 2], ctx),
            19 => self.undo_workflow(ctx),
            20 => self.redo_workflow(ctx),
            21 => self.undo_workflow(ctx),
            22 => {
                self.member_selection = (0..self.members.len()).collect();
                self.batch_colors(true);
                self.open_global_colors();
            }
            23 => {
                let mut theme = self.global_colors_draft.take().unwrap();
                theme.roles.insert("shell".into(), 4);
                theme.roles.insert("label".into(), 2);
                theme.roles.insert("text".into(), 1);
                self.apply_global_theme(theme).unwrap();
            }
            24 => {
                self.select_member_in_combination(0, ctx);
            }
            25 => {
                self.export_options.plate.follow_global_colors = false;
                self.export_options
                    .plate
                    .set_part_slot(crate::label::PATH, 5);
                self.commit_member();
                let mut theme = self.global_colors.clone();
                theme.roles.insert("label".into(), 3);
                self.apply_global_theme(theme).unwrap();
            }
            26 => {
                self.member_selection = [1].into();
                self.export_scope = workflow::ExportScope::Selected;
                self.export_options.format = crate::export::Format::ThreeMf;
                self.export_open = true;
            }

            27 => {
                self.export_open = false;
                self.select_member_in_combination(0, ctx);
                self.add_member(None, false, ctx);
                let draft = self.combination_spec.take().unwrap();
                if let Err(e) = self.apply_member_spec(&draft, ctx) {
                    let _ = std::fs::write(folder.join("combination-smoke-error.txt"), e);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            28 => self.undo_workflow(ctx),
            29 => self.redo_workflow(ctx),
            30 => {
                self.open_global_colors();
                self.global_color_preview
                    .request(&self.catalog, &self.home, AssemblyKind::Drawer);
                let draft = self.global_colors_draft.as_mut().unwrap();
                let module = "01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F.stl";
                draft
                    .components
                    .insert(crate::catalog::colors::component(module), 3);
                draft
                    .components
                    .insert(crate::catalog::colors::component(crate::label::PATH), 2);
                draft
                    .component_text
                    .insert(crate::catalog::colors::component(crate::label::PATH), 6);
                draft.components.insert(
                    crate::catalog::colors::component(crate::label::SUSTAINER_PATH),
                    2,
                );
                draft.component_text.insert(
                    crate::catalog::colors::component(crate::label::SUSTAINER_PATH),
                    6,
                );
            }
            31 => self.global_color_preview.request(
                &self.catalog,
                &self.home,
                AssemblyKind::Sustainer,
            ),
            32 | 34 => {
                if let Err(e) = self.global_color_preview.smoke_select_exploded_label() {
                    let _ = std::fs::write(folder.join("combination-smoke-error.txt"), e);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            33 => {
                self.global_color_preview
                    .request(&self.catalog, &self.home, AssemblyKind::Drawer)
            }
            35 => {
                self.global_color_preview.isolate = true;
                self.global_color_preview.explode(1.);
            }
            _ => {
                let _ = std::fs::write(
                    folder.join("combination-smoke-result.txt"),
                    format!(
                        "PASS: overall workflow; color inheritance and custom override; batch selection and scoped export review; delete undo/redo; two pages; single-member label editing; overall size change; incompatible resize and unsupported move rejected; saved overall camera; 3D object controls; confirmed add/remove; L/S+S and XL/S+M groups; supporting lid closure; size dialog; pair ground move/undo/remove; detailed overall colors with S3 drawer and box previews; exploded ray selection and isolated color editing. GPU: {}. Parts: {}",
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
        let mut group: Vec<_> = layout
            .supports
            .iter()
            .enumerate()
            .filter(|(i, below)| {
                members[*i].placement.level == members[index].placement.level && *below == support
            })
            .map(|(i, _)| i)
            .collect();
        if group.len() == 2 {
            group.sort_by_key(|&i| members[i].placement.column);
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
#[cfg(test)]
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
#[cfg(test)]
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

fn unique_member_name(members: &[Member], requested: &str) -> String {
    if !members.iter().any(|m| m.name == requested) {
        return requested.to_owned();
    }
    (2..)
        .map(|i| format!("{requested} ({i})"))
        .find(|name| !members.iter().any(|m| &m.name == name))
        .unwrap()
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
                    exterior: Default::default(),
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
    fn supported_pairs_can_be_detached_and_removed_without_losing_configuration() {
        let mut base = preset();
        base.selection.width = "L".into();
        let mut a = preset();
        a.label.text = "A".into();
        let mut b = preset();
        b.label.text = "B".into();
        let members = vec![
            Member {
                exterior: Default::default(),
                name: "base".into(),
                placement: Default::default(),
                preset: base,
            },
            Member {
                exterior: Default::default(),
                name: "A".into(),
                placement: Placement {
                    level: 1,
                    ..Default::default()
                },
                preset: a,
            },
            Member {
                exterior: Default::default(),
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
            colors: Default::default(),
            preset: a.clone(),
            members: vec![
                Member {
                    exterior: Default::default(),
                    name: "A".into(),
                    placement: Default::default(),
                    preset: a,
                },
                Member {
                    exterior: Default::default(),
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
            colors: Default::default(),
            preset: p.clone(),
            members: vec![
                Member {
                    exterior: Default::default(),
                    name: "A".into(),
                    placement: Default::default(),
                    preset: p,
                },
                Member {
                    exterior: Default::default(),
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
        export_members(
            &library,
            &home,
            &target,
            crate::export::Format::Stl,
            &saved,
            None,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_dir(target.join("01-S3-1F")).unwrap().count(),
            2
        );
        assert_eq!(
            std::fs::read_dir(target.join("02-S3-1F")).unwrap().count(),
            3
        );
        assert!(target.join("combination.toml").is_file());
        let subset = temp.path().join("selected-export");
        export_members(
            &library,
            &home,
            &subset,
            crate::export::Format::Stl,
            &saved,
            Some(&[1]),
        )
        .unwrap();
        assert!(!subset.join("01-S3-1F").exists());
        assert_eq!(
            std::fs::read_dir(subset.join("02-S3-1F")).unwrap().count(),
            3
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(subset.join("exported-members.json")).unwrap())
                .unwrap();
        assert_eq!(manifest.as_array().unwrap().len(), 1);
        assert_eq!(manifest[0]["name"], "B");
        assert!(
            export_members(
                &library,
                &home,
                &temp.path().join("empty"),
                crate::export::Format::Stl,
                &saved,
                Some(&[])
            )
            .is_err()
        );
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
                &saved,
                None
            )
            .is_err()
        );
        assert!(!library.join("bad").exists());
    }
}
