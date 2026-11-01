//! Object-relative interactions. Legacy placement coordinates stay private to
//! persistence and constraint solving; the UI exposes only objects and sides.
use super::*;
use glam::Vec3;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Left,
    Right,
    Above,
    Ground,
}
impl Side {
    pub(super) fn label(self, zh: bool) -> &'static str {
        match self {
            Self::Left => t(zh, "左侧", "Left"),
            Self::Right => t(zh, "右侧", "Right"),
            Self::Above => t(zh, "上方", "Above"),
            Self::Ground => t(zh, "地面", "Ground"),
        }
    }
}
/// Insert/snap a group relative to a concrete object. Existing attachment
/// groups move together when a side slot is inserted. Validate before applying.
pub(super) fn position_relative(
    members: &[Member],
    moving: &[usize],
    anchor: usize,
    side: Side,
) -> Result<Vec<Member>, String> {
    if moving.is_empty()
        || moving.len() > 2
        || moving
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != moving.len()
        || anchor >= members.len()
        || moving.contains(&anchor)
        || moving.iter().any(|&i| i >= members.len())
    {
        return Err("请选择另一个对象作为参照 / Choose another target object".into());
    }
    let base: Vec<_> = members
        .iter()
        .enumerate()
        .filter(|(i, _)| !moving.contains(i))
        .map(|(_, m)| m.clone())
        .collect();
    validate_layout(&base, 0.)?;
    let target = &members[anchor];
    let mut widths = std::collections::BTreeMap::<u32, i32>::new();
    for m in &base {
        let w = crate::constraints::stacking::width(&m.preset.selection.width)?;
        widths
            .entry(m.placement.column)
            .and_modify(|v| *v = (*v).min(w))
            .or_insert(w);
    }
    let last = *widths.keys().max().ok_or("没有参照对象 / No target")?;
    let mut starts = vec![0i32];
    for c in 0..=last {
        starts.push(starts[c as usize] + widths.get(&c).copied().unwrap_or(1648));
    }
    let target_end = starts[target.placement.column as usize]
        + crate::constraints::stacking::width(&target.preset.selection.width)?;
    let (column, level, insert) = match side {
        Side::Above => (target.placement.column, target.placement.level + 1, false),
        Side::Left => (target.placement.column, target.placement.level, true),
        Side::Right => (
            (target.placement.column + 1..=last + 1)
                .find(|&c| starts[c as usize] >= target_end)
                .unwrap_or(last + 1),
            target.placement.level,
            true,
        ),
        Side::Ground => (last + 1, 0, true),
    };
    let mut next = members.to_vec();
    for (i, m) in next.iter_mut().enumerate() {
        if moving.contains(&i) {
            continue;
        }
        if insert && m.placement.column >= column {
            m.placement.column += moving.len() as u32;
        }
        // Reuse a free adjacent slot when reattaching an existing pair.
        if !insert
            && moving.len() == 2
            && widths.contains_key(&(column + 1))
            && m.placement.column > column
        {
            m.placement.column += 1;
        }
    }
    for (slot, &i) in moving.iter().enumerate() {
        next[i].placement = Placement {
            column: column + slot as u32,
            level,
            offset: [0.; 3],
        };
    }
    validate_layout(&next, 0.)?;
    Ok(next)
}
fn project(point: Vec3, camera: &Camera, rect: egui::Rect) -> Option<egui::Pos2> {
    let v = camera.matrix(rect.aspect_ratio()).project_point3(point);
    (v.is_finite() && (0. ..=1.).contains(&v.z)).then(|| {
        egui::pos2(
            rect.center().x + v.x * rect.width() * 0.5,
            rect.center().y - v.y * rect.height() * 0.5,
        )
    })
}
fn dimensions((lo, hi): (Vec3, Vec3)) -> String {
    let s = hi - lo;
    format!("{:.1} × {:.1} × {:.1} mm", s.x, s.z, s.y)
}
impl App {
    pub(super) fn combination_dimensions(&self, ui: &mut egui::Ui) {
        ui.separator();
        if self.scene_key != "combination" || self.busy || self.preview.has_pending() {
            ui.small(t(
                self.zh,
                "正在计算源模型尺寸…",
                "Measuring source geometry…",
            ));
            return;
        }
        if let Some(&bounds) = self.scene.member_bounds.get(self.active_member) {
            ui.label(t(
                self.zh,
                "所选对象 · 宽 × 深 × 高",
                "Selected · W × D × H",
            ));
            ui.strong(dimensions(bounds));
        }
        if !self.scene.member_bounds.is_empty() {
            let lo = self
                .scene
                .member_bounds
                .iter()
                .map(|v| v.0)
                .fold(Vec3::splat(f32::INFINITY), Vec3::min);
            let hi = self
                .scene
                .member_bounds
                .iter()
                .map(|v| v.1)
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
            ui.label(t(self.zh, "组合闭合外形", "Overall closed envelope"));
            ui.strong(dimensions((lo, hi)));
        }
        ui.small(t(
            self.zh,
            "闭盖外形包含配件及已显示的五金示意；单位 mm。",
            "Closed-lid envelope includes accessories and displayed hardware schematics; units mm.",
        ));
    }
    pub(super) fn apply_spatial_move(
        &mut self,
        source: usize,
        target: usize,
        side: Side,
        ctx: &egui::Context,
    ) {
        self.commit_member();
        let pair = paired_members(&self.members, source);
        let group = if pair.len() == 2 { pair } else { vec![source] };
        match position_relative(&self.members, &group, target, side) {
            Ok(candidate) => {
                self.combination_layout_history
                    .push(self.members.iter().map(|m| m.placement).collect());
                if self.combination_layout_history.len() > 20 {
                    self.combination_layout_history.remove(0);
                }
                self.members = candidate;
                self.restore_member(source);
                self.combination_view = true;
                self.combination_move_source = None;
                self.assembly(ctx);
            }
            Err(e) => self.status = e,
        }
    }
    /// Foreground egui areas are attached to projected member extents. The
    /// return value prevents the camera/picking layer from consuming their input.
    pub(in crate::app) fn combination_viewport_actions(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
        if self.scene_key != "combination"
            || self.members.is_empty()
            || self.busy
            || self.preview.has_pending()
            || self.combination_spec.is_some()
        {
            return false;
        }
        let z = self.zh;
        let selected = self.active_member;
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.combination_move_source = None;
        }
        let mut consumed = false;
        let response = egui::Area::new(egui::Id::new("spatial-tools"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.left_top() + egui::vec2(8., 8.))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width((rect.width() - 32.).max(140.));
                    if let Some(source) = self.combination_move_source {
                        ui.strong(format!(
                            "{}：{}",
                            t(z, "移动", "Move"),
                            self.members[source].name
                        ));
                        ui.label(t(
                            z,
                            "点击另一个模型，再点它旁边的放置按钮。Esc 取消。",
                            "Pick another model, then choose its placement button. Esc cancels.",
                        ));
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(t(z, "移到地面", "Place on ground")).clicked() {
                                let pair = paired_members(&self.members, source);
                                let target = (0..self.members.len())
                                    .find(|i| *i != source && !pair.contains(i));
                                if let Some(target) = target {
                                    self.apply_spatial_move(source, target, Side::Ground, ctx);
                                }
                            }
                            if ui.button(t(z, "取消移动", "Cancel move")).clicked() {
                                self.combination_move_source = None;
                            }
                        });
                    } else {
                        ui.strong(&self.members[selected].name);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(t(z, "安装配件…", "Accessories…")).clicked() {
                                self.open_exterior();
                            }
                            if ui.button(t(z, "修改尺寸…", "Change size…")).clicked() {
                                self.edit_member_size();
                            }
                            if ui.button(t(z, "编辑内部 →", "Edit interior →")).clicked() {
                                self.focus_member(ctx);
                            }
                            let pair = paired_members(&self.members, selected);
                            if ui
                                .button(if pair.len() == 2 {
                                    t(z, "移动配对…", "Move pair…")
                                } else {
                                    t(z, "移动对象…", "Move object…")
                                })
                                .clicked()
                            {
                                self.combination_move_source = Some(selected);
                            }
                            if ui
                                .add_enabled(
                                    self.members.len() > 1,
                                    egui::Button::new(t(z, "删除", "Remove")),
                                )
                                .clicked()
                            {
                                self.remove_member(ctx);
                            }
                            if pair.len() == 2
                                && ui.button(t(z, "删除配对", "Remove pair")).clicked()
                            {
                                self.remove_members(&pair, ctx);
                            }
                        });
                        if let Some(&bounds) = self.scene.member_bounds.get(selected) {
                            ui.small(format!(
                                "{} · {}",
                                t(z, "闭合外形 宽×深×高", "Closed W×D×H"),
                                dimensions(bounds)
                            ));
                        }
                    }
                });
            });
        consumed |= response.response.contains_pointer();
        if self.scene_key != "combination"
            || self.active_member != selected
            || self.combination_spec.is_some()
        {
            return true;
        }
        let Some(&(lo, hi)) = self.scene.member_bounds.get(selected) else {
            return consumed;
        };
        let center = (lo + hi) * 0.5;
        for (side, point) in [
            (Side::Left, Vec3::new(lo.x, center.y, center.z)),
            (Side::Above, Vec3::new(center.x, hi.y, center.z)),
            (Side::Right, Vec3::new(hi.x, center.y, center.z)),
        ] {
            let Some(point) = project(point, &self.camera, rect) else {
                continue;
            };
            let pos = egui::pos2(
                (point.x - 36.).clamp(
                    rect.left() + 4.,
                    (rect.right() - 104.).max(rect.left() + 4.),
                ),
                point.y.clamp(
                    rect.top() + 114.,
                    (rect.bottom() - 40.).max(rect.top() + 114.),
                ),
            );
            let response = egui::Area::new(egui::Id::new(format!("attachment-{side:?}")))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        if let Some(source) = self.combination_move_source {
                            let pair = paired_members(&self.members, source);
                            if ui
                                .add_enabled(
                                    selected != source && !pair.contains(&selected),
                                    egui::Button::new(format!(
                                        "{}{}",
                                        t(z, "放在", "Place "),
                                        side.label(z)
                                    )),
                                )
                                .clicked()
                            {
                                self.apply_spatial_move(source, selected, side, ctx);
                            }
                        } else {
                            ui.menu_button(format!("＋{}", side.label(z)), |ui| {
                                for (label, kind) in [
                                    (t(z, "添加盒子…", "Add box…"), Some(AssemblyKind::Sustainer)),
                                    (t(z, "添加抽屉…", "Add drawer…"), Some(AssemblyKind::Drawer)),
                                    (t(z, "复制所选对象…", "Copy selected…"), None),
                                ] {
                                    if ui.button(label).clicked() {
                                        self.add_member(kind, side == Side::Above, ctx);
                                        if let Some(draft) = &mut self.combination_spec {
                                            draft.side = side;
                                        }
                                        ui.close();
                                    }
                                }
                            });
                        }
                    });
                });
            consumed |= response.response.contains_pointer();
        }
        consumed || egui::Popup::is_any_open(ctx)
    }
    pub(in crate::app) fn paint_spatial_dimensions(&self, ui: &egui::Ui, rect: egui::Rect) {
        if self.explosion > 0.001 {
            return;
        }
        let Some(&(lo, hi)) = self.scene.member_bounds.get(self.active_member) else {
            return;
        };
        let painter = ui.painter().with_clip_rect(rect);
        let stroke = egui::Stroke::new(1., Color32::LIGHT_BLUE);
        for (a, b, value) in [
            (
                Vec3::new(lo.x, lo.y, hi.z),
                Vec3::new(hi.x, lo.y, hi.z),
                hi.x - lo.x,
            ),
            (
                Vec3::new(hi.x, lo.y, hi.z),
                Vec3::new(hi.x, lo.y, lo.z),
                hi.z - lo.z,
            ),
            (
                Vec3::new(lo.x, lo.y, lo.z),
                Vec3::new(lo.x, hi.y, lo.z),
                hi.y - lo.y,
            ),
        ] {
            let (Some(a), Some(b)) = (
                project(a, &self.camera, rect),
                project(b, &self.camera, rect),
            ) else {
                continue;
            };
            if a.distance(b) < 24. {
                continue;
            }
            painter.line_segment([a, b], stroke);
            let normal = (b - a).normalized().rot90() * 4.;
            for p in [a, b] {
                painter.line_segment([p - normal, p + normal], stroke);
            }
            let center = a.lerp(b, 0.5) + normal * 2.;
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                format!("{value:.1} mm"),
                egui::FontId::proportional(12.),
                Color32::LIGHT_BLUE,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn member(width: &str, column: u32, level: u32) -> Member {
        let p: Preset = toml::from_str(
            "bom=[]\n[selection]\nwidth='S'\ndepth=3\nheight='1F'\nhidden_key=false\n",
        )
        .unwrap();
        let mut p = p;
        p.selection.width = width.into();
        Member {
            exterior: Default::default(),
            name: width.into(),
            placement: Placement {
                column,
                level,
                offset: [0.; 3],
            },
            preset: p,
        }
    }
    #[test]
    fn side_insertion_preserves_the_existing_stack() {
        let original = vec![
            member("S", 0, 0),
            member("S", 0, 1),
            member("M", 1, 0),
            member("S", 0, 0),
        ];
        let placed = position_relative(&original, &[3], 0, Side::Right).unwrap();
        assert_eq!(placed[0].placement.column, placed[1].placement.column);
        assert!(placed[3].placement.column < placed[2].placement.column);
        let p = crate::constraints::stacking::plan(&modules(&placed), 0.).unwrap();
        assert!((p.left[3] - p.left[0] - 164.8).abs() < 0.01);
    }
    #[test]
    fn pair_above_wide_object_reserves_space_and_unsupported_moves_fail() {
        let original = vec![
            member("L", 0, 0),
            member("M", 1, 0),
            member("S", 0, 0),
            member("S", 0, 0),
        ];
        let placed = position_relative(&original, &[2, 3], 0, Side::Above).unwrap();
        assert_eq!(placed[1].placement.column, 2);
        assert!(position_relative(&placed, &[2], 1, Side::Above).is_err());
        let detached = position_relative(&placed, &[2, 3], 1, Side::Ground).unwrap();
        assert_eq!(detached[2].placement.level, 0);
        assert_eq!(original[2].placement.level, 0);
    }
}
