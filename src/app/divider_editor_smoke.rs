//! Native pointer regression for the linked box divider views.
use super::*;
use crate::constraints::{AssemblyRules, PartId};
use crate::render::MaterialOverride;
use crate::scene::Target;
use crate::sustainer_dividers::{self as dividers, Axis, Placement};
use egui::{Pos2, Rect};
use glam::Mat4;

const BOX: Target = Target {
    member: 0,
    drawer: None,
};

fn slot(lane: usize, slot: usize) -> Placement {
    Placement {
        axis: Axis::Width,
        lane,
        slot,
    }
}

#[derive(Default)]
pub(super) struct State {
    pub(super) done_rect: Option<Rect>,
    seats: BTreeMap<Placement, Mat4>,
    entry_camera: Option<Mat4>,
    pub(super) rendered: Option<(Scene, Vec<MaterialOverride>)>,
    error: Option<String>,
}

impl App {
    pub(super) fn setup_divider_editor_smoke(&mut self, ctx: &egui::Context) {
        let setup = (|| -> Result<(), String> {
            let mut p = self.member_snapshot();
            p.assembly_kind = AssemblyKind::Sustainer;
            p.drawers.clear();
            p.selection = Selection {
                width: "L".into(),
                depth: 5,
                height: "2F".into(),
                hidden_key: false,
            };
            p.sustainer_options = SustainerOptions {
                body: SustainerBody::Sealed,
                split_roof: false,
            };
            p.sustainer_angle = 0.;
            tower::configure(&mut p, &self.catalog, true)?;
            p.sustainer_dividers.specification = dividers::spec(&p.selection, p.sustainer_options);
            let mut layers = Vec::new();
            for level in [0, 1] {
                p.sustainer_angle = if level == BOX.member as u32 { 0. } else { 70. };
                p.sustainer_dividers.items = if level != BOX.member as u32 {
                    vec![slot(1, 2)]
                } else {
                    (0..2)
                        .flat_map(|lane| (0..4).map(move |row| slot(lane, row)))
                        .collect()
                };
                p.sustainer_dividers.sync_bom(
                    &mut p.bom,
                    &self.catalog,
                    &p.selection,
                    p.sustainer_options,
                )?;
                layers.push(Member {
                    name: "L5 2F".into(),
                    preset: p.clone(),
                    placement: crate::scene::Placement {
                        level,
                        ..Default::default()
                    },
                    exterior: Default::default(),
                });
            }
            self.members = layers;
            self.restore_member(BOX.member);
            self.tower_edit_slot(BOX.member, None, ctx);
            Ok(())
        })();
        self.divider_editor_smoke.error = setup.err();
    }

    fn divider_smoke_seats(&self) -> Result<BTreeMap<Placement, Mat4>, String> {
        let mut seats = BTreeMap::new();
        for (i, placement) in self.sustainer_dividers.items.iter().enumerate() {
            let (path, instance) = self
                .sustainer_dividers
                .selected_part(i, &self.catalog, &self.selection, self.sustainer_options)
                .ok_or("Missing divider slot source")?;
            let part = self
                .scene
                .parts
                .iter()
                .find(|part| part.pose.instance == BOX.instance(instance) && part.pose.path == path)
                .ok_or("Missing divider mesh at the active layer")?;
            if !self.scene.relations.iter().any(|r| {
                r.child == PartId::of(part)
                    && r.parent.path.contains("/Modules/")
                    && r.label.contains("Divider layout: Sustainer wall slots")
            }) {
                return Err(format!("Divider at {placement:?} has no housing mount"));
            }
            seats.insert(placement.clone(), part.pose.matrix(0.));
        }
        Ok(seats)
    }

