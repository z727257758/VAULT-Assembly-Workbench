//! Native pointer regression for the drawer divider workspace.
use super::*;
use crate::{
    constraints::{AssemblyRules, PartId},
    dividers::{self, Grid, Layout, Placement},
    render::MaterialOverride,
    scene::Target,
};
use egui::Rect;
use glam::{Mat3, Mat4, Vec3};

const DRAWER: Target = Target {
    member: 0,
    drawer: Some(1),
};
const SIBLING: Target = Target {
    member: 0,
    drawer: Some(0),
};

#[derive(Default)]
pub(super) struct State {
    pub(super) plan_rect: Option<Rect>,
    pub(super) add_rect: Option<Rect>,
    pub(super) done_rect: Option<Rect>,
    pub(super) rendered: Option<(Scene, Vec<MaterialOverride>)>,
    sibling_layout: Layout,
    entry_camera: Option<Mat4>,
    error: Option<String>,
}

impl App {
    pub(super) fn setup_drawer_divider_smoke(&mut self, ctx: &egui::Context) {
        let setup = (|| -> Result<(), String> {
            let mut preset = self.member_snapshot();
            preset.assembly_kind = AssemblyKind::Drawer;
            preset.selection = Selection {
                width: "S".into(),
                depth: 4,
                height: "2F".into(),
                hidden_key: false,
            };
            let mut housing = tower::drawer_pattern(&preset, &self.catalog, &["1F", "1F"], true)?;
            for (index, drawer) in housing.drawers.iter_mut().enumerate() {
                drawer.divider_layout = Layout {
                    enabled: true,
                    specification: dividers::spec(&drawer.selection),
                    ..Default::default()
                };
                drawer.drawer_extension = if index == 0 { 0.45 } else { 0.2 };
            }
            let sibling = &mut housing.drawers[0];
            let path = self
                .catalog
                .accessories(&sibling.selection)
                .into_iter()
                .find(|item| {
                    item.path.contains("/20MM TALL/") && dividers::code(&item.path) == Some(10)
                })
                .ok_or("Missing S4 full-width divider")?
                .path;
            sibling.divider_layout.items.push(Placement {
                path,
                start: [1, 8],
                vertical: false,
            });
            sibling
                .divider_layout
                .validate(&self.catalog, &sibling.selection)?;
            sibling
                .divider_layout
                .sync_bom(&mut sibling.bom, &sibling.selection);
            self.drawer_divider_smoke.sibling_layout = sibling.divider_layout.clone();
            self.members = vec![Member {
                name: "S4".into(),
                preset: housing,
                placement: Default::default(),
                exterior: Default::default(),
            }];
            self.restore_member(DRAWER.member);
            self.tower_edit_slot(DRAWER.member, DRAWER.drawer, ctx);
            Ok(())
        })();
        self.drawer_divider_smoke.error = setup.err();
    }

