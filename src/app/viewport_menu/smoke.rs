use super::*;

fn menu_item(ctx: &egui::Context, title: &str) -> Option<egui::Pos2> {
    let layers = ctx.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
    ctx.graphics(|graphics| {
        layers
            .into_iter()
            .filter(|layer| layer.order == egui::Order::Foreground)
            .find_map(|layer| {
                graphics.get(layer)?.all_entries().find_map(|entry| {
                    if let egui::Shape::Text(shape) = &entry.shape
                        && shape.galley.text() == title
                    {
                        let center = shape.pos + shape.galley.rect.center().to_vec2();
                        entry.clip_rect.contains(center).then_some(center)
                    } else {
                        None
                    }
                })
            })
    })
}

impl App {
    pub(in crate::app) fn viewport_menu_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(16));
        let folder = self.home.join("output/viewport-menu-ui");
        let result = (|| -> Result<bool, String> {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            if self.smoke_start.elapsed() > Duration::from_secs(120) {
                return Err(format!(
                    "Timed out at stage {}: {}",
                    self.smoke_stage, self.status
                ));
            }
            if self.busy
                || self.preview.has_pending()
                || self.editor_smoke_pointer.pending()
                || self.scene_key != "combination"
                || self.scene.parts.is_empty()
            {
                return Ok(false);
            }
            let add = t(self.zh, "在上方添加抽屉层", "Add drawer layer above");
            let pan_id = egui::Id::new("smoke-viewport-menu-pan");
            match self.smoke_stage {
                0 => {
                    let Some(rect) = self.inline_edit.viewport else {
                        return Ok(false);
                    };
                    let mut found = None;
                    'rows: for y in (24..76).step_by(3) {
                        for x in (22..81).step_by(3) {
                            let pos = rect.min
                                + rect.size() * egui::vec2(x as f32 / 100., y as f32 / 100.);
                            if ctx.layer_id_at(pos) != Some(egui::LayerId::background()) {
                                continue;
                            }
                            let (origin, direction) = self.camera.ray(pos, rect);
                            if self
                                .scene
                                .pick(origin, direction, self.explosion)
                                .is_some_and(|hit| {
                                    let picked = Target::of(&self.scene.parts[hit.part]);
                                    self.members[picked.member].placement.level == 0
                                })
                            {
                                found = Some(pos);
                                break 'rows;
                            }
                        }
                    }
                    let Some(point) = found else {
                        return Err("Could not pick the lowest layer".into());
                    };
                    self.editor_smoke_pointer.secondary(point, point);
                }
                1 => {
                    if !self.capture_native_smoke(ctx, &folder.join("menu.png"))? {
                        return Ok(false);
                    }
                    if self.members[self.active_member].placement.level != 0 {
                        return Err("Right click did not select the clicked layer".into());
                    }
                    for title in [
                        t(self.zh, "编辑", "Edit"),
                        t(self.zh, "上移一层", "Move up one layer"),
                        t(self.zh, "颜色分组", "Color group"),
                    ] {
                        menu_item(ctx, title)
                            .ok_or_else(|| format!("Missing viewport option {title}"))?;
                    }
                    self.editor_smoke_pointer
                        .click(menu_item(ctx, add).ok_or("Missing add-above action")?);
                }
                2 => {
                    let Some(rect) = self.inline_edit.viewport else {
                        return Ok(false);
                    };
                    if tower::rows(&self.members)?.len() != 4
                        || self.members[self.active_member].placement.level != 1
                    {
                        return Err("Adding through the menu did not insert directly above the clicked layer".into());
                    }
                    ctx.data_mut(|data| data.insert_temp(pan_id, self.camera.pan));
                    let center = rect.center();
                    self.editor_smoke_pointer
                        .secondary(center, center + egui::vec2(48., 14.));
                }
                _ => {
                    if ctx.data(|data| data.get_temp::<[f32; 2]>(pan_id)) == Some(self.camera.pan)
                        || menu_item(ctx, add).is_some()
                    {
                        return Err("Right drag did not pan or opened a menu".into());
                    }
                    std::fs::write(folder.join("result.txt"), "PASS: right-click selected the hit member, presented editing, layer movement and color groups, inserted a drawer layer directly above the target, and secondary drag panned without opening the menu.").map_err(|e| e.to_string())?;
                    let _ = std::fs::remove_file(folder.join("error.txt"));
                    return Ok(true);
                }
            }
            self.smoke_stage += 1;
            Ok(false)
        })();
        match result {
            Ok(false) => return,
            Ok(true) => {}
            Err(error) => {
                let _ = std::fs::write(folder.join("error.txt"), error);
                let _ = std::fs::remove_file(folder.join("result.txt"));
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