    fn check_divider_smoke(&self, missing: Option<&Placement>) -> Result<(), String> {
        let layout = &self.sustainer_dividers;
        let count = 8 - usize::from(missing.is_some());
        let meshes = self
            .scene
            .parts
            .iter()
            .filter(|p| BOX.contains(p) && p.pose.path.starts_with(dividers::PREFIX))
            .count();
        let qty: u32 = self
            .bom
            .iter()
            .filter(|b| b.enabled && b.group == dividers::GROUP)
            .map(|b| b.qty)
            .sum();
        if self.inline_edit.target != Some(BOX)
            || layout.items.len() != count
            || meshes != count
            || qty as usize != count
            || *layout != self.members[BOX.member].preset.sustainer_dividers
            || self.members[1].preset.sustainer_dividers.items != [slot(1, 2)]
            || self.members[1].preset.sustainer_angle != 70.
            || (AssemblyRules::mechanism(
                &self.scene,
                Target {
                    member: 1,
                    drawer: None,
                },
            )?
            .value
                - 70.)
                .abs()
                > 0.01
            || self.sustainer_angle != 0.
            || AssemblyRules::mechanism(&self.scene, BOX)?.value.abs() > 0.01
        {
            return Err(format!(
                "Layout, member, BOM or solved mesh count differs at stage {}: {layout:?}, meshes={meshes}, qty={qty}",
                self.smoke_stage
            ));
        }
        let seats = self.divider_smoke_seats()?;
        for (slot, original) in &self.divider_editor_smoke.seats {
            let ok = if missing == Some(slot) {
                !seats.contains_key(slot)
            } else {
                seats
                    .get(slot)
                    .is_some_and(|seat| seat.abs_diff_eq(*original, 0.02))
            };
            if !ok {
                return Err(format!("Incorrect 3D seat for {slot:?}"));
            }
        }
        Ok(())
    }

    fn check_divider_preview_assembly(&self) -> Result<(), String> {
        let (display, materials) = self
            .divider_editor_smoke
            .rendered
            .as_ref()
            .ok_or("Missing divider editor render")?;
        let lid = AssemblyRules::mechanism(display, BOX)?;
        let other = Target {
            member: 1,
            drawer: None,
        };
        let closed = AssemblyRules::mechanism(display, other)?;
        if display.parts.len() != self.scene.parts.len()
            || !lid.supports_upper
            || lid.slide <= 0.
            || lid.value < 70.
            || closed.value != closed.min
            || closed.latch != 0.
            || closed.slide != 0.
            || self.scene.member_bounds != display.member_bounds
        {
            return Err("Divider editor did not open the lower case in the full assembly".into());
        }
        let mut expected = self.scene.clone();
        AssemblyRules::set_lid_angle_for(&mut expected, &closed.child, closed.min)?;
        let mut other_visible = false;
        for ((source, shown), material) in expected.parts.iter().zip(&display.parts).zip(materials)
        {
            if shown.visible {
                let opacity = match material {
                    MaterialOverride::Opacity(value) => *value,
                    MaterialOverride::Color(color) => color[3],
                };
                let expected = if BOX.contains(shown) {
                    1.
                } else {
                    self.interior_exterior_opacity as f32 / 100.
                };
                if opacity != expected {
                    return Err(format!(
                        "Incorrect divider preview opacity for {}",
                        shown.pose.path
                    ));
                }
            }
            if !BOX.contains(source) {
                other_visible |= shown.visible;
                if PartId::of(source) != PartId::of(shown)
                    || source.mesh_key() != shown.mesh_key()
                    || source.visible != shown.visible
                    || !source
                        .pose
                        .matrix(0.)
                        .abs_diff_eq(shown.pose.matrix(0.), 0.02)
                {
                    return Err("Divider editor modified a neighboring member".into());
                }
            }
        }
        if !other_visible {
            return Err("Divider editor hid the other member".into());
        }
        Ok(())
    }

    fn divider_plan_point(&self, slot: &Placement) -> Option<Pos2> {
        let rect = self.sustainer_divider_editor.plan_rect?;
        let geometry = self.scene.interior_geometry(BOX.member)?;
        let [a, b] = geometry.line(slot)?;
        let [left, rear, right, front] = geometry.bounds;
        let x = ((a[0] + b[0]) * 0.5 - left) / (right - left);
        let y = ((a[1] + b[1]) * 0.5 - rear) / (front - rear);
        Some(rect.left_top() + egui::vec2(rect.width() * x, rect.height() * y))
    }