    fn check_drawer_divider_smoke_layout(&self, rows: &[i32]) -> Result<(), String> {
        let [sibling, active] = self.members[DRAWER.member].preset.drawers.as_slice() else {
            return Err("The fixture's drawers disappeared".into());
        };
        let layout = &self.divider_layout;
        if self.inline_edit.target != Some(DRAWER)
            || !layout.enabled
            || *layout != active.divider_layout
            || sibling.divider_layout != self.drawer_divider_smoke.sibling_layout
            || layout.items.len() != rows.len()
            || active.drawer_extension != 0.2
            || sibling.drawer_extension != 0.45
        {
            return Err(format!(
                "The drawer layout or stored extension changed unexpectedly: {layout:?}"
            ));
        }
        let grid = Grid::new(&self.selection)?;
        let expected = layout.bom(grid);
        let actual: Vec<_> = self
            .bom
            .iter()
            .filter(|item| item.enabled && item.group == dividers::GROUP)
            .map(|item| (&item.path, item.qty))
            .collect();
        if !expected
            .iter()
            .map(|item| (&item.path, item.qty))
            .eq(actual)
        {
            return Err("The divider bodies, pins or end pieces are missing from the BOM".into());
        }
        if layout.items.is_empty() {
            return Ok(());
        }
        let drawer = self
            .scene
            .parts
            .iter()
            .find(|part| DRAWER.contains(part) && part.pose.path.contains("/Boxes/"))
            .ok_or("Missing edited drawer mesh")?;
        for (index, (placement, &row)) in layout.items.iter().zip(rows).enumerate() {
            if placement.vertical
                || placement.start != [grid.full_start(false), row]
                || !grid.full(placement)
            {
                return Err(format!("Divider did not snap to row {row}: {placement:?}"));
            }
            let part = self
                .scene
                .parts
                .iter()
                .find(|part| {
                    part.pose.instance == DRAWER.instance(index as u32)
                        && part.pose.path == placement.path
                })
                .ok_or("Missing the inserted divider mesh")?;
            if !self.scene.relations.iter().any(|relation| {
                relation.child == PartId::of(part)
                    && relation.parent == PartId::of(drawer)
                    && relation
                        .label
                        .ends_with("Divider layout: hole-grid placement")
            }) {
                return Err("The divider mesh has no drawer mounting relation".into());
            }
            let [x, z] = grid.center(placement);
            let local = drawer.pose.matrix(0.).inverse() * part.pose.matrix(0.);
            if !(Vec3::new(x, drawer.mesh.min.y + 3.02 - part.mesh.min.y, -z))
                .abs_diff_eq(local.w_axis.truncate(), 0.02)
                || !Mat3::from_mat4(local).abs_diff_eq(Mat3::IDENTITY, 0.001)
            {
                return Err(format!("The divider mesh is not seated at row {row}"));
            }
        }
        Ok(())
    }

    fn check_drawer_divider_smoke_render(&self) -> Result<(), String> {
        let (display, materials) = self
            .drawer_divider_smoke
            .rendered
            .as_ref()
            .ok_or("Missing drawer divider editor render")?;
        let opened = AssemblyRules::mechanism(display, DRAWER)?;
        let stored = AssemblyRules::mechanism(&self.scene, DRAWER)?;
        let closed = AssemblyRules::mechanism(display, SIBLING)?;
        if (opened.value - opened.max).abs() > 0.01
            || stored.value >= opened.value
            || (closed.value - closed.min).abs() > 0.01
            || display.parts.len() != self.scene.parts.len()
            || materials.len() != display.parts.len()
        {
            return Err("The editor preview did not open only the edited drawer".into());
        }
        let mut dividers = 0;
        let mut sibling_visible = false;
        for (part, material) in display
            .parts
            .iter()
            .zip(materials)
            .filter(|(part, _)| part.visible)
        {
            let alpha = match material {
                MaterialOverride::Opacity(value) => *value,
                MaterialOverride::Color(value) => value[3],
            };
            let divider = part.pose.path.contains("/Grid Dividers/");
            if DRAWER.contains(part) && divider {
                dividers += usize::from(dividers::code(&part.pose.path).is_some());
                if alpha != 1. {
                    return Err("An edited divider or accessory was rendered translucent".into());
                }
            }
            if SIBLING.contains(part) {
                sibling_visible = true;
                if alpha != self.interior_exterior_opacity as f32 / 100. {
                    return Err("A sibling part has the wrong exterior opacity".into());
                }
            }
        }
        if !sibling_visible || dividers != self.divider_layout.items.len() {
            return Err("The rendered assembly lost a divider or surrounding parts".into());
        }
        Ok(())
    }

