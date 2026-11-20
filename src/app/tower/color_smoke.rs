//! Exercise the rendered build-panel rows, the preview's group menu and the saved print config.
use super::*;
use crate::catalog::colors::{ROLES, component, role};
use crate::print_plate::Config;
use egui::{Event, LayerId, Pos2};

const GUIDE: &str = "01. DRAWERS/07. Universal/Runners/x3 Runner Guide.stl";
const SLIDE: &str = "01. DRAWERS/07. Universal/Runners/x3 Runner Slide.stl";
const SLOT: u32 = 6;

// Read the actual texts painted by egui so that duplicated, hidden or incorrectly
// labelled rows cannot pass through a separate representation built by the test.
struct Text {
    layer: LayerId,
    value: String,
    center: Pos2,
}

fn painted(ctx: &egui::Context) -> Vec<Text> {
    let mut layers = ctx.memory(|m| m.layer_ids().collect::<Vec<_>>());
    if !layers.contains(&LayerId::background()) {
        layers.push(LayerId::background());
    }
    ctx.graphics(|g| {
        let mut texts = Vec::new();
        for layer in layers {
            if let Some(list) = g.get(layer) {
                for entry in list.all_entries() {
                    if let egui::Shape::Text(text) = &entry.shape {
                        let center = text.pos + text.galley.rect.center().to_vec2();
                        if entry.clip_rect.contains(center) {
                            texts.push(Text {
                                layer,
                                value: text.galley.text().into(),
                                center,
                            });
                        }
                    }
                }
            }
        }
        texts
    })
}

fn control(texts: &[Text], title: &str, sidebar: bool) -> Result<Pos2, String> {
    let mut found = texts.iter().filter(|t| {
        (t.layer == LayerId::background()) == sidebar
            && (t.value == title || t.value.split(" · ").next() == Some(title))
    });
    match (found.next(), found.next()) {
        (Some(text), None) => Ok(text.center),
        _ => Err(format!(
            "Expected exactly one visible control '{title}' (sidebar={sidebar})"
        )),
    }
}

impl App {
    pub(super) fn setup_color_groups_smoke(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.zh = true;
        // A project from the color-bucket UI could store two slots for functional parts.
        let mut old = serde_json::to_value(self.tower_snapshot()).map_err(|e| e.to_string())?;
        old["colors"] =
            serde_json::json!({"components": {(component(GUIDE)): 2, (component(SLIDE)): 4}});
        self.global_colors = serde_json::from_value::<CombinationPreset>(old)
            .map_err(|e| e.to_string())?
            .colors;
        self.global_colors.validate()?;
        self.global_colors.roles.insert("functional".into(), 2);
        self.global_colors.roles.insert("accessory".into(), 3);
        self.global_colors.palette[(SLOT - 1) as usize] = [155, 83, 166];
        self.tower.name = "配色分组验收".into();
        let mut members = Vec::new();
        for level in 0..2 {
            let mut p = fresh_like(&self.member_snapshot());
            p.selection = Selection {
                width: "S".into(),
                depth: 3,
                height: "1F".into(),
                hidden_key: false,
            };
            p.assembly_kind = AssemblyKind::Drawer;
            configure(&mut p, &self.catalog, true)?;
            let accessories = self.catalog.accessories(&p.selection);
            if level == 0 {
                let plate = accessories
                    .iter()
                    .find(|e| {
                        e.path.contains("/Gridfinity Baseplate/") && !e.path.contains("Handle")
                    })
                    .ok_or("Missing S3 baseplate")?;
                p.gridfinity.baseplate = plate.path.clone();
                Catalog::set_accessory(&mut p.bom, plate.path.clone(), true, 1);
                p.gridfinity.bins.push(Default::default());
                p.gridfinity.sync_shared();
                p.gridfinity.sync_bom(&mut p.bom);
            } else {
                let short = accessories
                    .iter()
                    .find(|e| {
                        e.path.contains("/Grid Dividers/")
                            && matches!(crate::dividers::code(&e.path), Some(5..=7))
                    })
                    .ok_or("Missing short divider")?;
                p.divider_layout = crate::dividers::Layout {
                    enabled: true,
                    specification: crate::dividers::spec(&p.selection),
                    items: vec![crate::dividers::Placement {
                        path: short.path.clone(),
                        start: [2, 3],
                        vertical: false,
                    }],
                };
                p.divider_layout.sync_bom(&mut p.bom, &p.selection);
            }
            let mut config = Config {
                follow_global_colors: true,
                ..Default::default()
            };
            self.global_colors.apply(&mut config);
            p.print_config = Some(config);
            members.push(Member {
                name: String::new(),
                preset: p,
                placement: crate::scene::Placement {
                    level,
                    ..Default::default()
                },
                exterior: Default::default(),
            });
        }
        self.tower_apply(members, 0, ctx)
    }

    pub(in crate::app) fn color_groups_smoke_input(&mut self, input: &mut egui::RawInput) {
        input.focused = true;
        input.events.retain(|e| {
            !matches!(
                e,
                Event::PointerMoved(_) | Event::PointerGone | Event::PointerButton { .. }
            )
        });
        if let Some(events) = self.tower.color_smoke_input.pop_front() {
            input.events.extend(events);
        }
    }

