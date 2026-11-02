use super::*;
impl App {
    pub(super) fn stopper_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        let folder = self.home.join("output/stopper-validation");
        let _ = std::fs::create_dir_all(&folder);
        if self.smoke_start.elapsed() > Duration::from_secs(90) {
            let _ = std::fs::write(folder.join("smoke-error.txt"), &self.status);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 {
            let instance = if self.smoke_stage < 2 { 1 } else { 0 };
            self.export_options.plate.preview_colors = false;
            self.explosion = 0.;
            self.ghost = false;
            self.selected = None;
            for p in &mut self.scene.parts {
                let stopper =
                    p.pose.path.ends_with("/Runner Stopper.stl") && p.pose.instance == instance;
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
            self.camera.zoom = 4.;
            if let Some(p) = self.scene.parts.iter().find(|p| {
                p.pose.path.ends_with("/Runner Stopper.stl") && p.pose.instance == instance
            }) {
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
        if self.smoke_stage == 3 {
            let valid = self.selection.depth == 7
                && self
                    .scene
                    .relations
                    .iter()
                    .filter(|r| r.label == crate::runners::STOPPER_SEAT)
                    .count()
                    == 2;
            let file = if valid {
                "smoke-result.txt"
            } else {
                "smoke-error.txt"
            };
            let _ = std::fs::write(
                folder.join(file),
                if valid {
                    "PASS: S7 / 1F; both measured stopper seats; installed and uncovered port screenshots."
                } else {
                    "Missing measured stopper relations"
                },
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.smoke_stage += 1;
            self.smoke_frame = 0;
            self.screenshot_requested = false;
        }
    }
}