    pub(super) fn divider_editor_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(16));
        let folder = self.home.join("output/divider-editor-ui");
        let run = (|| -> Result<(), String> {
            if let Some(error) = self.divider_editor_smoke.error.take() {
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
            let left = slot(0, 1);
            let right = slot(1, 2);
            if stage == 0 {
                if self.scene_key != "combination" || self.inline_edit.pending_focus {
                    return Ok(());
                }
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                self.check_divider_smoke(None)?;
                self.divider_editor_smoke.seats = self.divider_smoke_seats()?;
                self.divider_editor_smoke.entry_camera =
                    Some(self.camera.matrix(ctx.available_rect().aspect_ratio()));
                self.open_sustainer_divider_editor(ctx);
            } else if stage == 5 {
                self.check_divider_smoke(Some(&right))?;
                let expected = self
                    .divider_editor_smoke
                    .entry_camera
                    .ok_or("Missing original camera")?;
                if self.sustainer_divider_editor.open
                    || !expected.abs_diff_eq(
                        self.camera.matrix(ctx.available_rect().aspect_ratio()),
                        0.0001,
                    )
                {
                    return Err("Done did not restore the assembly view and camera".into());
                }
                let saved = self.project_snapshot(self.member_snapshot());
                let text = toml::to_string(&saved).map_err(|e| e.to_string())?;
                let restored: CombinationPreset =
                    toml::from_str(&text).map_err(|e| e.to_string())?;
                restored.validate(&self.catalog)?;
                for (original, loaded) in self.members.iter().zip(&restored.members) {
                    if original.preset.sustainer_dividers != loaded.preset.sustainer_dividers {
                        return Err("Saved project lost the edited divider layout".into());
                    }
                }
                std::fs::write(folder.join("smoke-result.txt"), "PASS: L5 2F sealed case, eight dividers across two lanes. The lower case opened in the full assembly with the other member visible and stationary. Native plan and 3D slot clicks, mounted mesh transforms, BOM, camera restoration and project roundtrip.").map_err(|e| e.to_string())?;
                if folder.join("smoke-error.txt").exists() {
                    std::fs::remove_file(folder.join("smoke-error.txt"))
                        .map_err(|e| e.to_string())?;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return Ok(());
            } else {
                let absent = match stage {
                    2 => Some(&left),
                    4 => Some(&right),
                    _ => None,
                };
                self.check_divider_smoke(absent)?;
                if !self.divider_model.loaded(&self.sustainer_dividers) {
                    return Ok(());
                }
                self.check_divider_preview_assembly()?;
                let Some(plan_point) = self.divider_plan_point(&left) else {
                    return Ok(());
                };
                let slot = if stage == 3 { &right } else { &left };
                let Some(model_point) = self.divider_model.slot_point(slot) else {
                    return Ok(());
                };
                let plan = self
                    .sustainer_divider_editor
                    .plan_rect
                    .ok_or("Missing plan")?;
                if plan.contains(model_point) || !ctx.available_rect().contains(model_point) {
                    return Err(
                        "The 3D slot is occluded by the plan or outside the workspace".into(),
                    );
                }
                let click = match stage {
                    1 => {
                        if !self.capture_native_smoke(ctx, &folder.join("l5-2f.png"))? {
                            return Ok(());
                        }
                        plan_point
                    }
                    2 | 3 => model_point,
                    4 => {
                        if !self.capture_native_smoke(ctx, &folder.join("after-3d.png"))? {
                            return Ok(());
                        }
                        self.divider_editor_smoke
                            .done_rect
                            .ok_or("Missing Done button")?
                            .center()
                    }
                    _ => return Err(format!("Unknown divider smoke stage {stage}")),
                };
                self.editor_smoke_pointer.click(click);
                self.interior_exterior_opacity = match stage {
                    1 => 20,
                    2 => 50,
                    _ => 30,
                };
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