    fn click_color_control(&mut self, pos: Pos2) {
        for pressed in [true, false] {
            self.tower.color_smoke_input.push_back(vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
    }

    fn check_color_rows(&self, texts: &[Text], regrouped: bool) -> Result<(), String> {
        let bom = self.tower_print_items();
        let mut total = 0;
        for (key, name, _) in ROLES {
            let center = control(texts, name, true)?;
            let expected: u32 = bom
                .iter()
                .filter(|item| {
                    let category = if regrouped && item.path == GUIDE {
                        "accessory"
                    } else {
                        role(&item.path)
                    };
                    category == key
                })
                .map(|item| item.qty)
                .sum();
            let count = format!("{expected} 件");
            if expected == 0
                || !texts.iter().any(|t| {
                    t.layer == LayerId::background()
                        && t.value == count
                        && (t.center.y - center.y).abs() < 3.
                })
            {
                return Err(format!("Rendered group {name} should show {count}"));
            }
            total += expected;
        }
        if total != bom.iter().map(|item| item.qty).sum::<u32>() {
            return Err("Color rows do not total the enabled BOM".into());
        }
        Ok(())
    }

    fn check_saved_colors(&mut self, folder: &Path) -> Result<(), String> {
        let project = self.tower_snapshot();
        let path = folder.join("project.toml");
        std::fs::write(
            &path,
            toml::to_string_pretty(&project).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let read: CombinationPreset =
            toml::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        read.validate(&self.catalog)?;
        if read.colors != self.global_colors
            || read.colors.role_for(GUIDE) != "accessory"
            || read.colors.role_slot("accessory") != SLOT
            || self.renderer.print_colors.is_none()
            || self.renderer.member_colors.len() != read.members.len()
        {
            return Err("Saved project or active preview did not retain the edited group".into());
        }
        for (member, preview) in read.members.iter().zip(&self.renderer.member_colors) {
            for job in member.preset.print_jobs(&member.name) {
                let export = job
                    .preset
                    .print_config
                    .ok_or("Missing member export colors")?;
                if [preview, &export].iter().any(|config| {
                    config.palette != read.colors.palette
                        || config.materials != read.colors.materials
                }) {
                    return Err(
                        "Member preview or export lost the project RGB palette or materials".into(),
                    );
                }
                for item in job.items.iter().filter(|item| item.enabled) {
                    let slot = read.colors.group_slot(&item.path);
                    if preview.slot(&item.path) != slot
                        || export.slot(&item.path) != slot
                        || (item.path == GUIDE && slot != SLOT)
                        || (item.path == SLIDE && slot != 2)
                    {
                        return Err(format!(
                            "Preview/export slot differs for {} (expected {slot})",
                            item.path
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn color_groups_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(16));
        let folder = self.home.join("output/color-groups-ui");
        let result = (|| -> Result<(), String> {
            if !self.tower.error.is_empty() {
                return Err(self.tower.error.clone());
            }
            if self.smoke_start.elapsed() > Duration::from_secs(180)
                || (!self.busy && !self.preview.has_pending() && self.failed_preview.is_some())
            {
                return Err(self.status.clone());
            }
            if self.busy
                || self.preview.has_pending()
                || self.scene_key != "combination"
                || !self.tower.color_smoke_input.is_empty()
            {
                return Ok(());
            }
            self.smoke_frame += 1;
            if self.smoke_frame < 4 {
                return Ok(());
            }
            let ui = painted(ctx);
            match self.smoke_stage {
                0 => {
                    self.check_color_rows(&ui, false)?;
                    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                    if !self.capture_native_smoke(ctx, &folder.join("groups.png"))? {
                        return Ok(());
                    }
                    self.selected = Some(
                        self.scene
                            .parts
                            .iter()
                            .position(|p| p.visible && p.pose.path == GUIDE)
                            .ok_or("No runner guide in assembled fixture")?,
                    );
                }
                1 => self.click_color_control(control(&ui, "颜色分组", false)?),
                2 => {
                    for (_, name, _) in ROLES {
                        control(&ui, name, false)?;
                    }
                    self.click_color_control(control(&ui, "配件", false)?);
                }
                3 => {
                    if self.global_colors.role_for(GUIDE) != "accessory" {
                        return Err("Preview menu did not reassign the guide".into());
                    }
                    self.check_color_rows(&ui, true)?;
                    self.click_color_control(control(&ui, "配件", true)?);
                }
                4 => self.click_color_control(control(&ui, &SLOT.to_string(), false)?),
                5 => self.click_color_control(control(&ui, "应用", false)?),
                _ => {
                    self.check_color_rows(&ui, true)?;
                    let row = control(&ui, "配件", true)?;
                    let material = self.global_colors.materials[(SLOT - 1) as usize].name();
                    for label in ["紫色", material] {
                        if !ui.iter().any(|t| {
                            t.layer == LayerId::background()
                                && t.value == label
                                && (t.center.y - row.y).abs() < 20.
                        }) {
                            return Err(format!("Recolored accessory row missing '{label}'"));
                        }
                    }
                    if !self.capture_native_smoke(ctx, &folder.join("regrouped.png"))? {
                        return Ok(());
                    }
                    self.check_saved_colors(&folder)?;
                    std::fs::write(folder.join("smoke-result.txt"), "PASS: Five unique groups matched the full BOM before and after moving all runner guides through the preview menu. Changing the accessory slot through its row persisted across both members, their preview and export materials and a saved project round trip.").map_err(|e| e.to_string())?;
                    let _ = std::fs::remove_file(folder.join("smoke-error.txt"));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return Ok(());
                }
            }
            self.smoke_stage += 1;
            self.smoke_frame = 0;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(
                folder.join("smoke-error.txt"),
                format!("Stage {}: {error}", self.smoke_stage),
            );
            let _ = std::fs::remove_file(folder.join("smoke-result.txt"));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
