use super::*;
use crate::constraints::{AssemblyRules, PartId};
use crate::scene::Target;
use crate::ui_style;
mod smoke;

enum Command {
    Edit,
    Label,
    Motion(PartId, f32),
    Layer(tower::Action),
    MoveMember,
    AddMember(AssemblyKind),
}

fn entry(
    ui: &mut egui::Ui,
    command: &mut Option<Command>,
    enabled: bool,
    text: &str,
    action: Command,
) {
    if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
        *command = Some(action);
        ui.close();
    }
}

fn dimension_toggle_id() -> egui::Id {
    egui::Id::new("viewport-member-dimensions")
}

impl App {
    pub(super) fn viewport_shows_dimensions(&self, ctx: &egui::Context) -> bool {
        ctx.data(|data| data.get_temp::<bool>(dimension_toggle_id()))
            .unwrap_or(!self.inline_edit.open)
    }

    pub(super) fn viewport_context_menu(&mut self, response: &egui::Response) {
        let ctx = &response.ctx;
        let popup = egui::Popup::default_response_id(response);
        let target_key = response.id.with("context-part");
        let ready = self.scene_key == "combination"
            && self.combination_view
            && !self.busy
            && !self.preview.has_pending()
            && self.export_rx.is_none();
        if !ready {
            egui::Popup::close_id(ctx, popup);
            return;
        }
        if response.secondary_clicked() {
            let hit = response.interact_pointer_pos().and_then(|pos| {
                let (origin, direction) = self.camera.ray(pos, response.rect);
                self.scene.pick(origin, direction, self.explosion)
            });
            let Some(hit) = hit else {
                egui::Popup::close_id(ctx, popup);
                return;
            };
            self.select_combination_part(hit.part, ctx);
            self.last_hit = Some(hit);
            let id = PartId::of(&self.scene.parts[hit.part]);
            ctx.data_mut(|data| data.insert_temp(target_key, id));
        } else if !response.context_menu_opened() {
            return;
        }
        let Some(id) = ctx.data(|data| data.get_temp::<PartId>(target_key)) else {
            return;
        };
        let Some(part) = self
            .selected
            .and_then(|index| self.scene.parts.get(index))
            .filter(|part| PartId::of(part) == id)
        else {
            egui::Popup::close_id(ctx, popup);
            return;
        };
        let mut target = Target::of(part);
        if let Some(member) = self.members.get(target.member) {
            if target.drawer.is_none() && !member.preset.drawers.is_empty() {
                target.drawer = self.active_drawer.or(Some(0));
            }
        } else {
            egui::Popup::close_id(ctx, popup);
            return;
        }
        let mut command = None;
        response.context_menu(|ui| self.viewport_menu_contents(ui, target, &mut command));
        let Some(command) = command else {
            return;
        };
        let result = match command {
            Command::Edit => {
                self.tower_edit_slot(target.member, target.drawer, ctx);
                Ok(())
            }
            Command::Label => {
                self.open_preview_label(None, ctx);
                Ok(())
            }
            Command::Motion(child, goal) => self.animate_joint_to(&child, goal, ctx),
            Command::Layer(action) => self.tower_action(action, ctx).map(|()| {
                self.tower
                    .reveal_layer(self.members[self.active_member].placement.level);
            }),
            Command::MoveMember => {
                self.close_inline_editor(ctx);
                self.combination_move_source = Some(target.member);
                Ok(())
            }
            Command::AddMember(kind) => {
                self.add_member(Some(kind), true, ctx);
                Ok(())
            }
        };
        if let Err(error) = result {
            self.status = error.clone();
            self.tower.error = error;
        }
        ctx.request_repaint();
    }

