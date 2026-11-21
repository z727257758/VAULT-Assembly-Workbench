//! Native regression for top layout, source matching and rendered footprint.
use super::*;
use crate::catalog::exterior::Role;
use crate::scene::MEMBER_STRIDE;

impl App {
    pub(in crate::app) fn setup_roof_smoke(&mut self, ctx: &egui::Context) {
        let mut preset = fresh_like(&self.member_snapshot());
        preset.selection = Selection {
            width: "L".into(),
            depth: 3,
            height: "2F".into(),
            hidden_key: false,
        };
        if let Err(error) = configure(&mut preset, &self.catalog, false) {
            self.status = error;
            return;
        }
        let exterior = crate::catalog::exterior::Exterior {
            roof: Roof::Grid,
            top_handle: true,
            ..Default::default()
        };
        let member = Member {
            name: "Top configuration".into(),
            preset,
            placement: Default::default(),
            exterior,
        };
        self.tower.name = "顶部配置 · 跟随整体尺寸".into();
        if let Err(error) = self.tower_apply(vec![member], 0, ctx) {
            self.tower.error = error;
        }
        self.tower.open_layer_card = Some(LayerCard::Roof);
        self.smoke_stage = 0;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
        self.explosion = 0.;
    }
    pub(in crate::app) fn roof_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        let folder = self.home.join("output/top-configuration");
        let failure = if !self.tower.error.is_empty() {
            Some(self.tower.error.clone())
        } else if self.smoke_start.elapsed() > Duration::from_secs(240) {
            Some(format!("Timeout: {}", self.status))
        } else if !self.busy && !self.preview.has_pending() && self.failed_preview.is_some() {
            Some(self.status.clone())
        } else {
            None
        };
        if let Some(error) = failure {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("error.txt"), error);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 {
            self.camera.fit(&self.scene, 0.);
        }
        if self.smoke_frame >= 10 && !self.screenshot_requested {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        let Some(image) = image else {
            return;
        };
        let result = (|| -> Result<(), String> {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                folder.join(format!("top-{}.png", self.smoke_stage)),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            let expected: &[(&str, u8)] = match self.smoke_stage {
                0 | 1 => &[("L", 3)],
                2 => &[("S", 3), ("S", 3)],
                3 => &[("S", 4), ("M", 4)],
                4 => &[("XL", 4)],
                _ => &[("L", 5)],
            };
            let mut sources = Vec::new();
            for (i, m) in self.members.iter().enumerate() {
                for part in
                    self.catalog
                        .exterior_components(&m.preset.selection, &m.exterior, false)?
                {
                    if part.role != Role::Roof {
                        continue;
                    }
                    if !self.scene.parts.iter().any(|p| {
                        p.pose.path == part.bom.path
                            && (p.pose.instance / MEMBER_STRIDE) as usize == i
                    }) {
                        return Err("Top print source missing from native preview".into());
                    }
                    sources.push(part.bom.path);
                }
            }
            if sources.len() != expected.len()
                || sources
                    .iter()
                    .zip(expected)
                    .any(|(path, (w, d))| !path.ends_with(&format!("/{w}{d} Grid Roof.stl")))
            {
                return Err(format!(
                    "Incorrect top sources at step {}: {sources:?}",
                    self.smoke_stage
                ));
            }
            if self.members.len() == 2 {
                let centers: Vec<_> = (0..2)
                    .map(|i| {
                        let p = self
                            .scene
                            .parts
                            .iter()
                            .find(|p| {
                                p.pose.path.contains("/Modules/")
                                    && p.pose.instance / MEMBER_STRIDE == i
                            })
                            .ok_or("Missing shell")?;
                        let (lo, hi) = crate::constraints::world_bounds(p);
                        Ok((lo.x + hi.x) * 0.5)
                    })
                    .collect::<Result<_, String>>()?;
                let span = self
                    .members
                    .iter()
                    .map(|m| {
                        crate::constraints::stacking::width(&m.preset.selection.width)
                            .map(|w| w as f32 / 20.)
                    })
                    .sum::<Result<f32, _>>()?;
                if (centers[1] - centers[0] - span).abs() > 0.3 {
                    return Err("Paired layer columns drifted outside the common footprint".into());
                }
            }
            if self.scene.calibrated {
                return Err("Estimated top was labeled verified".into());
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::write(folder.join("error.txt"), error);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.smoke_frame = 0;
        self.screenshot_requested = false;
        let result = match self.smoke_stage {
            0 => self.tower_action(Action::Row(0, "2F".into(), true), ctx),
            1 => self.tower_action(Action::RoofLayout(Layout::Apart), ctx),
            2 => self.tower_action(Action::Resize("XL".into(), 4), ctx),
            3 => self.tower_action(Action::RoofLayout(Layout::Whole), ctx),
            4 => self.tower_action(Action::Resize("L".into(), 5), ctx),
            5 => self.tower_action(Action::Row(0, "2F".into(), false), ctx),
            6 => {
                self.tower.print_list = true;
                Ok(())
            }
            _ => {
                let saved = self.tower_snapshot();
                let result = (|| -> Result<(), String> {
                    let text = toml::to_string_pretty(&saved).map_err(|e| e.to_string())?;
                    let restored: CombinationPreset =
                        toml::from_str(&text).map_err(|e| e.to_string())?;
                    restored.validate(&self.catalog)?;
                    std::fs::write(folder.join("project.toml"), text).map_err(|e| e.to_string())?;
                    std::fs::write(folder.join("result.txt"),"PASS: L3 whole top, split S+S layer with one L3 top, two S3 tops, resized S4+M4 tops, one XL4 top, one L5 top over S+S, merged L5 layer, print sources, handle settings and project roundtrip. Source models read-only; placement estimates unverified.").map_err(|e|e.to_string())
                })();
                if let Err(error) = result {
                    let _ = std::fs::write(folder.join("error.txt"), error);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        };
        if let Err(error) = result {
            self.tower.error = error;
        } else if matches!(self.smoke_stage, 0 | 5) {
            // These fixture steps edit the top layer rather than the roof card.
            self.tower.open_layer_card = Some(LayerCard::Roof);
        } else if self.tower.open_layer_card != Some(LayerCard::Roof) {
            self.tower.error = "Roof configuration closed its card".into();
        }
        self.smoke_stage += 1;
    }
}
