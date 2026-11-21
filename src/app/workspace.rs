use super::*;
use crate::constraints::{AssemblyRules, MotionKind};
use crate::scene::Target;
use crate::ui_style;
use std::sync::Arc;

/// Cache an opened display copy while the loaded assembly and its poses match.
pub(super) struct OpenInterior {
    target: Target,
    source: usize,
    poses: Vec<(usize, [f32; 3], [f32; 3], bool)>,
    scene: Result<Arc<Scene>, String>,
}

impl OpenInterior {
    fn solve(source: &Scene, target: Target, kind: MotionKind) -> Result<Arc<Scene>, String> {
        let joint = AssemblyRules::mechanism(source, target)?;
        if joint.kind != kind || joint.max <= joint.min {
            return Err(match kind {
                MotionKind::Drawer => {
                    "抽屉没有可用的滑动行程 / Drawer has no available slide travel"
                }
                MotionKind::Lid => "箱盖没有可用的开合行程 / Lid has no available travel",
            }
            .into());
        }
        let mut scene = source.clone();
        match kind {
            MotionKind::Drawer => AssemblyRules::set_slider(&mut scene, &joint.child, joint.max)?,
            MotionKind::Lid => {
                let plan = AssemblyRules::plan_motion(source, &joint.child, joint.max)?;
                AssemblyRules::apply_motion_sample(
                    &mut scene,
                    &joint.child,
                    plan.sample(plan.duration),
                )?;
            }
        }
        Ok(Arc::new(scene))
    }

    pub(super) fn get(
        cache: &mut Option<Self>,
        source: &Scene,
        target: Target,
        kind: MotionKind,
    ) -> Result<Arc<Scene>, String> {
        let key = source.parts.as_ptr() as usize;
        let poses = || {
            source
                .parts
                .iter()
                .map(|p| (p.mesh_key(), p.pose.position, p.pose.rotation, p.visible))
        };
        if cache.as_ref().is_none_or(|old| {
            old.target != target || old.source != key || !poses().eq(old.poses.iter().copied())
        }) {
            *cache = Some(Self {
                target,
                source: key,
                poses: poses().collect(),
                scene: Self::solve(source, target, kind),
            });
        }
        cache.as_ref().unwrap().scene.clone()
    }
}

pub(super) fn model_and_plan_rects(stage: egui::Rect) -> (egui::Rect, egui::Rect) {
    let margin = (stage.width() * 0.016).clamp(6., 14.);
    let space = stage.shrink(margin);
    if space.width() < 440. && space.height() > space.width() {
        let split = space.top() + space.height() * 0.45;
        return (
            egui::Rect::from_min_max(stage.min, egui::pos2(stage.right(), split - 4.)),
            egui::Rect::from_min_max(egui::pos2(space.left(), split + 4.), space.max),
        );
    }
    let width = (space.width() * 0.30)
        .clamp(208., 328.)
        .min(space.width() * 0.46);
    let split = space.right() - width;
    (
        egui::Rect::from_min_max(stage.min, egui::pos2(split - 8., stage.bottom())),
        egui::Rect::from_min_max(egui::pos2(split, space.top()), space.max),
    )
}

impl App {
    pub(super) fn interior_opacity_ui(&mut self, ui: &mut egui::Ui) {
        let z = self.zh;
        let opacity_control = |ui: &mut egui::Ui| {
            let label = ui.label(t(z, "外部零件不透明度", "Exterior part opacity"));
            ui.spacing_mut().slider_width = (ui.available_width() - 56.).clamp(32., 160.);
            if ui
                .add(egui::Slider::new(&mut self.interior_exterior_opacity, 20..=50).suffix("%"))
                .labelled_by(label.id)
                .changed()
            {
                ui.ctx().request_repaint();
            }
        };
        let inline_width = if z { 220. } else { 245. };
        if ui.available_width() >= inline_width {
            ui.horizontal(opacity_control);
        } else {
            ui.vertical(opacity_control);
        }
    }

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
            .frame(ui_style::panel_frame(
                &ctx.style(),
                egui::Margin::symmetric(16, 8),
            ))
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
                            .add_enabled(!self.busy, ui_style::quiet_button(label))
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
        let files = self.tower_file_count();
        ui.horizontal_wrapped(|ui| {
            for (tab, print, title) in [
                (0, false, t(z, "搭建", "Build")),
                (0, true, t(z, "打印清单", "Print list")),
                (1, false, t(z, "零件库", "Parts library")),
            ] {
                let selected = self.tab == tab && (tab != 0 || self.tower.print_list == print);
                let title = if print && files > 0 {
                    format!("{title}  {files}")
                } else {
                    title.into()
                };
                if ui.add(ui_style::choice_button(selected, title)).clicked() {
                    if tab == 0 {
                        self.open_project_tab(print, ctx);
                    } else {
                        self.tab = tab;
                    }
                }
            }
        });
        if self.tab == 2 {
            ui.strong(t(z, "装配说明", "Manuals"));
        }
        ui.separator();
    }

    pub(super) fn open_project_tab(&mut self, print: bool, ctx: &egui::Context) {
        if !self.combination_view && !self.members.is_empty() {
            self.show_combination(ctx);
        }
        self.tab = 0;
        self.tower.print_list = print;
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

    pub(super) fn stage_controls(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let z = self.zh;
        egui::Area::new(egui::Id::new("stage-project-hud"))
            .order(egui::Order::Middle)
            .fixed_pos(rect.left_top() + egui::vec2(16., 16.))
            .show(ctx, |ui| {
                ui_style::floating_frame(ui).show(ui, |ui| {
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
                                preview_modes::Mode::Layers => t(z, "分层预览", "Layer preview"),
                                _ => t(z, "三维装配预览", "3D assembly preview"),
                            }
                        });
                    });
                    if !self.busy && !self.preview.has_pending() && self.failed_preview.is_some() {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            t(z, "预览更新失败", "Preview update failed"),
                        )
                        .on_hover_text(&self.status);
                        if ui
                            .add(ui_style::quiet_button(t(z, "重试", "Retry")))
                            .clicked()
                        {
                            self.assembly(ctx);
                        }
                    }
                    if !self.scene.assembly
                        && !self.bom.is_empty()
                        && ui
                            .add(ui_style::quiet_button(t(
                                z,
                                "← 装配预览",
                                "← Assembly view",
                            )))
                            .clicked()
                    {
                        if !self.combination_view && !self.members.is_empty() {
                            self.open_project_tab(self.tower.print_list, ctx);
                        } else {
                            self.assembly(ctx);
                        }
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
                ui_style::floating_frame(ui).show(ui, |ui| {
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
        let card = egui::Rect::from_center_size(
            bounds.center(),
            bounds.size().min(egui::vec2(420., 240.)),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
            ui_style::card_frame(ui, false).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.);
                    ui.heading(t(
                        z,
                        "从一个抽屉，搭出你的收纳塔",
                        "Build your storage, one layer at a time",
                    ));
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
                        .add_enabled_ui(!self.busy, |ui| {
                            ui.add_sized(
                                [160., 40.],
                                ui_style::primary_button(t(z, "打开模型库", "Open model library")),
                            )
                        })
                        .inner
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
        });
    }
}
