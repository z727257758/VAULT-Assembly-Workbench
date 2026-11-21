use super::*;
use crate::sustainer_dividers;

impl App {
    pub(super) fn open_sustainer_divider_editor(&mut self, ctx: &egui::Context) {
        if self.assembly_kind != AssemblyKind::Sustainer
            || !sustainer_dividers::supported(&self.selection, self.sustainer_options)
        {
            return;
        }
        let target = crate::scene::Target {
            member: self.active_member,
            drawer: None,
        };
        let member_preview = !self.combination_view
            && inline_edit::target_preset(&self.members, target)
                .is_some_and(|p| p.assembly_kind == AssemblyKind::Sustainer);
        if member_preview {
            self.open_inline_target(target.member, None, ctx);
        }
        self.tab = 0;
        self.tower.print_list = false;
        self.preview_modes.mode = preview_modes::Mode::Assembly;
        self.divider_model = Default::default();
        self.sustainer_divider_editor.open = true;
        if !member_preview && !self.divider_scene_matches() {
            self.assembly(ctx);
        }
        ctx.request_repaint();
    }

    pub(super) fn close_sustainer_divider_editor(&mut self) {
        self.sustainer_divider_editor
            .close(&self.sustainer_dividers);
        self.divider_model = Default::default();
    }

    fn divider_scene_matches(&self) -> bool {
        self.scene.assembly
            && if self.combination_view {
                self.scene_key == "combination"
            } else {
                self.scene_key == self.sustainer_options.key(&self.selection)
            }
    }

    pub(super) fn sustainer_divider_workspace(&mut self, ui: &mut egui::Ui) {
        if !sustainer_dividers::supported(&self.selection, self.sustainer_options) {
            self.close_sustainer_divider_editor();
            ui.ctx().request_repaint();
            return;
        }
        let z = self.zh;
        let spec = sustainer_dividers::spec(&self.selection, self.sustainer_options);
        let geometry = self
            .scene
            .interior_geometry(self.active_member)
            .filter(|g| self.divider_scene_matches() && g.specification == spec)
            .cloned();
        let mut layout = self.sustainer_dividers.clone();
        let (model_rect, card_rect) = workspace::model_and_plan_rects(ui.max_rect());
        let mut model = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("divider-model")
                .max_rect(model_rect),
        );
        model.set_clip_rect(model_rect);
        if let Some(g) = &geometry {
            self.divider_model_ui(&mut model, g, &mut layout);
        }

        let mut card = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("divider-inspector")
                .max_rect(card_rect),
        );
        card.set_clip_rect(card_rect);
        let mut close = false;
        ui_style::floating_frame(&card).show(&mut card, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let size = format!(
                    "{}{} {}",
                    self.selection.width, self.selection.depth, self.selection.height
                );
                let name = t(z, "翻盖箱", "Case");
                if self.combination_view
                    && let Some(member) = self.members.get(self.active_member)
                {
                    let level = member.placement.level + 1;
                    ui.strong(if z {
                        format!("第 {level} 层 · {name} {size}")
                    } else {
                        format!("Layer {level} · {name} {size}")
                    });
                } else {
                    ui.strong(format!("{name} {size}"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let done = ui.add(ui_style::primary_button(t(z, "完成", "Done")));
                    if self.smoke {
                        self.divider_editor_smoke.done_rect = Some(done.rect);
                    }
                    close = done.clicked();
                });
            });
            self.interior_opacity_ui(ui);
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("divider-properties")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let failed =
                        self.failed_preview.is_some() && !self.busy && !self.preview.has_pending();
                    if !failed || geometry.is_some() {
                        self.sustainer_divider_editor
                            .panel(ui, geometry.as_ref(), &mut layout, z);
                    }
                    if let Some(error) = &self.sustainer_divider_editor.error {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    if failed {
                        ui.colored_label(ui.visuals().error_fg_color, &self.status);
                        if ui.button(t(z, "重新加载", "Reload")).clicked() {
                            self.assembly(ui.ctx());
                        }
                    }
                });
        });
        if layout != self.sustainer_dividers {
            match layout.validate(&self.catalog, &self.selection, self.sustainer_options) {
                Ok(()) => {
                    self.sustainer_divider_editor.error = None;
                    self.sustainer_dividers = layout;
                    self.assembly(ui.ctx());
                }
                Err(error) => self.sustainer_divider_editor.error = Some(error),
            }
        }
        if close {
            self.close_sustainer_divider_editor();
            ui.ctx().request_repaint();
        }
    }
}