    fn viewport_menu_contents(
        &mut self,
        ui: &mut egui::Ui,
        target: Target,
        command: &mut Option<Command>,
    ) {
        let z = self.zh;
        let member = &self.members[target.member];
        let level = member.placement.level;
        let preset = inline_edit::target_preset(&self.members, target);
        let layer = if z {
            format!("第 {} 层", level + 1)
        } else {
            format!("Layer {}", level + 1)
        };
        let rows = tower::rows(&self.members).ok();
        let row = rows
            .as_ref()
            .and_then(|rows| rows.iter().find(|row| row.level == level));
        let side = if row.is_some_and(|row| row.members.len() > 1) {
            if member.placement.column == 0 {
                t(z, "左 · ", "Left · ")
            } else {
                t(z, "右 · ", "Right · ")
            }
        } else {
            ""
        };
        let title = format!(
            "{layer} · {side}{} {}{} {}",
            member.preset.assembly_kind.label(z),
            member.preset.selection.width,
            member.preset.selection.depth,
            preset.map_or(member.preset.selection.height.as_str(), |p| &p
                .selection
                .height),
        );
        ui.set_min_width(205.);
        ui.label(egui::RichText::new(title).small().weak());
        entry(
            ui,
            command,
            preset.is_some(),
            t(z, "编辑", "Edit"),
            Command::Edit,
        );
        if let Ok(mechanism) = AssemblyRules::mechanism(&self.scene, target) {
            let (open, close) = animation::motion_labels(mechanism.kind, z);
            let goal = self.motion_toggle_goal(&mechanism);
            entry(
                ui,
                command,
                goal.is_some(),
                if goal == Some(mechanism.min) {
                    close
                } else {
                    open
                },
                Command::Motion(mechanism.child, goal.unwrap_or(mechanism.min)),
            );
        }
        entry(
            ui,
            command,
            preset.is_some(),
            t(z, "编辑标签", "Edit label"),
            Command::Label,
        );
        self.tower_preview_color_menu(ui);
        ui.separator();

        if let Some(rows) = &rows
            && let Some(row) = row
            && let Some(top) = rows.last().map(|row| row.level)
        {
            for (enabled, label, destination) in [
                (
                    level < top,
                    t(z, "上移一层", "Move up one layer"),
                    level.saturating_add(1),
                ),
                (
                    level > 0,
                    t(z, "下移一层", "Move down one layer"),
                    level.saturating_sub(1),
                ),
                (level < top, t(z, "移到最上面", "Move to top"), top),
                (level > 0, t(z, "移到最下面", "Move to bottom"), 0),
            ] {
                entry(
                    ui,
                    command,
                    enabled,
                    label,
                    Command::Layer(tower::Action::Move(level, destination)),
                );
            }
            ui.separator();
            let can_add = self.members.len() + row.members.len() <= 64;
            entry(
                ui,
                command,
                can_add,
                t(z, "在上方添加相同的层", "Duplicate layer above"),
                Command::Layer(tower::Action::Duplicate(level)),
            );
            for (kind, label) in [
                (
                    AssemblyKind::Drawer,
                    t(z, "在上方添加抽屉层", "Add drawer layer above"),
                ),
                (
                    AssemblyKind::Sustainer,
                    t(z, "在上方添加翻盖箱层", "Add box layer above"),
                ),
            ] {
                entry(
                    ui,
                    command,
                    can_add,
                    label,
                    Command::Layer(tower::Action::Add(kind, Some(level))),
                );
            }
            ui.separator();
            ui.add_enabled_ui(rows.len() > 1, |ui| {
                ui.menu_button(
                    egui::RichText::new(t(z, "删除这一层…", "Delete this layer…"))
                        .color(ui.visuals().error_fg_color),
                    |ui| {
                        if ui
                            .add(ui_style::danger_button(t(
                                z,
                                "确认删除这一层",
                                "Confirm layer deletion",
                            )))
                            .clicked()
                        {
                            *command = Some(Command::Layer(tower::Action::Delete(level)));
                            ui.close();
                        }
                    },
                );
            });
        } else {
            entry(
                ui,
                command,
                self.members.len() > 1 && self.explosion == 0.,
                t(z, "移动对象…", "Move object…"),
                Command::MoveMember,
            );
            for (kind, label) in [
                (
                    AssemblyKind::Drawer,
                    t(z, "在上方添加抽屉…", "Add drawer above…"),
                ),
                (
                    AssemblyKind::Sustainer,
                    t(z, "在上方添加箱子…", "Add box above…"),
                ),
            ] {
                entry(
                    ui,
                    command,
                    self.members.len() < 64,
                    label,
                    Command::AddMember(kind),
                );
            }
        }
        ui.separator();
        let mut show = self.viewport_shows_dimensions(ui.ctx());
        if ui
            .add_enabled(
                self.explosion == 0.,
                egui::Checkbox::new(&mut show, t(z, "显示尺寸", "Show dimensions")),
            )
            .changed()
        {
            ui.data_mut(|data| data.insert_temp(dimension_toggle_id(), show));
            ui.close();
        }
    }
}
