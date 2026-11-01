use super::*;
impl App {
    pub(super) fn large_stl_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        let folder = self.home.join("output/large-stl");
        if self.smoke_start.elapsed() > Duration::from_secs(120) {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("smoke-error.txt"), &self.status);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 {
            self.export_options.plate.preview_colors = false;
            self.camera.fit(&self.scene, 0.);
            self.camera.pitch = 0.65;
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
            let triangles = self
                .scene
                .parts
                .iter()
                .map(|p| p.mesh.vertices.len() / 3)
                .sum::<usize>();
            if triangles != 6_749_642 || self.renderer.mesh_chunk_count() < 2 {
                return Err(format!(
                    "Incorrect large mesh: {triangles} triangles / {} GPU chunks",
                    self.renderer.mesh_chunk_count()
                ));
            }
            let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                folder.join("xl3-render.png"),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(folder.join("smoke-result.txt"),format!("PASS: full XL3 module; {triangles} triangles; {} GPU chunks; no decimation. GPU: {}",self.renderer.mesh_chunk_count(),self.renderer.state.adapter.get_info().name)).map_err(|e|e.to_string())
        })();
        if let Err(e) = result {
            let _ = std::fs::write(folder.join("smoke-error.txt"), e);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
