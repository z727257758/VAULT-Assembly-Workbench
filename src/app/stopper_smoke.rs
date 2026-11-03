use super::*;
impl App {
    pub(super) fn stopper_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        let folder = self.home.join("output/stopper-validation");
        let _ = std::fs::create_dir_all(&folder);
        if self.smoke_start.elapsed() > Duration::from_secs(180) {
            let _ = std::fs::write(folder.join("smoke-error.txt"), &self.status);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 {
            let instance = if self.smoke_stage % 4 < 2 { 1 } else { 0 };
            self.export_options.plate.preview_colors = false;
            self.explosion = 0.;
            self.ghost = false;
            self.selected = None;
            for p in &mut self.scene.parts {
                let stopper =
                    p.pose.path.ends_with("/Runner Stopper.stl") && p.pose.instance % 2 == instance;
                p.visible = p.pose.path.contains("/Modules/")
                    || (stopper && self.smoke_stage.is_multiple_of(2));
                p.color = if stopper {
                    [1., 0.4, 0.05, 1.]
                } else {
                    [0.65, 0.65, 0.65, 1.]
                };
            }
            self.camera.fit(&self.scene, 0.);
            self.camera.yaw = if instance == 1 { 1.4 } else { -1.4 };
            self.camera.pitch = 0.15;
            self.camera.zoom = if self.smoke_stage < 4 { 4. } else { 1.4 };
            if self.smoke_stage < 4
                && let Some(p) = self.scene.parts.iter().find(|p| {
                    p.pose.path.ends_with("/Runner Stopper.stl") && p.pose.instance % 2 == instance
                })
            {
                self.camera.center = p.pose.position.into();
            }
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
        let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        let result = image::save_buffer(
            folder.join(format!("side-{}.png", self.smoke_stage)),
            &bytes,
            image.size[0] as u32,
            image.size[1] as u32,
            image::ColorType::Rgba8,
        );
        if let Err(e) = result {
            let _ = std::fs::write(folder.join("smoke-error.txt"), e.to_string());
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.smoke_stage % 4 == 3 {
            let count = match self.selection.height.as_str() {
                "2F" => 4,
                "4F" => 8,
                _ => 2,
            };
            let valid = self.selection.depth == 7
                && self.scene.constraint_warning.is_none()
                && self
                    .scene
                    .relations
                    .iter()
                    .filter(|r| r.label == crate::runners::STOPPER_SEAT)
                    .count()
                    == count
                && self
                    .scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.path.contains("/Runners/"))
                    .count()
                    == count * 3;
            if !valid {
                let _ = std::fs::write(
                    folder.join("smoke-error.txt"),
                    format!(
                        "{}: incorrect runner count or constraint warning: {:?}",
                        self.selection.height, self.scene.constraint_warning
                    ),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            if self.smoke_stage == 11 {
                let _ = std::fs::write(
                    folder.join("smoke-result.txt"),
                    "PASS: S7 / 1F, 2F, 4F; 1/2/4 runner pairs; measured stopper seats on both sides; installed and uncovered port screenshots.",
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.smoke_stage += 1;
                self.smoke_frame = 0;
                self.screenshot_requested = false;
                self.selection.height = if self.smoke_stage == 4 { "2F" } else { "4F" }.into();
                self.bom = self.catalog.reconfigure(&self.selection, &self.bom);
                self.assembly(ctx);
            }
        } else {
            self.smoke_stage += 1;
            self.smoke_frame = 0;
            self.screenshot_requested = false;
        }
    }
}
