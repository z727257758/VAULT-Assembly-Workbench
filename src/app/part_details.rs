use super::*;
use crate::{
    constraints::PartId,
    mesh::Mesh,
    print_plate::{
        Config,
        estimate::{GeometrySettings, ModelVolume},
    },
    scene::{Part, Target},
};
use std::sync::{Arc, Weak, mpsc};

type GeometryKey = Vec<(Weak<Mesh>, GeometrySettings)>;
type MeasuredVolumes = Vec<Option<ModelVolume>>;

struct MassSource {
    mesh: Arc<Mesh>,
    settings: GeometrySettings,
    infill: u32,
    mass_factor: f64,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) matching: Option<PartId>,
    pending: Option<(GeometryKey, mpsc::Receiver<MeasuredVolumes>)>,
    measured: Option<(GeometryKey, MeasuredVolumes)>,
}

fn matching_parts<'a>(scene: &'a Scene, part: &'a Part) -> impl Iterator<Item = &'a Part> {
    scene.parts.iter().filter(|candidate| {
        candidate.visible
            && candidate.pose.path == part.pose.path
            && candidate.pose.body == part.pose.body
    })
}

impl State {
    fn mass(&mut self, sources: Vec<MassSource>, ctx: &egui::Context) -> Option<Option<f64>> {
        if let Some((key, receiver)) = &self.pending {
            match receiver.try_recv() {
                Ok(volumes) => {
                    self.measured = Some((key.clone(), volumes));
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.measured = Some((key.clone(), vec![None; key.len()]));
                    self.pending = None;
                }
            }
        }
        let key: GeometryKey = sources
            .iter()
            .map(|source| (Arc::downgrade(&source.mesh), source.settings))
            .collect();
        if let Some((cached, volumes)) = &self.measured
            && cached.len() == key.len()
            && cached
                .iter()
                .zip(&key)
                .all(|((a, settings_a), (b, settings_b))| a.ptr_eq(b) && settings_a == settings_b)
        {
            return Some(
                sources
                    .iter()
                    .zip(volumes)
                    .map(|(source, volume)| {
                        volume.map(|v| v.mass_g(source.infill, source.mass_factor))
                    })
                    .sum(),
            );
        }
        if self.pending.is_none() {
            let ctx = ctx.clone();
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let mut volumes = Vec::new();
                let measured = sources
                    .iter()
                    .map(|source| {
                        let mesh = Arc::as_ptr(&source.mesh);
                        let settings = source.settings;
                        if let Some((_, _, volume)) = volumes
                            .iter()
                            .find(|(cached, profile, _)| *cached == mesh && *profile == settings)
                        {
                            return *volume;
                        }
                        let volume = settings.measure(&source.mesh);
                        volumes.push((mesh, settings, volume));
                        volume
                    })
                    .collect();
                let _ = sender.send(measured);
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
    pub(super) fn matching_selection(&self) -> bool {
        self.preview_modes.mode == preview_modes::Mode::Exploded
            && self.part_details.matching.as_ref().is_some_and(|id| {
                self.selected
                    .and_then(|index| self.scene.parts.get(index))
                    .is_some_and(|part| {
                        part.visible
                            && id.instance == part.pose.instance
                            && id.path == part.pose.path
                            && id.body == part.pose.body
                    })
            })
    }

    pub(super) fn preview_selected_parts(&self) -> impl Iterator<Item = &Part> {
        let part = self
            .selected
            .and_then(|index| self.scene.parts.get(index))
            .filter(|part| part.visible);
        let matching = part.filter(|_| self.matching_selection());
        part.filter(|_| matching.is_none()).into_iter().chain(
            matching
                .into_iter()
                .flat_map(|part| matching_parts(&self.scene, part)),
        )
    }

    pub(super) fn part_details_visible(&self) -> bool {
        self.tab == 0
            && !self.tower.print_list
            && !self.dimensions
            && !self.gridfinity_open
            && !self.divider_editor.open
            && !self.sustainer_divider_editor.open
            && self.preview_modes.mode == preview_modes::Mode::Exploded
    }

    fn part_print_config(&self, part: &Part) -> &Config {
        let target = Target::of(part);
        self.members
            .get(target.member)
            .filter(|_| self.scene_key == "combination")
            .and_then(|member| {
                target
                    .drawer
                    .and_then(|index| member.preset.drawers.get(index))
                    .unwrap_or(&member.preset)
                    .print_config
                    .as_ref()
            })
            .unwrap_or(&self.export_options.plate)
    }

    pub(super) fn part_selection_tools(&mut self, ui: &mut egui::Ui, summary: bool) {
        if !self.part_details_visible() {
            return;
        }
        let Some(part) = self
            .selected
            .and_then(|i| self.scene.parts.get(i))
            .filter(|p| p.visible)
        else {
            return;
        };
        let z = self.zh;
        let matching = self.matching_selection();
        let same = matching_parts(&self.scene, part).count();
        let select = if z {
            format!("选中同类 {same}")
        } else {
            format!("Select matching ({same})")
        };
        let id = PartId::of(part);
        if summary {
            let name = details_title(&id.path, id.path.rsplit('/').next().unwrap_or(&id.path), z);
            if matching {
                ui.weak(selected_count(same, z));
            }
            ui.strong(name);
        }
        if ui
            .button(if matching {
                t(z, "聚焦同类", "Focus matching")
            } else {
                t(z, "聚焦零件", "Focus part")
            })
            .clicked()
        {
            self.focus_preview_part(ui.ctx());
        }
        if !matching && same > 1 && ui.button(select).clicked() {
            self.preview_modes.cancel_focus();
            self.part_details.matching = Some(id);
        }
        if self.scene_key == "combination" {
            self.tower_preview_color_menu(ui);
        }
    }

    pub(super) fn part_details_window(&mut self, ctx: &egui::Context) {
        if !self.matching_selection() {
            self.part_details.matching = None;
        }
        if !self.part_details_visible() {
            self.preview_modes.cancel_focus();
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
        let same = matching_parts(&self.scene, part).count();
        let matching = self.matching_selection();
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
        let config = self.part_print_config(part);
        let appearance = config.appearance(config.slot(&path));
        let material = self.material_library.name(appearance.material).to_owned();
        let color = tower::colors::part_color_label(appearance, z);
        let [r, g, b] = appearance.rgb;
        let group = tower::colors::part_group_name(&self.global_colors, &path, z);
        let sources: Option<Vec<_>> = self
            .preview_selected_parts()
            .map(|part| {
                let config = self.part_print_config(part);
                let slot = config.slot(&part.pose.path);
                let appearance = config.appearance(slot);
                Some(MassSource {
                    mesh: Arc::clone(&part.mesh),
                    settings: config.into(),
                    infill: config.infill,
                    mass_factor: self.material_library.mass_factor(
                        appearance.material,
                        config.printer,
                        slot,
                    )?,
                })
            })
            .collect();
        let mass = match sources {
            Some(sources) => match self.part_details.mass(sources, ctx) {
                None => t(z, "计算中…", "Calculating…").into(),
                Some(Some(mass)) if mass < 0.01 => "< 0.01 g".into(),
                Some(Some(mass)) if mass < 1. => format!("≈ {mass:.2} g"),
                Some(Some(mass)) => format!("≈ {mass:.1} g"),
                Some(None) => "—".into(),
            },
            None => "—".into(),
        };

        let bounds = ctx.content_rect();
        let mut open = true;
        let title = details_title(&path, file, z);
        let heading = if matching {
            selected_count(same, z)
        } else {
            title.clone()
        };
        egui::Window::new("selected-part-details")
            .id(egui::Id::new("selected-part-details"))
            .title_bar(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16., 210.))
            .movable(false)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                let width = 288.;
                ui.set_width(width);
                ui.horizontal(|ui| {
                    let title_width = width - 26. - ui.spacing().item_spacing.x;
                    ui.allocate_ui_with_layout(
                        egui::vec2(title_width, 24.),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&heading).size(16.).strong())
                                    .truncate(),
                            )
                            .on_hover_text(&heading);
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
                        if matching {
                            row(ui, &format!("× {same}"), &title);
                        } else {
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
                        }
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
                        if !matching {
                            row(ui, t(z, "耗材", "Filament"), &format!("{material} · {color}"));
                        }
                        let weight_label = if matching {
                            t(z, "合计估重", "Total est. mass")
                        } else {
                            t(z, "模型估重", "Est. model mass")
                        };
                        row(ui, weight_label, &mass).on_hover_text(t(
                            z,
                            "按展示网格的截面、壁数、顶底实心层、填充密度、耗材密度和流量比估算。实际切片用量以 Bambu Studio 为准。",
                            "Estimated from display-mesh sections, wall loops, solid shells, infill, filament density and flow ratio. See Bambu Studio for sliced filament usage.",
                        ));
                        if !matching {
                            ui.add_space(9.);
                            ui.weak(t(z, "数量与位置", "Quantity & location"));
                            let count = format!("{same} {}", t(z, "件", "pcs"));
                            row(ui, t(z, "同类", "Matching"), &count).on_hover_text(t(
                                z,
                                "同一源文件与实体编号的展示实例",
                                "Displayed copies with the same source file and body number",
                            ));
                            row(ui, t(z, "位置", "Location"), &location);
                        }
                    });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    self.part_selection_tools(ui, false);
                });
            });
        if !open {
            self.preview_modes.cancel_focus();
            self.selected = None;
            self.last_hit = None;
            self.part_details.matching = None;
        }
    }
}

fn selected_count(count: usize, zh: bool) -> String {
    if zh {
        format!("已选 {count} 个零件")
    } else {
        format!("{count} parts selected")
    }
}
