use super::*;
use crate::{mesh::Mesh, scene::Target};
use std::sync::{Arc, Weak, mpsc};

#[derive(Default)]
pub(super) struct State {
    pending: Option<(Weak<Mesh>, mpsc::Receiver<Option<f64>>)>,
    measured: Option<(Weak<Mesh>, Option<f64>)>,
}

impl State {
    fn volume(&mut self, mesh: &Arc<Mesh>, ctx: &egui::Context) -> Option<Option<f64>> {
        if let Some((key, receiver)) = &self.pending {
            let result = match receiver.try_recv() {
                Ok(volume) => Some(volume),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(None),
            };
            if let Some(result) = result {
                self.measured = Some((key.clone(), result));
                self.pending = None;
            }
        }
        let key = Arc::downgrade(mesh);
        if let Some((cached, volume)) = &self.measured
            && cached.ptr_eq(&key)
        {
            return Some(*volume);
        }
        if self.pending.is_none() {
            let source = Arc::clone(mesh);
            let ctx = ctx.clone();
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(source.volume_mm3());
                ctx.request_repaint();
            });
            self.pending = Some((key, receiver));
        }
        None
    }
}

fn field<R>(ui: &mut egui::Ui, title: &str, value: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.add_sized(
            [66., 20.],
            egui::Label::new(RichText::new(title).weak()).halign(egui::Align::Min),
        );
        value(ui)
    })
    .inner
}

fn row(ui: &mut egui::Ui, title: &str, value: &str) -> egui::Response {
    field(ui, title, |ui| ui.add(egui::Label::new(value).wrap()))
}

fn details_title(path: &str, file: &str, zh: bool) -> String {
    if let Some(spec) = file.strip_suffix(" Facade.stl")
        && path.starts_with(crate::sustainer::PREFIX)
    {
        return format!("{} {spec}", t(zh, "翻盖箱面板", "Case facade"));
    }
    workflow::color_component_name(path, zh)
}

