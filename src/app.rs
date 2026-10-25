use crate::{
    catalog::{Bom, Catalog, Selection},
    render::{Camera, Renderer, ViewOptions},
    scene::{Definition, Scene},
};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

fn t(zh: bool, cn: &'static str, en: &'static str) -> &'static str {
    if zh { cn } else { en }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AssemblyKind {
    #[default]
    Drawer,
    Sustainer,
}
impl AssemblyKind {
    fn label(self, zh: bool) -> &'static str {
        match self {
            Self::Drawer => t(zh, "抽屉", "Drawer"),
            Self::Sustainer => t(zh, "盒子（Sustainer）", "Box (Sustainer)"),
        }
    }
    fn supported(self) -> bool {
        self == Self::Drawer
    }
}
#[derive(Serialize, Deserialize)]
struct Preset {
    #[serde(default)]
    print_config: Option<crate::print_plate::Config>,
    #[serde(default)]
    gridfinity: crate::gridfinity::Layout,
    #[serde(default)]
    label: crate::label::Config,
    #[serde(default)]
    assembly_kind: AssemblyKind,
    selection: Selection,
    bom: Vec<Bom>,
    #[serde(default)]
    divider_layout: crate::dividers::Layout,
}
#[derive(Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    export: crate::export::Options,
    #[serde(default)]
    model_root: String,
    language: String,
}
impl Settings {
    fn load(home: &Path) -> Option<Self> {
        let read = |name: &str| {
            std::fs::read_to_string(home.join(name))
                .ok()
                .and_then(|s| toml::from_str::<Self>(s.trim_start_matches('\u{feff}')).ok())
        };
        read("state/settings.toml").or_else(|| {
            read("config/default.toml").map(|mut s| {
                // Even an old distributed default must not auto-open its author's model library.
                s.model_root.clear();
                s
            })
        })
    }
    fn startup_root(&self) -> Option<PathBuf> {
        let root = PathBuf::from(&self.model_root);
        (!root.as_os_str().is_empty() && root.is_absolute() && root.is_dir()).then_some(root)
    }
}

enum Loaded {
    Catalog(Result<Catalog, String>),
    Scene {
        revision: u64,
        finished: bool,
        key: String,
        result: Result<Scene, String>,
    },
    Image(Result<(String, egui::ColorImage), String>),
    Collision(Result<crate::collision::Report, String>),
}
#[derive(Clone, Serialize)]
struct SceneRequest {
    gridfinity: crate::gridfinity::Layout,
    home: PathBuf,
    label: crate::label::Config,
    items: Vec<Bom>,
    root: PathBuf,
    title: String,
    assembly: bool,
    key: String,
    divider_layout: crate::dividers::Layout,
    selection: Selection,
}

type ExportResult = Result<PathBuf, String>;

