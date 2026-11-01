use super::*;
use crate::catalog::exterior::{Base, Board, Exterior, Roof};
impl App {
    pub(super) fn setup_exterior_smoke(&mut self) {
        let stage = self.smoke_stage;
        let (w, kind) = if stage == 2 {
            ("L", AssemblyKind::Drawer)
        } else if stage == 3 {
            ("XL", AssemblyKind::Sustainer)
        } else {
            ("S", AssemblyKind::Drawer)
        };
        let count = if stage < 2 { 3 } else { 1 };
        self.members.clear();
        for i in 0..count {
            let mut p = self.member_snapshot();
            p.assembly_kind = kind;
            p.selection = Selection {
                width: w.into(),
                height: if w == "XL" { "1F" } else { "2F" }.into(),
                ..Default::default()
            };
            p.label = Default::default();
            p.gridfinity = Default::default();
            p.divider_layout = Default::default();
            p.sustainer_dividers = Default::default();
            p.sustainer_options = Default::default();
            p.sustainer_angle = 0.;
            p.bom = if kind == AssemblyKind::Sustainer {
                self.catalog
                    .sustainer_bom(&p.selection, p.sustainer_options)
                    .unwrap()
            } else {
                self.catalog.bom(&p.selection)
            };
            let mut e = Exterior::default();
            if stage < 2 {
                e.backbone = true;
                if i == 1 {
                    e.stationary_handle = true;
                }
                if i == 0 {
                    e.base = Base::Trolley;
                }
                if i == count - 1 {
                    e.pull_handle = true;
                    e.pull_extension = 180.;
                    e.roof = Roof::Grid;
                    e.top_handle = true;
                }
            } else if stage == 2 {
                e.base = Base::Plain;
                e.backbone = true;
                e.stationary_handle = true;
                e.roof = Roof::Bowl;
            } else if stage == 3 {
                e.base = Base::Plain;
                e.backbone = true;
                e.board = Board::Ikea;
            } else if stage == 4 {
                e.wall_mount = true;
            } else if stage == 5 {
                e.roof = Roof::UnderTable;
            }
            self.members.push(Member {
                name: format!("配件测试 {}", i + 1),
                placement: crate::scene::Placement {
                    level: i,
                    ..Default::default()
                },
                preset: p,
                exterior: e,
            });
        }
        self.restore_member(count as usize - 1);
        self.combination_view = true;
    }
    pub(super) fn exterior_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        let folder = self.home.join("output/assembly-accessories-review");
        let _ = std::fs::create_dir_all(&folder);
        if self.smoke_start.elapsed() > Duration::from_secs(240)
            || (!self.busy && !self.preview.has_pending() && self.failed_preview.is_some())
        {
            let _ = std::fs::write(folder.join("smoke-error.txt"), &self.status);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 {
            self.camera.fit(&self.scene, 0.);
            self.camera.yaw = if self.smoke_stage == 1 { 0.5 } else { 3.65 };
            self.camera.pitch = 0.3;
            self.camera.zoom = 1.1;
            self.export_options.plate.preview_colors = false;
            if self.smoke_stage == 6 {
                self.open_exterior();
            }
        }
        if self.smoke_frame >= if self.smoke_stage == 6 { 40 } else { 10 }
            && !self.screenshot_requested
        {
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
            if self.scene.calibrated {
                return Err("Estimated attachments must remain unverified".into());
            }
            let mut solved = self.scene.clone();
            crate::constraints::solve(&mut solved)?;
            if solved.parts.iter().zip(&self.scene.parts).any(|(a, b)| {
                (glam::Vec3::from(a.pose.position) - glam::Vec3::from(b.pose.position)).length()
                    > 0.03
            }) {
                return Err("Unstable exterior graph".into());
            }
            let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                folder.join(format!("render-{}.png", self.smoke_stage)),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            let mut report = String::new();
            for p in &self.scene.parts {
                if p.pose.path.starts_with("03.") || p.pose.path.contains("/Roofs/") {
                    report.push_str(&format!(
                        "{} #{} pos {:?} rot {:?} bounds {:?}\n",
                        p.pose.path,
                        p.pose.instance,
                        p.pose.position,
                        p.pose.rotation,
                        crate::constraints::world_bounds(p)
                    ));
                }
            }
            std::fs::write(
                folder.join(format!("poses-{}.txt", self.smoke_stage)),
                report,
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::write(folder.join("smoke-error.txt"), e);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.smoke_stage == 6 {
            let _ = std::fs::write(
                folder.join("smoke-result.txt"),
                "PASS: trolley, handles, roofs, base, backbone, pegboard, wall/table mounting; stable constraints; estimated calibration.",
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.smoke_stage += 1;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
        self.setup_exterior_smoke();
        self.assembly(ctx);
    }
}