impl App {
    pub(super) fn part_details_window(&mut self, ctx: &egui::Context) {
        if self.tab != 0
            || self.tower.print_list
            || self.dimensions
            || self.gridfinity_open
            || self.divider_editor.open
            || self.sustainer_divider_editor.open
            || self.preview_modes.mode != preview_modes::Mode::Exploded
        {
            return;
        }
        let Some(part) = self
            .selected
            .and_then(|index| self.scene.parts.get(index))
            .filter(|part| part.visible)
        else {
            return;
        };
        let z = self.zh;
        let path = part.pose.path.clone();
        let file = path.rsplit('/').next().unwrap_or(&path);
        let id = file
            .strip_suffix(" Facade.stl")
            .filter(|_| path.starts_with(crate::sustainer::PREFIX))
            .and_then(|spec| spec.strip_suffix('F'))
            .and_then(|spec| {
                let (family, height) = spec.split_at(spec.find(char::is_numeric)?);
                Some(format!("sf-{family}-{height}"))
            })
            .unwrap_or_else(|| path.clone());
        let size = part.mesh.size;
        let dimension = |n: f32| format!("{n:.1}").trim_end_matches(".0").to_owned();
        let same = self
            .scene
            .parts
            .iter()
            .filter(|p| p.pose.path == path && p.pose.body == part.pose.body)
            .count();
        let target = Target::of(part);
        let member = (self.scene_key == "combination")
            .then(|| self.members.get(target.member))
            .flatten();
        let location = member.map_or_else(
            || self.scene.title.clone(),
            |member| {
                let level = member.placement.level + 1;
                let preset = &member.preset;
                let kind = if preset.assembly_kind == AssemblyKind::Sustainer {
                    t(z, "翻盖箱", "Case")
                } else {
                    t(z, "抽屉", "Drawer")
                };
                let mut location = if z {
                    format!(
                        "第 {level} 层 · {kind} {}{}",
                        preset.selection.width, preset.selection.depth
                    )
                } else {
                    format!(
                        "Layer {level} · {kind} {}{}",
                        preset.selection.width, preset.selection.depth
                    )
                };
                if let Some(drawer) = target.drawer {
                    location.push_str(&format!(" · {} {}", t(z, "子抽屉", "Drawer"), drawer + 1));
                }
                location
            },
        );
        let config = member
            .and_then(|m| {
                target
                    .drawer
                    .and_then(|idx| m.preset.drawers.get(idx))
                    .unwrap_or(&m.preset)
                    .print_config
                    .as_ref()
            })
            .unwrap_or(&self.export_options.plate);
        let slot = config.slot(&path);
        let appearance = config.appearance(slot);
        let material = self.material_library.name(appearance.material).to_owned();
        let density = self.material_library.density(appearance.material);
        let color = tower::colors::part_color_label(appearance, z);
        let [r, g, b] = appearance.rgb;
        let group = tower::colors::part_group_name(&self.global_colors, &path, z);
        let mass = match density {
            Some(density) => match self.part_details.volume(&part.mesh, ctx) {
                None => t(z, "计算中…", "Calculating…").into(),
                Some(Some(volume)) => format!("≈ {:.1} g", volume * density / 1000.),
                Some(None) => "—".into(),
            },
            None => "—".into(),
        };

        let bounds = ctx.content_rect();
        let mut open = true;
        let mut focus = false;
        let title = details_title(&path, file, z);
        egui::Window::new("selected-part-details")
            .id(egui::Id::new("selected-part-details"))
            .title_bar(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16., 210.))
            .movable(false)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_width(288.);
                ui.horizontal(|ui| {
                    let title_width = ui.available_width() - 26. - ui.spacing().item_spacing.x;
                    ui.allocate_ui_with_layout(
                        egui::vec2(title_width, 24.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&title).size(16.).strong())
                                    .truncate(),
                            )
                            .on_hover_text(&title);
                        },
                    );
                    if ui
                        .add_sized([26., 24.], egui::Button::new("×").frame(false))
                        .on_hover_text(t(z, "关闭详情", "Close details"))
                        .clicked()
                    {
                        open = false;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("selected-part-fields")
                    .max_height((bounds.height() - 300.).max(175.))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8., 7.);
                        ui.weak(t(z, "零件", "Part"));
                        field(ui, "ID", |ui| {
                            ui.add_sized(
                                [126., 18.],
                                egui::Label::new(RichText::new(&id).monospace())
                                    .halign(egui::Align::Min)
                                    .truncate(),
                            )
                            .on_hover_text(&id);
                            if ui.small_button(t(z, "复制", "Copy")).clicked() {
                                ctx.copy_text(id.clone());
                            }
                        });
                        row(ui, t(z, "文件", "File"), file);
                        row(
                            ui,
                            t(z, "尺寸", "Size"),
                            &format!(
                                "{} × {} × {} mm",
                                dimension(size.x),
                                dimension(size.y),
                                dimension(size.z)
                            ),
                        );
                        ui.add_space(9.);
                        ui.weak(t(z, "打印", "Print"));
                        field(ui, t(z, "分组", "Group"), |ui| {
                            let (dot, _) = ui.allocate_exact_size(
                                egui::Vec2::splat(12.),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle(
                                dot.center(),
                                4.,
                                Color32::from_rgb(r, g, b),
                                ui.visuals().widgets.noninteractive.bg_stroke,
                            );
                            ui.label(&group);
                        });
                        row(ui, t(z, "耗材", "Filament"), &format!("{material} · {color}"));
                        row(ui, t(z, "实体估重", "Solid weight"), &mass).on_hover_text(t(
                            z,
                            "按当前网格体积和所选耗材密度估算，未计入切片的填充、支撑和耗材损耗。",
                            "Estimated from the displayed mesh volume and filament density, before slicing.",
                        ));
                        ui.add_space(9.);
                        ui.weak(t(z, "数量与位置", "Quantity & location"));
                        row(ui, t(z, "同类", "Matching"), &format!("{same} {}", t(z, "件", "pcs")))
                            .on_hover_text(t(
                                z,
                                "同一源文件与实体编号的展示实例",
                                "Displayed copies with the same source file and body number",
                            ));
                        row(ui, t(z, "位置", "Location"), &location);
                    });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    focus = ui.button(t(z, "聚焦零件", "Focus part")).clicked();
                    if self.scene_key == "combination" {
                        self.tower_preview_color_menu(ui);
                    }
                });
            });
        if !open {
            self.selected = None;
            self.last_hit = None;
        } else if focus {
            self.focus_preview_part();
            ctx.request_repaint();
        }
    }
}