struct DividerView {
    camera: Camera,
    explosion: f32,
    dimensions: bool,
    key: String,
    shown_key: String,
}
struct AssemblyView {
    key: String,
    scene: Scene,
    camera: Camera,
    explosion: f32,
    selected: Option<usize>,
    dimensions: bool,
    ghost: bool,
}
pub struct App {
    gridfinity: crate::gridfinity::Layout,
    gridfinity_open: bool,
    gridfinity_editor: crate::gridfinity_editor::Editor,
    gridfinity_selected: Option<u32>,
    gridfinity_error: Option<String>,
    home: PathBuf,
    catalog: Catalog,
    last_model_root: PathBuf,
    selection: Selection,
    bom: Vec<Bom>,
    divider_layout: crate::dividers::Layout,
    divider_editor: crate::divider_editor::Editor,
    divider_view: Option<DividerView>,
    layout_cache: BTreeMap<String, crate::dividers::Layout>,
    scene: Scene,
    renderer: Renderer,
    camera: Camera,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
    busy: bool,
    preview: crate::preview::Queue<SceneRequest>,
    failed_preview: Option<SceneRequest>,
    assembly_view: Option<AssemblyView>,
    status: String,
    zh: bool,
    tab: u8,
    query: String,
    category: usize,
    explosion: f32,
    selected: Option<usize>,
    ghost: bool,
    dimensions: bool,
    calibration: bool,
    debug_ui: bool,
    export_options: crate::export::Options,
    export_open: bool,
    colors_open: bool,
    export_rx: Option<Receiver<ExportResult>>,
    export_last: Option<PathBuf>,
    label: crate::label::Config,
    label_open: bool,
    image: Option<(String, egui::TextureHandle)>,
    smoke: bool,
    smoke_start: Instant,
    smoke_frame: u32,
    screenshot_requested: bool,
    smoke_stage: u8,
    scene_key: String,
    joints_open: bool,
    fixed_parent: Option<usize>,
    last_hit: Option<crate::constraints::SurfaceHit>,
    parent_face: Option<crate::constraints::SurfaceHit>,
    child_face: Option<crate::constraints::SurfaceHit>,
    mate_gap: f32,
    mate_twist: f32,
    collision_report: Option<crate::collision::Report>,
    collision_tolerance: f32,
}
impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        home: PathBuf,
        smoke: bool,
        debug_ui: bool,
    ) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let mut fonts = egui::FontDefinitions::default();
        for path in ["msyh.ttc", "simhei.ttf"]
            .into_iter()
            .filter_map(crate::paths::windows_font)
        {
            if let Ok(data) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("chinese".into(), egui::FontData::from_owned(data).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("chinese".into());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("chinese".into());
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(23, 29, 37);
        style.visuals.window_fill = Color32::from_rgb(23, 29, 37);
        style.visuals.selection.bg_fill = Color32::from_rgb(35, 87, 115);
        style.spacing.item_spacing = egui::vec2(8., 8.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
        cc.egui_ctx.set_style(style);
        let settings = Settings::load(&home);
        let root = settings.as_ref().and_then(Settings::startup_root);
        let last_model_root = settings
            .as_ref()
            .map(|s| PathBuf::from(&s.model_root))
            .unwrap_or_default();
        let zh = settings.as_ref().is_none_or(|s| s.language != "en");
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::Title(crate::app_name(zh).into()));
        let (tx, rx) = mpsc::channel();
        let renderer = Renderer::new(
            cc.wgpu_render_state
                .clone()
                .expect("wgpu renderer required"),
        );
        let mut app = Self {
            gridfinity: Default::default(),
            gridfinity_open: false,
            gridfinity_editor: Default::default(),
            gridfinity_selected: None,
            gridfinity_error: None,
            home,
            catalog: Catalog::default(),
            last_model_root,
            selection: Selection::default(),
            bom: vec![],
            divider_layout: Default::default(),
            divider_editor: Default::default(),
            divider_view: None,
            layout_cache: Default::default(),
            scene: Scene::default(),
            renderer,
            camera: Camera::default(),
            tx,
            rx,
            busy: false,
            preview: Default::default(),
            failed_preview: None,
            assembly_view: None,
            status: String::new(),
            zh,
            tab: 0,
            query: String::new(),
            category: 0,
            explosion: 1.,
            selected: None,
            ghost: false,
            dimensions: false,
            calibration: false,
            debug_ui,
            export_options: settings
                .as_ref()
                .map(|s| s.export.clone())
                .unwrap_or_default(),
            export_open: false,
            colors_open: false,
            export_rx: None,
            export_last: None,
            label: Default::default(),
            label_open: false,
            image: None,
            smoke,
            smoke_start: Instant::now(),
            smoke_frame: 0,
            screenshot_requested: false,
            smoke_stage: 0,
            scene_key: String::new(),
            joints_open: false,
            fixed_parent: None,
            last_hit: None,
            parent_face: None,
            child_face: None,
            mate_gap: 0.2,
            mate_twist: 0.,
            collision_report: None,
            collision_tolerance: 0.01,
        };
        if let Some(root) = root {
            app.scan(root, &cc.egui_ctx);
        } else if !app.last_model_root.as_os_str().is_empty() {
            app.status = format!(
                "{} {}",
                t(
                    zh,
                    "上次模型库目录不可用，请点击“打开模型库”重新选择：",
                    "Previous library unavailable. Use Open library to locate it:"
                ),
                app.last_model_root.display()
            );
        } else {
            app.status = t(
                zh,
                "请选择 VAULT 模型库目录",
                "Choose a VAULT model library folder",
            )
            .into();
        }
        app
    }
    fn scan(&mut self, root: PathBuf, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = t(self.zh, "正在扫描模型库…", "Scanning library…").into();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result=Catalog::scan(root).and_then(|c| if c.stl_count()==0 {Err("所选目录没有 STL 模型，请选择模型库根目录 / No STL models in the selected folder".into())} else {Ok(c)});
            let _ = tx.send(Loaded::Catalog(result));
            ctx.request_repaint();
        });
    }
    fn start_scene(
        &mut self,
        items: Vec<Bom>,
        root: PathBuf,
        title: String,
        assembly: bool,
        ctx: &egui::Context,
    ) {
        let key = if assembly {
            format!(
                "{}{}-{}",
                self.selection.width, self.selection.depth, self.selection.height
            )
        } else {
            String::new()
        };
        self.preview.schedule(
            SceneRequest {
                gridfinity: self.gridfinity.clone(),
                home: self.home.clone(),
                label: self.label.clone(),
                items,
                root,
                title,
                assembly,
                key,
                divider_layout: if assembly {
                    self.divider_layout.clone()
                } else {
                    Default::default()
                },
                selection: self.selection.clone(),
            },
            Instant::now(),
            if assembly {
                Duration::from_millis(300)
            } else {
                Duration::ZERO
            },
        );
        self.failed_preview = None;
        self.status = t(self.zh, "正在更新预览…", "Updating preview…").into();
        ctx.request_repaint();
    }
    fn pump_preview(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let Some((revision, request)) = self.preview.take_ready(Instant::now()) else {
            return;
        };
        self.busy = true;
        let cancel = self.preview.cancellation();
        self.failed_preview = Some(request.clone());
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let display_rules = self.home.join("config/display-rules.toml");
        std::thread::spawn(move || {
            let result = crate::display::Rules::load(&display_rules).and_then(|rules| {
                let originals: Vec<_> = request
                    .items
                    .iter()
                    .filter(|b| !b.path.starts_with(crate::gridfinity::PREFIX))
                    .cloned()
                    .collect();
                crate::preview::check(Some(&cancel))?;
                // Reuse the assembled non-Gridfinity scene while only bins change.
                static BASE: std::sync::Mutex<Option<(String, Scene)>> =
                    std::sync::Mutex::new(None);
                let mut base_request = request.clone();
                base_request.gridfinity = Default::default();
                base_request.items = originals.clone();
                let source_stamps: Vec<_> = originals
                    .iter()
                    .filter(|b| b.enabled)
                    .map(|b| {
                        let meta = std::fs::metadata(request.root.join(&b.path));
                        meta.map(|m| (m.len(), m.modified().ok())).ok()
                    })
                    .collect();
                let signature = format!(
                    "{}{}{:?}",
                    toml::to_string(&base_request).map_err(|e| e.to_string())?,
                    std::fs::read_to_string(&display_rules).unwrap_or_default(),
                    source_stamps
                );
                let previous = BASE
                    .lock()
                    .unwrap()
                    .as_ref()
                    .filter(|(key, _)| key == &signature)
                    .map(|(_, scene)| scene.clone());
                let mut scene = if let Some(scene) = previous {
                    scene
                } else {
                    let mut scene = if originals.is_empty() && !request.assembly {
                        Scene {
                            title: request.title.clone(),
                            ..Default::default()
                        }
                    } else {
                        Scene::load_with_rules(
                            &request.root,
                            &originals,
                            request.title.clone(),
                            request.assembly,
                            &rules,
                        )?
                    };
                    if request.assembly {
                        crate::dividers::apply(
                            &mut scene,
                            &request.divider_layout,
                            &request.selection,
                        )?;
                    }
                    crate::label::apply(&mut scene, &request.label)?;
                    *BASE.lock().unwrap() = Some((signature, scene.clone()));
                    scene
                };
                crate::preview::check(Some(&cancel))?;
                if !request.gridfinity.bins.is_empty() {
                    let mut draft = scene.clone();
                    crate::gridfinity::apply_preview(
                        &mut draft,
                        &request.home,
                        &request.gridfinity,
                        &request.selection,
                        &request.items,
                        true,
                        Some(&cancel),
                    )?;
                    draft.title += "（简化预览；正在生成精确模型）";
                    let _ = tx.send(Loaded::Scene {
                        revision,
                        finished: false,
                        key: request.key.clone(),
                        result: Ok(draft),
                    });
                    ctx.request_repaint();
                }
                crate::gridfinity::apply_preview(
                    &mut scene,
                    &request.home,
                    &request.gridfinity,
                    &request.selection,
                    &request.items,
                    false,
                    Some(&cancel),
                )?;
                Ok(scene)
            });
            let _ = tx.send(Loaded::Scene {
                revision,
                finished: true,
                key: request.key,
                result,
            });
            ctx.request_repaint();
        });
    }
    fn assembly(&mut self, ctx: &egui::Context) {
        let spec = crate::dividers::spec(&self.selection);
        if self.divider_layout.specification != spec {
            if !self.divider_layout.specification.is_empty() {
                self.layout_cache.insert(
                    self.divider_layout.specification.clone(),
                    self.divider_layout.clone(),
                );
            }
            self.divider_layout =
                self.layout_cache
                    .get(&spec)
                    .cloned()
                    .unwrap_or(crate::dividers::Layout {
                        specification: spec,
                        ..Default::default()
                    });
            self.divider_editor.reset();
        }
        if let Err(e) = self.divider_layout.validate(&self.catalog, &self.selection) {
            self.status = e;
            return;
        }
        self.divider_layout.sync_bom(&mut self.bom, &self.selection);
        if self.divider_layout.enabled
            || !self
                .bom
                .iter()
                .any(|b| b.enabled && b.path.contains("/Gridfinity Baseplate/"))
        {
            self.gridfinity.bins.clear();
            self.gridfinity_open = false;
        }
        self.gridfinity.baseplate = self
            .bom
            .iter()
            .find(|b| b.enabled && b.path.contains("/Gridfinity Baseplate/"))
            .map(|b| b.path.clone())
            .unwrap_or_default();
        if let Err(e) = self.gridfinity.validate(&self.selection) {
            self.status = e;
            return;
        }
        self.gridfinity.sync_bom(&mut self.bom);
        let title = format!(
            "{}{} / {}",
            self.selection.width, self.selection.depth, self.selection.height
        );
        self.start_scene(
            self.bom.clone(),
            self.catalog.root.clone(),
            title,
            true,
            ctx,
        );
    }
    fn single(&mut self, path: String, ctx: &egui::Context) {
        let name = crate::accessories::info(&path, self.zh)
            .map(|m| m.name)
            .unwrap_or_else(|| {
                Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into()
            });
        self.start_scene(
            vec![Bom {
                path,
                qty: 1,
                group: "Single".into(),
                enabled: true,
            }],
            self.catalog.root.clone(),
            name,
            false,
            ctx,
        );
    }
    fn save_settings(&self) -> Result<(), String> {
        let settings = Settings {
            export: self.export_options.clone(),
            model_root: self.last_model_root.to_string_lossy().into(),
            language: if self.zh { "zh" } else { "en" }.into(),
        };
        self.write_state(
            "settings.toml",
            toml::to_string_pretty(&settings).map_err(|e| e.to_string())?,
        )
    }
    fn write_state(&self, name: &str, text: String) -> Result<(), String> {
        let dir = self.home.join("state");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(name), text).map_err(|e| e.to_string())
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.rx.try_recv() {
            self.busy = matches!(
                &result,
                Loaded::Scene {
                    finished: false,
                    ..
                }
            );
            match result {
                Loaded::Catalog(Ok(c)) => {
                    self.last_model_root = c.root.clone();
                    self.catalog = c;
                    self.divider_layout = Default::default();
                    self.label = Default::default();
                    self.gridfinity = Default::default();
                    self.layout_cache.clear();
                    self.divider_editor.reset();
                    self.assembly_view = None;
                    self.scene_key.clear();
                    self.bom = self.catalog.bom(&self.selection);
                    self.status = format!(
                        "{} STL · {}",
                        self.catalog.stl_count(),
                        self.catalog.root.display()
                    );
                    if let Err(e) = self.save_settings() {
                        self.status = e;
                    }
                    if self.catalog.stl_count() > 0
                        && self
                            .catalog
                            .resolve("01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F.stl")
                            .is_some()
                    {
                        self.assembly(ctx);
                    }
                }
                Loaded::Scene {
                    revision,
                    finished,
                    key,
                    result,
                } => {
                    if !self.preview.is_current(revision) {
                        continue;
                    }
                    let mut s = match result {
                        Ok(scene) => scene,
                        Err(e) => {
                            self.status = format!(
                                "{}: {e}",
                                t(
                                    self.zh,
                                    "预览更新失败，保留原画面",
                                    "Preview update failed; previous view retained"
                                )
                            );
                            continue;
                        }
                    };
                    if finished {
                        self.failed_preview = None;
                    }
                    if self.scene.assembly {
                        self.assembly_view = Some(AssemblyView {
                            key: self.scene_key.clone(),
                            scene: self.scene.clone(),
                            camera: self.camera.clone(),
                            explosion: self.explosion,
                            selected: self.selected,
                            dimensions: self.dimensions,
                            ghost: self.ghost,
                        });
                    }
                    let previous = self
                        .assembly_view
                        .as_ref()
                        .filter(|v| s.assembly && v.key == key);
                    let preserve = previous.is_some();
                    let selected_id = previous
                        .and_then(|v| v.selected.and_then(|i| v.scene.parts.get(i)))
                        .map(crate::constraints::PartId::of);
                    self.scene_key = key;
                    if s.assembly
                        && let Ok(text) = std::fs::read_to_string(
                            self.home
                                .join(format!("state/assembly-{}.toml", self.scene_key)),
                        )
                    {
                        match toml::from_str::<Definition>(&text) {
                            Ok(def) => s.apply(&def),
                            Err(e) => self.status = format!("Assembly definition: {e}"),
                        }
                    }
                    if preserve {
                        let previous = previous.unwrap();
                        s.retain_assembly_state(&previous.scene);
                        self.camera = previous.camera.clone();
                        self.explosion = previous.explosion;
                        self.dimensions = previous.dimensions;
                        self.ghost = previous.ghost;
                        self.selected = selected_id.and_then(|id| {
                            s.parts
                                .iter()
                                .position(|p| crate::constraints::PartId::of(p) == id)
                        });
                    } else {
                        self.selected = None;
                        self.dimensions = false;
                        self.ghost = false;
                        if !self.scene.assembly {
                            self.explosion = if s.assembly { 1. } else { 0. };
                        }
                        if !s.assembly {
                            self.explosion = 0.;
                        }
                        self.camera = Camera::default();
                        self.camera.fit(&s, self.explosion);
                        if !s.assembly
                            && s.parts.iter().any(|p| {
                                p.pose.path == crate::label::PATH
                                    || p.pose.path.starts_with(crate::gridfinity::PREFIX)
                            })
                        {
                            self.camera.yaw = std::f32::consts::PI;
                            self.camera.pitch = 1.25;
                        }
                    }
                    self.renderer.upload(&s);
                    self.scene = s;
                    self.fixed_parent = None;
                    self.last_hit = None;
                    self.parent_face = None;
                    self.child_face = None;
                    self.collision_report = None;
                    self.status = t(
                        self.zh,
                        "模型已载入；拖动旋转，右键平移，滚轮缩放，单击选择",
                        "Ready: drag to orbit, right drag to pan, scroll to zoom, click to select",
                    )
                    .into();
                    if !finished {
                        self.status = "简化预览：精确盒体正在后台并行生成，完成后自动替换。".into();
                    }
                }
                Loaded::Collision(Ok(report)) => {
                    if report.is_current(&self.scene) {
                        self.status = format!(
                            "{} {}",
                            report.pairs.len(),
                            t(
                                self.zh,
                                "对网格表面交叉；详见装配关系窗口",
                                "mesh surface crossings; see Assembly relations"
                            )
                        );
                        self.collision_report = Some(report);
                    } else {
                        self.status = t(
                            self.zh,
                            "检查期间装配已改变，请重新检查",
                            "Assembly changed during the check; run again",
                        )
                        .into();
                    }
                }
                Loaded::Image(Ok((name, image))) => {
                    self.image = Some((
                        name,
                        ctx.load_texture("reference-image", image, Default::default()),
                    ));
                }
                Loaded::Catalog(Err(e)) | Loaded::Image(Err(e)) | Loaded::Collision(Err(e)) => {
                    self.status = format!("{}: {e}", t(self.zh, "失败", "Error"))
                }
            }
        }
    }
    fn open_asset(&mut self, path: &str, ctx: &egui::Context) {
        if path.to_lowercase().ends_with(".stl") {
            self.single(path.into(), ctx);
        } else if path.to_lowercase().ends_with(".pdf") {
            if let Err(e) = open_file(&self.catalog.root.join(path)) {
                self.status = e;
            }
        } else if !self.busy {
            self.busy = true;
            let full = self.catalog.root.join(path);
            let name = path.to_owned();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = image::open(full)
                    .map(|im| {
                        let im = im.thumbnail(2400, 2400).to_rgba8();
                        let size = [im.width() as usize, im.height() as usize];
                        (name, egui::ColorImage::from_rgba_unmultiplied(size, &im))
                    })
                    .map_err(|e| e.to_string());
                let _ = tx.send(Loaded::Image(result));
                ctx.request_repaint();
            });
        }
    }
    fn check_intersections(&mut self, ctx: &egui::Context) {
        if self.busy || self.scene.parts.len() < 2 {
            return;
        }
        self.busy = true;
        self.status = t(
            self.zh,
            "正在后台检查合拢状态的网格表面交叉…",
            "Checking assembled mesh surface crossings in background…",
        )
        .into();
        let scene = self.scene.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let tolerance = self.collision_tolerance;
        std::thread::spawn(move || {
            let _ = tx.send(Loaded::Collision(crate::collision::check(
                &scene, tolerance,
            )));
            ctx.request_repaint();
        });
    }
    fn relations_window(&mut self, ctx: &egui::Context) {
        if !self.debug_ui || !self.joints_open {
            return;
        }
        let z = self.zh;
        let mut open = true;
        egui::Window::new(t(z,"装配关系与相交检查","Assembly relations & intersection check"))
            .id(egui::Id::new("assembly-relations")).default_size([480.,600.]).open(&mut open)
            .vscroll(true).show(ctx,|ui|{
            if !self.scene.assembly {ui.label(t(z,"请先打开完整装配预览。","Open a complete assembly first."));return;}
            ui.label(t(z,"关系作用于合拢位置；爆炸位移仅用于展示。根零件作为固定基准。","Relations solve assembled poses. Exploded offsets are display-only; root parts are grounded."));
            if ui.button(t(z,"合拢并适配画面","Assemble and fit view")).clicked(){self.explosion=0.;self.camera.fit(&self.scene,0.);}
            ui.collapsing(t(z,"基础关系（普通抽屉）","Basic relations (standard drawer)"),|ui|{
                ui.label(t(z,"抽屉沿 Z 滑动；面板位于盒体前侧；已识别的弹簧按手册嵌入背槽、套住定位柱。其余估算关系及接触区域仍需核对。","Drawer slides along Z; recognized springs seat in the rear pockets around the bosses. Other estimated relations and contact regions still require verification."));
                if ui.add_enabled(self.scene.relations.is_empty(),egui::Button::new(t(z,"生成基础关系","Generate basic relations"))).clicked(){
                    match crate::constraints::basic_relations(&mut self.scene){Ok(n)=>self.status=format!("{n} {}",t(z,"个关系已建立（待核对）","relations created (verify)")),Err(e)=>self.status=e}
                }
            });
            if ui.button(t(z,"面板子装配整体旋转 180°","Rotate whole front subassembly 180°")).on_hover_text(t(z,"绕面板前后轴旋转；Key、帽盖、标签和弹簧保持相对配合，保存时一并保留。","Rotate around the panel normal; preserve Key, cap, label and spring mates, including saved poses.")).clicked() && let Some(i)=self.scene.parts.iter().position(|p|p.pose.path.contains("/Facades/")){
                    let axis=self.scene.parts[i].pose.matrix(0.).transform_vector3(glam::Vec3::Y).normalize();
                    match crate::constraints::rotate_subassembly(&mut self.scene,i,glam::Quat::from_axis_angle(axis,std::f32::consts::PI)){
                        Ok(())=>{self.collision_report=None;self.status=t(z,"面板整组已旋转，内部配合关系保持；请重新检查交叉。","Front group rotated with internal mates preserved; rerun crossing check.").into();},Err(e)=>self.status=e
                    }
            }
            ui.separator();ui.strong(t(z,"1. 固定或滑动关系","1. Fixed or slider joint"));
            ui.label(t(z,"先选父零件并记录，再选子零件建立关系。","Select and record a parent, then select the child."));
            if ui.add_enabled(self.selected.is_some(),egui::Button::new(t(z,"记录当前选中为父件","Record selected parent"))).clicked(){self.fixed_parent=self.selected;}
            if let Some(parent)=self.fixed_parent{ui.label(format!("{}: {}",t(z,"父件","Parent"),self.scene.parts[parent].label(z)));}
            ui.horizontal(|ui|{for(slider,label)in [(false,t(z,"固定当前相对位置","Fix current relative pose")),(true,t(z,"建立滑动关系","Create slider"))]{
                if ui.add_enabled(self.fixed_parent.is_some()&&self.selected.is_some(),egui::Button::new(label)).clicked(){
                    let result=crate::constraints::Relation::fixed(&self.scene,self.fixed_parent.unwrap(),self.selected.unwrap(),slider)
                        .and_then(|r|crate::constraints::add(&mut self.scene,r));
                    self.status=match result{Ok(_)=>t(z,"关系已建立","Relation created").into(),Err(e)=>e};
                }
            }});
            ui.separator();ui.strong(t(z,"2. 选面贴合","2. Mate picked faces"));
            ui.label(t(z,"在视口单击父件的接触面→记录固定面；再单击子件接触面→记录移动面。选择平面区域；圆角上的三角面不代表完整安装面。","Click a planar parent face and record it, then a child face. A triangle on a fillet is not a full mounting datum."));
            let valid=self.last_hit.filter(|h|Some(h.part)==self.selected);
            ui.horizontal(|ui|{
                if ui.add_enabled(valid.is_some(),egui::Button::new(t(z,"记录固定面","Record parent face"))).clicked(){self.parent_face=valid;}
                if ui.add_enabled(valid.is_some(),egui::Button::new(t(z,"记录移动面","Record child face"))).clicked(){self.child_face=valid;}
            });
            for(label,hit)in [(t(z,"固定面","Parent face"),self.parent_face),(t(z,"移动面","Child face"),self.child_face)]{if let Some(h)=hit{ui.label(format!("{label}: {}",self.scene.parts[h.part].label(z)));}}
            ui.horizontal(|ui|{ui.label(t(z,"间隙 mm","Gap mm"));ui.add(egui::DragValue::new(&mut self.mate_gap).speed(0.05).range(-10. ..=100.));ui.label(t(z,"扭转 °","Twist °"));ui.add(egui::DragValue::new(&mut self.mate_twist).speed(1.).range(-180. ..=180.));});
            if ui.add_enabled(self.parent_face.is_some()&&self.child_face.is_some(),egui::Button::new(t(z,"法线相对贴合并固定","Align opposing normals and fix"))).clicked(){
                let result=crate::constraints::Relation::face(&self.scene,self.parent_face.unwrap(),self.child_face.unwrap(),self.mate_gap,self.mate_twist)
                    .and_then(|r|crate::constraints::add(&mut self.scene,r));
                self.status=match result{Ok(_)=>t(z,"面关系已建立；请执行相交检查","Face mate created; run intersection check").into(),Err(e)=>e};
            }
            ui.separator();ui.strong(format!("{} ({})",t(z,"当前关系","Current relations"),self.scene.relations.len()));
            let previous=self.scene.relations.clone();let mut changed=false;let mut remove=None;
            for(index,r)in self.scene.relations.iter_mut().enumerate(){
                let child=self.scene.parts.iter().find(|p|crate::constraints::PartId::of(p)==r.child).map(|p|p.label(z)).unwrap_or_else(||r.child.path.clone());
                egui::CollapsingHeader::new(format!("{} · {}",index+1,child)).id_salt(("joint",index)).show(ui,|ui|{
                    ui.label(&r.label);ui.label(format!("{}: {}",t(z,"父件","Parent"),r.parent.path.rsplit('/').next().unwrap()));
                    match &mut r.joint {
                        crate::constraints::Joint::Fixed{..}=>{ui.label(t(z,"固定 6 个自由度","All 6 DOFs fixed"));},
                        crate::constraints::Joint::Face{gap,..}=>{ui.horizontal(|ui|{ui.label(t(z,"面间隙 mm","Face gap mm"));changed|=ui.add(egui::DragValue::new(gap).speed(0.05).range(-10. ..=100.)).changed();});},
                        crate::constraints::Joint::Slider{offset,min,max,axis,..}=>{
                            ui.label(format!("{}: {:.2}, {:.2}, {:.2}",t(z,"父件局部轴","Parent local axis"),axis[0],axis[1],axis[2]));
                            let old_axis=*axis;
                            let mut choice=(0..3).find(|i|axis[*i].abs()>0.9999).unwrap_or(3);
                            let old_choice=choice;let mut negative=choice<3&&axis[choice]<0.;let old_negative=negative;
                            ui.horizontal(|ui|{
                                egui::ComboBox::from_id_salt(("slider-axis",index)).selected_text(["X","Y","Z",t(z,"自定义","Custom")][choice]).show_ui(ui,|ui|{for(i,label)in ["X","Y","Z"].iter().enumerate(){ui.selectable_value(&mut choice,i,*label);}});
                                ui.checkbox(&mut negative,t(z,"反向","Reverse"));
                            });
                            if choice!=old_choice||negative!=old_negative {if choice<3{*axis=[0.;3];axis[choice]=if negative{-1.}else{1.};}else{*axis=old_axis.map(|v|-v);}changed=true;}

                            changed|=ui.add(egui::Slider::new(offset,*min..=*max).text(t(z,"位移 mm","Travel mm"))).changed();
                            ui.horizontal(|ui|{for (label,value) in [(t(z,"下限","Min"),min),(t(z,"上限","Max"),max)]{ui.label(label);changed|=ui.add(egui::DragValue::new(value).speed(1.).range(-10000. ..=10000.)).changed();}});
                        }
                    }
                    if ui.button(t(z,"解除此关系（保留当前位置）","Remove relation, keep pose")).clicked(){remove=Some(index);}
                });
            }
            if let Some(index)=remove{self.scene.relations.remove(index);changed=true;}
            if changed{if let Err(e)=crate::constraints::solve(&mut self.scene){self.scene.relations=previous;self.status=e;}else{self.scene.calibrated=false;}}
            if ui.button(t(z,"保存装配位置与关系","Save poses and relations")).clicked(){
                self.status=match toml::to_string_pretty(&self.scene.definition()).map_err(|e|e.to_string()).and_then(|s|self.write_state(&format!("assembly-{}.toml",self.scene_key),s)){Ok(_)=>t(z,"装配位置与关系已保存","Poses and relations saved").into(),Err(e)=>e};
            }
            ui.separator();ui.strong(t(z,"3. 合拢状态相交检查","3. Assembled intersection check"));
            ui.label(t(z,"检查实际三角面交叉，包含隐藏零件。共面接触和完整包覆不计入；无结果不代表已通过公差检查。","Tests actual triangle crossings, including hidden parts. Excludes coplanar contact and complete containment; not a tolerance certificate."));
            ui.horizontal(|ui|{ui.label(t(z,"数值容差 mm","Numerical tolerance mm"));ui.add(egui::DragValue::new(&mut self.collision_tolerance).speed(0.001).range(0.0001..=1.));if ui.add_enabled(!self.busy,egui::Button::new(t(z,"检查表面交叉","Check crossings"))).clicked(){self.check_intersections(ctx);}});
            if let Some(report)=&self.collision_report{
                if !report.is_current(&self.scene){ui.colored_label(Color32::YELLOW,t(z,"装配已改变，结果过期，请重新检查。","Assembly changed; results are stale. Run again."));}
                else{ui.label(format!("{} {} · {:.4} mm",report.pairs.len(),t(z,"对交叉","crossing pairs"),report.tolerance));for pair in &report.pairs{
                    let label=format!("{} ↔ {}",self.scene.parts[pair.a].label(z),self.scene.parts[pair.b].label(z));
                    if ui.button(label).on_hover_text(format!("{:.2}, {:.2}, {:.2} mm",pair.point.x,pair.point.y,pair.point.z)).clicked(){for(i,p)in self.scene.parts.iter_mut().enumerate(){p.visible=i==pair.a||i==pair.b;}self.selected=Some(pair.a);self.explosion=0.;self.camera.fit(&self.scene,0.);}
                }}
            }
        });
        self.joints_open = open;
    }
    fn toolbar(&mut self, ctx: &egui::Context) {
        let z = self.zh;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(5.);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(crate::app_name(z))
                        .size(24.)
                        .strong()
                        .color(Color32::from_rgb(108, 205, 221)),
                );
                ui.label(
                    RichText::new(t(z, "非官方辅助工具", "Unofficial Companion Tool"))
                        .small()
                        .color(Color32::GRAY),
                );
                ui.separator();
                if ui
                    .selectable_label(self.scene.assembly, t(z, "装配预览", "Assembly view"))
                    .clicked()
                {
                    self.assembly(ctx);
                }
                if !self.scene.assembly {
                    ui.label(t(z, "单件预览", "Single part view"));
                }
                ui.add_enabled_ui(!self.busy, |ui| {
                    if ui
                        .button(t(z, "打开模型库", "Open library"))
                        .on_hover_text(self.last_model_root.display().to_string())
                        .clicked()
                    {
                        let mut dialog = rfd::FileDialog::new();
                        if let Some(dir) = crate::paths::directory_hint(&self.last_model_root) {
                            dialog = dialog.set_directory(dir);
                        }
                        if let Some(root) = dialog.pick_folder() {
                            self.scan(root, ctx);
                        }
                    }
                    if ui.button(t(z, "保存方案", "Save preset")).clicked() {
                        self.save_preset();
                    }
                    if ui.button(t(z, "载入方案", "Load preset")).clicked() {
                        self.load_preset(ctx);
                    }
                    if ui.button(t(z, "配色", "Colors")).clicked() {
                        self.colors_open = true;
                    }
                    if ui.button(t(z, "导出", "Export")).clicked() {
                        self.export_open = true;
                    }
                    if self.export_rx.is_some() {
                        ui.spinner();
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(if z { "English" } else { "中文" }).clicked() {
                        self.zh = !z;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                            crate::app_name(self.zh).into(),
                        ));
                        if let Err(e) = self.save_settings() {
                            self.status = e;
                        }
                    }
                });
            });
            ui.add_space(5.);
        });
    }
    fn left(&mut self, ctx: &egui::Context) {
        let z = self.zh;
        egui::SidePanel::left("library")
            .default_width(290.)
            .width_range(235. ..=470.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, 0, t(z, "装配配置", "Assembly"));
                    ui.selectable_value(&mut self.tab, 1, t(z, "全部组件", "Library"));
                    ui.selectable_value(&mut self.tab, 2, t(z, "说明资料", "Manuals"));
                });
                ui.separator();
                if self.tab == 0 {
                    let before = (
                        self.selection.width.clone(),
                        self.selection.depth,
                        self.selection.height.clone(),
                        self.selection.hidden_key,
                    );
                    ui.add_enabled_ui(!self.catalog.root.as_os_str().is_empty(), |ui| {
                        ui.label(t(z, "方案类型", "Assembly type"));
                        egui::ComboBox::from_id_salt("assembly-kind")
                            .width(ui.available_width()-10.)
                            .selected_text(AssemblyKind::Drawer.label(z))
                            .show_ui(ui,|ui| {
                                for kind in [AssemblyKind::Drawer,AssemblyKind::Sustainer] {
                                    let label=if kind.supported() {kind.label(z).to_string()} else {format!("{} · {}",kind.label(z),t(z,"暂未支持","Not yet supported"))};
                                    ui.add_enabled(kind.supported(),egui::Button::selectable(kind==AssemblyKind::Drawer,label));
                                }
                            });
                        ui.separator();
                        ui.strong(t(z, "抽屉规格", "Drawer specifications"));
                        egui::Grid::new("specifications")
                            .num_columns(2)
                            .spacing([12., 12.])
                            .show(ui, |ui| {
                                ui.label(t(z, "宽度", "Width"));
                                egui::ComboBox::from_id_salt("width")
                                    .selected_text(&self.selection.width)
                                    .show_ui(ui, |ui| {
                                        for w in ["S", "M", "L", "XL"] {
                                            ui.selectable_value(
                                                &mut self.selection.width,
                                                w.into(),
                                                w,
                                            );
                                        }
                                    });
                                ui.end_row();
                                ui.label(t(z, "深度", "Depth"));
                                egui::ComboBox::from_id_salt("depth")
                                    .selected_text(format!("{} grids", self.selection.depth))
                                    .show_ui(ui, |ui| {
                                        for d in 3..=7 {
                                            ui.selectable_value(
                                                &mut self.selection.depth,
                                                d,
                                                d.to_string(),
                                            );
                                        }
                                    });
                                ui.end_row();
                                if self.selection.width == "XL" {
                                    self.selection.height = "1F".into();
                                }
                                ui.label(t(z, "层高", "Height"));
                                egui::ComboBox::from_id_salt("height")
                                    .selected_text(&self.selection.height)
                                    .show_ui(ui, |ui| {
                                        for h in if self.selection.width == "XL" {
                                            vec!["1F"]
                                        } else {
                                            vec!["1F", "2F", "4F"]
                                        } {
                                            ui.selectable_value(
                                                &mut self.selection.height,
                                                h.into(),
                                                h,
                                            );
                                        }
                                    });
                                ui.end_row();
                            });
                        ui.add_space(8.);
                        ui.checkbox(
                            &mut self.selection.hidden_key,
                            t(z, "隐藏钥匙（两件方案）", "Hidden key (two-file option)"),
                        );
                        let after = (
                            self.selection.width.clone(),
                            self.selection.depth,
                            self.selection.height.clone(),
                            self.selection.hidden_key,
                        );
                        if before != after {
                            if let Err(e)=self.gridfinity.validate(&self.selection) {
                                self.selection.width=before.0;self.selection.depth=before.1;self.selection.height=before.2;self.selection.hidden_key=before.3;
                                self.status=format!("{e}；请先调整或移除盒子布局");
                            } else {
                            let next = self.catalog.reconfigure(&self.selection, &self.bom);
                            let removed = self.bom.iter().filter(|b| b.enabled
                                && !next.iter().any(|n|n.path == b.path)).count();
                            self.bom = next;
                            if !self.gridfinity.bins.is_empty() && !self.bom.iter().any(|b|b.enabled && b.path.contains("/Gridfinity Baseplate/"))
                                && let Some(plate)=self.catalog.accessories(&self.selection).into_iter().filter(|e|e.path.contains("/Gridfinity Baseplate/")).min_by_key(|e|e.path.contains("Handle")) {
                                    self.select_accessory(plate.path,true,1);
                            }
                            self.assembly(ctx);
                            self.status = format!("{} ({removed})", t(
                                z,
                                "已保留兼容配件和数量，正在更新预览。移除不匹配的已选文件",
                                "Compatible choices kept; updating preview. Removed incompatible selected files",
                            ));
                        }
                        }
                        ui.label(RichText::new(t(z, "配置变更后自动更新预览", "Preview updates automatically")).small().color(Color32::GRAY));
                    });
                    ui.label(
                        RichText::new(t(
                            z,
                            "Sustainer 工具箱方案暂未支持；Gridfinity 盒子在下方收纳配置中编辑。",
                            "Box assemblies are not yet supported. Browse their models in Library.",
                        ))
                        .small()
                        .color(Color32::GRAY),
                    );
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.strong(t(z,"标签文字","Label text"));
                        if ui.button(t(z,"编辑标签…","Edit label…")).clicked() {self.label_open=true;}
                    });
                    if self.label.active() {ui.label(self.label.text.replace(char::from(10)," / "));}
                    else {ui.small(t(z,"原始空白标签","Original blank label"));}
                    ui.separator();
                    ui.strong(t(z, "抽屉内部配置", "Drawer inserts"));
                    let has_dividers=self.divider_layout.enabled || self.bom.iter().any(|b|b.enabled && b.path.contains("/Grid Dividers/"));
                    let mut mode=if has_dividers {2} else if self.bom.iter().any(|b|b.enabled && b.path.contains("/Gridfinity Baseplate/")) {1} else {0};
                    let previous=mode;
                    let baseplate=self.catalog.accessories(&self.selection).into_iter()
                        .filter(|e|e.path.contains("/Gridfinity Baseplate/"))
                        .min_by_key(|e|e.path.contains("Handle"))
                        .map(|e|e.path.clone());
                    ui.horizontal_wrapped(|ui| {
                        ui.radio_value(&mut mode,0,t(z,"无内部配置","None"));
                        ui.add_enabled_ui(baseplate.is_some(),|ui| {ui.radio_value(&mut mode,1,t(z,"Gridfinity 收纳","Gridfinity storage"));});
                        ui.add_enabled_ui(crate::dividers::Grid::new(&self.selection).is_ok(),|ui| {ui.radio_value(&mut mode,2,t(z,"收纳隔板","Storage dividers"));});
                    });
                    if mode!=previous {
                        self.divider_layout.remove(&mut self.bom,&self.selection);
                        for b in &mut self.bom {
                            if b.path.contains("/Gridfinity Baseplate/") {b.enabled=false;}
                        }
                        self.layout_cache.remove(&crate::dividers::spec(&self.selection));
                        self.divider_editor.open=false;
                        self.divider_editor.reset();
                        if mode==1 {
                            if let Some(path)=baseplate {self.select_accessory(path,true,1);}
                        } else if mode==2 {
                            self.divider_layout.enabled=true;
                        }
                        self.assembly(ctx);
                    }
                    ui.label(RichText::new(t(z,"底板与隔板互斥，切换会移除当前内部配置。","Baseplates and dividers are exclusive. Switching removes the current insert.")).small().color(Color32::GRAY));
                    if mode==2 {
                        ui.horizontal_wrapped(|ui| {
                            let label=if self.divider_layout.items.is_empty() {t(z,"添加收纳隔板","Add storage dividers")} else {t(z,"编辑收纳隔板","Edit storage dividers")};
                            if ui.button(label).clicked() {self.divider_editor.open=true;}
                            if ui.button(t(z,"移除收纳隔板","Remove storage dividers")).clicked() {self.remove_dividers(ctx);}
                        });
                        ui.label(format!("{} {}",self.divider_layout.items.len(),t(z,"块收纳隔板","storage dividers")));
                    }
                    if mode==1 {
                        let plates: Vec<_> = self.catalog.accessories(&self.selection).into_iter()
                            .filter(|e|e.path.contains("/Gridfinity Baseplate/")).collect();
                        let variant_name = |path: &str| {
                            if path.contains("2 Handles") {t(z,"双提手","Two handles")}
                            else if path.contains("1 Handle") {t(z,"单提手","One handle")}
                            else {t(z,"标准版","Standard")}
                        };
                        let current=self.bom.iter().find(|b|b.enabled && b.path.contains("/Gridfinity Baseplate/")).map(|b|b.path.clone()).unwrap_or_default();
                        let mut chosen=current.clone();
                        ui.label(t(z,"底板类型","Baseplate type"));
                        egui::ComboBox::from_id_salt("gridfinity-baseplate-type")
                            .width(ui.available_width())
                            .selected_text(variant_name(&current))
                            .show_ui(ui,|ui| {
                                for plate in &plates {ui.selectable_value(&mut chosen,plate.path.clone(),variant_name(&plate.path));}
                            });
                        if chosen!=current {
                            let mut candidate=self.gridfinity.clone();candidate.baseplate=chosen.clone();
                            match candidate.validate(&self.selection) {
                                Ok(())=>{self.select_accessory(chosen,true,1);self.assembly(ctx);self.gridfinity_error=None;}
                                Err(e)=>{self.gridfinity_error=Some(format!("{e}；请先调整盒子布局"));}
                            }
                        }
                        if let Ok(grid)=crate::gridfinity::grid_for(&self.selection,&self.gridfinity.baseplate) {
                            ui.small(format!("{} × {} {} · {}",grid[0],grid[1],t(z,"格","cells"),t(z,"底板随抽屉规格匹配","Baseplate follows drawer size")));
                        }
                        if let Some(error)=&self.gridfinity_error {ui.colored_label(Color32::LIGHT_RED,error);}
                        ui.add_space(6.);
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(crate::gridfinity::grid(&self.selection).is_ok(),egui::Button::new(t(z,"编辑 Gridfinity 盒子","Edit Gridfinity bins"))).clicked() {self.gridfinity_open=true;}
                            if !self.gridfinity.bins.is_empty() && ui.button(t(z,"移除全部盒子","Remove all bins")).clicked(){self.gridfinity.bins.clear();self.assembly(ctx);}
                        });
                        ui.label(format!("{} {}",self.gridfinity.bins.len(),t(z,"个盒子","bins")));
                        ui.small(t(z,"盒高默认随层高配置，可在编辑器中调整。","Bin heights follow the drawer by default; adjust them in the editor."));
                    }
                } else {
                    ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(t(
                        z,
                        "搜索名称、用途或路径…",
                        "Search name, purpose or path…",
                    )));
                    if self.tab == 1 {
                        egui::ComboBox::from_id_salt("category")
                            .selected_text(
                                [
                                    t(z, "全部", "All"),
                                    t(z, "抽屉", "Drawers"),
                                    t(z, "工具箱", "Sustainers"),
                                    t(z, "改装 / 安装", "Modifications"),
                                    t(z, "资料", "References"),
                                ][self.category],
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in [
                                    t(z, "全部", "All"),
                                    t(z, "抽屉", "Drawers"),
                                    t(z, "工具箱", "Sustainers"),
                                    t(z, "改装 / 安装", "Modifications"),
                                    t(z, "资料", "References"),
                                ]
                                .iter()
                                .enumerate()
                                {
                                    ui.selectable_value(&mut self.category, i, *name);
                                }
                            });
                    }
                    let q = self.query.to_lowercase();
                    let combinable: Vec<String> = self.catalog.bom(&self.selection).into_iter().map(|b|b.path)
                        .chain(self.catalog.accessories(&self.selection).into_iter().map(|e|e.path)).collect();
                    let mut folders: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
                    let mut count = 0;
                    for e in &self.catalog.entries {
                        let search = crate::accessories::info(&e.path,z).map_or_else(||e.path.clone(),
                            |m|format!("{} {} {} {}",e.path,m.name,m.group,m.description));
                        if !search.to_lowercase().contains(&q) {
                            continue;
                        }
                        if self.tab == 2 && e.name.to_lowercase().ends_with(".stl") {
                            continue;
                        }
                        if self.tab == 1 && self.category != 0 && e.category != self.category {
                            continue;
                        }
                        count += 1;
                        folders
                            .entry(
                                e.path
                                    .rsplit_once('/')
                                    .map(|p| p.0)
                                    .unwrap_or("References")
                                    .into(),
                            )
                            .or_default()
                            .push((e.name.clone(), e.path.clone()));
                    }
                    ui.label(format!("{} {}", count, t(z, "个文件", "files")));
                    let mut open = None;
                    let mut add = None;
                    egui::ScrollArea::vertical()
                        .id_salt("catalog")
                        .show(ui, |ui| {
                            for (folder, files) in folders {
                                egui::CollapsingHeader::new(format!(
                                    "{} ({})",
                                    folder,
                                    files.len()
                                ))
                                .default_open(!q.is_empty() || self.tab == 2)
                                .show(ui, |ui| {
                                    for (name, path) in files {
                                        ui.horizontal(|ui| {
                                            if name.to_lowercase().ends_with(".stl") && combinable.contains(&path)
                                                && ui.small_button("+").clicked()
                                            {
                                                add = Some(path.clone());
                                            }
                                            let meta = crate::accessories::info(&path,z);
                                            let label = meta.as_ref().map_or(name.as_str(),|m|m.name.as_str());
                                            let tooltip = meta.as_ref().map_or_else(||path.clone(),|m|format!("{}\n{}",m.description,path));
                                            if ui
                                                .selectable_label(false, label)
                                                .on_hover_text(tooltip)
                                                .clicked()
                                            {
                                                open = Some(path);
                                            }
                                        });
                                    }
                                });
                            }
                        });
                    if let Some(path) = open {
                        self.open_asset(&path, ctx);
                    }
                    if let Some(path) = add {
                        self.add_to_bom(path, ctx);
                    }
                }
            });
    }
    fn remove_dividers(&mut self, ctx: &egui::Context) {
        self.divider_editor.open = false;
        self.divider_layout.remove(&mut self.bom, &self.selection);
        self.layout_cache
            .remove(&crate::dividers::spec(&self.selection));
        self.divider_editor.reset();
        self.assembly(ctx);
    }
    fn select_accessory(&mut self, path: String, enabled: bool, qty: u32) {
        if enabled && path.contains("/Gridfinity Baseplate/") {
            let mut next = self.gridfinity.clone();
            next.baseplate = path.clone();
            if let Err(e) = next.validate(&self.selection) {
                self.status = format!("{e}；请先调整盒子再切换底板");
                return;
            }
            self.gridfinity = next;
            self.divider_editor.open = false;
            self.layout_cache
                .remove(&crate::dividers::spec(&self.selection));
            self.divider_editor.reset();
        }
        self.divider_layout
            .select_accessory(&mut self.bom, &self.selection, path, enabled, qty);
    }
    fn add_to_bom(&mut self, path: String, ctx: &egui::Context) {
        if self.divider_layout.enabled && path.contains("/Grid Dividers/") {
            self.divider_editor.open = true;
            self.status = "请在分隔编辑器中添加隔板，配套端件和销自动统计".into();
            return;
        }
        if !self
            .catalog
            .bom(&self.selection)
            .iter()
            .any(|b| b.path == path)
            && !self
                .catalog
                .accessories(&self.selection)
                .iter()
                .any(|e| e.path == path)
        {
            self.status=t(self.zh,"当前组合只支持单个抽屉及收纳底板、内部分隔；此组件可单独预览。","Current assembly supports one drawer, baseplates and dividers; preview this component separately.").into();
            return;
        }
        let qty = self
            .bom
            .iter()
            .find(|b| b.path == path)
            .map_or(1, |b| b.qty);
        self.select_accessory(path, true, qty);
        self.assembly(ctx);
        self.status = t(
            self.zh,
            "已加入打印清单；请核对替代版本和所需数量",
            "Added to BOM; check alternatives and quantities",
        )
        .into();
    }
    fn bottom(&mut self, ctx: &egui::Context) {
        let z = self.zh;
        egui::TopBottomPanel::bottom("bottom").resizable(true)
            .default_height(240.).height_range(180. ..=400.).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong(t(z, "打印清单", "PRINT LIST"));
                ui.label(format!("{} {}", self.bom.iter().filter(|i| i.enabled).count(), t(z, "个已选文件", "selected files")));
                ui.separator();
                ui.label(RichText::new(t(z, "份数按原 STL 统计；Springs、Rails 每份各含 2 个实体。导出按份数保留打印支撑，并应用标签文字。", "Copies count source files; Springs and Rails each contain 2 bodies. Exports preserve supports and apply label lettering for every copy.")).small().color(Color32::GRAY));
            });
            let mut preview = None;
            let mut changed = false;
            let mut enabled_variant = None;
            egui::ScrollArea::vertical().id_salt("bom").auto_shrink([false,false])
                .max_height((ui.available_height()-65.).max(90.)).show(ui, |ui| {
                egui::Grid::new("bom-table").striped(true).num_columns(5).min_col_width(30.).show(ui, |ui| {
                    for text in [t(z,"选用","Use"),t(z,"文件份数","File copies"),t(z,"组件 / 配件","Component / accessory"),t(z,"分组","Group"),t(z,"状态","Status")] {ui.strong(text);}
                    ui.end_row();
                    for item in &mut self.bom {
                        let auto_caps=item.path.ends_with("/Facade Cap.stl");
                        if auto_caps {item.qty=crate::catalog::facade_cap_count(&self.selection);}
                        let required=item.path.contains("/Geared Rails/");
                        let locked=required || item.group==crate::dividers::GROUP || item.group==crate::gridfinity::GROUP;
                        if required {item.enabled=true;item.qty=1;}
                        if ui.add_enabled(!locked,egui::Checkbox::new(&mut item.enabled, "")).on_hover_text(if required {t(z,"默认必备：一份包含上下两根齿条","Required: one file contains both racks")} else {""}).changed() {
                            changed = true;
                            if item.enabled {enabled_variant = Some((item.path.clone(),item.qty));}
                        }
                        changed |= ui.add_enabled(!locked && !auto_caps,egui::DragValue::new(&mut item.qty).range(1..=100)).on_hover_text(if auto_caps {t(z,"数量按面板规格和外露螺丝孔自动配置","Quantity follows exposed facade screw recesses")}else{""}).changed();
                        let metadata = crate::accessories::info(&item.path, z);
                        let label = metadata.as_ref().map_or_else(||item.name().to_string(),|m|m.name.clone());
                        let tooltip = metadata.as_ref().map_or_else(||item.path.clone(),|m|format!("{}\n{}",m.description,item.path));
                        if ui.link(label).on_hover_text(tooltip).clicked() {preview = Some(item.path.clone());}
                        ui.label(metadata.as_ref().map_or(group_label(z, &item.group),|m|m.group));
                        let exists = self.catalog.resolve(&item.path).is_some() || item.path.starts_with(crate::gridfinity::PREFIX);
                        ui.colored_label(if exists {Color32::from_rgb(117,189,157)} else {Color32::LIGHT_RED}, if exists {t(z,"可用","Ready")} else {t(z,"缺失","Missing")});
                        ui.end_row();
                    }
                });
            });
            if let Some((path,qty)) = enabled_variant {self.select_accessory(path,true,qty);}
            if changed {self.assembly(ctx);}
            if let Some(path) = preview {self.single(path,ctx);}
            ui.separator();
            ui.horizontal(|ui| {
                if self.busy || self.preview.has_pending() {ui.spinner();}
                ui.label(&self.status);
                if !self.busy && !self.preview.has_pending() && self.failed_preview.is_some()
                    && ui.button(t(z,"重试","Retry")).clicked() {
                    let request = self.failed_preview.take().unwrap();
                    self.preview.schedule(request, Instant::now(), Duration::ZERO);
                }
            });
            ui.label(RichText::new(if self.selection.width=="XL" {
                t(z,"XL 五金按专用结构核对；不直接沿用普通抽屉数量。","Check XL hardware separately; standard drawer counts may not apply.")
            } else {
                t(z,"五金：M4 × 14 mm，1F ×4、2F/4F ×6；顶提手选装 M6 × 50 mm ×2。","Hardware: M4 × 14 mm, 1F ×4 / 2F and 4F ×6; optional top handle M6 × 50 mm ×2.")
            }).small().color(Color32::GRAY));
        });
    }
    fn interior_scene(&self) -> Scene {
        let mut scene = self.scene.clone();
        for p in &mut scene.parts {
            p.visible = crate::dividers::interior_part(&p.pose.path);
        }
        scene
    }
    fn divider_view(&mut self, ctx: &egui::Context) {
        if self.divider_editor.open {
            if !self.scene.assembly {
                if !self.preview.has_pending()
                    && !self.busy
                    && self.failed_preview.is_none()
                    && self.catalog.stl_count() > 0
                {
                    self.assembly(ctx);
                }
                return;
            }
            let needs_fit = self
                .divider_view
                .as_ref()
                .is_none_or(|v| v.shown_key != self.scene_key);
            if self.divider_view.is_none() {
                self.divider_view = Some(DividerView {
                    camera: self.camera.clone(),
                    explosion: self.explosion,
                    dimensions: self.dimensions,
                    key: self.scene_key.clone(),
                    shown_key: String::new(),
                });
            }
            if needs_fit {
                self.explosion = 0.;
                self.dimensions = false;
                self.camera = Camera::default();
                self.camera.fit(&self.interior_scene(), 0.);
                self.camera.yaw = 0.6;
                self.camera.pitch = 0.9;
                self.divider_view.as_mut().unwrap().shown_key = self.scene_key.clone();
            }
        } else if let Some(view) = self.divider_view.take() {
            self.explosion = view.explosion;
            self.dimensions = view.dimensions;
            if view.key == self.scene_key {
                self.camera = view.camera;
            } else {
                self.camera.fit(&self.scene, self.explosion);
            }
        }
    }
    fn selected_color_properties(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(part) = self.selected.and_then(|i| self.scene.parts.get(i)) else {
            return;
        };
        let path = part.pose.path.clone();
        let linked = self
            .scene
            .parts
            .iter()
            .filter(|p| p.pose.path == path)
            .count();
        let has_text = path == crate::label::PATH
            || self
                .gridfinity
                .bins
                .iter()
                .any(|b| b.path() == path && b.tab);
        let z = self.zh;
        let mut changed = false;
        ui.strong(t(z, "打印颜色", "Print color"));
        ui.add_enabled_ui(self.export_rx.is_none(), |ui| {
            let c = &mut self.export_options.plate;
            let old = c.part_filaments.get(&path).copied().unwrap_or(0);
            let mut choice = old;
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut choice, 0, t(z, "跟随默认", "Use default"));
                for slot in 1..=6 {
                    let [r, g, b] = c.palette[(slot - 1) as usize];
                    let button = egui::Button::new(egui::RichText::new(format!(" {slot} ")).color(
                        if u32::from(r) + u32::from(g) + u32::from(b) > 380 {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        },
                    ))
                    .fill(Color32::from_rgb(r, g, b))
                    .stroke(egui::Stroke::new(
                        if choice == slot { 3. } else { 1. },
                        if choice == slot {
                            Color32::LIGHT_BLUE
                        } else {
                            Color32::GRAY
                        },
                    ));
                    if ui.add(button).on_hover_text(c.color_hex(slot)).clicked() {
                        choice = slot;
                    }
                }
            });
            if choice != old {
                if choice == 0 {
                    c.part_filaments.remove(&path);
                } else {
                    c.part_filaments.insert(path.clone(), choice);
                }
                c.preview_colors = true;
                changed = true;
            }
            if has_text {
                ui.label(t(z, "文字颜色", "Text color"));
                let old_text = c.text_filaments.get(&path).copied().unwrap_or(0);
                let mut text_choice = old_text;
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut text_choice, 0, t(z, "跟随底板", "Match body"));
                    for slot in 1..=6 {
                        let [r, g, b] = c.palette[(slot - 1) as usize];
                        let ink = if u32::from(r) + u32::from(g) + u32::from(b) > 380 {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        };
                        let button =
                            egui::Button::new(RichText::new(format!(" {slot} ")).color(ink))
                                .fill(Color32::from_rgb(r, g, b))
                                .stroke(egui::Stroke::new(
                                    if text_choice == slot { 3. } else { 1. },
                                    if text_choice == slot {
                                        Color32::LIGHT_BLUE
                                    } else {
                                        Color32::GRAY
                                    },
                                ));
                        if ui.add(button).on_hover_text(c.color_hex(slot)).clicked() {
                            text_choice = slot;
                        }
                    }
                });
                if text_choice != old_text {
                    if text_choice == 0 {
                        c.text_filaments.remove(&path);
                    } else {
                        c.text_filaments.insert(path.clone(), text_choice);
                    }
                    c.preview_colors = true;
                    changed = true;
                }
                ui.small(t(
                    z,
                    "文字与底板同盘多色打印；凹字只给凹底着色，保留凹陷。",
                    "Body and text print together; engraved floors retain their recess.",
                ));
            }
            ui.small(format!(
                "{} {} · {}",
                t(z, "当前颜色槽", "Current slot"),
                c.slot(&path),
                c.color_hex(c.slot(&path))
            ));
            ui.small(format!(
                "{} {linked} {}",
                t(z, "同步修改同源的", "Applies to"),
                t(
                    z,
                    "个展示部件；同一源文件的所有份数共用颜色。",
                    "display parts; all source copies share the color."
                )
            ));
            if ui
                .small_button(t(z, "编辑颜色槽…", "Edit palette…"))
                .clicked()
            {
                self.colors_open = true;
            }
        });
        ui.separator();
        if changed {
            if let Err(e) = self.save_settings() {
                self.status = e;
            }
            ctx.request_repaint();
        }
    }
    fn right(&mut self, ctx: &egui::Context) {
        let z = self.zh;
        egui::SidePanel::right("properties")
            .default_width(290.)
            .width_range(240. ..=450.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading(t(z, "装配与属性", "Assembly & properties"));
                ui.horizontal(|ui| {
                    if ui.small_button(t(z, "显示全部", "Show all")).clicked() {
                        for p in &mut self.scene.parts {
                            p.visible = true;
                        }
                    }
                    if ui.small_button(t(z, "隔离选中", "Isolate")).clicked()
                        && let Some(sel) = self.selected
                    {
                        for (i, p) in self.scene.parts.iter_mut().enumerate() {
                            p.visible = i == sel;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("parts")
                    .max_height(230.)
                    .show(ui, |ui| {
                        for (i, p) in self.scene.parts.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut p.visible, "");

                                if ui
                                    .selectable_label(
                                        self.selected == Some(i),
                                        format!("{} · {}", i + 1, p.label(z)),
                                    )
                                    .on_hover_text(&p.pose.path)
                                    .clicked()
                                {
                                    self.selected = Some(i);
                                }
                            });
                        }
                    });
                ui.separator();
                let mut save = false;
                self.selected_color_properties(ui, ctx);
                if let Some(i) = self.selected {
                    let controlled=self.scene.relations.iter().any(|r|r.child==crate::constraints::PartId::of(&self.scene.parts[i]));
                    let p = &mut self.scene.parts[i];
                    ui.label(RichText::new(p.label(z)).strong());
                    if p.pose.body.is_some() {
                        ui.colored_label(
                            Color32::from_rgb(108, 205, 221),
                            t(
                                z,
                                "展示时拆分 · 源 STL 未修改",
                                "Split for display · source STL unchanged",
                            ),
                        );
                        ui.label(format!(
                            "{} {}",
                            p.source_bodies,
                            t(z, "个实体 / 份源文件", "bodies per source file")
                        ));
                        let s = p.source_size;
                        ui.small(format!(
                            "{}: {:.2} × {:.2} × {:.2} mm",
                            t(z, "源文件尺寸", "Source size"),
                            s.x,
                            s.y,
                            s.z
                        ));
                    }
                    let s = p.mesh.size;
                    ui.label(format!(
                        "X  {:.2} mm\nY  {:.2} mm\nZ  {:.2} mm",
                        s.x, s.y, s.z
                    ));
                    ui.label(format!(
                        "{} {}",
                        p.mesh.vertices.len() / 3,
                        t(z, "三角面", "triangles")
                    ));
                    ui.label(
                        RichText::new(t(
                            z,
                            "尺寸为当前展示实体，按原始轴向，单位假定为 mm。",
                            "Dimensions describe this display body in original axes, assumed mm.",
                        ))
                        .small()
                        .color(Color32::GRAY),
                    );
                    if self.debug_ui && self.scene.assembly {
                        ui.separator();
                        ui.checkbox(
                            &mut self.calibration,
                            t(z, "编辑装配坐标", "Edit assembly coordinates"),
                        );
                        if self.calibration {
                            // Display/edit the current world offset, then bind it to
                            // the current orientation for subsequent assembly rotation.
                            if p.pose.travel_frame.is_some() {
                                p.pose.travel = (p.pose.matrix(1.).transform_point3(glam::Vec3::ZERO)
                                    - glam::Vec3::from(p.pose.position)).to_array();
                                p.pose.travel_frame = Some(p.pose.matrix(0.)
                                    .to_scale_rotation_translation().1.to_array());
                            }
                            let mut changed = false;
                            ui.label(t(
                                z,
                                "坐标为视口 X / Y向上 / Z，旋转为度。",
                                "Viewport X / Y up / Z; rotation in degrees.",
                            ));
                            if controlled {ui.colored_label(Color32::LIGHT_BLUE,t(z,"位置与角度由装配关系控制；可调整父件或关系参数。","Pose controlled by relation; edit parent or joint parameters."));}
                            for (field,(label, values, speed)) in [
                                (t(z, "位置 mm", "Position mm"), &mut p.pose.position, 0.5),
                                (t(z, "旋转 °", "Rotation °"), &mut p.pose.rotation, 0.5),
                                (t(z, "爆炸位移 mm", "Explosion mm"), &mut p.pose.travel, 1.),
                            ].into_iter().enumerate() {
                                ui.label(label);
                                ui.horizontal(|ui| {
                                    for v in values {
                                        changed |= ui
                                            .add_enabled(!controlled || field==2,
                                                egui::DragValue::new(v)
                                                    .speed(speed)
                                                    .range(-10000. ..=10000.),
                                            )
                                            .changed();
                                    }
                                });
                            }
                            if changed {
                                self.scene.calibrated = false;
                                if let Err(e)=crate::constraints::solve(&mut self.scene){self.status=e;}
                            }
                        }
                    }
                } else {
                    ui.label(t(
                        z,
                        "点击模型或装配树中的零件查看属性。",
                        "Select a part in the viewport or tree.",
                    ));
                }
                if self.debug_ui && self.scene.assembly {
                    if let Some(warning)=&self.scene.constraint_warning {ui.colored_label(Color32::LIGHT_RED,warning);}
                    ui.separator();
                    ui.colored_label(
                        if self.scene.calibrated {
                            Color32::LIGHT_GREEN
                        } else {
                            Color32::from_rgb(225, 184, 112)
                        },
                        if self.scene.calibrated {
                            t(z, "装配坐标已人工确认", "Assembly manually verified")
                        } else {
                            t(
                                z,
                                "装配坐标待校准 · 示意布局",
                                "Uncalibrated assembly · illustrative layout",
                            )
                        },
                    );
                    ui.checkbox(
                        &mut self.scene.calibrated,
                        t(z, "已人工核对本套装配", "I verified this assembly"),
                    );
                    if ui
                        .button(t(z, "保存本型号装配坐标", "Save assembly definition"))
                        .clicked()
                    {
                        save = true;
                    }
                    ui.label(
                        RichText::new(t(
                            z,
                            "保存到独立工程 state 目录，不修改 STL。",
                            "Saved in workspace/state; source STL stays unchanged.",
                        ))
                        .small()
                        .color(Color32::GRAY),
                    );
                }
                if save {
                    match toml::to_string_pretty(&self.scene.definition())
                        .map_err(|e| e.to_string())
                        .and_then(|s| {
                            self.write_state(&format!("assembly-{}.toml", self.scene_key), s)
                        }) {
                        Ok(_) => {
                            self.status =
                                t(z, "装配坐标已保存", "Assembly coordinates saved").into()
                        }
                        Err(e) => self.status = e,
                    }
                }
            });
    }
    fn viewport(&mut self, ctx: &egui::Context) {
        let z = self.zh;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(14, 18, 23))
                    .inner_margin(12.),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.debug_ui
                        && ui
                            .button(t(z, "装配关系 / 检查", "Relations / Check"))
                            .clicked()
                    {
                        self.joints_open = true;
                    }
                    if self.debug_ui {
                        ui.label(RichText::new("DEBUG").color(Color32::YELLOW));
                    }
                    ui.heading(if self.scene.title.is_empty() {
                        crate::app_name(z)
                    } else {
                        &self.scene.title
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t(z, "适配画面", "Fit view")).clicked() {
                            self.camera.fit(
                                &if self.divider_editor.open {
                                    self.interior_scene()
                                } else {
                                    self.scene.clone()
                                },
                                self.explosion,
                            );
                        }
                        ui.checkbox(&mut self.dimensions, t(z, "尺寸三视图", "Dimension views"));
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    for (label, yaw, pitch) in [
                        (t(z, "立体", "Orbit"), 0.65, 0.5),
                        (t(z, "正视", "Front"), 0., 0.),
                        (
                            t(z, "俯视", "Top"),
                            0.,
                            std::f32::consts::FRAC_PI_2 - 0.0001,
                        ),
                        (
                            t(z, "右视", "Right"),
                            std::f32::consts::FRAC_PI_2 - 0.0001,
                            0.,
                        ),
                    ] {
                        if ui.small_button(label).clicked() {
                            self.camera.yaw = yaw;
                            self.camera.pitch = pitch;
                        }
                    }
                    if self.scene.assembly {
                        ui.separator();
                        if ui
                            .selectable_label(self.explosion == 0., t(z, "合拢", "Assembled"))
                            .clicked()
                        {
                            self.explosion = 0.;
                            self.camera.fit(&self.scene, 0.);
                        }
                        if ui
                            .selectable_label(self.explosion == 1., t(z, "爆炸", "Exploded"))
                            .clicked()
                        {
                            self.explosion = 1.;
                            self.camera.fit(&self.scene, 1.);
                        }
                        ui.add(egui::Slider::new(&mut self.explosion, 0. ..=1.).text(t(
                            z,
                            "展开",
                            "Separation",
                        )));
                        if self.debug_ui
                            && ui
                                .small_button(t(z, "重排爆炸方向", "Reset explosion"))
                                .clicked()
                        {
                            match crate::explosion::reset(&mut self.scene) {
                                Ok(()) => {
                                    self.explosion = 1.;
                                    self.camera.fit(&self.scene, 1.);
                                }
                                Err(e) => self.status = e,
                            }
                        }
                        ui.checkbox(&mut self.ghost, t(z, "外壳透明", "Ghost shell"));
                    }
                    if ui
                        .checkbox(
                            &mut self.export_options.plate.preview_colors,
                            t(z, "打印颜色预览", "Print colors"),
                        )
                        .changed()
                    {
                        if let Err(e) = self.save_settings() {
                            self.status = e;
                        }
                        ctx.request_repaint();
                    }
                });
                ui.separator();
                if self.scene.parts.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(t(z, "打开模型库开始预览", "Open a model library to begin"));
                    });
                    return;
                }
                if self.dimensions {
                    let index = self.selected.unwrap_or(0);
                    let part = &self.scene.parts[index];
                    let mut p = part.clone();
                    p.visible = true;
                    p.pose.position = [0.; 3];
                    p.pose.rotation = [0.; 3];
                    p.pose.travel = [0.; 3];
                    let single = Scene {
                        title: String::new(),
                        parts: vec![p],
                        calibrated: true,
                        assembly: false,
                        ..Default::default()
                    };
                    // Reuse the current GPU scene using a temporary visibility/pose snapshot; no mesh re-upload.
                    let mut display = self.scene.clone();
                    for (i, p) in display.parts.iter_mut().enumerate() {
                        p.visible = i == index;
                        if i == index {
                            p.pose = single.parts[0].pose.clone();
                        }
                    }
                    ui.label(t(
                        z,
                        "所选展示实体 · 原始轴向 · 外包围盒尺寸",
                        "Selected display body · original axes · bounding-box dimensions",
                    ));
                    let size = part.mesh.size;
                    ui.columns(3, |cols| {
                        for (i, (label, yaw, pitch, w, h)) in [
                            (t(z, "正视 X/Z", "Front X/Z"), 0., 0., size.x, size.z),
                            (
                                t(z, "俯视 X/Y", "Top X/Y"),
                                0.,
                                std::f32::consts::FRAC_PI_2 - 0.0001,
                                size.x,
                                size.y,
                            ),
                            (
                                t(z, "右视 Y/Z", "Right Y/Z"),
                                std::f32::consts::FRAC_PI_2 - 0.0001,
                                0.,
                                size.y,
                                size.z,
                            ),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            let col = &mut cols[i];
                            col.strong(label);
                            let mut camera = Camera::default();
                            camera.fit(&single, 0.);
                            camera.yaw = yaw;
                            camera.pitch = pitch;
                            let width = col.available_width().max(20.);
                            let height = (ui_height(ctx) * 0.40).clamp(150., 380.);
                            let d = ctx.pixels_per_point();
                            let id = self.renderer.render(
                                i + 1,
                                [(width * d) as u32, (height * d) as u32],
                                &display,
                                &camera,
                                ViewOptions::default(),
                            );
                            col.add(egui::Image::new((id, egui::vec2(width, height))));
                            col.label(format!("← {:.2} mm →\n↕ {:.2} mm", w, h));
                        }
                    });
                } else {
                    let size = ui.available_size().max(egui::vec2(10., 10.));
                    let (rect, response) =
                        ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                    if response.dragged_by(egui::PointerButton::Primary) {
                        let delta = ctx.input(|i| i.pointer.delta());
                        self.camera.yaw -= delta.x * 0.008;
                        self.camera.pitch =
                            (self.camera.pitch + delta.y * 0.008).clamp(-1.55, 1.55);
                    }
                    if response.dragged_by(egui::PointerButton::Secondary)
                        || response.dragged_by(egui::PointerButton::Middle)
                    {
                        let d = ctx.input(|i| i.pointer.delta());
                        self.camera.pan[0] -= d.x / rect.width() * 2.;
                        self.camera.pan[1] += d.y / rect.height() * 2.;
                    }
                    if response.hovered() {
                        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                        self.camera.zoom =
                            (self.camera.zoom * (scroll * 0.002).exp()).clamp(0.05, 50.);
                    }
                    if response.clicked()
                        && let Some(p) = response.interact_pointer_pos()
                    {
                        let (o, d) = self.camera.ray(p, rect);
                        self.last_hit = if self.divider_editor.open {
                            self.interior_scene().pick(o, d, self.explosion)
                        } else {
                            self.scene.pick(o, d, self.explosion)
                        };
                        self.selected = self.last_hit.map(|h| h.part);
                    }
                    if response.double_clicked() {
                        self.camera.fit(
                            &if self.divider_editor.open {
                                self.interior_scene()
                            } else {
                                self.scene.clone()
                            },
                            self.explosion,
                        );
                    }
                    let dpi = ctx.pixels_per_point();
                    let id = self.renderer.render(
                        0,
                        [(size.x * dpi) as u32, (size.y * dpi) as u32],
                        &self.scene,
                        &self.camera,
                        ViewOptions {
                            explosion: self.explosion,
                            selected: if self.divider_editor.open {
                                self.divider_editor
                                    .selected_part(&self.divider_layout)
                                    .and_then(|(path, instance)| {
                                        self.scene.parts.iter().position(|p| {
                                            p.pose.path == path && p.pose.instance == instance
                                        })
                                    })
                            } else {
                                self.selected
                            },
                            ghost: self.ghost,
                            interior_only: self.divider_editor.open && self.scene.assembly,
                            colliding: self
                                .collision_report
                                .as_ref()
                                .filter(|r| r.is_current(&self.scene))
                                .map(|r| r.pairs.iter().flat_map(|p| [p.a, p.b]).collect())
                                .unwrap_or_default(),
                        },
                    );
                    ui.painter().image(
                        id,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                        Color32::WHITE,
                    );
                    if self.export_options.plate.preview_colors
                        && !self.divider_editor.open
                        && let Some(part) = self
                            .selected
                            .and_then(|i| self.scene.parts.get(i))
                            .filter(|p| p.visible)
                    {
                        let matrix = self.camera.matrix(rect.aspect_ratio())
                            * part.pose.matrix(self.explosion);
                        let corners: Vec<_> = (0..8)
                            .map(|i| {
                                let v = glam::Vec3::new(
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
                                let ndc = matrix.project_point3(v);
                                (
                                    egui::pos2(
                                        rect.center().x + ndc.x * rect.width() * 0.5,
                                        rect.center().y - ndc.y * rect.height() * 0.5,
                                    ),
                                    ndc.z,
                                )
                            })
                            .collect();
                        let painter = ui.painter().with_clip_rect(rect);
                        for i in 0..8 {
                            for bit in [1, 2, 4] {
                                let j = i ^ bit;
                                if i < j
                                    && (0. ..=1.).contains(&corners[i].1)
                                    && (0. ..=1.).contains(&corners[j].1)
                                {
                                    painter.line_segment(
                                        [corners[i].0, corners[j].0],
                                        egui::Stroke::new(1.5, Color32::LIGHT_BLUE),
                                    );
                                }
                            }
                        }
                    }
                    for (hit, color) in [
                        (self.parent_face, Color32::LIGHT_GREEN),
                        (self.child_face, Color32::LIGHT_BLUE),
                        (self.last_hit, Color32::YELLOW),
                    ] {
                        if let Some(hit) = hit
                            && let Some(part) = self.scene.parts.get(hit.part)
                        {
                            let point =
                                part.pose.matrix(self.explosion).transform_point3(hit.point);
                            let ndc = self
                                .camera
                                .matrix(rect.aspect_ratio())
                                .project_point3(point);
                            let screen = egui::pos2(
                                rect.center().x + ndc.x * rect.width() * 0.5,
                                rect.center().y - ndc.y * rect.height() * 0.5,
                            );
                            if rect.contains(screen) {
                                ui.painter().circle_stroke(
                                    screen,
                                    6.,
                                    egui::Stroke::new(2., color),
                                );
                                let normal = part
                                    .pose
                                    .matrix(self.explosion)
                                    .transform_vector3(hit.normal)
                                    .normalize_or_zero();
                                let end = self
                                    .camera
                                    .matrix(rect.aspect_ratio())
                                    .project_point3(point + normal * 10.);
                                let end_screen = egui::pos2(
                                    rect.center().x + end.x * rect.width() * 0.5,
                                    rect.center().y - end.y * rect.height() * 0.5,
                                );
                                ui.painter().arrow(
                                    screen,
                                    end_screen - screen,
                                    egui::Stroke::new(2., color),
                                );
                            }
                        }
                    }
                    ui.painter().text(
                        rect.left_bottom() + egui::vec2(12., -14.),
                        egui::Align2::LEFT_BOTTOM,
                        t(
                            z,
                            "拖动旋转 · 右键平移 · 滚轮缩放 · 单击选择",
                            "Drag orbit · Right drag pan · Scroll zoom · Click select",
                        ),
                        egui::FontId::proportional(12.),
                        Color32::GRAY,
                    );
                }
            });
    }
    fn save_preset(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("vault-configuration.toml")
            .add_filter(
                t(
                    self.zh,
                    "VAULT 装配工作台方案",
                    "VAULT Assembly Workbench preset",
                ),
                &["toml"],
            )
            .save_file()
        {
            let result = self
                .check_destination(&path)
                .and_then(|_| {
                    toml::to_string_pretty(&Preset {
                        print_config: Some(self.export_options.plate.clone()),
                        gridfinity: self.gridfinity.clone(),
                        label: self.label.clone(),
                        assembly_kind: AssemblyKind::Drawer,
                        selection: self.selection.clone(),
                        bom: self.bom.clone(),
                        divider_layout: self.divider_layout.clone(),
                    })
                    .map_err(|e| e.to_string())
                })
                .and_then(|s| std::fs::write(path, s).map_err(|e| e.to_string()));
            self.status = match result {
                Ok(_) => t(self.zh, "方案已保存", "Preset saved").into(),
                Err(e) => e,
            };
        }
    }
    fn load_preset(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                t(
                    self.zh,
                    "VAULT 装配工作台方案",
                    "VAULT Assembly Workbench preset",
                ),
                &["toml"],
            )
            .pick_file()
        {
            let result = std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|s| toml::from_str::<Preset>(&s).map_err(|e| e.to_string()));
            match result {
                Ok(p) => {
                    if !p.assembly_kind.supported() {
                        self.status=t(self.zh,"当前版本暂不支持盒子方案；当前抽屉配置已保留。","Box assemblies are not supported yet; the current drawer configuration was kept.").into();
                        return;
                    }
                    if let Some(config) = &p.print_config
                        && let Err(e) = config.validate()
                    {
                        self.status = e;
                        return;
                    }
                    if let Err(e) = p.gridfinity.validate(&p.selection) {
                        self.status = e;
                        return;
                    }
                    if let Err(e) = p.label.validate() {
                        self.status = e;
                        return;
                    }
                    if let Err(e) = p.divider_layout.validate(&self.catalog, &p.selection) {
                        self.status = format!("布局无效：{e}");
                        return;
                    }
                    if !["S", "M", "L", "XL"].contains(&p.selection.width.as_str())
                        || !(3..=7).contains(&p.selection.depth)
                        || !["1F", "2F", "4F"].contains(&p.selection.height.as_str())
                        || p.selection.width == "XL" && p.selection.height != "1F"
                        || p.bom.iter().any(|b| {
                            (!b.path.starts_with(crate::gridfinity::PREFIX)
                                && self.catalog.resolve(&b.path).is_none())
                                || b.qty == 0
                                || b.qty > 100
                        })
                    {
                        self.status = t(
                            self.zh,
                            "方案包含当前模型库中不存在或无效的组件",
                            "Preset has invalid or unavailable components",
                        )
                        .into();
                    } else {
                        if let Some(config) = p.print_config {
                            self.export_options.plate = config;
                        } else {
                            self.export_options.plate.part_filaments.clear();
                            self.export_options.plate.text_filaments.clear();
                        }
                        self.selection = p.selection;
                        self.label = p.label;
                        self.gridfinity = p.gridfinity;
                        self.divider_layout = p.divider_layout;
                        self.divider_layout.specification = crate::dividers::spec(&self.selection);
                        self.layout_cache.remove(&self.divider_layout.specification);
                        self.divider_editor.reset();
                        let excluded = p
                            .bom
                            .iter()
                            .filter(|b| {
                                b.enabled
                                    && !b.path.starts_with(crate::gridfinity::PREFIX)
                                    && !self
                                        .catalog
                                        .bom(&self.selection)
                                        .iter()
                                        .any(|n| n.path == b.path)
                                    && !self
                                        .catalog
                                        .accessories(&self.selection)
                                        .iter()
                                        .any(|n| n.path == b.path)
                            })
                            .count();
                        self.bom = self.catalog.reconfigure(&self.selection, &p.bom);
                        self.assembly(ctx);
                        self.status = format!(
                            "{} {excluded}",
                            t(
                                self.zh,
                                "方案已载入，默认齿条已补齐；已移除超出本版本组合范围的文件：",
                                "Preset loaded; required racks restored. Out-of-scope files removed:"
                            )
                        );
                    }
                }
                Err(e) => self.status = e,
            }
        }
    }
    fn check_destination(&self, path: &Path) -> Result<(), String> {
        let parent = if path.is_dir() {
            path
        } else {
            path.parent().ok_or("Invalid destination")?
        };
        let dest = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
        if let Ok(root) = std::fs::canonicalize(&self.catalog.root)
            && dest.starts_with(root)
        {
            return Err(t(
                self.zh,
                "请选择模型库之外的目录，保持源文件只读",
                "Choose a destination outside the read-only model library",
            )
            .into());
        }
        Ok(())
    }
    fn gridfinity_window(&mut self, ctx: &egui::Context) {
        if !self.gridfinity_open {
            self.gridfinity_editor.cancel();
            return;
        }
        let mut open = true;
        let mut draft = self.gridfinity.clone();
        let mut selected = self.gridfinity_selected;
        let mut preview = None;
        let mut focus = false;
        let mut action_error = None;
        let Ok(grid) = crate::gridfinity::grid_for(&self.selection, &self.gridfinity.baseplate)
        else {
            self.gridfinity_open = false;
            return;
        };
        let max = crate::gridfinity::max_units(&self.selection).unwrap_or(3);
        egui::Window::new(t(self.zh, "Gridfinity 盒子编辑", "Gridfinity bin editor"))
            .open(&mut open)
            .default_width(560.)
            .max_height((ctx.content_rect().height() - 120.).max(300.))
            .vscroll(true)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(format!(
                    "{} × {} 格 · {} · 自动盒高 {} U / {} mm（另含堆叠边）",
                    grid[0],
                    grid[1],
                    self.selection.height,
                    max,
                    max * 7
                ));
                ui.small("使用 Gridfinity Rebuilt 标准底脚与堆叠边；盒子与收纳隔板互斥。");
                ui.horizontal_wrapped(|ui| {
                    let add = ui.add(
                        egui::Button::new("添加盒子 / 拖入").sense(egui::Sense::click_and_drag()),
                    );
                    if add.drag_started_by(egui::PointerButton::Primary) {
                        self.gridfinity_editor.start_stamp(&draft, &self.selection);
                    }
                    if add.clicked() {
                        match draft.add(&self.selection, None) {
                            Ok(id) => selected = Some(id),
                            Err(e) => action_error = Some(e),
                        }
                    }
                    if ui
                        .add_enabled(selected.is_some(), egui::Button::new("复制"))
                        .clicked()
                    {
                        let template = draft.bins.iter().find(|b| Some(b.id) == selected).cloned();
                        match draft.add(&self.selection, template) {
                            Ok(id) => selected = Some(id),
                            Err(e) => action_error = Some(e),
                        }
                    }
                    if ui
                        .add_enabled(selected.is_some(), egui::Button::new("删除"))
                        .clicked()
                    {
                        draft.bins.retain(|b| Some(b.id) != selected);
                        selected = None;
                    }
                    if ui.button("查看底板与盒子").clicked() {
                        focus = true;
                    }
                    if ui.button("取消选择").clicked() {
                        selected = None;
                    }
                });
                ui.small("俯视：上方为抽屉后部；可以从上方按钮拖入，或在空白网格拖框添加。");
                self.gridfinity_editor
                    .canvas(ui, &mut draft, &self.selection, &mut selected);
                if let Some(b) = draft.bins.iter_mut().find(|b| Some(b.id) == selected) {
                    ui.separator();
                    ui.strong(format!("盒子 {}", b.id));
                    ui.horizontal(|ui| {
                        for (axis, name) in ["起始列", "起始行"].iter().enumerate() {
                            let mut n = b.cell[axis] + 1;
                            ui.label(*name);
                            ui.add(egui::DragValue::new(&mut n).range(1..=grid[axis]));
                            b.cell[axis] = n - 1;
                        }
                    });
                    ui.horizontal(|ui| {
                        for (axis, name) in ["宽度（格）", "深度（格）"].iter().enumerate()
                        {
                            ui.label(*name);
                            ui.add(egui::DragValue::new(&mut b.span[axis]).range(1..=grid[axis]));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut b.auto_height, "根据抽屉层高自动配置");
                        if !b.auto_height {
                            ui.label("高度 U");
                            ui.add(egui::DragValue::new(&mut b.height_units).range(3..=max));
                        }
                    });
                    ui.checkbox(&mut b.scoop, "取物圆弧").on_hover_text(
                        "在盒底与侧壁之间增加圆弧，方便取出小零件；开启或关闭会更新盒体。",
                    );
                    ui.checkbox(&mut b.tab, "标签台");
                    if b.tab {
                        ui.checkbox(&mut b.label.enabled, "标签文字");
                        if b.label.enabled {
                            ui.add(
                                egui::TextEdit::multiline(&mut b.label.text)
                                    .desired_rows(2)
                                    .char_limit(48)
                                    .hint_text("最多两行，34 × 10 mm 安全区；留空保留空白标签台"),
                            );
                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut b.label.style,
                                    crate::label::Style::Raised,
                                    "凸字",
                                );
                                ui.radio_value(
                                    &mut b.label.style,
                                    crate::label::Style::Engraved,
                                    "凹字",
                                );
                                ui.radio_value(
                                    &mut b.label.font,
                                    crate::label::Font::Chinese,
                                    "微软雅黑",
                                );
                                ui.radio_value(
                                    &mut b.label.font,
                                    crate::label::Font::Arial,
                                    "Arial",
                                );
                            });
                            ui.add(egui::Slider::new(&mut b.label.size, 2.0..=12.).text("字号 mm"));
                            ui.add(
                                egui::Slider::new(&mut b.label.depth, 0.2..=0.8)
                                    .text("字高 / 刻深 mm"),
                            );
                        }
                    }
                    if ui.button("单独预览此盒子").clicked() {
                        preview = Some(b.path());
                    }
                }
                if let Some(e) = &self.gridfinity_error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
                if self.busy || self.preview.has_pending() {
                    ui.spinner();
                    ui.label("正在后台更新；最多并行生成两个盒体，修改文字复用盒体。");
                }
                if crate::gridfinity::executable(&self.home).is_none() {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "未找到 OpenSCAD；请运行 tools/setup-gridfinity.ps1。",
                    );
                }
            });
        self.gridfinity_open = open;
        if !open {
            self.gridfinity_editor.cancel();
        }
        self.gridfinity_selected = selected;
        if let Some(e) = action_error {
            self.gridfinity_error = Some(e);
        }
        if draft != self.gridfinity {
            match draft.validate(&self.selection) {
                Ok(()) => {
                    self.gridfinity = draft;
                    self.gridfinity_error = None;
                    self.assembly(ctx);
                }
                Err(e) => self.gridfinity_error = Some(e),
            }
        }
        if let Some(path) = preview {
            self.single(path, ctx);
        }
        if focus {
            for p in &mut self.scene.parts {
                p.visible = p.pose.path.contains("/Boxes/")
                    || p.pose.path.contains("/Gridfinity Baseplate/")
                    || p.pose.path.starts_with(crate::gridfinity::PREFIX);
            }
            self.explosion = 0.;
            self.camera.fit(&self.scene, 0.);
            self.camera.yaw = 0.4;
            self.camera.pitch = 1.1;
        }
    }
    fn label_window(&mut self, ctx: &egui::Context) {
        if !self.label_open {
            return;
        }
        let z = self.zh;
        let mut open = true;
        let before = self.label.clone();
        let mut preview = false;
        egui::Window::new(t(z,"编辑标签文字","Edit label text")).id(egui::Id::new("label-editor"))
            .open(&mut open).resizable(false).default_width(440.).show(ctx,|ui|{
                ui.label(t(z,"基于 Label.stl，保留原标签外形与安装结构。","Based on Label.stl, preserving its shape and mounting features."));
                ui.checkbox(&mut self.label.enabled,t(z,"启用标签文字","Enable lettering"));
                ui.add_enabled_ui(self.label.enabled,|ui|{
                    ui.add(egui::TextEdit::multiline(&mut self.label.text).desired_rows(2).char_limit(48));
                    ui.small(t(z,"最多两行；文字居中，安全区域 38 × 12 mm。","Up to two centered lines within a 38 × 12 mm safe area."));
                    ui.horizontal(|ui|{
                        ui.radio_value(&mut self.label.style,crate::label::Style::Raised,t(z,"凸字","Raised"));
                        ui.radio_value(&mut self.label.style,crate::label::Style::Engraved,t(z,"凹字","Engraved"));
                    });
                    ui.horizontal(|ui|{
                        ui.label(t(z,"字体","Font"));
                        ui.selectable_value(&mut self.label.font,crate::label::Font::Chinese,t(z,"微软雅黑（中英文）","Microsoft YaHei"));
                        ui.selectable_value(&mut self.label.font,crate::label::Font::Arial,"Arial");
                    });
                    ui.add(egui::Slider::new(&mut self.label.size,2. ..=12.).text(t(z,"字号 mm","Size mm")));
                    ui.add(egui::Slider::new(&mut self.label.depth,0.2..=0.8).text(t(z,"字高 / 刻深 mm","Relief / depth mm")));
                    ui.horizontal(|ui|{
                        ui.label(t(z,"水平偏移 mm","Horizontal mm"));ui.add(egui::DragValue::new(&mut self.label.offset[0]).speed(0.1).range(-10. ..=10.));
                        ui.label(t(z,"垂直偏移 mm","Vertical mm"));ui.add(egui::DragValue::new(&mut self.label.offset[1]).speed(0.1).range(-10. ..=10.));
                    });
                });
                ui.horizontal(|ui|{
                    if ui.button(t(z,"单独预览标签","Preview label")).clicked(){preview=true;}
                    if ui.button(t(z,"恢复空白标签","Restore blank label")).clicked(){self.label=Default::default();}
                });
                ui.small(t(z,"停止输入后自动更新；文字会写入导出的 STL / 3MF，原文件不变。","Updates after typing; exported STL / 3MF includes the lettering. Source stays unchanged."));
                ui.small(t(z,"选中标签后，可在右侧属性设置独立文字颜色；3MF 保留同盘多色。","Select the label to set text color in Properties; 3MF keeps both materials together."));
                if let Err(e)=self.label.validate(){ui.colored_label(Color32::LIGHT_RED,e);}
                if self.failed_preview.is_some() && !self.busy && !self.preview.has_pending(){ui.colored_label(Color32::LIGHT_RED,&self.status);}
            });
        self.label_open = open;
        if before != self.label {
            if let Err(e) = self.label.validate() {
                self.status = e;
                return;
            }
            if self.label.active() {
                let qty = self
                    .bom
                    .iter()
                    .find(|b| b.path == crate::label::PATH)
                    .map_or(1, |b| b.qty);
                Catalog::set_accessory(&mut self.bom, crate::label::PATH.into(), true, qty);
            }
            if !self.scene.assembly
                && self
                    .scene
                    .parts
                    .iter()
                    .any(|p| p.pose.path == crate::label::PATH)
            {
                self.single(crate::label::PATH.into(), ctx);
            } else {
                self.assembly(ctx);
            }
        }
        if preview {
            self.single(crate::label::PATH.into(), ctx);
        }
    }
    fn color_window(&mut self, ctx: &egui::Context) {
        if !self.colors_open {
            return;
        }
        let mut open = true;
        let z = self.zh;
        let mut changed = false;
        egui::Window::new(t(z,"配色与分盘","Colors and plates")).id(egui::Id::new("print-colors"))
            .open(&mut open).default_width(570.).default_height(650.).vscroll(true).show(ctx,|ui| {
            ui.label(t(z,"编辑 6 个 PETG 颜色槽，并为整件模型指定颜色。同一源文件的所有份数共用颜色。","Edit six PETG color slots and assign whole-part colors. Copies of the same source share a color."));
            ui.add_enabled_ui(self.export_rx.is_none(),|ui| {
                let c=&mut self.export_options.plate;
                egui::Grid::new("color-palette").num_columns(3).show(ui,|ui| {
                    for i in 0..6 {
                        ui.label(format!("PETG {}",i+1));
                        if ui.color_edit_button_srgb(&mut c.palette[i]).changed(){changed=true;c.preview_colors=true;}
                        ui.monospace(c.color_hex(i as u32+1));ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(t(z,"默认颜色","Default color"));
                    egui::ComboBox::from_id_salt("default-filament").selected_text(format!("{} · {}",c.filament,c.color_hex(c.filament))).show_ui(ui,|ui| {
                        for slot in 1..=6 {let name=format!("{slot} · {}",c.color_hex(slot));if ui.selectable_value(&mut c.filament,slot,name).changed(){changed=true;c.preview_colors=true;}}
                    });
                    if ui.button(t(z,"全部恢复默认颜色","Reset assignments")).clicked(){c.part_filaments.clear();c.text_filaments.clear();changed=true;}
                });
                ui.separator();
                for item in self.bom.iter().filter(|b|b.enabled) {
                    ui.push_id(&item.path,|ui| {ui.horizontal(|ui| {
                        let slot=c.slot(&item.path);let mut choice=c.part_filaments.get(&item.path).copied().unwrap_or(0);
                        let [r,g,b]=c.palette[(slot-1) as usize];ui.colored_label(Color32::from_rgb(r,g,b),"●");
                        let name=crate::accessories::info(&item.path,z).map_or_else(||item.name().to_string(),|m|m.name);
                        ui.label(format!("{name} ×{}",item.qty));
                        egui::ComboBox::from_id_salt("part-color").selected_text(if choice==0 {format!("{} · {}",t(z,"默认","Default"),c.color_hex(slot))}else{format!("{slot} · {}",c.color_hex(slot))}).show_ui(ui,|ui| {
                            ui.selectable_value(&mut choice,0,t(z,"跟随默认颜色","Use default"));
                            for slot in 1..=6 {ui.selectable_value(&mut choice,slot,format!("{slot} · {}",c.color_hex(slot)));}
                        });
                        if choice!=c.part_filaments.get(&item.path).copied().unwrap_or(0) {
                            if choice==0 {c.part_filaments.remove(&item.path);}else{c.part_filaments.insert(item.path.clone(),choice);}
                            c.preview_colors=true;changed=true;
                        }
                    });});
                }
                ui.separator();ui.small(t(z,"3MF 按耗材组合分盘；文字与底板保持同盘。相同 RGB 的不同槽仍独立处理。STL 不记录颜色或打印盘。","3MF groups by material set; text stays with its body. STL stores neither colors nor plates."));
            });
        });
        self.colors_open = open;
        if changed {
            if let Err(e) = self.save_settings() {
                self.status = e;
            }
            ctx.request_repaint();
        }
    }
    fn export_window(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.export_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.export_rx = None;
                    match result {
                        Ok(path) => {
                            self.status = format!(
                                "{} {}",
                                t(self.zh, "导出完成：", "Export complete:"),
                                path.display()
                            );
                            self.export_last = Some(path);
                        }
                        Err(e) => {
                            self.status =
                                format!("{}: {e}", t(self.zh, "导出失败", "Export failed"))
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.export_rx = None;
                    self.status = "导出任务意外结束".into();
                }
            }
        }
        if !self.export_open {
            return;
        }
        let z = self.zh;
        let mut open = true;
        let mut start = false;
        egui::Window::new(t(z,"导出","Export")).id(egui::Id::new("export-options"))
            .open(&mut open).resizable(true).vscroll(true).default_width(540.).default_height(680.).show(ctx,|ui|{
                ui.label(t(z,"导出当前打印清单中勾选的零件，按份数生成。","Export checked items in the print list, including every copy."));
                let selected:Vec<_>=self.bom.iter().filter(|b|b.enabled).collect();
                ui.label(format!("{} {} / {} {}",selected.len(),t(z,"种源文件","source files"),selected.iter().map(|b|b.qty).sum::<u32>(),t(z,"份","copies")));
                ui.add_enabled_ui(self.export_rx.is_none(),|ui|{
                    for (format,label) in [
                        (crate::export::Format::Stl,t(z,"STL 文件夹（每份单独文件）","STL folder (one file per copy)")),
                        (crate::export::Format::ThreeMf,t(z,"3MF 工程（P1S 自动摆盘）","3MF project (P1S plate layout)")),
                    ] {ui.radio_value(&mut self.export_options.format,format,label);}
                    ui.separator();
                    if self.export_options.format==crate::export::Format::ThreeMf {
                        ui.strong("Bambu Lab P1S · 0.4 mm");
                        ui.small(t(z,"256 × 256 mm；参考配置限高 250 mm；自动避开左前角禁放区。","256 × 256 mm; profile height 250 mm; front-left keep-out reserved."));
                        let p=&mut self.export_options.plate;
                        ui.horizontal(|ui|{ui.label(t(z,"打印板","Plate"));ui.selectable_value(&mut p.textured,true,"Textured PEI");ui.selectable_value(&mut p.textured,false,"Smooth PEI / High Temp");});
                        ui.add(egui::Slider::new(&mut p.margin,0.0..=30.0).text(t(z,"平台边距 mm","Bed margin mm")));
                        ui.add(egui::Slider::new(&mut p.gap,0.0..=30.0).text(t(z,"零件间距 mm","Part spacing mm")));
                        ui.add(egui::Slider::new(&mut p.brim,0.0..=10.0).text(t(z,"外侧 Brim mm（0 关闭）","Outer brim mm (0 off)")));
                        ui.checkbox(&mut p.rotate,t(z,"允许平面旋转 90°","Allow 90° rotation on bed"));
                        ui.checkbox(&mut p.split,t(z,"同色零件放不下时自动增加打印盘","Add plates when one color overflows"));
                        ui.checkbox(&mut p.center,t(z,"每盘零件整体居中","Center each plate’s parts")).on_hover_text(t(z,"保留间距整体居中；若触及禁放区则保留原摆放。","Preserve spacing; keep original placement if centering hits the keep-out."));
                        ui.separator();
                        ui.add(egui::Slider::new(&mut p.layer,0.08..=0.28).step_by(0.01).text(t(z,"层高 mm","Layer height mm")));
                        ui.add(egui::Slider::new(&mut p.walls,1..=10).text(t(z,"壁数","Wall loops")));
                        ui.add(egui::Slider::new(&mut p.infill,0..=100).text(t(z,"螺旋填充 %","Gyroid infill %")));
                        ui.label(t(z,"按耗材组合分盘，文字与底板同盘；多色打印请在 Bambu Studio 确认 AMS 映射、冲刷和擦拭塔。","Separate plates per color slot; overflow adds plates within that slot."));
                        if ui.button(t(z,"配置零件颜色…","Configure part colors…")).clicked(){self.colors_open=true;}
                        ui.checkbox(&mut p.support_by_name,t(z,"为 Turn On Supports 零件启用支撑","Enable support for Turn On Supports parts"));
                        ui.small(t(z,"使用参考工程的 PETG 参数。保留原始打印朝向，未切片；请在 Bambu Studio 检查支撑、耗材和首层后打印。","Uses reference PETG settings. Original print orientation; unsliced. Review supports, filament and first layer in Bambu Studio."));
                    }
                    if ui.add_enabled(!selected.is_empty() && !self.busy && !self.preview.has_pending(),
                        egui::Button::new(t(z,"选择位置并导出…","Choose destination and export…"))).clicked() {start=true;}
                });
                if self.export_rx.is_some() {ui.horizontal(|ui|{ui.spinner();ui.label(t(z,"正在后台导出…","Exporting in background…"));});}
                if let Some(path)=&self.export_last {ui.label(path.to_string_lossy());}
            });
        self.export_open = open;
        if start {
            self.start_export(ctx);
        }
    }
    fn start_export(&mut self, ctx: &egui::Context) {
        let name = format!(
            "VAULT-{}{}-{}",
            self.selection.width, self.selection.depth, self.selection.height
        );
        let target = if self.export_options.format == crate::export::Format::Stl {
            rfd::FileDialog::new().pick_folder().map(|folder| {
                folder.join(format!(
                    "{name}-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ))
            })
        } else {
            rfd::FileDialog::new()
                .set_file_name(format!("{name}.3mf"))
                .add_filter("3MF", &["3mf"])
                .save_file()
        };
        let Some(target) = target else {
            return;
        };
        if self.export_options.format != crate::export::Format::Stl
            && !target
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("3mf"))
        {
            self.status = t(
                self.zh,
                "请使用 .3mf 文件扩展名。",
                "Please use the .3mf extension.",
            )
            .into();
            return;
        }
        if let Err(e) = self.check_destination(&target) {
            self.status = e;
            return;
        }
        if let Err(e) = self.save_settings() {
            self.status = e;
            return;
        }
        let items = self.bom.clone();
        let root = self.catalog.root.clone();
        let options = self.export_options.clone();
        let label = self.label.clone();
        let gf = self.gridfinity.clone();
        let gf_selection = self.selection.clone();
        let gf_home = self.home.clone();
        let (tx, rx) = mpsc::channel();
        self.export_rx = Some(rx);
        self.export_last = None;
        self.status = t(self.zh, "正在后台导出…", "Exporting in background…").into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::export::run_configured(
                &root,
                &items,
                &target,
                &options,
                &label,
                Some((&gf_home, &gf, &gf_selection)),
            );
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.toolbar(ctx);
        self.bottom(ctx);
        self.left(ctx);
        self.divider_view(ctx);
        self.right(ctx);
        self.color_window(ctx);
        self.renderer.print_colors = self
            .export_options
            .plate
            .preview_colors
            .then(|| self.export_options.plate.clone());
        self.viewport(ctx);
        self.relations_window(ctx);
        self.export_window(ctx);
        self.label_window(ctx);
        self.gridfinity_window(ctx);
        if self.divider_editor.show(
            ctx,
            &self.catalog,
            &self.selection,
            &mut self.divider_layout,
        ) {
            if !self.divider_layout.enabled {
                self.divider_layout.remove(&mut self.bom, &self.selection);
                self.layout_cache
                    .remove(&crate::dividers::spec(&self.selection));
            }
            self.assembly(ctx);
        }
        if let Some((name, texture)) = &self.image {
            let mut open = true;
            egui::Window::new(name)
                .open(&mut open)
                .default_size([800., 600.])
                .show(ctx, |ui| {
                    ui.add(egui::Image::new(texture).fit_to_exact_size(
                        texture.size_vec2()
                            * ((ui.available_width() / texture.size_vec2().x).min(1.)),
                    ));
                });
            if !open {
                self.image = None;
            }
        }
        self.pump_preview(ctx);
        if self.busy || self.preview.has_pending() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if self.smoke {
            ctx.request_repaint_after(Duration::from_millis(100));
            if !self.busy && !self.preview.has_pending() && !self.scene.parts.is_empty() {
                self.smoke_frame += 1;
                if self.smoke_frame == 5 && [36, 37].contains(&self.smoke_stage) {
                    self.selected = Some(0);
                    self.camera.yaw = 0.;
                    self.camera.pitch = 1.2;
                }
                if self.smoke_frame == 5 && [31, 32].contains(&self.smoke_stage) {
                    for p in &mut self.scene.parts {
                        p.visible = p.pose.path.contains("/Facades/")
                            || p.pose.path.ends_with("/Facade Cap.stl")
                            || p.pose.path.ends_with("/Label.stl");
                        if p.pose.path.ends_with("/Facade Cap.stl") {
                            p.color = [1., 0.5, 0.15, 1.];
                        }
                    }
                    self.explosion = 0.;
                    self.ghost = false;
                    self.selected = None;
                    self.camera.fit(&self.scene, 0.);
                    self.camera.yaw = 0.15;
                    self.camera.pitch = 0.15;
                }
                if self.smoke_frame == 5 && [27, 29, 30].contains(&self.smoke_stage) {
                    for p in &mut self.scene.parts {
                        p.visible = p.pose.path.contains("/Boxes/")
                            || p.pose.path.contains("/Gridfinity Baseplate/")
                            || p.pose.path.starts_with(crate::gridfinity::PREFIX);
                    }
                    self.explosion = if self.smoke_stage == 29 { 1. } else { 0. };
                    self.camera.fit(&self.scene, self.explosion);
                    self.camera.yaw = 0.4;
                    self.camera.pitch = 1.1;
                    self.gridfinity_open = self.smoke_stage == 27;
                }
                if self.smoke_frame == 5 && self.smoke_stage == 28 {
                    self.camera.fit(&self.scene, 0.);
                    self.camera.yaw = std::f32::consts::PI;
                    self.camera.pitch = 1.05;
                }
                if self.smoke_frame == 5 && [21, 22, 24, 25].contains(&self.smoke_stage) {
                    self.label_open = true;
                    self.selected = None;
                    self.camera.fit(&self.scene, 0.);
                    self.camera.yaw = std::f32::consts::PI;
                    self.camera.pitch = 1.25;
                    self.explosion = 0.;
                }
                if self.smoke_frame == 5 && [23, 26].contains(&self.smoke_stage) {
                    for p in &mut self.scene.parts {
                        p.visible =
                            p.pose.path.contains("/Facades/") || p.pose.path == crate::label::PATH;
                    }
                    self.explosion = 0.;
                    self.camera.fit(&self.scene, 0.);
                    self.camera.yaw = 0.;
                    self.camera.pitch = 0.;
                }
                if self.smoke_frame == 5 && self.smoke_stage == 0 {
                    self.selected = self.scene.parts.iter().position(|p| p.pose.body.is_some());
                }
                if self.smoke_frame == 5 && self.smoke_stage == 12 {
                    for p in &mut self.scene.parts {
                        p.visible = p.pose.path.contains("/Boxes/")
                            || p.pose.path.contains("/Gridfinity Baseplate/");
                    }
                    self.explosion = 0.;
                    self.ghost = false;
                    self.selected = self
                        .scene
                        .parts
                        .iter()
                        .position(|p| p.pose.path.contains("/Gridfinity Baseplate/"));
                    self.camera.fit(&self.scene, 0.);
                    self.camera.yaw = 0.7;
                    self.camera.pitch = 0.8;
                }
                if self.smoke_frame == 5 && self.smoke_stage == 14 {
                    for p in &mut self.scene.parts {
                        p.visible =
                            p.pose.path.contains("/Facades/") || p.pose.path.contains("Key");
                    }
                    self.explosion = 0.;
                    self.ghost = false;
                    self.selected = self
                        .scene
                        .parts
                        .iter()
                        .position(|p| p.pose.path.contains("Hidden Key Part 2"));
                    self.camera.fit(&self.scene, 0.);
                    self.camera.yaw = 0.6;
                    self.camera.pitch = 0.35;
                }
                if self.smoke_frame == 5 && self.smoke_stage == 16 {
                    self.selected = None;
                    self.divider_editor.open = true;
                }
                if self.smoke_frame == 5 && self.smoke_stage == 17 {
                    for p in &mut self.scene.parts {
                        p.visible = crate::dividers::interior_part(&p.pose.path);
                    }
                    self.explosion = 0.;
                    self.ghost = false;
                    self.camera.fit(&self.scene, 0.);
                    self.camera.yaw = 0.6;
                    self.camera.pitch = 0.9;
                }
                if self.smoke_frame
                    > (if [16, 19, 21, 24, 27].contains(&self.smoke_stage) {
                        40
                    } else {
                        8
                    })
                    && !self.screenshot_requested
                {
                    self.screenshot_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
            }
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let dir = self.home.join("output");
                    let _ = std::fs::create_dir_all(&dir);
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    let result = image::save_buffer(
                        dir.join(
                            [
                                "native-smoke.png",
                                "native-dimensions.png",
                                "native-assembled.png",
                                "native-relations.png",
                                "native-front-panel.png",
                                "native-springs-rear.png",
                                "native-module-drawer.png",
                                "native-latch-seat.png",
                                "native-runners-closed.png",
                                "native-runners-open.png",
                                "native-runner-stopper.png",
                                "native-geared-racks.png",
                                "native-baseplate-seated.png",
                                "native-baseplate-exploded.png",
                                "native-hidden-key.png",
                                "native-hidden-key-exploded.png",
                                "native-divider-editor.png",
                                "native-divider-layout.png",
                                "native-divider-exploded.png",
                                "native-export.png",
                                "native-export-p1s.png",
                                "native-label-raised.png",
                                "native-label-engraved.png",
                                "native-label-assembled.png",
                                "native-label-double-raised.png",
                                "native-label-double-engraved.png",
                                "native-label-double-assembled.png",
                                "native-gridfinity-editor.png",
                                "native-gridfinity-bin.png",
                                "native-gridfinity-exploded.png",
                                "native-gridfinity-height-2f.png",
                                "native-facade-caps-m1.png",
                                "native-facade-caps-m2.png",
                                "native-print-colors.png",
                                "native-print-colors-assembly.png",
                                "native-center-export.png",
                                "native-text-colors-raised.png",
                                "native-text-colors-engraved.png",
                                "native-english-name.png",
                            ][self.smoke_stage as usize],
                        ),
                        &bytes,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    );
                    let _ = std::fs::write(
                        dir.join("smoke-result.txt"),
                        format!(
                            "GPU: {}\nSTL files: {}\nInstances: {}\nScreenshot: {:?}\nStatus: {}",
                            self.renderer.state.adapter.get_info().name,
                            self.catalog.stl_count(),
                            self.scene.parts.len(),
                            result,
                            self.status
                        ),
                    );
                    if self.smoke_stage == 38 {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else {
                        self.smoke_stage += 1;
                        self.smoke_frame = 0;
                        self.screenshot_requested = false;
                        match self.smoke_stage {
                            1 => {
                                self.dimensions = true;
                                self.selected =
                                    self.scene.parts.iter().position(|p| p.pose.body.is_some());
                            }
                            2 => {
                                self.dimensions = false;
                                self.explosion = 0.;
                                self.ghost = true;
                                self.camera.fit(&self.scene, 0.);
                                self.check_intersections(ctx);
                            }
                            3 => {
                                self.joints_open = true;
                            }
                            4 => {
                                self.joints_open = false;
                                self.ghost = false;
                                self.selected = None;
                                self.collision_report = None;
                                for p in &mut self.scene.parts {
                                    let n = p.pose.path.rsplit('/').next().unwrap_or("");
                                    p.visible = p.pose.path.contains("/Facades/")
                                        || [
                                            "Key (Turn On Supports).stl",
                                            "Key Cap.stl",
                                            "Label.stl",
                                            "Facade Cap.stl",
                                        ]
                                        .contains(&n);
                                }
                                self.camera.fit(&self.scene, 0.);
                                self.camera.yaw = 0.25;
                                self.camera.pitch = 0.25;
                            }
                            5 => {
                                for p in &mut self.scene.parts {
                                    p.visible = p.pose.path.contains("/Facades/")
                                        || p.pose.path.ends_with("/Springs.stl")
                                        || p.pose.path.ends_with("/Key (Turn On Supports).stl");
                                }
                                self.camera.fit(&self.scene, 0.);
                                self.camera.yaw = std::f32::consts::PI + 0.25;
                                self.camera.pitch = 0.45;
                            }
                            6 => {
                                for p in &mut self.scene.parts {
                                    p.visible = p.pose.path.contains("/Modules/")
                                        || p.pose.path.contains("/Boxes/")
                                        || p.pose.path.contains("/Facades/");
                                }
                                self.camera.fit(&self.scene, 0.);
                                self.camera.yaw = 0.65;
                                self.camera.pitch = 0.5;
                                self.ghost = true;
                            }
                            7 => {
                                for p in &mut self.scene.parts {
                                    p.visible = p.pose.path.contains("/Modules/")
                                        || p.pose.path.ends_with("/Latch.stl");
                                }
                                self.camera.fit(&self.scene, 0.);
                                self.camera.yaw = 0.65;
                                self.camera.pitch = -1.1;
                                self.ghost = false;
                            }
                            8 => {
                                for p in &mut self.scene.parts {
                                    p.visible = p.pose.path.contains("/Modules/")
                                        || p.pose.path.contains("/Boxes/")
                                        || p.pose.path.contains("/Runners/");
                                }
                                self.camera.fit(&self.scene, 0.);
                                self.camera.yaw = 0.9;
                                self.camera.pitch = 0.5;
                                self.ghost = true;
                            }
                            9 => {
                                for r in &mut self.scene.relations {
                                    if r.child.path.contains("/Boxes/")
                                        && let crate::constraints::Joint::Slider { offset, .. } =
                                            &mut r.joint
                                    {
                                        *offset = 50.;
                                    }
                                }
                                let _ = crate::constraints::solve(&mut self.scene);
                                self.camera.fit(&self.scene, 0.);
                            }
                            10 => {
                                for r in &mut self.scene.relations {
                                    if r.child.path.contains("/Boxes/")
                                        && let crate::constraints::Joint::Slider { offset, .. } =
                                            &mut r.joint
                                    {
                                        *offset = 0.;
                                    }
                                }
                                let _ = crate::constraints::solve(&mut self.scene);
                                for p in &mut self.scene.parts {
                                    p.visible = p.pose.path.contains("/Modules/")
                                        || (p.pose.path.contains("/Runners/")
                                            && p.pose.instance == 1);
                                }
                                self.camera.fit(&self.scene, 0.);
                                self.camera.yaw = 1.25;
                                self.camera.pitch = 0.2;
                                self.camera.zoom = 3.5;
                                if let Some(p) = self.scene.parts.iter().find(|p| {
                                    p.pose.path.ends_with("/Runner Stopper.stl")
                                        && p.pose.instance == 1
                                }) {
                                    self.camera.center = p.pose.position.into();
                                }
                            }
                            11 => {
                                for p in &mut self.scene.parts {
                                    p.visible = p.pose.path.contains("/Boxes/")
                                        || p.pose.path.contains("/Geared Rails/");
                                }
                                self.ghost = false;
                                self.camera.fit(&self.scene, 0.);
                                self.camera.yaw = 0.7;
                                self.camera.pitch = 0.45;
                                self.camera.zoom = 1.2;
                            }
                            12 => {
                                let path="01. DRAWERS/01. S Families/S3/Gridfinity Baseplate/S3 Baseplate_With 1 Handle.stl".to_string();
                                Catalog::set_accessory(&mut self.bom, path, true, 1);
                                self.assembly(ctx);
                            }
                            13 => {
                                self.explosion = 1.;
                                self.camera.fit(&self.scene, 1.);
                            }
                            14 => {
                                self.selection.hidden_key = true;
                                self.bom = self.catalog.reconfigure(&self.selection, &self.bom);
                                self.assembly(ctx);
                            }
                            15 => {
                                self.explosion = 1.;
                                self.camera.fit(&self.scene, 1.);
                            }
                            16 => {
                                self.divider_layout=crate::dividers::Layout {
                                    enabled:true,specification:crate::dividers::spec(&self.selection),
                                    items:[4,8].into_iter().map(|y|crate::dividers::Placement {
                                        path:"01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/10-S, x3.stl".into(),start:[1,y],vertical:false,
                                    }).collect(),
                                };
                                self.assembly(ctx);
                            }
                            17 => {
                                self.divider_editor.open = false;
                            }
                            18 => {
                                self.explosion = 1.;
                                self.camera.fit(&self.scene, 1.);
                            }
                            19 => {
                                self.export_open = true;
                                self.export_options.format = crate::export::Format::ThreeMf;
                            }
                            20 => {
                                self.export_options.format = crate::export::Format::ThreeMf;
                            }
                            21 => {
                                self.export_open = false;
                                self.label = crate::label::Config {
                                    enabled: true,
                                    text: "收纳 A8".into(),
                                    ..Default::default()
                                };
                                self.single(crate::label::PATH.into(), ctx);
                            }
                            27 => {
                                self.label_open = false;
                                self.divider_layout.remove(&mut self.bom, &self.selection);
                                self.select_accessory("01. DRAWERS/01. S Families/S3/Gridfinity Baseplate/S3 Baseplate.stl".into(),true,1);
                                self.gridfinity.bins = vec![
                                    crate::gridfinity::Bin {
                                        scoop: true,
                                        label: crate::label::Config {
                                            enabled: true,
                                            text: "收纳\nA8".into(),
                                            size: 4.,
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                    crate::gridfinity::Bin {
                                        id: 2,
                                        cell: [1, 1],
                                        span: [2, 1],
                                        ..Default::default()
                                    },
                                ];
                                self.gridfinity_selected = Some(1);
                                self.assembly(ctx);
                            }
                            28 => {
                                self.gridfinity_open = false;
                                self.single(self.gridfinity.bins[0].path(), ctx);
                            }
                            29 => {
                                self.assembly(ctx);
                            }
                            33 => {
                                for part in &mut self.scene.parts {
                                    part.visible = true;
                                }
                                self.camera.fit(&self.scene, 0.);
                                self.camera.yaw = 0.6;
                                self.camera.pitch = 0.5;
                                self.selected = None;
                                self.export_options.plate.preview_colors = true;
                                self.export_options.plate.palette[0] = [190, 190, 190];
                                self.export_options.plate.palette[1] = [255, 120, 20];
                                for item in &self.bom {
                                    if item.path.contains("Facade") {
                                        self.export_options
                                            .plate
                                            .part_filaments
                                            .insert(item.path.clone(), 2);
                                    }
                                }
                                self.colors_open = true;
                            }
                            38 => {
                                self.zh = false;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                                    crate::app_name(false).into(),
                                ));
                            }
                            36 | 37 => {
                                self.export_open = false;
                                self.label.enabled = true;
                                self.label.text = "收纳\nA8".into();
                                self.label.style = if self.smoke_stage == 36 {
                                    crate::label::Style::Raised
                                } else {
                                    crate::label::Style::Engraved
                                };
                                self.export_options.plate.preview_colors = true;
                                self.export_options
                                    .plate
                                    .part_filaments
                                    .insert(crate::label::PATH.into(), 3);
                                self.export_options
                                    .plate
                                    .text_filaments
                                    .insert(crate::label::PATH.into(), 2);
                                self.single(crate::label::PATH.into(), ctx);
                            }
                            35 => {
                                self.export_open = true;
                                self.export_options.format = crate::export::Format::ThreeMf;
                                self.export_options.plate.center = true;
                            }
                            34 => {
                                self.colors_open = false;
                                self.selected = self
                                    .scene
                                    .parts
                                    .iter()
                                    .position(|p| p.pose.path.ends_with("/Facade Cap.stl"));
                            }
                            31 | 32 => {
                                self.gridfinity = Default::default();
                                self.gridfinity_open = false;
                                self.divider_layout = Default::default();
                                self.selection.width = "M".into();
                                self.selection.depth = 4;
                                self.selection.height =
                                    if self.smoke_stage == 31 { "1F" } else { "2F" }.into();
                                self.bom = self.catalog.bom(&self.selection);
                                self.assembly(ctx);
                            }
                            30 => {
                                self.selection.height = "2F".into();
                                self.bom = self.catalog.reconfigure(&self.selection, &self.bom);
                                self.assembly(ctx);
                            }
                            24 => {
                                self.label.text = "收纳\nA8".into();
                                self.label.style = crate::label::Style::Raised;
                                self.single(crate::label::PATH.into(), ctx);
                            }
                            22 | 25 => {
                                self.label.style = crate::label::Style::Engraved;
                                self.single(crate::label::PATH.into(), ctx);
                            }
                            23 | 26 => {
                                self.label_open = false;
                                self.assembly(ctx);
                            }
                            _ => {}
                        }
                    }
                }
            }
            if self.smoke_start.elapsed() > Duration::from_secs(300) {
                let _ = std::fs::create_dir_all(self.home.join("output"));
                let _ = std::fs::write(self.home.join("output/smoke-error.txt"), &self.status);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
fn ui_height(ctx: &egui::Context) -> f32 {
    ctx.content_rect().height()
}
fn open_file(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn group_label(zh: bool, group: &str) -> &str {
    if !zh {
        return group;
    }
    match group {
        "Structure" => "外壳",
        "Drawer" => "抽屉盒",
        "Facade" => "面板",
        "Optional rails" | "Geared rails" => "齿条（必备）",
        "Runners" => "导轨",
        "Mechanism" => "机构",
        "Key" => "钥匙",
        "Roof" => "顶盖",
        "Base" => "底座",
        "Optional baseplate" => "底板（可选）",
        "Optional accessory" => "选装配件",
        "Divider layout" => "分隔布局",
        _ => group,
    }
}

#[cfg(test)]
mod accessory_export_tests {
    use super::*;
    #[test]
    fn library_settings_require_user_choice_and_remember_last_directory() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("config")).unwrap();
        std::fs::create_dir(home.path().join("state")).unwrap();
        let library = tempfile::tempdir().unwrap();
        let saved = Settings {
            model_root: library.path().to_string_lossy().into(),
            language: "en".into(),
            export: Default::default(),
        };
        let data = toml::to_string(&saved).unwrap();
        std::fs::write(home.path().join("config/default.toml"), &data).unwrap();
        let first = Settings::load(home.path()).unwrap();
        assert!(first.model_root.is_empty());
        assert!(first.startup_root().is_none());
        std::fs::write(home.path().join("state/settings.toml"), &data).unwrap();
        let restored = Settings::load(home.path()).unwrap();
        assert_eq!(restored.startup_root(), Some(library.path().to_path_buf()));
        drop(library);
        let missing = Settings::load(home.path()).unwrap();
        assert!(missing.startup_root().is_none());
        assert_eq!(missing.model_root, saved.model_root);
        let empty: Settings = toml::from_str("language = 'zh'").unwrap();
        assert!(empty.startup_root().is_none());
        std::fs::write(home.path().join("state/settings.toml"), "broken toml").unwrap();
        assert!(
            Settings::load(home.path())
                .unwrap()
                .startup_root()
                .is_none()
        );
    }
    #[test]
    fn preset_keeps_copies_source_paths_and_assembly_type() {
        let item = Bom {
            path: "01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/10-S, x3.stl".into(),
            qty: 4,
            group: "Optional accessory".into(),
            enabled: true,
        };
        let mut color_config = crate::print_plate::Config::default();
        color_config.part_filaments.insert(item.path.clone(), 2);
        color_config.palette[1] = [20, 120, 220];
        let preset = Preset {
            print_config: Some(color_config),
            gridfinity: Default::default(),
            label: Default::default(),
            assembly_kind: AssemblyKind::Drawer,
            selection: Selection::default(),
            bom: vec![item.clone()],
            divider_layout: Default::default(),
        };
        let restored: Preset = toml::from_str(&toml::to_string(&preset).unwrap()).unwrap();
        assert_eq!(restored.print_config.as_ref().unwrap().slot(&item.path), 2);
        assert_eq!(
            restored.print_config.as_ref().unwrap().palette[1],
            [20, 120, 220]
        );
        let mut legacy = toml::Value::try_from(&preset).unwrap();
        legacy.as_table_mut().unwrap().remove("print_config");
        legacy.as_table_mut().unwrap().remove("divider_layout");
        legacy.as_table_mut().unwrap().remove("assembly_kind");
        legacy.as_table_mut().unwrap().remove("label");
        let legacy: Preset = legacy.try_into().unwrap();
        assert!(legacy.print_config.is_none());
        assert!(!legacy.divider_layout.enabled);
        assert!(!legacy.label.enabled);
        assert_eq!(legacy.assembly_kind, AssemblyKind::Drawer);
        assert_eq!(restored.assembly_kind, AssemblyKind::Drawer);
        let mut box_preset = toml::Value::try_from(&preset).unwrap();
        box_preset["assembly_kind"] = toml::Value::String("sustainer".into());
        let box_preset: Preset = box_preset.try_into().unwrap();
        assert!(!box_preset.assembly_kind.supported());
        let mut with_layout = preset;
        with_layout.label = crate::label::Config {
            enabled: true,
            text: "标签".into(),
            style: crate::label::Style::Engraved,
            ..Default::default()
        };
        with_layout.divider_layout = crate::dividers::Layout {
            enabled: true,
            specification: "S3-1F".into(),
            items: vec![crate::dividers::Placement {
                path: item.path.clone(),
                start: [1, 5],
                vertical: false,
            }],
        };
        let roundtrip: Preset = toml::from_str(&toml::to_string(&with_layout).unwrap()).unwrap();
        assert_eq!(roundtrip.divider_layout, with_layout.divider_layout);
        assert_eq!(roundtrip.label, with_layout.label);
        assert_eq!(restored.bom[0].path, item.path);
        assert_eq!(restored.bom[0].qty, 4);
        assert!(restored.bom[0].enabled);
    }
}
