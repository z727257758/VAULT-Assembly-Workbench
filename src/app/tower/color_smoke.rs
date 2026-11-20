//! Check the material cards, preview group menu and saved print configuration through the UI.
use super::*;
use crate::catalog::colors::{ROLES, component, role};
use crate::print_plate::{ColorTheme, Config, FilamentMaterial, PaletteGroup};
use egui::{Event, LayerId, Pos2};

const GUIDE: &str = "01. DRAWERS/07. Universal/Runners/x3 Runner Guide.stl";
const SLIDE: &str = "01. DRAWERS/07. Universal/Runners/x3 Runner Slide.stl";
const MATERIAL: FilamentMaterial = FilamentMaterial::PlaMatte;

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

fn card_material_button(texts: &[Text], current: &str) -> Result<Pos2, String> {
    let header = control(texts, "配件", true)?;
    texts
        .iter()
        .filter(|t| {
            t.layer == LayerId::background() && t.value == current && t.center.y > header.y + 12.
        })
        .min_by(|a, b| a.center.y.total_cmp(&b.center.y))
        .map(|t| t.center)
        .ok_or_else(|| format!("Expanded accessory card has no {current} material button"))
}

fn swatch(ctx: &egui::Context, rgb: [u8; 3], below: f32) -> Result<Pos2, String> {
    let fill = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    ctx.graphics(|graphics| {
        graphics
            .get(LayerId::background())?
            .all_entries()
            .find_map(|entry| {
                if let egui::Shape::Circle(c) = &entry.shape
                    && c.fill == fill
                    && c.radius >= 6.
                    && c.center.y > below
                    && entry.clip_rect.contains(c.center)
                {
                    Some(c.center)
                } else {
                    None
                }
            })
    })
    .ok_or_else(|| format!("No visible color swatch matching {rgb:?}"))
}

impl App {
    pub(super) fn setup_color_groups_smoke(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.zh = true;
        self.material_library = Default::default();
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
        let accessory = read
            .colors
            .group_appearance(PaletteGroup::Role("accessory"));
        let rgb = self.color_smoke_swatch()?;
        if read.colors != self.global_colors
            || read.colors.role_for(GUIDE) != "accessory"
            || accessory.material != MATERIAL
            || accessory.rgb != rgb
            || self.renderer.print_colors.is_none()
            || self.renderer.member_colors.len() != read.members.len()
        {
            return Err("Saved project or active preview did not retain the edited group".into());
        }
        let original = ColorTheme::default();
        for (role, _, _) in ROLES {
            let group = PaletteGroup::Role(role);
            if role != "accessory"
                && read.colors.group_appearance(group) != original.group_appearance(group)
            {
                return Err(format!("Editing the accessory card also changed {role}"));
            }
        }
        for (member, preview) in read.members.iter().zip(&self.renderer.member_colors) {
            for job in member.preset.print_jobs(&member.name) {
                let export = job
                    .preset
                    .print_config
                    .ok_or("Missing member export colors")?;
                let json: serde_json::Value =
                    serde_json::from_slice(&export.project_json()?).map_err(|e| e.to_string())?;
                for item in job.items.iter().filter(|item| item.enabled) {
                    let slot = read.colors.group_slot(&item.path);
                    let expected = read.colors.appearance(slot);
                    if preview.slot(&item.path) != slot
                        || export.slot(&item.path) != slot
                        || preview.appearance(slot) != expected
                        || export.appearance(slot) != expected
                        || json["filament_colour"][(slot - 1) as usize] != export.color_hex(slot)
                        || (item.path == GUIDE
                            && !json["filament_settings_id"][(slot - 1) as usize]
                                .as_str()
                                .is_some_and(|name| name.contains(MATERIAL.name())))
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

    fn color_smoke_swatch(&self) -> Result<[u8; 3], String> {
        self.material_library
            .swatches(MATERIAL)
            .iter()
            .copied()
            .max_by_key(|[r, _, b]| i16::from(*b) - i16::from(*r))
            .ok_or_else(|| format!("No configured swatches for {}", MATERIAL.name()))
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
                4 => {
                    let button = card_material_button(&ui, "PLA Basic")?;
                    if !self.capture_native_smoke(ctx, &folder.join("card.png"))? {
                        return Ok(());
                    }
                    self.click_color_control(button);
                }
                5 => {
                    for material in FilamentMaterial::ALL {
                        control(&ui, material.name(), false)?;
                    }
                    if !self.capture_native_smoke(ctx, &folder.join("materials-menu.png"))? {
                        return Ok(());
                    }
                    self.click_color_control(control(&ui, MATERIAL.name(), false)?);
                }
                6 => {
                    if self
                        .global_colors
                        .group_appearance(PaletteGroup::Role("accessory"))
                        .material
                        != MATERIAL
                    {
                        return Err(
                            "Choosing a dropdown item did not update the group's material".into(),
                        );
                    }
                    let button = card_material_button(&ui, MATERIAL.name())?;
                    self.click_color_control(swatch(ctx, self.color_smoke_swatch()?, button.y)?);
                }
                7 => {
                    self.check_saved_colors(&folder)?;
                    self.click_color_control(control(&ui, "配件", true)?);
                }
                8 => {
                    self.check_color_rows(&ui, true)?;
                    if !self.capture_native_smoke(ctx, &folder.join("regrouped.png"))? {
                        return Ok(());
                    }
                    self.click_color_control(control(&ui, "设置", true)?);
                }
                9 => self.click_color_control(control(&ui, "材料", false)?),
                _ => {
                    if !ui
                        .iter()
                        .any(|t| t.layer != LayerId::background() && t.value.contains("添加颜色"))
                    {
                        return Err("Material settings did not open with swatch editing".into());
                    }
                    if !self.capture_native_smoke(ctx, &folder.join("material-settings.png"))? {
                        return Ok(());
                    }
                    std::fs::write(folder.join("smoke-result.txt"), "PASS: inline cards matched the BOM, selected a different material from the dropdown and changed color from its swatches. Regrouping and edits persisted for both members in the preview, the exported filament settings and the saved project. Other groups retained their colors and materials. Material settings opened from the toolbar.").map_err(|e| e.to_string())?;
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
