//! The model and the plan edit one drawer layout, committed once per completed gesture.
use super::*;
use crate::gridfinity::{self, Layout};

impl App {
    pub(super) fn open_bin_editor(&mut self, ctx: &egui::Context) {
        self.close_bin_editor();
        let target = crate::scene::Target {
            member: self.active_member,
            drawer: self.active_drawer,
        };
        let from_part_preview =
            !self.combination_view && inline_edit::target_preset(&self.members, target).is_some();
        if from_part_preview {
            let selected = self.gridfinity_selected;
            self.open_inline_target(target.member, target.drawer, ctx);
            self.gridfinity_selected = selected;
        }
        self.tab = 0;
        self.tower.print_list = false;
        self.preview_modes.mode = preview_modes::Mode::Assembly;
        self.gridfinity_open = true;
        let scene_key = if self.combination_view {
            "combination".to_owned()
        } else {
            format!(
                "{}{}-{}",
                self.selection.width, self.selection.depth, self.selection.height
            )
        };
        if !from_part_preview && (!self.scene.assembly || self.scene_key != scene_key) {
            self.assembly(ctx);
        }
        ctx.request_repaint();
    }

    pub(super) fn close_bin_editor(&mut self) {
        self.gridfinity_open = false;
        self.gridfinity_editor.cancel();
        self.gridfinity_editor.error = None;
        self.gridfinity_error = None;
        self.bin_view = Default::default();
    }

