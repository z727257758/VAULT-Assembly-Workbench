use super::*;

impl App {
    pub(in crate::app) fn setup_recipe_smoke(&mut self, ctx: &egui::Context) {
        self.smoke_stage = 0;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
        if let Err(error) = self.tower_from_recipe(0, ctx) {
            self.recipe_smoke_error(&error, ctx);
        }
    }
    fn recipe_smoke_error(&self, error: &str, ctx: &egui::Context) {
        let folder = self.home.join("output/preset-smoke");
        let _ = std::fs::create_dir_all(&folder);
        let _ = std::fs::write(folder.join("error.txt"), error);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    pub(in crate::app) fn recipe_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.smoke_start.elapsed() > Duration::from_secs(600) {
            self.recipe_smoke_error(&format!("Preset smoke timeout: {}", self.status), ctx);
            return;
        }
        if !self.busy && !self.preview.has_pending() && self.failed_preview.is_some() {
            self.recipe_smoke_error(&self.status, ctx);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 4 {
            self.camera.fit(&self.scene, self.explosion);
        }
        if self.smoke_frame >= 8 && !self.screenshot_requested {
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
        let index = self.smoke_stage as usize / 2;
        let expanded = self.smoke_stage % 2 == 1;
        let result = (|| -> Result<(), String> {
            let expected: usize = RECIPES[index].rows.iter().map(|r| r.len()).sum();
            if self.scene.member_bounds.len() != expected || self.members.len() != expected {
                return Err("Preset member count differs from the native scene".into());
            }
            if self.scene.parts.iter().any(|p| p.pose.validate().is_err()) {
                return Err("Preset has an invalid display pose".into());
            }
            let folder = self.home.join("output/preset-smoke");
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                folder.join(format!(
                    "{}-{}.png",
                    RECIPES[index].id,
                    if expanded { "exploded" } else { "closed" }
                )),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            if expanded && index + 1 == RECIPES.len() {
                self.renderer.wait_idle()?;
                std::fs::write(folder.join("result.txt"),format!("PASS: all {} built-in recipes created as independent projects, loaded and rendered closed and exploded; finite poses and member identities preserved. Surface diagnostics are separate in output/preset-audit.",RECIPES.len())).map_err(|e|e.to_string())?;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if expanded {
                self.tower_from_recipe(index + 1, ctx)?;
            } else {
                self.preview_modes.mode = super::super::preview_modes::Mode::Exploded;
                self.explosion = 1.;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.recipe_smoke_error(&error, ctx);
            return;
        }
        if expanded && index + 1 == RECIPES.len() {
            return;
        }
        self.smoke_stage += 1;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
    }
}
