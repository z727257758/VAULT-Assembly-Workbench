//! Independent, read-only sample scene for the overall color editor.
use super::*;
use crate::print_plate::{ColorTheme, Config};

#[derive(Default)]
pub(super) struct Preview {
    kind: AssemblyKind,
    pub bom: Vec<Bom>,
    pub scene: Scene,
    rx: Option<Receiver<Result<Scene, String>>>,
    renderer: Option<Renderer>,
    camera: Camera,
    pub selected: Option<String>,
    explosion: f32,
    ghost: bool,
    pub isolate: bool,
    error: Option<String>,
}
impl Preview {
    pub fn explode(&mut self, amount: f32) {
        self.explosion = amount;
        self.camera.fit(&self.display_scene(), amount);
    }
    fn matches_selection(&self, path: &str) -> bool {
        self.selected.as_deref().is_some_and(|selected| {
            crate::catalog::colors::component(selected) == crate::catalog::colors::component(path)
        })
    }
    fn display_scene(&self) -> Scene {
        let mut scene = self.scene.clone();
        if self.isolate && self.selected.is_some() {
            for part in &mut scene.parts {
                part.visible &= self.matches_selection(&part.pose.path);
            }
        }
        scene
    }
    fn select_at(&mut self, point: egui::Pos2, rect: egui::Rect) {
        let (o, d) = self.camera.ray(point, rect);
        self.selected = self
            .display_scene()
            .pick(o, d, self.explosion)
            .map(|hit| self.scene.parts[hit.part].pose.path.clone());
        if self.selected.is_none() {
            self.isolate = false;
        }
    }
    fn paint_selection(&self, ui: &egui::Ui, rect: egui::Rect) {
        let painter = ui.painter().with_clip_rect(rect);
        for part in self
            .scene
            .parts
            .iter()
            .filter(|p| p.visible && self.matches_selection(&p.pose.path))
        {
            let matrix = self.camera.matrix(rect.aspect_ratio()) * part.pose.matrix(self.explosion);
            let corners: [Option<egui::Pos2>; 8] = std::array::from_fn(|i| {
                let p = glam::Vec3::new(
                    if i & 1 == 0 {
                        part.mesh.min.x
                    } else {
                        part.mesh.max.x
                    },
                    if i & 2 == 0 {
                        part.mesh.min.y
                    } else {
                        part.mesh.max.y
                    },
                    if i & 4 == 0 {
                        part.mesh.min.z
                    } else {
                        part.mesh.max.z
                    },
                );
                let v = matrix.project_point3(p);
                (v.is_finite() && (0. ..=1.).contains(&v.z)).then(|| {
                    egui::pos2(
                        rect.center().x + v.x * rect.width() * 0.5,
                        rect.center().y - v.y * rect.height() * 0.5,
                    )
                })
            });
            for i in 0..8 {
                for axis in [1, 2, 4] {
                    if i & axis == 0
                        && let (Some(a), Some(b)) = (corners[i], corners[i | axis])
                    {
                        painter.line_segment([a, b], egui::Stroke::new(1.5, Color32::LIGHT_BLUE));
                    }
                }
            }
        }
    }
    /// Exercise the same projected ray selection used by pointer clicks.
    pub fn smoke_select_exploded_label(&mut self) -> Result<(), String> {
        self.explode(1.);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500., 500.));
        let part = self
            .scene
            .parts
            .iter()
            .find(|p| p.pose.path == self.kind.label_path())
            .cloned()
            .ok_or("Color preview has no label")?;
        for tri in part.mesh.vertices.chunks_exact(3).step_by(3) {
            let center = tri
                .iter()
                .map(|v| glam::Vec3::from(v.position))
                .sum::<glam::Vec3>()
                / 3.;
            let point = self
                .camera
                .matrix(1.)
                .project_point3(part.pose.matrix(1.).transform_point3(center));
            self.select_at(
                egui::pos2(250. + point.x * 250., 250. - point.y * 250.),
                rect,
            );
            if self.selected.as_deref() == Some(self.kind.label_path()) {
                return Ok(());
            }
        }
        Err("Exploded preview label cannot be selected by a projected ray".into())
    }

    pub fn ready(&self) -> bool {
        self.rx.is_none() && !self.scene.parts.is_empty()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn request(&mut self, catalog: &Catalog, home: &Path, kind: AssemblyKind) {
        self.kind = kind;
        self.selected = None;
        self.error = None;
        self.explosion = 0.;
        self.ghost = false;
        self.isolate = false;
        self.scene = Scene::default();
        let selection = Selection::default();
        let bom = match kind {
            AssemblyKind::Drawer => Ok(catalog.bom(&selection)),
            AssemblyKind::Sustainer => catalog.sustainer_bom(&selection, Default::default()),
        };
        let mut bom = match bom {
            Ok(bom) => bom,
            Err(e) => {
                self.error = Some(e);
                self.rx = None;
                self.bom.clear();
                return;
            }
        };
        Catalog::set_accessory(&mut bom, kind.label_path().into(), true, 1);
        self.bom = bom.clone();
        let root = catalog.root.clone();
        let home = home.to_owned();
        let (tx, rx) = mpsc::channel();
        // Replacing this receiver discards results from an older sample request.
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let result = (|| {
                let rules = crate::display::Rules::load(&home.join("config/display-rules.toml"))?;
                let mut scene =
                    Scene::load_with_rules(&root, &bom, "S3 / 1F".into(), true, &rules)?;
                crate::label::apply(
                    &mut scene,
                    &crate::label::Config {
                        enabled: true,
                        text: "VAULT".into(),
                        ..Default::default()
                    },
                )?;
                Ok(scene)
            })();
            let _ = tx.send(result);
        });
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        catalog: &Catalog,
        home: &Path,
        state: &eframe::egui_wgpu::RenderState,
        theme: &ColorTheme,
        z: bool,
    ) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.rx = None;
                    match result {
                        Ok(scene) => {
                            let renderer = self
                                .renderer
                                .get_or_insert_with(|| Renderer::new(state.clone()));
                            renderer.upload(&scene);
                            self.camera = Camera::default();
                            self.camera.fit(&scene, 0.);
                            self.scene = scene;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.error = Some(t(z, "示例加载中断", "Sample loading interrupted").into());
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(Duration::from_millis(50))
                }
            }
        }
        ui.strong(t(z, "三维配色预览 · S3 / 1F", "3D color preview · S3 / 1F"));
        let mut kind = self.kind;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut kind, AssemblyKind::Drawer, t(z, "抽屉", "Drawer"));
            ui.selectable_value(&mut kind, AssemblyKind::Sustainer, t(z, "盒子", "Box"));
            if ui.button(t(z, "适配画面", "Fit view")).clicked() {
                self.camera.fit(&self.display_scene(), self.explosion);
            }
        });
        if kind != self.kind {
            self.request(catalog, home, kind);
        }
        ui.horizontal(|ui| {
            if ui.button(t(z, "合拢", "Assemble")).clicked() {
                self.explode(0.);
            }
            if ui.button(t(z, "爆炸", "Explode")).clicked() {
                self.explode(1.);
            }
            let mut amount = self.explosion;
            if ui
                .add(egui::Slider::new(&mut amount, 0. ..=1.).text(t(z, "展开", "Spread")))
                .changed()
            {
                self.explode(amount);
            }
        });
        let can_isolate = self
            .scene
            .parts
            .iter()
            .any(|p| self.matches_selection(&p.pose.path));
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.ghost, t(z, "外壳透明", "Transparent shell"));
            if ui
                .add_enabled(
                    can_isolate,
                    egui::Checkbox::new(&mut self.isolate, t(z, "仅选中", "Isolate")),
                )
                .changed()
            {
                self.camera.fit(&self.display_scene(), self.explosion);
            }
            if ui.button(t(z, "显示全部", "Show all")).clicked() {
                self.isolate = false;
                self.camera.fit(&self.scene, self.explosion);
            }
        });
        ui.small(t(
            z,
            "拖动旋转，右键平移，滚轮缩放；点击零件在右侧配色。",
            "Drag to orbit, right drag to pan, scroll to zoom; click a part to edit its color.",
        ));
        if self.rx.is_some() {
            ui.spinner();
            ui.label(t(
                z,
                "正在后台加载最小规格示例…",
                "Loading the smallest sample…",
            ));
            return;
        }
        if let Some(e) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, e);
            return;
        }
        if self.scene.parts.is_empty() {
            return;
        }
        let size = egui::vec2(
            ui.available_width().max(40.),
            (ui.available_height() - 28.).max(100.),
        );
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = ui.input(|i| i.pointer.delta());
            self.camera.yaw -= delta.x * 0.008;
            self.camera.pitch = (self.camera.pitch + delta.y * 0.008).clamp(-1.55, 1.55);
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            let delta = ui.input(|i| i.pointer.delta());
            self.camera.pan[0] -= delta.x / size.x * 2.;
            self.camera.pan[1] += delta.y / size.y * 2.;
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.camera.zoom = (self.camera.zoom * (scroll * 0.002).exp()).clamp(0.1, 20.);
        }
        if response.double_clicked() {
            self.camera.fit(&self.display_scene(), self.explosion);
        }
        if response.clicked()
            && let Some(point) = response.interact_pointer_pos()
        {
            self.select_at(point, rect);
        }
        let display = self.display_scene();
        let renderer = self.renderer.as_mut().unwrap();
        let mut config = Config::default();
        theme.apply(&mut config);
        renderer.print_colors = Some(config);
        let d = ui.ctx().pixels_per_point();
        let id = renderer.render(
            0,
            [(size.x * d) as u32, (size.y * d) as u32],
            &display,
            &self.camera,
            ViewOptions {
                explosion: self.explosion,
                ghost: self.ghost,
                ..Default::default()
            },
        );
        ui.painter().image(
            id,
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
        self.paint_selection(ui, rect);
        ui.small(t(
            z,
            "示例只预览配色，不改变当前组合。",
            "This sample previews colors without changing your assembly.",
        ));
    }
}
