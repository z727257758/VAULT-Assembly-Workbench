mod animation;
mod appearance;
mod bin_editor_smoke;
mod bin_view;
mod bin_workspace;
mod combination;
mod divider_editor_smoke;
mod divider_view;
mod divider_workspace;
mod drawer_divider_smoke;
mod drawer_divider_workspace;
mod exterior_smoke;
mod inline_edit;
mod large_stl_smoke;
mod layer_reorder;
mod motion;
mod orientation;
mod part_details;
mod preview_modes;
mod print_list;
mod recipes;
mod settings_panel;
pub(crate) use recipes::audit as audit_presets;
mod stopper_smoke;
mod tower;
mod viewport_menu;
mod workflow;
mod workspace;
use crate::ui_style;
use crate::{
    catalog::{Bom, Catalog, Selection, SustainerBody, SustainerOptions},
    constraints::AssemblyKind,
    render::{Camera, Renderer, ViewOptions},
    scene::{Definition, Scene},
};
use combination::{CombinationPreset, Member};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

fn t(zh: bool, cn: &'static str, en: &'static str) -> &'static str {
    if zh { cn } else { en }
}
impl AssemblyKind {
    fn configured_bom(
        self,
        catalog: &Catalog,
        selection: &Selection,
        options: SustainerOptions,
        previous: &[Bom],
    ) -> Result<Vec<Bom>, String> {
        match self {
            Self::Drawer => Ok(catalog.reconfigure(selection, previous)),
            Self::Sustainer => catalog.reconfigure_sustainer(selection, options, previous),
        }
    }