    pub(super) fn bin_workspace(&mut self, ui: &mut egui::Ui) {
        let Ok(grid) = gridfinity::grid_for(&self.selection, &self.gridfinity.baseplate) else {
            self.close_bin_editor();
            ui.ctx().request_repaint();
            return;
        };
        let mut draft = self.gridfinity.clone();
        let mut selected = self.gridfinity_selected;
        let mut close = false;
        let stage = ui.max_rect();
        let margin = (stage.width() * 0.016).clamp(6., 14.);
        let space = stage.shrink(margin);
        let width = (space.width() * 0.30)
            .clamp(208., 328.)
            .min(space.width() * 0.46);
        let split = space.right() - width;
        let model_rect =
            egui::Rect::from_min_max(stage.min, egui::pos2(split - 8., stage.bottom()));
        let card_rect = egui::Rect::from_min_max(egui::pos2(split, space.top()), space.max);
        let mut model = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("bin-model")
                .max_rect(model_rect),
        );
        model.set_clip_rect(model_rect);
        self.bin_view_ui(&mut model, &mut draft, &mut selected);
        let mut card = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("bin-inspector")
                .max_rect(card_rect),
        );
        appearance::floating_frame(&card)
            .inner_margin(if width < 250. { 9. } else { 14. })
            .show(&mut card, |ui| {
                ui.set_width(ui.available_width());
                let z = self.zh;
                ui.horizontal(|ui| {
                    let spec = format!(
                        "{}{} {}",
                        self.selection.width, self.selection.depth, self.selection.height
                    );
                    let title = if let Some(member) = self.members.get(self.active_member) {
                        let level = member.placement.level + 1;
                        if z {
                            format!("第 {level} 层 · 抽屉 {spec}")
                        } else {
                            format!("Layer {level} · Drawer {spec}")
                        }
                    } else {
                        format!("{} {spec}", t(z, "抽屉", "Drawer"))
                    };
                    ui.strong(title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let done = ui.button(t(z, "完成", "Done"));
                        if self.smoke {
                            self.bin_editor_smoke.done_rect = Some(done.rect);
                        }
                        close = done.clicked();
                    });
                });
                if let Some(drawer) = self.active_drawer {
                    ui.weak(format!("{} {}", t(z, "子抽屉", "Drawer"), drawer + 1));
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("bin-properties")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        self.bin_inspector(ui, &mut draft, grid, &mut selected);
                    });
            });
        self.gridfinity_selected = selected;
        if draft != self.gridfinity {
            for bin in draft.bins.iter().filter(|b| b.label.active()) {
                draft.shared.size = draft.shared.size.max(bin.label.min_size());
            }
            draft.sync_shared();
            match draft.validate(&self.selection) {
                Ok(()) => {
                    self.gridfinity = draft;
                    self.gridfinity_error = None;
                    self.assembly(ui.ctx());
                }
                Err(error) => {
                    self.gridfinity_error = Some(error);
                    self.gridfinity_selected =
                        selected.filter(|&id| self.gridfinity.bins.iter().any(|b| b.id == id));
                }
            }
        }
        if close {
            self.close_bin_editor();
            ui.ctx().request_repaint();
        }
    }

    fn bin_inspector(
        &mut self,
        ui: &mut egui::Ui,
        layout: &mut Layout,
        grid: [u32; 2],
        selected: &mut Option<u32>,
    ) {
        let z = self.zh;
        let occupied: u32 = layout.bins.iter().map(|b| b.span[0] * b.span[1]).sum();
        let free = (grid[0] * grid[1]).saturating_sub(occupied);
        ui.horizontal(|ui| {
            ui.menu_button(t(z, "预设", "Presets"), |ui| {
                if ui
                    .add_enabled(
                        free > 0,
                        egui::Button::new(t(z, "填充空位", "Fill empty cells")),
                    )
                    .clicked()
                {
                    let previous_count = layout.bins.len();
                    while let Ok(id) = layout.add(&self.selection, None) {
                        *selected = Some(id);
                    }
                    if layout.bins.len() == previous_count {
                        self.gridfinity_error = Some(
                            t(
                                z,
                                "剩余空位容不下当前规格的盒子",
                                "The remaining space does not fit the current bin size",
                            )
                            .into(),
                        );
                    }
                    ui.close();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(if z {
                    format!("{} 个盒子 · 空余 {free} 格", layout.bins.len())
                } else {
                    format!("{} bins · {free} free cells", layout.bins.len())
                });
            });
        });
        let plan = self
            .gridfinity_editor
            .canvas(ui, layout, &self.selection, selected);
        if self.smoke {
            self.bin_editor_smoke.plan_rect = Some(plan);
        }
        ui.horizontal_wrapped(|ui| {
            let add = ui
                .add(egui::Button::new(t(z, "添加", "Add")).sense(egui::Sense::click_and_drag()))
                .on_hover_text(t(
                    z,
                    "点击放入第一个空位，或拖入简图",
                    "Click for the first available cell or drag into the plan",
                ));
            if add.drag_started_by(egui::PointerButton::Primary) {
                self.gridfinity_editor.start_stamp(layout, &self.selection);
            }
            if add.clicked() {
                match layout.add(&self.selection, None) {
                    Ok(id) => *selected = Some(id),
                    Err(error) => self.gridfinity_error = Some(error),
                }
            }
            if ui
                .add_enabled(
                    selected.is_some(),
                    egui::Button::new(t(z, "复制", "Duplicate")),
                )
                .clicked()
            {
                let template = layout
                    .bins
                    .iter()
                    .find(|b| Some(b.id) == *selected)
                    .cloned();
                match layout.add(&self.selection, template) {
                    Ok(id) => *selected = Some(id),
                    Err(error) => self.gridfinity_error = Some(error),
                }
            }
            if ui
                .add_enabled(
                    selected.is_some(),
                    egui::Button::new(t(z, "删除", "Delete")),
                )
                .clicked()
            {
                layout.bins.retain(|b| Some(b.id) != *selected);
                *selected = None;
            }
        });
        if let Some(error) = &self.gridfinity_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if self.failed_preview.is_some() && !self.busy && !self.preview.has_pending() {
            ui.colored_label(ui.visuals().error_fg_color, &self.status);
        }
        ui.separator();
        if let Some(bin) = layout.bins.iter_mut().find(|b| Some(b.id) == *selected) {
            ui.horizontal(|ui| {
                ui.strong(format!("{} {}", t(z, "盒子", "Bin"), bin.id));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let focused = self.bin_view.focus_bin == Some(bin.id);
                    let label = if focused {
                        t(z, "查看全部", "Show all")
                    } else {
                        t(z, "查看此盒", "Focus bin")
                    };
                    if ui.small_button(label).clicked() {
                        self.bin_view.focus_bin = (!focused).then_some(bin.id);
                    }
                });
            });
            egui::Grid::new("bin-cell-properties")
                .num_columns(2)
                .min_col_width(0.)
                .show(ui, |ui| {
                    for (axis, name) in [t(z, "列", "Column"), t(z, "行", "Row")]
                        .into_iter()
                        .enumerate()
                    {
                        let mut n = bin.cell[axis] + 1;
                        ui.label(name);
                        if ui
                            .add(egui::DragValue::new(&mut n).range(1..=grid[axis]))
                            .changed()
                        {
                            bin.cell[axis] = n - 1;
                        }
                        ui.end_row();
                    }
                    ui.label(t(z, "宽度", "Width"));
                    ui.add(egui::DragValue::new(&mut bin.span[0]).range(1..=grid[0]));
                    ui.end_row();
                });
            if bin.tab {
                ui.checkbox(&mut bin.label.enabled, t(z, "标签文字", "Label text"));
                if bin.label.enabled {
                    ui.add(
                        egui::TextEdit::multiline(&mut bin.label.text)
                            .desired_rows(2)
                            .char_limit(48)
                            .hint_text(t(z, "最多两行", "Up to two lines")),
                    );
                    crate::label::stroke_check_ui(ui, &bin.label.with_font(self.label_font));
                }
            }
        }
        if self.bin_view.focus_bin.is_some() && self.bin_view.focus_bin != *selected {
            self.bin_view.focus_bin = None;
        }
        if selected.is_some() {
            ui.collapsing(t(z, "标签预览", "Label preview"), |ui| {
                self.gridfinity_editor.selected_preview(
                    ui,
                    layout,
                    *selected,
                    &self.export_options.plate,
                    self.label_font,
                );
            });
        }
        ui.collapsing(t(z, "统一配置", "Shared bin settings"), |ui| {
            self.gridfinity_editor
                .shared_ui(ui, layout, &self.selection);
        });
        ui.collapsing(t(z, "操作说明", "How to edit"), |ui| {
            ui.label(t(z, "在空格拖动可添加盒子。拖动盒内可移动，选中后拖动左右边缘可改宽。", "Drag on an empty cell to add a bin. Drag a bin to move it, or drag its selected side edges to change its width."));
            ui.label(t(z, "深度和高度在统一配置中调整。双击简图中的盒子可编辑标签。", "Change depth and height in shared settings. Double-click a bin in the plan to edit its label."));
            ui.small(t(z, "Tab 切换盒子 · 方向键移动 · Shift + 左右改宽 · Ctrl + D 复制 · Delete 删除", "Tab cycles bins · Arrows move · Shift + Left/Right resizes · Ctrl + D duplicates · Delete removes"));
        });
    }
}