    pub(super) fn drawer_divider_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(16));
        let folder = self.home.join("output/drawer-divider-editor-ui");
        let run = (|| -> Result<(), String> {
            if let Some(error) = self.drawer_divider_smoke.error.take() {
                return Err(error);
            }
            if self.smoke_start.elapsed() > Duration::from_secs(120)
                || (!self.busy && !self.preview.has_pending() && self.failed_preview.is_some())
            {
                return Err(format!("Stage {}: {}", self.smoke_stage, self.status));
            }
            if self.busy || self.preview.has_pending() || self.editor_smoke_pointer.pending() {
                return Ok(());
            }
            let stage = self.smoke_stage;
            match stage {
                0 => {
                    if self.scene_key != "combination" || self.inline_edit.pending_focus {
                        return Ok(());
                    }
                    self.check_drawer_divider_smoke_layout(&[])?;
                    self.drawer_divider_smoke.entry_camera =
                        Some(self.camera.matrix(ctx.available_rect().aspect_ratio()));
                    self.open_drawer_divider_editor(ctx);
                    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                }
                1 => {
                    let Some(add) = self.drawer_divider_smoke.add_rect else {
                        return Ok(());
                    };
                    self.check_drawer_divider_smoke_render()?;
                    self.editor_smoke_pointer.click(add.center());
                }
                2 => {
                    let grid = Grid::new(&self.selection)?;
                    let plan = self
                        .drawer_divider_smoke
                        .plan_rect
                        .ok_or("Missing divider plan")?;
                    let [_, z] = grid.point([5.5, 4.]);
                    self.editor_smoke_pointer.click(egui::pos2(
                        plan.center().x,
                        plan.top() + (z - grid.rear) / (grid.front - grid.rear) * plan.height(),
                    ));
                }
                3 => {
                    self.check_drawer_divider_smoke_layout(&[4])?;
                    self.check_drawer_divider_smoke_render()?;
                    let point = self
                        .drawer_divider_view
                        .project_cell([5.5, 9.])
                        .ok_or("Missing projected hole grid point")?;
                    self.editor_smoke_pointer.click(point);
                }
                4 => {
                    self.check_drawer_divider_smoke_layout(&[4, 9])?;
                    self.check_drawer_divider_smoke_render()?;
                    if self.divider_editor.selected_index() != Some(1) {
                        return Err("The model did not select the second inserted divider".into());
                    }
                    if !self.capture_native_smoke(ctx, &folder.join("s4-drawer-dividers.png"))? {
                        return Ok(());
                    }
                    let done = self
                        .drawer_divider_smoke
                        .done_rect
                        .ok_or("Missing Done button")?;
                    self.editor_smoke_pointer.click(done.center());
                }
                _ => {
                    self.check_drawer_divider_smoke_layout(&[4, 9])?;
                    let camera = self
                        .drawer_divider_smoke
                        .entry_camera
                        .ok_or("Missing entry camera")?;
                    if self.divider_editor.open
                        || !camera.abs_diff_eq(
                            self.camera.matrix(ctx.available_rect().aspect_ratio()),
                            0.0001,
                        )
                    {
                        return Err("Done did not restore the original drawer view".into());
                    }
                    let saved = self.project_snapshot(self.member_snapshot());
                    let text = toml::to_string(&saved).map_err(|e| e.to_string())?;
                    let restored: CombinationPreset =
                        toml::from_str(&text).map_err(|e| e.to_string())?;
                    restored.validate(&self.catalog)?;
                    if !restored.members[DRAWER.member]
                        .preset
                        .drawers
                        .iter()
                        .zip(&self.members[DRAWER.member].preset.drawers)
                        .all(|(saved, actual)| saved.divider_layout == actual.divider_layout)
                    {
                        return Err("Project serialization lost a drawer divider layout".into());
                    }
                    std::fs::write(folder.join("smoke-result.txt"), "PASS: Clicks in the plan and opened model added two dividers to the selected S4 1F drawer. The open assembly preview rendered both fitted divider meshes and retained the sibling drawer. The BOM included their pins and end pieces. Done restored the assembly view and both layouts survived project serialization.")
                        .map_err(|e| e.to_string())?;
                    let _ = std::fs::remove_file(folder.join("smoke-error.txt"));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return Ok(());
                }
            }
            self.smoke_stage += 1;
            Ok(())
        })();
        if let Err(error) = run {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("smoke-error.txt"), error);
            let _ = std::fs::remove_file(folder.join("smoke-result.txt"));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