    fn label_path(self, selection: &Selection) -> &'static str {
        match self {
            Self::Drawer => crate::label::PATH,
            Self::Sustainer => crate::label::sustainer_path(selection),
        }
    }
    fn label(self, zh: bool) -> &'static str {
        match self {
            Self::Drawer => t(zh, "抽屉", "Drawer"),
            Self::Sustainer => t(zh, "盒子（Sustainer）", "Box (Sustainer)"),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Preset {
    #[serde(default)]
    drawers: Vec<Preset>,
    #[serde(default)]
    sustainer_dividers: crate::sustainer_dividers::Layout,
    #[serde(default)]
    print_config: Option<crate::print_plate::Config>,
    #[serde(default)]
    gridfinity: crate::gridfinity::Layout,
    #[serde(default)]
    label: crate::label::Config,
    #[serde(default)]
    assembly_kind: AssemblyKind,
    #[serde(default)]
    sustainer_options: SustainerOptions,
    #[serde(default)]
    sustainer_angle: f32,
    #[serde(default)]
    drawer_extension: f32,
    selection: Selection,
    bom: Vec<Bom>,
    #[serde(default)]
    divider_layout: crate::dividers::Layout,
}
#[derive(Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    material_library: crate::print_plate::MaterialLibrary,
    #[serde(default, deserialize_with = "crate::print_plate::ColorTheme::load")]
    global_colors: crate::print_plate::ColorTheme,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    color_presets: std::collections::BTreeMap<String, crate::print_plate::ColorTheme>,
    #[serde(default)]
    appearance: appearance::Appearance,
    #[serde(default)]
    export: crate::export::Options,
    #[serde(default)]
    model_root: String,
    language: String,
    #[serde(default)]
    label_font: Option<crate::label::Font>,
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
    Preset(Box<Result<CombinationPreset, String>>),
    Catalog(Result<Catalog, String>),
    Scene {
        revision: u64,
        finished: bool,
        key: String,
        result: Result<Box<Scene>, String>,
    },
    Image(Result<(String, egui::ColorImage), String>),
    Collision(Result<crate::collision::Report, String>),
}
#[derive(Clone, Serialize)]
struct SceneRequest {
    members: Vec<Member>,
    combination_gap: f32,
    sustainer_options: Option<SustainerOptions>,
    sustainer_dividers: crate::sustainer_dividers::Layout,
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
    animation: animation::State,
    inline_edit: inline_edit::State,
    appearance: appearance::Appearance,
    label_font: Option<crate::label::Font>,
    settings_page: Option<settings_panel::Page>,
    project_label_style: Option<crate::label::StyleSettings>,
    material_import: settings_panel::MaterialImport,
    tower: tower::State,
    active_drawer: Option<usize>,
    preview_modes: preview_modes::State,
    part_details: part_details::State,
    members: Vec<Member>,
    active_member: usize,
    combination_view: bool,
    combination_gap: f32,
    combination_move_source: Option<usize>,
    combination_spec: Option<combination::SpecEdit>,
    exterior_edit: Option<combination::exterior::Edit>,
    combination_view_state: Option<(Camera, f32, bool, bool)>,
    workflow_history: workflow::History,
    member_edit_baseline: Option<workflow::Snapshot>,
    global_colors: crate::print_plate::ColorTheme,
    color_presets: std::collections::BTreeMap<String, crate::print_plate::ColorTheme>,
    material_library: crate::print_plate::MaterialLibrary,
    member_selection: std::collections::BTreeSet<usize>,
    sustainer_dividers: crate::sustainer_dividers::Layout,
    sustainer_divider_editor: crate::sustainer_divider_editor::Editor,
    divider_model: divider_view::State,
    sustainer_layout_cache: BTreeMap<String, crate::sustainer_dividers::Layout>,
    assembly_kind: AssemblyKind,
    sustainer_options: SustainerOptions,
    sustainer_angle: f32,
    drawer_extension: f32,
    gridfinity: crate::gridfinity::Layout,
    gridfinity_open: bool,
    interior_exterior_opacity: u32,
    bin_view: bin_view::State,
    bin_editor_smoke: bin_editor_smoke::State,
    editor_smoke_pointer: bin_editor_smoke::Pointer,
    divider_editor_smoke: divider_editor_smoke::State,
    drawer_divider_smoke: drawer_divider_smoke::State,
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
    drawer_divider_view: drawer_divider_workspace::State,
    layout_cache: BTreeMap<String, crate::dividers::Layout>,
    scene: Scene,
    renderer: Renderer,
    camera: Camera,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
    busy: bool,
    preview: crate::preview::Queue<SceneRequest>,
    failed_preview: Option<SceneRequest>,
    pending_layer_reorder: Option<layer_reorder::Pending>,
    layer_reorder: Option<layer_reorder::Motion>,
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
    export_scope: workflow::ExportScope,
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
        let settings = Settings::load(&home);
        let appearance = settings.as_ref().map(|s| s.appearance).unwrap_or_default();
        appearance.apply(&cc.egui_ctx);
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
            animation: Default::default(),
            inline_edit: Default::default(),
            appearance,
            label_font: settings.as_ref().and_then(|s| s.label_font),
            settings_page: None,
            project_label_style: (!smoke).then_some(crate::label::StyleSettings {
                font: settings
                    .as_ref()
                    .and_then(|s| s.label_font)
                    .unwrap_or_default(),
                ..Default::default()
            }),
            material_import: Default::default(),
            tower: Default::default(),
            active_drawer: None,
            preview_modes: Default::default(),
            part_details: Default::default(),
            members: Vec::new(),
            active_member: 0,
            combination_view: !smoke,
            combination_gap: 0.,
            combination_move_source: None,
            combination_spec: None,
            exterior_edit: None,
            combination_view_state: None,
            workflow_history: Default::default(),
            member_edit_baseline: None,
            global_colors: settings
                .as_ref()
                .map(|s| s.global_colors.clone())
                .unwrap_or_default(),
            color_presets: settings
                .as_ref()
                .map(|s| s.color_presets.clone())
                .unwrap_or_default(),
            material_library: settings
                .as_ref()
                .map(|s| s.material_library.clone())
                .unwrap_or_default(),
            member_selection: Default::default(),
            assembly_kind: if smoke && std::env::args().any(|a| a == "--smoke-sustainer") {
                AssemblyKind::Sustainer
            } else {
                AssemblyKind::Drawer
            },
            sustainer_options: Default::default(),
            sustainer_angle: 0.,
            drawer_extension: 0.,
            gridfinity: Default::default(),
            gridfinity_open: false,
            interior_exterior_opacity: 30,
            bin_view: Default::default(),
            bin_editor_smoke: Default::default(),
            editor_smoke_pointer: Default::default(),
            divider_editor_smoke: Default::default(),
            drawer_divider_smoke: Default::default(),
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
            sustainer_dividers: Default::default(),
            sustainer_divider_editor: Default::default(),
            divider_model: Default::default(),
            sustainer_layout_cache: Default::default(),
            drawer_divider_view: Default::default(),
            layout_cache: Default::default(),
            scene: Scene::default(),
            renderer,
            camera: Camera::default(),
            tx,
            rx,
            busy: false,
            preview: Default::default(),
            failed_preview: None,
            pending_layer_reorder: None,
            layer_reorder: None,
            assembly_view: None,
            status: String::new(),
            zh,
            tab: 0,
            query: String::new(),
            category: 0,
            explosion: if smoke && std::env::args().any(|a| a == "--smoke-sustainer") {
                0.
            } else {
                1.
            },
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
            export_scope: workflow::ExportScope::All,
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
            smoke_stage: if smoke && std::env::args().any(|a| a == "--smoke-sustainer") {
                40
            } else {
                0
            },
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
        if smoke && std::env::args().any(|a| a == "--smoke-stopper") {
            app.selection.depth = 7;
        }
        if smoke && app.assembly_kind == AssemblyKind::Sustainer {
            app.export_options.plate.preview_colors = false;
        }
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
        app.tower_load(&cc.egui_ctx);
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
    fn layer_reorder_offsets(
        &self,
        ctx: &egui::Context,
        gap: f32,
        explosion: f32,
    ) -> Vec<glam::Vec3> {
        let offsets = self.layer_reorder.as_ref().map_or_else(Vec::new, |motion| {
            motion.offsets(ctx.input(|input| input.time), gap, explosion)
        });
        if !offsets.is_empty() {
            ctx.request_repaint();
        }
        offsets
    }
    fn start_scene(
        &mut self,
        items: Vec<Bom>,
        root: PathBuf,
        title: String,
        assembly: bool,
        ctx: &egui::Context,
    ) {
        self.animation.tracks.clear();
        if assembly && self.combination_view {
            self.commit_member();
        }
        let key = if assembly && self.combination_view {
            "combination".into()
        } else if assembly {
            if self.assembly_kind == AssemblyKind::Sustainer {
                self.sustainer_options.key(&self.selection)
            } else {
                format!(
                    "{}{}-{}",
                    self.selection.width, self.selection.depth, self.selection.height
                )
            }
        } else {
            String::new()
        };
        self.pending_layer_reorder = None;
        self.preview.schedule(
            SceneRequest {
                members: if assembly && self.combination_view {
                    self.members
                        .iter()
                        .cloned()
                        .map(|mut member| {
                            member
                                .preset
                                .apply_label_settings(self.project_label_style, self.label_font);
                            member
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                combination_gap: self.combination_gap,
                sustainer_options: (assembly && self.assembly_kind == AssemblyKind::Sustainer)
                    .then_some(self.sustainer_options),
                sustainer_dividers: if assembly && self.assembly_kind == AssemblyKind::Sustainer {
                    self.sustainer_dividers.clone()
                } else {
                    Default::default()
                },
                gridfinity: if self.assembly_kind == AssemblyKind::Drawer {
                    self.gridfinity_for_project()
                } else {
                    Default::default()
                },
                home: self.home.clone(),
                label: self.label_for_project(),
                items,
                root,
                title,
                assembly,
                key,
                divider_layout: if assembly && self.assembly_kind == AssemblyKind::Drawer {
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
            let result = if !request.members.is_empty() {
                combination::load(
                    &request.root,
                    &request.home,
                    &request.members,
                    request.combination_gap,
                    &cancel,
                )
            } else {
                crate::display::Rules::load(&display_rules).and_then(|rules| {
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
                        if let Some(options) = request.sustainer_options {
                            let catalog = Catalog {
                                root: request.root.clone(),
                                entries: request
                                    .items
                                    .iter()
                                    .map(|b| crate::catalog::Entry {
                                        path: b.path.clone(),
                                        name: b.name().to_string(),
                                        category: 2,
                                    })
                                    .collect(),
                            };
                            crate::sustainer_dividers::apply(
                                &mut scene,
                                &request.sustainer_dividers,
                                &catalog,
                                &request.selection,
                                options,
                            )?;
                        } else if request.assembly {
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
                        load_scene_definition(&mut draft, &request.home, &request.key)?;
                        draft.title += "（简化预览；正在生成精确模型）";
                        let _ = tx.send(Loaded::Scene {
                            revision,
                            finished: false,
                            key: request.key.clone(),
                            result: Ok(Box::new(draft)),
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
                    load_scene_definition(&mut scene, &request.home, &request.key)?;
                    Ok(scene)
                })
            };
            let _ = tx.send(Loaded::Scene {
                revision,
                finished: true,
                key: request.key,
                result: result.map(Box::new),
            });
            ctx.request_repaint();
        });
    }
    fn assembly(&mut self, ctx: &egui::Context) {
        if self.assembly_kind == AssemblyKind::Sustainer {
            let spec = crate::sustainer_dividers::spec(&self.selection, self.sustainer_options);
            if self.sustainer_dividers.specification != spec {
                if !self.sustainer_dividers.specification.is_empty() {
                    self.sustainer_layout_cache.insert(
                        self.sustainer_dividers.specification.clone(),
                        self.sustainer_dividers.clone(),
                    );
                }
                self.sustainer_dividers =
                    self.sustainer_layout_cache.get(&spec).cloned().unwrap_or(
                        crate::sustainer_dividers::Layout {
                            specification: spec,
                            ..Default::default()
                        },
                    );
                self.sustainer_divider_editor.reset();
            }
            let mut items = match self
                .catalog
                .sustainer_bom(&self.selection, self.sustainer_options)
            {
                Ok(items) => items,
                Err(e) => {
                    self.status = e;
                    return;
                }
            };
            if let Err(e) = self.sustainer_dividers.sync_bom(
                &mut items,
                &self.catalog,
                &self.selection,
                self.sustainer_options,
            ) {
                self.status = e;
                return;
            }
            if let Err(e) = self.sustainer_dividers.sync_bom(
                &mut self.bom,
                &self.catalog,
                &self.selection,
                self.sustainer_options,
            ) {
                self.status = e;
                return;
            }
            self.start_scene(
                items,
                self.catalog.root.clone(),
                format!(
                    "Sustainer {}{} / {}",
                    self.selection.width, self.selection.depth, self.selection.height
                ),
                true,
                ctx,
            );
            return;
        }
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
            if self.gridfinity_open {
                self.close_bin_editor();
            }
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
        if self.inline_edit.open {
            self.focus_preview_path(path, ctx);
            return;
        }
        if self.combination_view {
            self.remember_combination_view();
        }
        self.combination_view = false;
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
            material_library: self.material_library.clone(),
            global_colors: self.global_colors.clone(),
            color_presets: self.color_presets.clone(),
            appearance: self.appearance,
            export: self.export_options.clone(),
            model_root: self.last_model_root.to_string_lossy().into(),
            language: if self.zh { "zh" } else { "en" }.into(),
            label_font: self.label_font,
        };
        let text = toml::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        let folder = self.home.join("state");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(&folder).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut file, text.as_bytes()).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(folder.join("settings.toml"))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn label_for_project(&self) -> crate::label::Config {
        let mut label = self.label.clone();
        if let Some(style) = self.project_label_style {
            style.apply(&mut label);
        } else if let Some(font) = self.label_font {
            label.font = font;
        }
        label.for_module(&self.selection)
    }

    fn validate_label_edit(&mut self) -> Result<(), String> {
        let label = self.label_for_project();
        label.validate()?;
        if let Some(style) = self.project_label_style {
            style.apply(&mut self.label);
        }
        Ok(())
    }

    fn gridfinity_for_project(&self) -> crate::gridfinity::Layout {
        let mut layout = self.gridfinity.clone();
        if let Some(style) = self.project_label_style {
            layout.apply_label_style(style);
        } else {
            layout.apply_label_font(self.label_font);
        }
        layout
    }
    fn write_state(&self, name: &str, text: String) -> Result<(), String> {
        let dir = self.home.join("state");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(name), text).map_err(|e| e.to_string())
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.rx.try_recv() {
            if !matches!(&result, Loaded::Preset(_)) {
                self.busy = matches!(
                    &result,
                    Loaded::Scene {
                        finished: false,
                        ..
                    }
                );
            }
            match result {
                Loaded::Preset(result) => self.apply_loaded_project(*result, ctx),
                Loaded::Catalog(Ok(c)) => {
                    self.close_member_editors();
                    self.preview_modes = Default::default();
                    self.last_model_root = c.root.clone();
                    self.catalog = c;
                    self.members.clear();
                    self.active_drawer = None;
                    self.combination_move_source = None;
                    self.workflow_history.clear();
                    self.member_edit_baseline = None;
                    self.member_selection.clear();
                    self.combination_view_state = None;
                    self.active_member = 0;
                    self.combination_view = !self.smoke;
                    self.divider_layout = Default::default();
                    self.sustainer_dividers = Default::default();
                    self.sustainer_layout_cache.clear();
                    self.sustainer_divider_editor = Default::default();
                    self.label = Default::default();
                    self.gridfinity = Default::default();
                    self.layout_cache.clear();
                    self.divider_editor.reset();
                    self.assembly_view = None;
                    self.scene_key.clear();
                    self.bom = match self.configured_bom(&[]) {
                        Ok(b) => b,
                        Err(e) => {
                            self.status = e;
                            continue;
                        }
                    };
                    if self.smoke && std::env::args().any(|a| a == "--smoke-large-stl") {
                        let selection = Selection {
                            width: "XL".into(),
                            ..Default::default()
                        };
                        let items = self
                            .catalog
                            .bom(&selection)
                            .into_iter()
                            .filter(|b| b.path.contains("/Modules/"))
                            .collect();
                        self.tab = 1;
                        self.start_scene(
                            items,
                            self.catalog.root.clone(),
                            "XL3 · 原始壳体 / source housing".into(),
                            false,
                            ctx,
                        );
                        continue;
                    }
                    if self.smoke && std::env::args().any(|a| a == "--smoke-presets") {
                        self.setup_recipe_smoke(ctx);
                    }
                    if self.smoke && std::env::args().any(|a| a == "--smoke-top") {
                        self.setup_roof_smoke(ctx);
                    }
                    if self.smoke
                        && std::env::args().any(|a| {
                            matches!(
                                a.as_str(),
                                "--smoke-tower"
                                    | "--smoke-workspace"
                                    | "--smoke-build-panel"
                                    | "--smoke-color-groups"
                                    | "--smoke-project-menu"
                                    | "--smoke-viewport-menu"
                            )
                        })
                    {
                        self.setup_tower_smoke(ctx);
                    }
                    if self.smoke && std::env::args().any(|a| a == "--smoke-motion") {
                        self.setup_motion_smoke(ctx);
                    }
                    if self.smoke && std::env::args().any(|a| a == "--smoke-bin-editor") {
                        self.setup_bin_editor_smoke(ctx);
                    }
                    if self.smoke && std::env::args().any(|a| a == "--smoke-divider-editor") {
                        self.setup_divider_editor_smoke(ctx);
                    }
                    if self.smoke && std::env::args().any(|a| a == "--smoke-drawer-divider-editor")
                    {
                        self.setup_drawer_divider_smoke(ctx);
                    }
                    if self.smoke && std::env::args().any(|a| a == "--smoke-exterior") {
                        self.setup_exterior_smoke();
                    }
                    if self.smoke
                        && std::env::args()
                            .any(|a| a == "--smoke-combination" || a == "--smoke-preview-modes")
                    {
                        let plates = std::env::args().any(|a| a == "--smoke-preview-modes");
                        if plates {
                            self.export_options.plate = Default::default();
                        }
                        self.setup_combination_smoke(plates);
                    }
                    self.status = format!(
                        "{} STL · {}",
                        self.catalog.stl_count(),
                        self.catalog.root.display()
                    );
                    if !self.smoke
                        && let Err(e) = self.save_settings()
                    {
                        self.status = e;
                    }
                    if !self.smoke {
                        self.explosion = 0.;
                        self.global_colors = self.material_library.default_theme();
                        self.global_colors.apply(&mut self.export_options.plate);
                        self.export_options.plate.follow_global_colors = true;
                        self.tower_catalog_ready();
                    }
                    if self.smoke
                        && self.catalog.stl_count() > 0
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
                    let reorder = self.pending_layer_reorder.take();
                    let reorder = reorder
                        .filter(|pending| pending.revision == revision && key == "combination");
                    let mut s = match result {
                        Ok(scene) => *scene,
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
                    if preserve {
                        let previous = previous.unwrap();
                        if self.scene_key != "combination" {
                            s.retain_assembly_state(&previous.scene);
                        } else {
                            combination::retain_visibility(
                                &mut s,
                                &previous.scene,
                                reorder.as_ref().map(|pending| pending.sources.as_slice()),
                            );
                        }
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
                            self.explosion = if s.assembly
                                && self.assembly_kind == AssemblyKind::Drawer
                                && self.scene_key != "combination"
                            {
                                1.
                            } else {
                                0.
                            };
                        }
                        if !s.assembly {
                            self.explosion = 0.;
                        }
                        self.camera = Camera::default();
                        self.camera.fit(&s, self.explosion);
                        if !s.assembly
                            && s.parts.iter().any(|p| {
                                crate::label::is_label(&p.pose.path)
                                    || p.pose.path.starts_with(crate::gridfinity::PREFIX)
                            })
                        {
                            self.camera.yaw = std::f32::consts::PI;
                            self.camera.pitch = 1.25;
                        }
                    }
                    // Keep the assembly snapshot only while inspecting a single part.
                    // The accepted assembly supplies the state for the next refresh.
                    if s.assembly {
                        self.assembly_view = None;
                    }
                    if self.scene_key == "combination"
                        && !preserve
                        && let Some((camera, explosion, dimensions, ghost)) =
                            &self.combination_view_state
                    {
                        self.camera = camera.clone();
                        self.explosion = *explosion;
                        self.dimensions = *dimensions;
                        self.ghost = *ghost;
                    }
                    if s.assembly
                        && self.scene_key != "combination"
                        && crate::sustainer::lid_angle(&s).is_some()
                    {
                        let lid_moved = !preserve
                            || crate::sustainer::lid_angle(&self.scene)
                                .is_none_or(|angle| (angle - self.sustainer_angle).abs() > 0.001);
                        if let Err(e) =
                            crate::sustainer::set_lid_angle(&mut s, self.sustainer_angle)
                        {
                            self.status = e;
                        }
                        if lid_moved {
                            self.camera.fit(&s, self.explosion);
                        }
                    }
                    if s.assembly
                        && self.scene_key != "combination"
                        && self.assembly_kind == AssemblyKind::Drawer
                        && self.drawer_extension > 0.
                        && let Ok(m) = crate::constraints::AssemblyRules::mechanism(
                            &s,
                            crate::scene::Target {
                                member: 0,
                                drawer: None,
                            },
                        )
                        && let Err(error) = crate::constraints::AssemblyRules::set_slider(
                            &mut s,
                            &m.child,
                            m.min + self.drawer_extension * (m.max - m.min),
                        )
                    {
                        self.status = error;
                    }
                    if let Some(pending) = &reorder {
                        self.layer_reorder = layer_reorder::Motion::new(
                            pending,
                            &self.scene,
                            &s,
                            self.layer_reorder.as_ref(),
                            ctx.input(|i| i.time),
                        );
                    } else if self
                        .layer_reorder
                        .as_ref()
                        .is_some_and(|motion| motion.revision != revision)
                    {
                        self.layer_reorder = None;
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
                    if let Some(warning) = &self.scene.constraint_warning {
                        self.status = warning.clone();
                    }
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
        self.close_inline_editor(ctx);
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
            let previous=self.scene.relations.clone();let mut changed=false;let mut remove=None;let mut motion=None;
            for(index,r)in self.scene.relations.iter_mut().enumerate(){
                let child=self.scene.parts.iter().find(|p|crate::constraints::PartId::of(p)==r.child).map(|p|p.label(z)).unwrap_or_else(||r.child.path.clone());
                egui::CollapsingHeader::new(format!("{} · {}",index+1,child)).id_salt(("joint",index)).show(ui,|ui|{
                    ui.label(&r.label);ui.label(format!("{}: {}",t(z,"父件","Parent"),r.parent.path.rsplit('/').next().unwrap()));
                    match &mut r.joint {
                        crate::constraints::Joint::RunnerGuide{inner_travel,outer_travel,..}=>{
                            ui.label(format!("{} · {:.1} mm + {:.1} mm",t(z,"随抽屉两段伸出：内轨 / 中轨","Driven by drawer: inner / middle travel"),inner_travel,outer_travel));
                        }
                        crate::constraints::Joint::Hinge{angle,min,max,..}=>{let mut value=*angle;if ui.add(egui::Slider::new(&mut value,*min..=*max).text(t(z,"转角 °","Angle °"))).changed(){motion=Some((r.child.clone(),value));}},
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

                            let mut value=*offset;if ui.add(egui::Slider::new(&mut value,*min..=*max).text(t(z,"位移 mm","Travel mm"))).changed(){motion=Some((r.child.clone(),value));}
                            ui.horizontal(|ui|{for (label,value) in [(t(z,"下限","Min"),min),(t(z,"上限","Max"),max)]{ui.label(label);changed|=ui.add(egui::DragValue::new(value).speed(1.).range(-10000. ..=10000.)).changed();}});
                        }
                    }
                    if ui.add(ui_style::danger_button(t(z,"解除此关系（保留当前位置）","Remove relation, keep pose"))).clicked(){remove=Some(index);}
                });
            }
            if let Some(index)=remove{self.scene.relations.remove(index);changed=true;}
            if changed{if let Err(e)=crate::constraints::solve(&mut self.scene){self.scene.relations=previous;self.status=e;}else{self.scene.calibrated=false;}}
            if let Some((child,value))=motion && let Err(error)=self.apply_joint_motion(&child,value,ctx){self.status=error;}
            if ui.add(ui_style::primary_button(t(z,"保存装配位置与关系","Save poses and relations"))).clicked(){
                self.status=match toml::to_string_pretty(&self.scene.definition()).map_err(|e|e.to_string()).and_then(|s|self.write_state(&format!("assembly-{}.toml",self.scene_key),s)){Ok(_)=>t(z,"装配位置与关系已保存","Poses and relations saved").into(),Err(e)=>e};
            }
            ui.separator();ui.strong(t(z,"3. 合拢状态相交检查","3. Assembled intersection check"));
            ui.label(t(z,"检查实际三角面交叉，包含隐藏零件。共面接触和完整包覆不计入；无结果不代表已通过公差检查。","Tests actual triangle crossings, including hidden parts. Excludes coplanar contact and complete containment; not a tolerance certificate."));
            ui.horizontal(|ui|{ui.label(t(z,"数值容差 mm","Numerical tolerance mm"));ui.add(egui::DragValue::new(&mut self.collision_tolerance).speed(0.001).range(0.0001..=1.));if ui.add_enabled(!self.busy,egui::Button::new(t(z,"检查表面交叉","Check crossings"))).clicked(){self.check_intersections(ctx);}});
            if let Some(report)=&self.collision_report{
                if !report.is_current(&self.scene){ui.colored_label(ui.visuals().warn_fg_color,t(z,"装配已改变，结果过期，请重新检查。","Assembly changed; results are stale. Run again."));}
                else{ui.label(format!("{} {} · {:.4} mm",report.pairs.len(),t(z,"对交叉","crossing pairs"),report.tolerance));for pair in &report.pairs{
                    let label=format!("{} ↔ {}",self.scene.parts[pair.a].label(z),self.scene.parts[pair.b].label(z));
                    if ui.button(label).on_hover_text(format!("{:.2}, {:.2}, {:.2} mm",pair.point.x,pair.point.y,pair.point.z)).clicked(){for(i,p)in self.scene.parts.iter_mut().enumerate(){p.visible=i==pair.a||i==pair.b;}self.selected=Some(pair.a);self.explosion=0.;self.camera.fit(&self.scene,0.);}
                }}
            }
        });
        self.joints_open = open;
    }
    fn left(&mut self, ctx: &egui::Context) {
        let z = self.zh;
        egui::SidePanel::left("library")
            .default_width(400.)
            .width_range(360. ..=560.)
            .resizable(true)
            .frame(ui_style::panel_frame(
                &ctx.style(),
                egui::Margin::symmetric(20, 16),
            ))
            .show(ctx, |ui| {
                self.workspace_header(ui, ctx);
                if self.tab == 0 {
                    self.tower_panel(ui, ctx);
                    return;
                }
                self.tower_footer_panel(ui, ctx);
                egui::ScrollArea::vertical()
                    .id_salt(("configuration-panel-scroll", self.tab))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
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
                        let combinable: Vec<String> = self
                            .catalog
                            .bom(&self.selection)
                            .into_iter()
                            .map(|b| b.path)
                            .chain(
                                self.catalog
                                    .accessories(&self.selection)
                                    .into_iter()
                                    .map(|e| e.path),
                            )
                            .collect();
                        let mut folders: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
                        let mut count = 0;
                        for e in &self.catalog.entries {
                            let search = crate::accessories::info(&e.path, z).map_or_else(
                                || e.path.clone(),
                                |m| format!("{} {} {} {}", e.path, m.name, m.group, m.description),
                            );
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
                                                if name.to_lowercase().ends_with(".stl")
                                                    && combinable.contains(&path)
                                                    && ui.small_button("+").clicked()
                                                {
                                                    add = Some(path.clone());
                                                }
                                                let meta = crate::accessories::info(&path, z);
                                                let label = meta
                                                    .as_ref()
                                                    .map_or(name.as_str(), |m| m.name.as_str());
                                                let tooltip = meta.as_ref().map_or_else(
                                                    || path.clone(),
                                                    |m| format!("{}\n{}", m.description, path),
                                                );
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
                    });
            });
    }

    fn remove_dividers(&mut self, ctx: &egui::Context) {
        self.close_drawer_divider_editor();
        self.divider_layout.remove(&mut self.bom, &self.selection);
        self.layout_cache
            .remove(&crate::dividers::spec(&self.selection));
        self.divider_editor.reset();
        self.assembly(ctx);
    }
    fn configured_bom(&self, old: &[Bom]) -> Result<Vec<Bom>, String> {
        self.assembly_kind.configured_bom(
            &self.catalog,
            &self.selection,
            self.sustainer_options,
            old,
        )
    }
    fn select_accessory(&mut self, path: String, enabled: bool, qty: u32) -> bool {
        let drawer = self.assembly_kind == AssemblyKind::Drawer;
        let change_plate = drawer && enabled && path.contains("/Gridfinity Baseplate/");
        if let Err(error) = print_list::set_accessory(
            &self.selection,
            &mut self.bom,
            drawer.then_some((&mut self.divider_layout, &mut self.gridfinity)),
            path,
            enabled,
            qty,
        ) {
            self.status = error;
            return false;
        }
        if change_plate {
            self.close_drawer_divider_editor();
            self.layout_cache
                .remove(&crate::dividers::spec(&self.selection));
            self.divider_editor.reset();
        }
        true
    }
    fn add_to_bom(&mut self, path: String, ctx: &egui::Context) {
        if self.assembly_kind == AssemblyKind::Drawer
            && self.divider_layout.enabled
            && path.contains("/Grid Dividers/")
        {
            self.open_drawer_divider_editor(ctx);
            return;
        }
        let allowed = if self.assembly_kind == AssemblyKind::Sustainer {
            self.catalog
                .sustainer_bom(&self.selection, self.sustainer_options)
                .unwrap_or_default()
                .iter()
                .any(|b| b.path == path)
        } else {
            self.catalog
                .bom(&self.selection)
                .iter()
                .any(|b| b.path == path)
                || self
                    .catalog
                    .accessories(&self.selection)
                    .iter()
                    .any(|e| e.path == path)
        };
        if !allowed {
            self.status = t(
                self.zh,
                "此组件与当前装配方案不匹配，可单独预览。",
                "This component is outside the current assembly; preview it separately.",
            )
            .into();
            return;
        }
        let qty = self
            .bom
            .iter()
            .find(|b| b.path == path)
            .map_or(1, |b| b.qty);
        if !self.select_accessory(path, true, qty) {
            return;
        }
        if !self.members.is_empty() {
            self.combination_view = true;
        }
        self.assembly(ctx);
        self.status = t(
            self.zh,
            "已加入打印清单；请核对替代版本和所需数量",
            "Added to BOM; check alternatives and quantities",
        )
        .into();
    }
    fn selected_color_properties(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.commit_member();
        let Some(part) = self.selected.and_then(|i| self.scene.parts.get(i)) else {
            return;
        };
        let path = part.pose.path.clone();
        let linked = self
            .scene
            .parts
            .iter()
            .filter(|p| {
                (self.scene_key != "combination"
                    || p.pose.instance / crate::scene::MEMBER_STRIDE == self.active_member as u32)
                    && crate::print_plate::color_key(&p.pose.path)
                        == crate::print_plate::color_key(&path)
            })
            .count();
        let has_text = crate::label::is_label(&path)
            || self.gridfinity.bins.iter().any(|b| {
                b.tab
                    && ((!b.detachable() && b.path() == path)
                        || (b.detachable() && b.label_path() == path))
            });
        let z = self.zh;
        let mut changed = false;
        ui.strong(t(z, "打印颜色", "Print color"));
        ui.add_enabled_ui(self.export_rx.is_none(), |ui| {
            let c = &mut self.export_options.plate;
            let old = c.part_assignment(&path);
            let mut choice = old;
            color_slot_choices(ui, c, &mut choice, t(z, "跟随默认", "Use default"));
            if choice != old {
                c.set_part_slot(&path,choice);
                c.preview_colors = true;
                changed = true;
            }
            if has_text {
                ui.label(t(z, "文字颜色", "Text color"));
                let old_text = c.text_assignment(&path);
                let mut text_choice = old_text;
                color_slot_choices(ui, c, &mut text_choice, t(z, "跟随文字默认色", "Default text color"));
                if text_choice != old_text {
                    c.set_text_slot(&path,text_choice);
                    c.preview_colors = true;
                    changed = true;
                }
                ui.small(t(
                    z,
                    "文字与底板同盘多色打印；平面分色保持齐平，凹字保留凹陷。",
                    "Body and text print together; flat colors remain flush, engraved text retains its recess.",
                ));
            }
            ui.small(format!(
                "{} {} · {}",
                t(z, "当前颜色槽", "Current slot"),
                c.slot(&path),
                c.color_hex(c.slot(&path))
            ));
            if crate::print_plate::color_key(&path)!=path {
                ui.small(t(z,"同类型统一配色：小盒与可拆标牌分别同步，新增同类零件自动继承；文字颜色也按类型同步。","Colors are shared by type: bins and removable labels are separate groups, including future parts and text colors."));
                ui.small(format!("{} {linked}",t(z,"当前同步部件数","Linked display parts")));
            } else {
                ui.small(format!("{} {linked} {}",t(z,"同步修改同源的","Applies to"),t(z,"个展示部件；同一源文件的所有份数共用颜色。","display parts; all source copies share the color.")));
            }
            if ui
                .small_button(t(z, "编辑颜色槽…", "Edit palette…"))
                .clicked()
            {
                self.colors_open = true;
            }
        });
        ui.separator();
        if changed {
            if self.combination_view {
                self.push_workflow_snapshot(self.workflow_snapshot());
            }
            self.export_options.plate.follow_global_colors = false;
            if let Err(e) = self.save_settings() {
                self.status = e;
            }
            ctx.request_repaint();
        }
    }
    fn part_properties_window(&mut self, ctx: &egui::Context) {
        if !self.tower.show_parts || self.preview_modes.mode == preview_modes::Mode::Plates {
            return;
        }
        let z = self.zh;
        let bounds = ctx.content_rect();
        let mut open = true;
        egui::Window::new(t(z, "零件与属性", "Parts & properties"))
            .id(egui::Id::new("floating-part-properties"))
            .open(&mut open)
            .default_width(290.)
            .default_pos(bounds.right_top() + egui::vec2(-330., 102.))
            .max_height((bounds.height() - 150.).max(180.))
            .vscroll(true)
            .show(ctx, |ui| {
                if self.scene_key == "combination" {self.combination_properties(ui);return;}
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
                let mut expand = None;
                ui.horizontal(|ui| {
                    ui.strong(format!("{} ({})", t(z, "元件", "Parts"), self.scene.parts.len()));
                    if ui.small_button(t(z, "展开全部", "Expand all")).clicked() {
                        expand = Some(true);
                    }
                    if ui.small_button(t(z, "收起全部", "Collapse all")).clicked() {
                        expand = Some(false);
                    }
                });
                let selected_id = self.selected.and_then(|index| self.scene.parts.get(index)).map(crate::constraints::PartId::of);
                let selection_key = ui.make_persistent_id("part_tree_selection");
                let reveal = ui.data_mut(|data| {
                    let previous = data.get_temp::<Option<crate::constraints::PartId>>(selection_key).flatten();
                    data.insert_temp(selection_key, selected_id.clone());
                    selected_id.is_some() && previous != selected_id
                });
                let tree = part_tree(&self.scene, &self.bom, z);
                for group in &tree {
                    group.prepare(ctx, expand, self.selected, reveal);
                }
                egui::ScrollArea::vertical()
                    .id_salt("parts")
                    .max_height(230.)
                    .show(ui, |ui| {
                        for group in &tree {
                            group.show(ui, &mut self.scene, &mut self.selected, z, reveal);
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
                            ui.visuals().hyperlink_color,
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
                        .weak(),
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
                            if controlled {ui.colored_label(ui.visuals().hyperlink_color,t(z,"位置与角度由装配关系控制；可调整父件或关系参数。","Pose controlled by relation; edit parent or joint parameters."));}
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
                    if let Some(warning)=&self.scene.constraint_warning {ui.colored_label(ui.visuals().error_fg_color,warning);}
                    ui.separator();
                    ui.colored_label(
                        if self.scene.calibrated {
                            ui.visuals().hyperlink_color
                        } else {
                            ui.visuals().warn_fg_color
                        },
                        if self.scene.calibrated {
                            t(z, "装配坐标已人工确认", "Assembly manually verified")
                        } else if !self.scene.mating_checks.is_empty() {
                            t(z,"已按孔轴及安装面定位 · 待实物核对","Located from axes and seating faces · physical review pending")
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
                        .add(ui_style::primary_button(t(z, "保存本型号装配坐标", "Save assembly definition")))
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
                        .weak(),
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
        self.tower.show_parts = open;
    }
    fn viewport(&mut self, ctx: &egui::Context) {
        let z = self.zh;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.extreme_bg_color)
                    .inner_margin(0.),
            )
            .show(ctx, |ui| {
                appearance::paint_stage(ui, ui.max_rect());
                if self.divider_editor.open && self.assembly_kind == AssemblyKind::Drawer {
                    self.drawer_divider_workspace(ui);
                    return;
                }
                if self.gridfinity_open && self.assembly_kind == AssemblyKind::Drawer {
                    self.bin_workspace(ui);
                    return;
                }
                if self.sustainer_divider_editor.open
                    && self.assembly_kind == AssemblyKind::Sustainer
                {
                    self.sustainer_divider_workspace(ui);
                    return;
                }
                self.stage_controls(ctx, ui.max_rect());
                self.inline_editor_ui(ctx, ui.max_rect());
                if self.preview_modes_ui(ui, ctx) {
                    return;
                }
                if self.scene.parts.is_empty() {
                    self.workspace_empty(ui, ctx);
                    return;
                }
                if self.dimensions {
                    ui.add_space(110.);
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
                    let member_offsets = self.layer_reorder_offsets(ctx, 0., self.explosion);
                    let reordering =
                        self.pending_layer_reorder.is_some() || !member_offsets.is_empty();
                    let spatial_input = !reordering
                        && self.explosion == 0.
                        && !self.tower_regular()
                        && self.combination_viewport_actions(ctx, rect);
                    if !spatial_input && response.dragged_by(egui::PointerButton::Primary) {
                        let delta = ctx.input(|i| i.pointer.delta());
                        self.camera.yaw -= delta.x * 0.008;
                        self.camera.pitch =
                            (self.camera.pitch + delta.y * 0.008).clamp(-1.55, 1.55);
                    }
                    if !spatial_input
                        && (response.dragged_by(egui::PointerButton::Secondary)
                            || response.dragged_by(egui::PointerButton::Middle))
                    {
                        let d = ctx.input(|i| i.pointer.delta());
                        self.camera.pan[0] -= d.x / rect.width() * 2.;
                        self.camera.pan[1] += d.y / rect.height() * 2.;
                    }
                    if !spatial_input && response.hovered() {
                        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                        self.camera.zoom =
                            (self.camera.zoom * (scroll * 0.002).exp()).clamp(0.05, 50.);
                    }
                    if !spatial_input
                        && !reordering
                        && response.clicked()
                        && let Some(p) = response.interact_pointer_pos()
                    {
                        let (o, d) = self.camera.ray(p, rect);
                        self.last_hit = self.scene.pick(o, d, self.explosion);
                        self.part_details.matching = None;
                        self.selected = self.last_hit.map(|h| h.part);
                        if self.scene_key == "combination"
                            && let Some(index) = self.selected
                        {
                            self.select_combination_part(index, ctx);
                        }
                    }
                    if !spatial_input
                        && !reordering
                        && self.combination_move_source.is_none()
                        && response.double_clicked()
                        && self.scene_key == "combination"
                        && !self.busy
                        && !self.preview.has_pending()
                        && self.selected.is_some()
                    {
                        self.focus_member(ctx);
                    } else if !spatial_input && !reordering && response.double_clicked() {
                        self.camera.fit(&self.scene, self.explosion);
                    }
                    if !reordering && (!spatial_input || response.context_menu_opened()) {
                        self.viewport_context_menu(&response);
                    }
                    let dpi = ctx.pixels_per_point();
                    let id = self.renderer.render(
                        0,
                        [(size.x * dpi) as u32, (size.y * dpi) as u32],
                        &self.scene,
                        &self.camera,
                        ViewOptions {
                            explosion: self.explosion,
                            selected: self.selected,
                            ghost: self.ghost,
                            member_offsets,
                            print_beds: None,
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
                    if !reordering && self.scene_key == "combination" && self.explosion == 0. {
                        self.paint_active_member(ui, rect);
                        if self.viewport_shows_dimensions(ctx) {
                            self.paint_spatial_dimensions(ui, rect);
                        }
                    }
                    if !reordering
                        && (self.explosion != 0.
                            || self.export_options.plate.preview_colors
                                && self.scene_key != "combination")
                    {
                        self.paint_preview_selection(
                            ui,
                            rect,
                            &self.scene,
                            &self.camera,
                            self.explosion,
                        );
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
                        egui::TextStyle::Small.resolve(ui.style()),
                        ui.visuals().weak_text_color(),
                    );
                }
            });
    }
    fn save_preset(&mut self) {
        self.commit_member();
        let saved = self.project_snapshot(self.member_snapshot());
        self.save_project_file(&saved);
    }

    fn save_project_file(&mut self, saved: &CombinationPreset) {
        if let Err(e) = saved.validate(&self.catalog) {
            self.status = e;
            return;
        }
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
                .and_then(|_| toml::to_string_pretty(saved).map_err(|e| e.to_string()))
                .and_then(|s| std::fs::write(path, s).map_err(|e| e.to_string()));
            self.status = match result {
                Ok(_) => t(self.zh, "方案已保存", "Preset saved").into(),
                Err(e) => e,
            };
        }
    }
    fn load_preset(&mut self, ctx: &egui::Context) {
        self.tower_capture();
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
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = std::fs::read_to_string(path)
                    .map_err(|e| e.to_string())
                    .and_then(|s| {
                        toml::from_str::<CombinationPreset>(&s).map_err(|e| e.to_string())
                    });
                let _ = tx.send(Loaded::Preset(Box::new(result)));
                ctx.request_repaint();
            });
        }
    }
    fn apply_loaded_project(
        &mut self,
        result: Result<CombinationPreset, String>,
        ctx: &egui::Context,
    ) {
        let result = result.and_then(|mut saved| {
            saved.prepare_restore(&self.catalog, &self.export_options.plate)?;
            let active = saved
                .members
                .get_mut(saved.active_member)
                .map(|m| &mut m.preset)
                .unwrap_or(&mut saved.preset);
            active.sustainer_options = active.sustainer_options.normalized(&active.selection);
            active.sustainer_dividers.specification =
                crate::sustainer_dividers::spec(&active.selection, active.sustainer_options);
            active.divider_layout.specification = crate::dividers::spec(&active.selection);
            let bom = active.configured_bom(&self.catalog)?;
            let excluded = active
                .bom
                .iter()
                .filter(|b| b.enabled && !bom.iter().any(|item| item.path == b.path))
                .count();
            active.bom = bom;
            Ok((saved, excluded))
        });
        match result {
            Ok((saved, excluded)) => {
                self.tower_imported_project(saved.project_name.clone());
                self.restore_project(saved);
                self.assembly(ctx);
                self.status = format!(
                    "{} {excluded}",
                    t(
                        self.zh,
                        "方案已载入；已移除不匹配的文件：",
                        "Preset loaded. Incompatible files removed:"
                    )
                );
            }
            Err(e) => self.status = e,
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
    fn label_window(&mut self, ctx: &egui::Context) {
        if !self.label_open {
            return;
        }
        let z = self.zh;
        let mut open = true;
        let path = self.assembly_kind.label_path(&self.selection);
        let narrow_label = path == crate::label::SUSTAINER_PATH;
        if self.label.has_legacy_placeholder() {
            self.label.text.clear();
        }
        let before = self.label.clone();
        let default_text = crate::label::Config::specification_text(&self.selection);
        let mut lettering = self.label.for_module(&self.selection).active();
        let mut preview = false;
        egui::Window::new(t(z,"编辑标签文字","Edit label text")).id(egui::Id::new("label-editor"))
            .open(&mut open).resizable(false).default_width(440.).show(ctx,|ui|{
                ui.label(if narrow_label {t(z,"基于盒子原始标签，保留外形与安装结构。","Based on the original Sustainer label, preserving its shape and mounting features.")} else {t(z,"基于 Label.stl，保留原标签外形与安装结构。","Based on Label.stl, preserving its shape and mounting features.")});
                if ui.checkbox(&mut lettering,t(z,"启用标签文字","Enable lettering")).changed(){
                    self.label.enabled = lettering;
                    self.label.blank = !lettering;
                }
                ui.add_enabled_ui(lettering,|ui|{
                    if ui.add(egui::TextEdit::multiline(&mut self.label.text).hint_text(&default_text).desired_rows(2).char_limit(48)).changed(){
                        self.label.enabled = !self.label.text.trim().is_empty();
                    }
                    ui.small(if narrow_label {t(z,"最多两行；文字居中，安全区域 32 × 12 mm。","Up to two centered lines within a 32 × 12 mm safe area.")} else {t(z,"最多两行；文字居中，安全区域 38 × 12 mm。","Up to two centered lines within a 38 × 12 mm safe area.")});
                    crate::label::stroke_check_label_ui(ui, &self.label_for_project(), path);
                });
                ui.horizontal(|ui|{
                    if ui.button(t(z,"特写标签","Focus label")).clicked(){preview=true;}
                    if ui.button(t(z,"恢复空白标签","Restore blank label")).clicked(){
                        self.label.enabled = false;
                        self.label.text.clear();
                        self.label.blank = true;
                    }
                });
                if let Err(e)=self.label_for_project().validate(){ui.colored_label(ui.visuals().error_fg_color,e);}
                if self.failed_preview.is_some() && !self.busy && !self.preview.has_pending(){ui.colored_label(ui.visuals().error_fg_color,&self.status);}
            });
        self.label_open = open;
        if before != self.label {
            if let Err(e) = self.validate_label_edit() {
                self.label = before;
                self.status = e;
                return;
            }
            if self.label.active() {
                let qty = self
                    .bom
                    .iter()
                    .find(|b| b.path == path)
                    .map_or(1, |b| b.qty);
                Catalog::set_accessory(&mut self.bom, path.into(), true, qty);
            }
            if !self.scene.assembly && self.scene.parts.iter().any(|p| p.pose.path == path) {
                self.single(path.into(), ctx);
            } else {
                self.assembly(ctx);
            }
        }
        if preview {
            self.single(path.into(), ctx);
        }
    }
    fn color_window(&mut self, ctx: &egui::Context) {
        if !self.colors_open {
            return;
        }
        self.commit_member();
        let mut open = true;
        let z = self.zh;
        let mut changed = false;
        egui::Window::new(t(z,"当前成员配色","Current member colors")).id(egui::Id::new("print-colors"))
            .open(&mut open).default_width(570.).default_height(650.).vscroll(true).show(ctx,|ui| {
            ui.add_enabled_ui(self.export_rx.is_none(),|ui| {
                let c=&mut self.export_options.plate;
                egui::Grid::new("color-palette").num_columns(3).show(ui,|ui| {
                    for slot in 1..=c.slot_count() {
                        let mut appearance = c.appearance(slot);
                         workflow::material_picker(ui, (slot - 1) as usize, &mut appearance.material, &self.material_library, z);
                        ui.color_edit_button_srgb(&mut appearance.rgb);
                        if appearance != c.appearance(slot) {
                            c.set_appearance(slot, appearance);
                            changed = true;
                            c.preview_colors = true;
                        }
                        ui.monospace(c.color_hex(slot));ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(t(z,"默认颜色","Default color"));
                    egui::ComboBox::from_id_salt("default-filament").selected_text(format!("{} · {}",c.filament,c.color_hex(c.filament))).show_ui(ui,|ui| {
                        for slot in 1..=c.slot_count() {let name=format!("{slot} · {}",c.color_hex(slot));if ui.selectable_value(&mut c.filament,slot,name).changed(){changed=true;c.preview_colors=true;}}
                    });
                    if ui.button(t(z,"全部恢复分组颜色","Reset to group colors")).clicked(){self.global_colors.apply(c);changed=true;}
                });
                ui.separator();
                let mut color_rows=std::collections::BTreeSet::new();
                for item in self.bom.iter().filter(|b|b.enabled) {
                    let key=crate::print_plate::color_key(&item.path);
                    if !color_rows.insert(key.to_owned()) {continue;}
                    let quantity:u32=self.bom.iter().filter(|b|b.enabled && crate::print_plate::color_key(&b.path)==key).map(|b|b.qty).sum();
                    ui.push_id(&item.path,|ui| {ui.horizontal(|ui| {
                        let slot=c.slot(&item.path);let mut choice=c.part_assignment(&item.path);
                        let [r,g,b]=c.appearance(slot).rgb;ui.colored_label(Color32::from_rgb(r,g,b),"●");
                        let name=match key {
                            crate::print_plate::BIN_COLOR_KEY=>t(z,"Gridfinity 小盒（同类型）","Gridfinity bins (shared type)").to_owned(),
                            crate::print_plate::LABEL_COLOR_KEY=>t(z,"Cullenect 可拆标牌（同类型）","Cullenect labels (shared type)").to_owned(),
                            _=>crate::accessories::info(&item.path,z).map_or_else(||item.name().to_string(),|m|m.name)
                        };
                        ui.label(format!("{name} ×{quantity}"));
                        egui::ComboBox::from_id_salt("part-color").selected_text(if choice==0 {format!("{} · {}",t(z,"默认","Default"),c.color_hex(slot))}else{format!("{slot} · {}",c.color_hex(slot))}).show_ui(ui,|ui| {
                            ui.selectable_value(&mut choice,0,t(z,"跟随默认颜色","Use default"));
                            for slot in 1..=c.slot_count() {ui.selectable_value(&mut choice,slot,format!("{slot} · {}",c.color_hex(slot)));}
                        });
                        if choice!=c.part_assignment(&item.path) {
                            c.set_part_slot(&item.path,choice);
                            c.preview_colors=true;changed=true;
                        }
                    });});
                }
                ui.separator();ui.small(t(z,"3MF 按耗材组合分盘；文字与底板保持同盘。相同 RGB 的不同槽仍独立处理。STL 不记录颜色或打印盘。","3MF groups by material set; text stays with its body. STL stores neither colors nor plates."));
            });
        });
        self.colors_open = open;
        if changed {
            if self.combination_view {
                self.push_workflow_snapshot(self.workflow_snapshot());
            }
            self.export_options.plate.follow_global_colors = false;
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
                let total = self.export_review_ui(ui);
                ui.add_enabled_ui(self.export_rx.is_none(),|ui|{
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(match self.export_options.format {
                            crate::export::Format::Stl => t(z, "STL 文件夹", "STL folder"),
                            crate::export::Format::ThreeMf => t(z, "3MF 工程", "3MF project"),
                        });
                        if ui.button(t(z, "导出设置…", "Export settings…")).clicked() {
                            self.settings_page = Some(settings_panel::Page::Export);
                        }
                        if self.export_options.format == crate::export::Format::ThreeMf
                            && ui.button(t(z, "打印机…", "Printer…")).clicked() {
                            self.settings_page = Some(settings_panel::Page::Printer);
                        }
                        if self.export_options.format == crate::export::Format::ThreeMf
                            && self.export_scope == workflow::ExportScope::Current
                        {
                            if ui.button(t(z, "打印参数…", "Print parameters…")).clicked() {
                                self.settings_page = Some(settings_panel::Page::Print);
                            }
                            if ui.button(t(z, "零件配色…", "Part colors…")).clicked() {
                                self.colors_open = true;
                            }
                        }
                    });
                    if self.export_options.format == crate::export::Format::ThreeMf {
                        ui.small(t(z, "3MF 为未切片工程。请在 Bambu Studio 核对耗材、支撑与首层。", "3MF projects are unsliced. Review filament, supports, and first layer in Bambu Studio."));
                    }
                    if ui.add_enabled((total > 0 && total <= 1000) && !self.busy && !self.preview.has_pending(),
                        ui_style::primary_button(t(z,"选择位置并导出…","Choose destination and export…"))).clicked() {start=true;}
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
        if !self.members.is_empty()
            && (self.export_scope != workflow::ExportScope::Current
                || self.active_drawer.is_none()
                    && !self.members[self.active_member].preset.drawers.is_empty())
        {
            self.export_combination(ctx);
            return;
        }
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
        let member = self.current_print_member();
        let items = match print_list::checked_jobs([(self.active_member, &member)], &self.catalog) {
            Ok(jobs) => jobs
                .into_iter()
                .flat_map(|(_, job)| job.items)
                .collect::<Vec<_>>(),
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let root = self.catalog.root.clone();
        let mut options = self.export_options.clone();
        options.material_library = std::sync::Arc::new(self.material_library.clone());
        let label = self.label_for_project();
        let gf = if self.assembly_kind == AssemblyKind::Drawer {
            self.gridfinity_for_project()
        } else {
            Default::default()
        };
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
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        if self.smoke
            && std::env::args().any(|arg| {
                matches!(
                    arg.as_str(),
                    "--smoke-bin-editor"
                        | "--smoke-divider-editor"
                        | "--smoke-drawer-divider-editor"
                        | "--smoke-viewport-menu"
                )
            })
        {
            self.editor_smoke_pointer.inject(input);
        }
        if self.smoke && std::env::args().any(|arg| arg == "--smoke-color-groups") {
            self.color_groups_smoke_input(input);
        }
    }

    fn on_exit(&mut self) {
        self.tower_flush();
        if let Err(error) = self.renderer.wait_idle() {
            eprintln!("{error}");
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.preview_modes.release_inactive_plates();
        self.poll(ctx);
        self.tower_tick(ctx);
        self.animation_tick(ctx);
        self.workspace_toolbar(ctx);
        self.left(ctx);
        if self.tab != 0 || self.tower.print_list || self.preview_modes.special() {
            if self.divider_editor.open {
                self.close_drawer_divider_editor();
            }
            if self.sustainer_divider_editor.open {
                self.close_sustainer_divider_editor();
            }
            if self.gridfinity_open {
                self.close_bin_editor();
            }
            if self.inline_edit.open {
                self.close_inline_editor(ctx);
            }
        }
        self.color_window(ctx);
        self.renderer.print_colors = self
            .export_options
            .plate
            .preview_colors
            .then(|| self.export_options.plate.clone());
        if self.combination_view {
            self.commit_member();
        }
        self.renderer.member_colors = if self.scene_key == "combination" {
            self.members
                .iter()
                .map(|m| m.preset.print_config.clone().unwrap_or_default())
                .collect()
        } else {
            Vec::new()
        };
        self.renderer.drawer_colors = if self.scene_key == "combination" {
            self.members
                .iter()
                .map(|m| {
                    m.preset
                        .drawers
                        .iter()
                        .map(|d| d.print_config.clone().unwrap_or_default())
                        .collect()
                })
                .collect()
        } else {
            Vec::new()
        };
        self.viewport(ctx);
        self.part_properties_window(ctx);
        self.part_details_window(ctx);
        self.relations_window(ctx);
        self.export_window(ctx);
        self.combination_spec_window(ctx);
        self.exterior_window(ctx);
        self.label_window(ctx);
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
        self.settings_panel(ctx);
        self.project_settings_panel(ctx);
        self.pump_preview(ctx);
        if self.busy || self.preview.has_pending() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-settings") {
            self.settings_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-motion") {
            self.motion_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-bin-editor") {
            self.bin_editor_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-divider-editor") {
            self.divider_editor_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-drawer-divider-editor") {
            self.drawer_divider_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-presets") {
            self.recipe_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-top") {
            self.roof_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-viewport-menu") {
            self.viewport_menu_smoke(ctx);
            return;
        }
        if self.smoke
            && std::env::args().any(|a| {
                matches!(
                    a.as_str(),
                    "--smoke-tower"
                        | "--smoke-workspace"
                        | "--smoke-build-panel"
                        | "--smoke-color-groups"
                        | "--smoke-project-menu"
                )
            })
        {
            self.tower_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-stopper") {
            self.stopper_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-exterior") {
            self.exterior_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-large-stl") {
            self.large_stl_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-preview-modes") {
            self.preview_modes_smoke(ctx);
            return;
        }
        if self.smoke && std::env::args().any(|a| a == "--smoke-combination") {
            self.combination_smoke(ctx);
            return;
        }
        if self.smoke {
            ctx.request_repaint_after(Duration::from_millis(100));
            if !self.busy && !self.preview.has_pending() && !self.scene.parts.is_empty() {
                self.smoke_frame += 1;
                if self.smoke_frame == 5 && [55, 56].contains(&self.smoke_stage) {
                    for part in &mut self.scene.parts {
                        part.visible = part.pose.path.contains("/Modules/")
                            || part.pose.path == crate::sustainer::STACKING_LATCH;
                    }
                    self.selected = None;
                    self.explosion = 0.;
                    self.camera.fit(&self.scene, 0.);
                    self.camera.yaw = 0.3;
                    self.camera.pitch = -1.1;
                }
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
                        p.visible = p.pose.path.contains("/Facades/")
                            || crate::label::is_label(&p.pose.path);
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
                    self.open_drawer_divider_editor(ctx);
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
                                "native-text-colors-flat.png",
                                "native-sustainer-closed.png",
                                "native-sustainer-open.png",
                                "native-sustainer-exploded.png",
                                "native-sustainer-l-sealed.png",
                                "native-sustainer-s-split.png",
                                "native-sustainer-xl-split.png",
                                "native-sustainer-divider-s-editor.png",
                                "native-sustainer-divider-s-open.png",
                                "native-sustainer-divider-l-editor.png",
                                "native-sustainer-divider-l-sealed.png",
                                "native-sustainer-divider-xl-sealed.png",
                                "native-sustainer-label-raised.png",
                                "native-sustainer-label-engraved.png",
                                "native-sustainer-label-flat.png",
                                "native-sustainer-label-assembled.png",
                                "native-sustainer-latch-s-bottom.png",
                                "native-sustainer-latch-xl-bottom.png",
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
                    if self.smoke_stage == 56 {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else {
                        self.smoke_stage += 1;
                        self.smoke_frame = 0;
                        self.screenshot_requested = false;
                        match self.smoke_stage {
                            55 | 56 => {
                                self.label_open = false;
                                self.export_options.plate.preview_colors = false;
                                self.sustainer_dividers = Default::default();
                                self.sustainer_options = Default::default();
                                self.sustainer_angle = 0.;
                                self.selection = Selection {
                                    width: if self.smoke_stage == 55 { "S" } else { "XL" }.into(),
                                    height: if self.smoke_stage == 55 { "2F" } else { "1F" }.into(),
                                    ..Default::default()
                                };
                                self.bom = self
                                    .catalog
                                    .sustainer_bom(&self.selection, self.sustainer_options)
                                    .unwrap();
                                self.assembly(ctx);
                            }
                            51..=53 => {
                                self.sustainer_divider_editor.open = false;
                                self.label_open = true;
                                self.label = crate::label::Config {
                                    enabled: true,
                                    text: "收纳\nA8".into(),
                                    style: match self.smoke_stage {
                                        51 => crate::label::Style::Raised,
                                        52 => crate::label::Style::Engraved,
                                        _ => crate::label::Style::Flat,
                                    },
                                    ..Default::default()
                                };
                                self.export_options.plate.preview_colors = true;
                                self.export_options
                                    .plate
                                    .set_part_slot(crate::label::SUSTAINER_PATH, 3);
                                self.export_options
                                    .plate
                                    .set_text_slot(crate::label::SUSTAINER_PATH, 2);
                                self.single(crate::label::SUSTAINER_PATH.into(), ctx);
                            }
                            54 => {
                                self.label_open = false;
                                self.sustainer_angle = 0.;
                                self.sustainer_options = Default::default();
                                self.selection = Selection {
                                    depth: 6,
                                    height: "2F".into(),
                                    ..Default::default()
                                };
                                self.label.text = "S6-2F".into();
                                let path = self.assembly_kind.label_path(&self.selection);
                                self.export_options.plate.set_part_slot(path, 3);
                                self.export_options.plate.set_text_slot(path, 2);
                                self.bom = self
                                    .catalog
                                    .sustainer_bom(&self.selection, self.sustainer_options)
                                    .unwrap();
                                self.assembly(ctx);
                            }
                            46 | 48..=50 => {
                                self.selection = Selection {
                                    width: match self.smoke_stage {
                                        46 => "S",
                                        50 => "XL",
                                        _ => "L",
                                    }
                                    .into(),
                                    depth: if self.smoke_stage == 49 { 5 } else { 3 },
                                    height: if self.smoke_stage == 49 { "2F" } else { "1F" }.into(),
                                    hidden_key: false,
                                };
                                self.sustainer_options = SustainerOptions {
                                    body: match self.smoke_stage {
                                        46 => SustainerBody::Flat,
                                        48 => SustainerBody::Dividers,
                                        _ => SustainerBody::Sealed,
                                    },
                                    split_roof: false,
                                };
                                let mut items = Vec::new();
                                let lanes = match self.smoke_stage {
                                    49 => 2,
                                    50 => 3,
                                    _ => 1,
                                };
                                for lane in 0..lanes {
                                    for slot in 0..if self.smoke_stage == 49 { 4 } else { 3 } {
                                        items.push(crate::sustainer_dividers::Placement {
                                            axis: crate::sustainer_dividers::Axis::Width,
                                            slot,
                                            lane,
                                        });
                                    }
                                }
                                if self.smoke_stage == 48 {
                                    for slot in 0..2 {
                                        items.push(crate::sustainer_dividers::Placement {
                                            axis: crate::sustainer_dividers::Axis::Depth,
                                            slot,
                                            lane: 0,
                                        });
                                    }
                                }
                                self.sustainer_dividers = crate::sustainer_dividers::Layout {
                                    specification: crate::sustainer_dividers::spec(
                                        &self.selection,
                                        self.sustainer_options,
                                    ),
                                    items,
                                };
                                self.sustainer_divider_editor.open = true;
                                self.sustainer_divider_editor.reset();
                                self.sustainer_angle = 85.;
                                self.bom = self
                                    .catalog
                                    .sustainer_bom(&self.selection, self.sustainer_options)
                                    .unwrap();
                                self.assembly(ctx);
                            }
                            47 => {
                                self.sustainer_divider_editor.open = false;
                                self.camera.fit(&self.scene, 0.);
                            }
                            40 => {
                                self.assembly_kind = AssemblyKind::Sustainer;
                                self.export_options.plate.preview_colors = false;
                                self.sustainer_options = Default::default();
                                self.sustainer_angle = 0.;
                                self.selection = Selection::default();
                                self.zh = true;
                                self.gridfinity_open = false;
                                self.label_open = false;
                                self.close_drawer_divider_editor();
                                self.joints_open = false;
                                self.dimensions = false;
                                self.export_open = false;
                                self.colors_open = false;
                                self.selected = None;
                                self.ghost = false;
                                self.explosion = 0.;
                                self.bom = self
                                    .catalog
                                    .sustainer_bom(&self.selection, self.sustainer_options)
                                    .unwrap();
                                self.assembly(ctx);
                            }
                            41 | 42 => {
                                self.sustainer_angle =
                                    if self.smoke_stage == 41 { 80. } else { 0. };
                                if let Err(e) = crate::sustainer::set_lid_angle(
                                    &mut self.scene,
                                    self.sustainer_angle,
                                ) {
                                    self.status = e;
                                }
                                self.explosion = if self.smoke_stage == 42 { 1. } else { 0. };
                                self.camera.fit(&self.scene, self.explosion);
                            }
                            43..=45 => {
                                self.selection = Selection {
                                    width: match self.smoke_stage {
                                        43 => "L",
                                        44 => "S",
                                        _ => "XL",
                                    }
                                    .into(),
                                    depth: match self.smoke_stage {
                                        43 => 4,
                                        44 => 6,
                                        _ => 7,
                                    },
                                    height: if self.smoke_stage == 43 { "2F" } else { "1F" }.into(),
                                    hidden_key: false,
                                };
                                self.sustainer_options = SustainerOptions {
                                    body: if self.smoke_stage == 43 {
                                        SustainerBody::Sealed
                                    } else {
                                        SustainerBody::Flat
                                    },
                                    split_roof: self.smoke_stage >= 44,
                                };
                                self.sustainer_angle = 65.;
                                self.explosion = 0.;
                                self.bom = self
                                    .catalog
                                    .sustainer_bom(&self.selection, self.sustainer_options)
                                    .unwrap();
                                self.assembly(ctx);
                            }
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
                                let drawer = crate::constraints::AssemblyRules::mechanism(
                                    &self.scene,
                                    crate::scene::Target {
                                        member: 0,
                                        drawer: None,
                                    },
                                )
                                .expect("Runner smoke needs a drawer slider");
                                crate::constraints::AssemblyRules::set_slider(
                                    &mut self.scene,
                                    &drawer.child,
                                    drawer.max,
                                )
                                .expect("Open both runner stages");
                                self.camera.fit(&self.scene, 0.);
                            }
                            10 => {
                                crate::constraints::AssemblyRules::close_drawers(&mut self.scene)
                                    .expect("Return both runner stages to their closed datums");
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
                                self.close_drawer_divider_editor();
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
                                        detachable_label: true,
                                        label: crate::label::Config {
                                            enabled: true,
                                            text: "收纳 A8".into(),
                                            size: 5.,
                                            depth: 0.2,
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
                                self.gridfinity.adopt_shared();
                                self.export_options
                                    .plate
                                    .set_part_slot(&self.gridfinity.bins[0].path(), 3);
                                self.export_options
                                    .plate
                                    .set_part_slot(&self.gridfinity.bins[0].label_path(), 4);
                                self.export_options
                                    .plate
                                    .set_text_slot(&self.gridfinity.bins[0].label_path(), 1);
                                self.export_options.plate.preview_colors = true;
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
                            39 => {
                                self.zh = true;
                                self.label.style = crate::label::Style::Flat;
                                self.label.depth = 0.4;
                                self.label_open = true;
                                self.single(crate::label::PATH.into(), ctx);
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
// Runs only inside the preview worker, including when the base mesh cache hits.
fn load_scene_definition(scene: &mut Scene, home: &Path, key: &str) -> Result<(), String> {
    if !scene.assembly {
        return Ok(());
    }
    let text = match std::fs::read_to_string(home.join(format!("state/assembly-{key}.toml"))) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    let definition: Definition =
        toml::from_str(&text).map_err(|e| format!("Assembly definition: {e}"))?;
    scene.apply(&definition)
}
fn color_slot_choices(
    ui: &mut egui::Ui,
    config: &crate::print_plate::Config,
    selected: &mut u32,
    default: &str,
) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(selected, 0, default);
        for slot in 1..=config.slot_count() {
            let [r, g, b] = config.appearance(slot).rgb;
            if ui_style::swatch(
                ui,
                Color32::from_rgb(r, g, b),
                *selected == slot,
                &slot.to_string(),
            )
            .on_hover_text(config.color_hex(slot))
            .clicked()
            {
                *selected = slot;
            }
        }
    });
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

// Presentation groups only; mating relationships remain owned by constraints.rs.
struct PartTreeNode {
    key: String,
    label: String,
    parts: Vec<usize>,
    children: Vec<PartTreeNode>,
}
// Stop at the nearest major subassembly: a facade stays separate from its
// drawer, while keys, caps and springs inherit the facade through their mates.
fn part_tree_group_owner(index: usize, categories: &[&str], parents: &[Option<usize>]) -> usize {
    let mut current = index;
    for _ in 0..categories.len() {
        if matches!(
            categories[current],
            "Structure" | "Drawer" | "Facade" | "Roof"
        ) {
            return current;
        }
        let Some(parent) = parents[current].filter(|&parent| parent < categories.len()) else {
            break;
        };
        current = parent;
    }
    // Partial previews and invalid/cyclic relations must never hide a part.
    index
}
fn part_tree(scene: &Scene, bom: &[Bom], zh: bool) -> Vec<PartTreeNode> {
    let categories: Vec<_> = scene
        .parts
        .iter()
        .map(|part| {
            bom.iter()
                .find(|item| item.path == part.pose.path)
                .map_or("", |item| item.group.as_str())
        })
        .collect();
    let indices: HashMap<_, _> = scene
        .parts
        .iter()
        .enumerate()
        .map(|(index, part)| (crate::constraints::PartId::of(part), index))
        .collect();
    let mut parents = vec![None; scene.parts.len()];
    for relation in &scene.relations {
        if let (Some(&child), Some(&parent)) =
            (indices.get(&relation.child), indices.get(&relation.parent))
        {
            parents[child] = Some(parent);
        }
    }
    let mut groups: Vec<PartTreeNode> = Vec::new();
    for (index, part) in scene.parts.iter().enumerate() {
        let path = &part.pose.path;
        let owner = part_tree_group_owner(index, &categories, &parents);
        let owner_path = &scene.parts[owner].pose.path;
        let item = bom.iter().find(|item| item.path == *owner_path);
        let metadata = crate::accessories::info(path, zh);
        let (key, label) = if let Some(item) = item.filter(|item| !item.group.is_empty()) {
            let label = match item.group.as_str() {
                "Structure" => t(zh, "外壳组件", "Housing assembly"),
                "Drawer" => t(zh, "抽屉组件", "Drawer assembly"),
                "Facade" => t(zh, "面板组件", "Front panel assembly"),
                "Roof" => t(zh, "顶盖组件", "Lid assembly"),
                other => group_label(zh, other),
            };
            (item.group.clone(), label.to_owned())
        } else if let Some(info) = &metadata {
            let key = crate::accessories::info(path, false)
                .unwrap()
                .group
                .to_owned();
            (key, info.group.to_owned())
        } else {
            ("Other".into(), t(zh, "其他元件", "Other parts").into())
        };
        let group_index = groups
            .iter()
            .position(|group| group.key == key)
            .unwrap_or_else(|| {
                groups.push(PartTreeNode {
                    key,
                    label,
                    parts: Vec::new(),
                    children: Vec::new(),
                });
                groups.len() - 1
            });
        let group = &mut groups[group_index];
        group.parts.push(index);
        let source_key = format!("{}:{path}", group.key);
        if let Some(source) = group
            .children
            .iter_mut()
            .find(|source| source.key == source_key)
        {
            source.parts.push(index);
        } else {
            group.children.push(PartTreeNode {
                key: source_key,
                label: metadata.map_or_else(
                    || path.rsplit('/').next().unwrap_or(path).to_owned(),
                    |info| info.name,
                ),
                parts: vec![index],
                children: Vec::new(),
            });
        }
    }
    groups
}
fn part_tree_leaf(
    ui: &mut egui::Ui,
    scene: &mut Scene,
    selected: &mut Option<usize>,
    index: usize,
    zh: bool,
    reveal: bool,
) {
    let part = &mut scene.parts[index];
    ui.push_id(crate::constraints::PartId::of(part), |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut part.visible, "").on_hover_text(t(
                zh,
                "显示 / 隐藏元件",
                "Show / hide part",
            ));
            let response = ui
                .add(
                    egui::Button::selectable(
                        *selected == Some(index),
                        format!("{} · {}", index + 1, part.label(zh)),
                    )
                    .truncate(),
                )
                .on_hover_text(&part.pose.path);
            if reveal && *selected == Some(index) {
                response.scroll_to_me(Some(egui::Align::Center));
            }
            if response.clicked() {
                *selected = Some(index);
            }
        });
    });
}
impl PartTreeNode {
    fn id(&self) -> egui::Id {
        egui::Id::new(("assembly_part_tree", &self.key))
    }
    fn prepare(
        &self,
        ctx: &egui::Context,
        expand: Option<bool>,
        selected: Option<usize>,
        reveal: bool,
    ) {
        let open = expand.or_else(|| {
            (reveal && selected.is_some_and(|index| self.parts.contains(&index))).then_some(true)
        });
        if let Some(open) = open {
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ctx,
                self.id(),
                true,
            );
            state.set_open(open);
            state.store(ctx);
            // Bulk actions and selection reveal must reach children even when a parent is closed.
            ctx.animate_bool_with_time(self.id(), open, 0.);
        }
        for child in &self.children {
            child.prepare(ctx, expand, selected, reveal);
        }
    }
    fn show(
        &self,
        ui: &mut egui::Ui,
        scene: &mut Scene,
        selected: &mut Option<usize>,
        zh: bool,
        reveal: bool,
    ) {
        ui.push_id(&self.key, |ui| {
            let contains_selected = selected.is_some_and(|index| self.parts.contains(&index));
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                self.id(),
                true,
            );
            state
                .show_header(ui, |ui| {
                    let visible_count = self
                        .parts
                        .iter()
                        .filter(|&&index| scene.parts[index].visible)
                        .count();
                    let mut visible = visible_count == self.parts.len();
                    let mixed = visible_count > 0 && !visible;
                    if ui
                        .add(egui::Checkbox::new(&mut visible, "").indeterminate(mixed))
                        .on_hover_text(t(zh, "显示 / 隐藏整组", "Show / hide group"))
                        .changed()
                    {
                        for &index in &self.parts {
                            scene.parts[index].visible = visible;
                        }
                    }
                    let label = RichText::new(format!("{} ({})", self.label, self.parts.len()));
                    ui.add(
                        egui::Label::new(if contains_selected {
                            label.strong()
                        } else {
                            label
                        })
                        .truncate(),
                    )
                    .on_hover_text(&self.label);
                })
                .body(|ui| {
                    if self.children.is_empty() {
                        for &index in &self.parts {
                            part_tree_leaf(ui, scene, selected, index, zh, reveal);
                        }
                    } else {
                        for child in &self.children {
                            if child.parts.len() == 1 {
                                part_tree_leaf(ui, scene, selected, child.parts[0], zh, reveal);
                            } else {
                                child.show(ui, scene, selected, zh, reveal);
                            }
                        }
                    }
                });
        });
    }
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
        "Roof connector" => "分体顶盖连接件",
        "Lower hinges" => "下铰链",
        "Upper hinges" => "上铰链",
        "Hinge rods" => "铰链轴杆",
        "Locks" => "前锁组件",
        "Stacking latches" => "堆叠卡闩",
        "Label" => "标签",
        "Base" => "底座",
        "Optional baseplate" => "底板（可选）",
        "Optional accessory" => "选装配件",
        "Sustainer divider layout" => "箱子隔板布局",
        "Divider layout" => "分隔布局",
        _ => group,
    }
}

#[cfg(test)]
mod accessory_export_tests {
    use super::*;
    #[test]
    fn tree_groups_follow_subassemblies_without_absorbing_other_assemblies() {
        // Housing -> drawer -> facade -> key -> cap; springs also mate to facade.
        // The two runner instances attach to different assemblies.
        let categories = [
            "Structure",
            "Drawer",
            "Facade",
            "Key",
            "Mechanism",
            "Mechanism",
            "Runners",
            "Runners",
            "Optional accessory",
            "Roof",
            "Upper hinges",
            "Mechanism",
        ];
        let parents = [
            None,
            Some(0),
            Some(1),
            Some(2),
            Some(3),
            Some(2),
            Some(0),
            Some(1),
            None,
            Some(0),
            Some(9),
            Some(99),
        ];
        let owners: Vec<_> = (0..categories.len())
            .map(|i| part_tree_group_owner(i, &categories, &parents))
            .collect();
        assert_eq!(owners, [0, 1, 2, 2, 2, 2, 0, 1, 8, 9, 9, 11]);
        // Broken graphs retain the original categories and terminate.
        let cyclic = [Some(1), Some(0)];
        assert_eq!(part_tree_group_owner(0, &["Key", "Mechanism"], &cyclic), 0);
        assert_eq!(part_tree_group_owner(1, &["Key", "Mechanism"], &cyclic), 1);
    }

    #[test]
    fn library_settings_require_user_choice_and_remember_last_directory() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("config")).unwrap();
        std::fs::create_dir(home.path().join("state")).unwrap();
        let library = tempfile::tempdir().unwrap();
        let saved = Settings {
            material_library: Default::default(),
            global_colors: Default::default(),
            color_presets: Default::default(),
            appearance: appearance::Appearance::Dark,
            model_root: library.path().to_string_lossy().into(),
            language: "en".into(),
            export: Default::default(),
            label_font: Some(crate::label::Font::ArialBold),
        };
        let data = toml::to_string(&saved).unwrap();
        std::fs::write(home.path().join("config/default.toml"), &data).unwrap();
        let first = Settings::load(home.path()).unwrap();
        assert!(first.appearance == appearance::Appearance::Dark);
        assert!(first.model_root.is_empty());
        assert!(first.startup_root().is_none());
        std::fs::write(home.path().join("state/settings.toml"), &data).unwrap();
        let restored = Settings::load(home.path()).unwrap();
        assert_eq!(restored.startup_root(), Some(library.path().to_path_buf()));
        assert!(restored.appearance == appearance::Appearance::Dark);
        assert_eq!(restored.label_font, saved.label_font);
        drop(library);
        let missing = Settings::load(home.path()).unwrap();
        assert!(missing.startup_root().is_none());
        assert_eq!(missing.model_root, saved.model_root);
        let empty: Settings = toml::from_str("language = 'zh'").unwrap();
        assert!(empty.startup_root().is_none());
        assert!(empty.appearance == appearance::Appearance::Light);
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
            drawers: vec![],
            sustainer_dividers: Default::default(),
            print_config: Some(color_config),
            gridfinity: Default::default(),
            label: Default::default(),
            assembly_kind: AssemblyKind::Drawer,
            sustainer_options: Default::default(),
            sustainer_angle: 0.,
            drawer_extension: 0.,
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
        legacy.as_table_mut().unwrap().remove("sustainer_options");
        legacy.as_table_mut().unwrap().remove("sustainer_angle");
        legacy.as_table_mut().unwrap().remove("label");
        let legacy: Preset = legacy.try_into().unwrap();
        assert!(legacy.print_config.is_none());
        assert!(!legacy.divider_layout.enabled);
        assert!(!legacy.label.enabled);
        assert_eq!(legacy.assembly_kind, AssemblyKind::Drawer);
        assert_eq!(restored.assembly_kind, AssemblyKind::Drawer);
        let mut box_preset = toml::Value::try_from(&preset).unwrap();
        box_preset["assembly_kind"] = toml::Value::String("sustainer".into());
        let mut box_preset: Preset = box_preset.try_into().unwrap();
        box_preset.sustainer_options = SustainerOptions {
            body: SustainerBody::Sealed,
            split_roof: true,
        };
        box_preset.sustainer_angle = 72.;
        box_preset.label = crate::label::Config {
            enabled: true,
            text: "盒子 A8".into(),
            style: crate::label::Style::Flat,
            ..Default::default()
        };
        let box_preset: Preset = toml::from_str(&toml::to_string(&box_preset).unwrap()).unwrap();
        assert_eq!(box_preset.assembly_kind, AssemblyKind::Sustainer);
        assert_eq!(box_preset.sustainer_options.body, SustainerBody::Sealed);
        assert!(box_preset.sustainer_options.split_roof);
        assert_eq!(box_preset.sustainer_angle, 72.);
        assert!(box_preset.label.active());
        assert_eq!(box_preset.label.text, "盒子 A8");
        assert_eq!(box_preset.label.style, crate::label::Style::Flat);
        assert_eq!(legacy.sustainer_angle, 0.);
        assert_eq!(legacy.sustainer_options, SustainerOptions::default());
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
