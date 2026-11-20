use super::*;
mod color_smoke;
pub(super) mod colors;
mod layers;
mod modifications;
mod roofs;
pub(super) struct State {
    pub name: String,
    rename_draft: Option<String>,
    pub show_parts: bool,
    pub print_list: bool,
    pub(super) open_layer: Option<u32>,
    pub(super) layer_group_request: Option<bool>,
    scroll_to_layer: Option<u32>,
    card_positions: BTreeMap<u32, f32>,
    reorder: Option<ReorderMotion>,
    scroll_to_top: bool,
    color_edit: Option<colors::Edit>,
    open_color: Option<String>,
    reveal_colors: bool,
    color_smoke_input: std::collections::VecDeque<Vec<egui::Event>>,
    error: String,
    reset_inserts: bool,
    projects: Vec<Project>,
    current: usize,
    loaded: bool,
    pending_restore: bool,
    read: Option<Receiver<Result<Projects, String>>>,
    write: Option<Receiver<Result<(), String>>>,
    key: Vec<u8>,
    saved_key: Vec<u8>,
    due: Option<Instant>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            name: default_name(),
            rename_draft: None,
            show_parts: false,
            print_list: false,
            open_layer: Some(0),
            layer_group_request: None,
            scroll_to_layer: None,
            card_positions: BTreeMap::new(),
            reorder: None,
            scroll_to_top: true,
            color_edit: None,
            open_color: None,
            reveal_colors: false,
            color_smoke_input: Default::default(),
            error: String::new(),
            reset_inserts: false,
            projects: vec![],
            current: 0,
            loaded: false,
            pending_restore: false,
            read: None,
            write: None,
            key: vec![],
            saved_key: vec![],
            due: None,
        }
    }
}
impl State {
    pub(super) fn clear_reorder(&mut self) {
        self.reorder = None;
        self.card_positions.clear();
    }

    pub(super) fn reveal_layer(&mut self, level: u32) {
        self.open_layer = Some(level);
        self.layer_group_request = Some(true);
        self.scroll_to_layer = Some(level);
        self.print_list = false;
    }

    fn title_ui(&mut self, ui: &mut egui::Ui, z: bool) {
        let id = ui.make_persistent_id("project-title-edit");
        if let Some(draft) = &mut self.rename_draft {
            let response = ui.add(
                egui::TextEdit::singleline(draft)
                    .id(id)
                    .font(egui::TextStyle::Heading)
                    .hint_text(t(z, "项目名称", "Project name"))
                    .char_limit(80)
                    .desired_width(ui.available_width().min(330.)),
            );
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.rename_draft = None;
                response.surrender_focus();
            } else if response.lost_focus() || response.clicked_elsewhere() {
                self.finish_rename();
            }
            return;
        }

        let name = self.name.trim();
        let name = if name.is_empty() {
            t(z, "未命名项目", "Untitled project")
        } else {
            name
        };
        let response = ui
            .add(
                egui::Label::new(RichText::new(name).heading().strong())
                    .truncate()
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::Text)
            .on_hover_text(format!("{name}\n{}", t(z, "点击改名", "Click to rename")));
        if response.clicked() {
            self.rename_draft = Some(self.name.clone());
            ui.memory_mut(|memory| memory.request_focus(id));
            ui.ctx().request_repaint();
        }
    }

    fn finish_rename(&mut self) {
        if let Some(draft) = self.rename_draft.take() {
            let name = draft.trim();
            if !name.is_empty() {
                self.name = name.into();
            }
        }
    }
}

const REORDER_SECONDS: f64 = 0.25;

struct ReorderMotion {
    start: f64,
    previous_y: BTreeMap<u32, f32>,
}

impl ReorderMotion {
    fn remaining(&self, now: f64) -> f32 {
        let t = ((now - self.start) / REORDER_SECONDS).clamp(0., 1.) as f32;
        1. - t * t * (3. - 2. * t)
    }
}

fn moved_level(level: u32, from: u32, to: u32) -> u32 {
    if level == from {
        to
    } else if from < to && level > from && level <= to {
        level - 1
    } else if from > to && level >= to && level < from {
        level + 1
    } else {
        level
    }
}

#[cfg(test)]
#[test]
fn reorder_animation_follows_the_same_layer_permutation() {
    assert_eq!(
        [0, 1, 2, 3, 4].map(|level| moved_level(level, 1, 3)),
        [0, 3, 1, 2, 4]
    );
    assert_eq!(
        [0, 1, 2, 3, 4].map(|level| moved_level(level, 3, 1)),
        [0, 2, 3, 1, 4]
    );
    let motion = ReorderMotion {
        start: 2.,
        previous_y: BTreeMap::from([(3, 170.)]),
    };
    assert_eq!(motion.remaining(2.), 1.);
    assert_eq!(motion.remaining(2. + REORDER_SECONDS / 2.), 0.5);
    assert_eq!(motion.remaining(2. + REORDER_SECONDS), 0.);
}

pub(super) fn default_name() -> String {
    "工作台抽屉柜".into()
}

#[cfg(test)]
#[test]
fn title_rename_accepts_enter_and_cancels_escape() {
    let ctx = egui::Context::default();
    let mut state = State::default();
    let frame = |state: &mut State, events| {
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| state.title_ui(ui, true));
            },
        );
    };
    let click_title = |state: &mut State| {
        let pos = egui::pos2(18., 18.);
        frame(state, vec![]);
        for pressed in [true, false] {
            frame(
                state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(state.rename_draft.is_some());
        frame(state, vec![]);
    };
    for (draft, key) in [
        ("  工作台  ", egui::Key::Enter),
        ("取消", egui::Key::Escape),
    ] {
        click_title(&mut state);
        state.rename_draft = Some(draft.into());
        frame(
            &mut state,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(state.name, "工作台");
        assert!(state.rename_draft.is_none());
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Project {
    name: String,
    saved: CombinationPreset,
}

impl Project {
    fn summary(&self, zh: bool) -> String {
        match rows(&self.saved.members) {
            Ok(layers) if !layers.is_empty() => {
                let first = &layers[0];
                format!(
                    "{}{} · {} {}",
                    first.width,
                    first.depth,
                    layers.len(),
                    t(zh, "层", "layers")
                )
            }
            _ if self.saved.members.is_empty() => {
                let selection = &self.saved.preset.selection;
                format!(
                    "{}{} · {}",
                    selection.width, selection.depth, selection.height
                )
            }
            _ => format!(
                "{} {}",
                self.saved.members.len(),
                t(zh, "个成员", "members")
            ),
        }
    }

    fn duplicate(mut self, projects: &[Self], zh: bool) -> Self {
        let base = format!("{}{}", self.name, t(zh, " 副本", " copy"));
        let mut name = base.clone();
        let mut suffix = 2;
        while projects.iter().any(|project| project.name == name) {
            name = format!("{base} {suffix}");
            suffix += 1;
        }
        self.name = name.clone();
        self.saved.project_name = name;
        self
    }
}

enum ProjectAction {
    Switch(usize),
    Starter,
    Recipe(usize),
    Duplicate,
    Open,
    Save,
}

fn project_menu_item(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    command: ProjectAction,
    action: &mut Option<ProjectAction>,
) -> egui::Response {
    let response = ui.add_enabled(enabled, egui::Button::new(label));
    if response.clicked() {
        *action = Some(command);
        ui.close();
    }
    response
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Projects {
    current: usize,
    items: Vec<Project>,
}

pub(super) struct PrintJob {
    pub name: String,
    pub preset: Preset,
    pub items: Vec<Bom>,
    pub drawer: Option<usize>,
}
impl Preset {
    pub(super) fn print_jobs(&self, name: &str) -> Vec<PrintJob> {
        let job = |name, preset: Preset, mut items: Vec<Bom>, drawer| {
            for b in &mut items {
                b.normalize_print(&preset.selection);
            }
            PrintJob {
                name,
                preset,
                items,
                drawer,
            }
        };
        if self.drawers.is_empty() {
            return vec![job(name.into(), self.clone(), self.bom.clone(), None)];
        }
        let mut shell = self.clone();
        shell.drawers.clear();
        shell.gridfinity = Default::default();
        shell.label.enabled = false;
        let mut jobs = vec![job(
            format!("{name} · 模块"),
            shell,
            crate::catalog::tower::housing_items(&self.bom),
            None,
        )];
        for (i, drawer) in self.drawers.iter().enumerate() {
            jobs.push(job(
                format!("{name} · 抽屉 {}", i + 1),
                drawer.clone(),
                crate::catalog::tower::drawer_items(&drawer.bom),
                Some(i),
            ));
        }
        jobs
    }
    pub(super) fn printing_bom(&self) -> Vec<Bom> {
        self.print_jobs("")
            .into_iter()
            .flat_map(|j| j.items)
            .collect()
    }
}

pub(super) fn rows(members: &[Member]) -> Result<Vec<crate::constraints::tower::Row>, String> {
    crate::constraints::tower::rows(
        &members
            .iter()
            .map(|m| crate::constraints::stacking::Module {
                name: &m.name,
                selection: &m.preset.selection,
                placement: m.placement,
                is_box: m.preset.assembly_kind == AssemblyKind::Sustainer,
            })
            .collect::<Vec<_>>(),
    )
}
pub(super) fn configure(p: &mut Preset, catalog: &Catalog, reset: bool) -> Result<(), String> {
    if reset {
        p.gridfinity = Default::default();
        p.divider_layout = Default::default();
        p.sustainer_dividers = Default::default();
        p.bom.clear();
    }
    if p.assembly_kind == AssemblyKind::Sustainer {
        p.drawer_extension = 0.;
    }
    p.sustainer_options = p.sustainer_options.normalized(&p.selection);
    p.bom = p.configured_bom(catalog)?;
    for child in &mut p.drawers {
        child.selection.width = p.selection.width.clone();
        child.selection.depth = p.selection.depth;
        configure(child, catalog, reset)?;
    }
    p.validate_member(catalog).map_err(|e|format!("{e}；如需更换不兼容尺寸，可勾选重置内部配置。 / Reset incompatible inserts explicitly to change size."))
}
fn fresh_like(p: &Preset) -> Preset {
    let mut next = p.clone();
    next.drawers.clear();
    next.drawer_extension = 0.;
    next.gridfinity = Default::default();
    next.divider_layout = Default::default();
    next.sustainer_dividers = Default::default();
    next.bom.clear();
    next.label.text.clear();
    next.label.enabled = false;
    next
}
fn normalize(members: &mut [Member]) {
    members.sort_by_key(|m| (m.placement.level, m.placement.column));
    for (i, m) in members.iter_mut().enumerate() {
        m.name = format!(
            "第 {:02} 层 · {}{} {}",
            m.placement.level + 1,
            m.preset.selection.width,
            m.preset.selection.depth,
            if m.preset.assembly_kind == AssemblyKind::Drawer {
                "抽屉"
            } else {
                "箱子"
            }
        );
        m.name.push_str(&format!(" / {}", m.placement.column + 1));
        m.placement.offset = [0.; 3];
        let _ = i;
    }
}
fn resize(
    members: &[Member],
    catalog: &Catalog,
    width: &str,
    depth: u8,
    reset: bool,
) -> Result<Vec<Member>, String> {
    let original_rows = rows(members)?;
    let mut next = Vec::new();
    for row in original_rows {
        let split = width == "XL" || row.members.len() == 2 && width == "L";
        let widths = crate::constraints::tower::columns(width, split)?;
        for (column, w) in widths.iter().enumerate() {
            let mut member = members[*row.members.get(column).unwrap_or(&row.members[0])].clone();
            member.placement.column = column as u32;
            member.preset.selection.width = (*w).into();
            member.preset.selection.depth = depth;
            configure(&mut member.preset, catalog, reset)?;
            next.push(member);
        }
    }
    normalize(&mut next);
    roofs::reconcile(members, &mut next)?;
    Ok(next)
}
fn set_row(
    members: &[Member],
    catalog: &Catalog,
    level: u32,
    height: &str,
    split: bool,
    reset: bool,
) -> Result<Vec<Member>, String> {
    let all = rows(members)?;
    let row = all
        .iter()
        .find(|r| r.level == level)
        .ok_or("层已移除 / Layer removed")?;
    let widths = crate::constraints::tower::columns(&row.width, split)?;
    let mut next: Vec<_> = members
        .iter()
        .filter(|m| m.placement.level != level)
        .cloned()
        .collect();
    for (column, w) in widths.iter().enumerate() {
        let mut member = members[*row.members.get(column).unwrap_or(&row.members[0])].clone();
        member.placement.column = column as u32;
        member.preset.selection.width = (*w).into();
        member.preset.selection.height = height.into();
        if !member.preset.drawers.is_empty()
            && crate::constraints::tower::drawer_heights(
                height,
                &member
                    .preset
                    .drawers
                    .iter()
                    .map(|d| d.selection.height.as_str())
                    .collect::<Vec<_>>(),
            )
            .is_err()
        {
            return Err("请先将层内抽屉切换为整格，再修改层高 / Choose a full-height drawer before changing layer height".into());
        }
        configure(&mut member.preset, catalog, reset)?;
        next.push(member);
    }
    normalize(&mut next);
    roofs::reconcile(members, &mut next)?;
    Ok(next)
}
pub(super) fn drawer_pattern(
    p: &Preset,
    catalog: &Catalog,
    heights: &[&str],
    reset: bool,
) -> Result<Preset, String> {
    crate::constraints::tower::drawer_heights(&p.selection.height, heights)?;
    let old = if p.drawers.is_empty() {
        vec![p.clone()]
    } else {
        p.drawers.clone()
    };
    let mut next = p.clone();
    next.gridfinity = Default::default();
    next.divider_layout = Default::default();
    next.sustainer_dividers = Default::default();
    next.drawers.clear();
    let mut children = Vec::new();
    for (i, height) in heights.iter().enumerate() {
        let mut child = old.get(i).cloned().unwrap_or_else(|| fresh_like(&old[0]));
        child.drawers.clear();
        child.selection.height = (*height).into();
        configure(&mut child, catalog, reset)?;
        children.push(child);
    }
    if children.len() == 1 {
        let mut single = children.remove(0);
        single.selection = p.selection.clone();
        configure(&mut single, catalog, reset)?;
        return Ok(single);
    }
    next.drawers = children;
    configure(&mut next, catalog, reset)?;
    Ok(next)
}
impl App {
    pub(super) fn tower_regular(&self) -> bool {
        rows(&self.members).is_ok()
    }
    pub(super) fn tower_edit_slot(
        &mut self,
        member: usize,
        drawer: Option<usize>,
        ctx: &egui::Context,
    ) {
        self.open_inline_target(member, drawer, ctx);
    }
    fn tower_snapshot(&mut self) -> CombinationPreset {
        self.commit_member();
        let preset = self
            .members
            .get(self.active_member)
            .map(|m| m.preset.clone())
            .unwrap_or_else(|| self.member_snapshot());
        self.project_snapshot(preset)
    }
    fn tower_apply(
        &mut self,
        mut next: Vec<Member>,
        level: u32,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if next.is_empty() || next.len() > 64 {
            return Err("项目需要 1–64 个模块 / Project requires 1–64 modules".into());
        }
        normalize(&mut next);
        let candidate = CombinationPreset {
            project_name: self.tower.name.clone(),
            colors: self.global_colors.clone(),
            preset: next[0].preset.clone(),
            members: next.clone(),
            active_member: 0,
            combination_gap: 0.,
        };
        candidate.validate(&self.catalog)?;
        self.record_workflow();
        self.tower.clear_reorder();
        self.members = next;
        let active = self
            .members
            .iter()
            .position(|m| m.placement.level == level)
            .unwrap_or(0);
        self.restore_member(active);
        self.combination_view = true;
        self.combination_gap = 0.;
        self.combination_move_source = None;
        self.combination_spec = None;
        self.member_selection.clear();
        self.combination_view_state = None;
        self.tower.open_layer = Some(level);
        self.tower.error.clear();
        self.assembly(ctx);
        Ok(())
    }
    fn tower_apply_project(
        &mut self,
        mut project: Project,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        project
            .saved
            .prepare_restore(&self.catalog, &self.export_options.plate)?;
        self.tower.clear_reorder();
        self.tower.name = project.name;
        self.tower.rename_draft = None;
        self.tower.color_edit = None;
        self.tower.open_color = None;
        self.tower.reveal_colors = false;
        self.restore_project(project.saved);
        self.tower.open_layer = Some(self.members[self.active_member].placement.level);
        self.tower.print_list = false;
        self.tab = 0;
        self.tower.scroll_to_top = true;
        self.preview_modes.mode = super::preview_modes::Mode::Assembly;
        self.assembly(ctx);
        Ok(())
    }
    pub(super) fn tower_capture(&mut self) {
        if self.members.is_empty() {
            return;
        }
        let saved = self.tower_snapshot();
        if saved.validate(&self.catalog).is_err() {
            return;
        }
        let project = Project {
            name: self.tower.name.clone(),
            saved,
        };
        if let Some(current) = self.tower.projects.get_mut(self.tower.current) {
            *current = project;
        } else {
            self.tower.current = self.tower.projects.len();
            self.tower.projects.push(project);
        }
    }
    fn tower_new(&mut self, ctx: &egui::Context) {
        self.tower_capture();
        let mut p = fresh_like(&self.member_snapshot());
        p.selection = Selection::default();
        p.assembly_kind = AssemblyKind::Drawer;
        if let Err(e) = configure(&mut p, &self.catalog, false) {
            self.tower.error = e;
            return;
        }
        let member = Member {
            name: "第 01 层 · S3 抽屉".into(),
            placement: Default::default(),
            preset: p.clone(),
            exterior: Default::default(),
        };
        let name = format!("工作台抽屉柜 {}", self.tower.projects.len() + 1);
        let project = Project {
            name: name.clone(),
            saved: CombinationPreset {
                project_name: name,
                colors: self.global_colors.clone(),
                preset: p,
                members: vec![member],
                active_member: 0,
                combination_gap: 0.,
            },
        };
        if let Err(e) = self.tower_add_project(project, ctx) {
            self.tower.error = e;
        }
    }
    pub(super) fn tower_from_recipe(
        &mut self,
        index: usize,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let recipe = super::recipes::RECIPES
            .get(index)
            .ok_or("Unknown combination preset")?;
        // Finish and validate the complete candidate before changing the session.
        let saved = recipe.build(&self.catalog, &self.global_colors, self.zh)?;
        self.tower_capture();
        let project = Project {
            name: saved.project_name.clone(),
            saved,
        };
        self.tower_add_project(project, ctx)
    }

    fn tower_duplicate_project(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let source = Project {
            name: self.tower.name.clone(),
            saved: self.tower_snapshot(),
        };
        self.tower_capture();
        let project = source.duplicate(&self.tower.projects, self.zh);
        self.tower_add_project(project, ctx)
    }

    fn tower_add_project(&mut self, project: Project, ctx: &egui::Context) -> Result<(), String> {
        self.tower_apply_project(project.clone(), ctx)?;
        self.tower.current = self.tower.projects.len();
        self.tower.projects.push(project);
        Ok(())
    }
    pub(super) fn tower_load(&mut self, ctx: &egui::Context) {
        if self.smoke {
            self.tower.loaded = true;
            return;
        }
        let path = self.home.join("state/projects.toml");
        let (tx, rx) = mpsc::channel();
        self.tower.read = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = match std::fs::read_to_string(path) {
                Ok(text) => toml::from_str(&text).map_err(|e| e.to_string()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Projects::default()),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub(super) fn tower_tick(&mut self, ctx: &egui::Context) {
        if self.smoke {
            return;
        }
        if let Some(result) = self.tower.read.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.tower.read = None;
            self.tower.loaded = true;
            match result {
                Ok(saved) => {
                    self.tower.current = saved.current.min(saved.items.len().saturating_sub(1));
                    self.tower.projects = saved.items;
                    self.tower.pending_restore = !self.tower.projects.is_empty();
                }
                Err(e) => {
                    self.tower.error = format!("本地项目读取失败：{e}");
                    self.tower.loaded = false;
                }
            }
        }
        if !self.tower.loaded || self.catalog.root.as_os_str().is_empty() || self.members.is_empty()
        {
            return;
        }
        if self.tower.pending_restore {
            if self.busy || self.preview.has_pending() {
                return;
            }
            self.tower.pending_restore = false;
            let project = self.tower.projects[self.tower.current].clone();
            if let Err(e) = self.tower_apply_project(project, ctx) {
                self.tower.error = e;
                self.tower.loaded = false;
                return;
            }
        }
        self.tower_capture();
        let data = Projects {
            current: self.tower.current,
            items: self.tower.projects.clone(),
        };
        let key = toml::to_string(&data).unwrap_or_default().into_bytes();
        if key != self.tower.key {
            self.tower.key = key;
            self.tower.due = Some(Instant::now() + Duration::from_millis(600));
        }
        if let Some(result) = self.tower.write.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.tower.write = None;
            if let Err(e) = result {
                self.tower.error = format!("本地项目保存失败：{e}");
            }
        }
        if self.tower.write.is_none()
            && self.tower.due.is_some_and(|due| Instant::now() >= due)
            && self.tower.key != self.tower.saved_key
        {
            self.tower.due = None;
            self.tower.saved_key = self.tower.key.clone();
            let bytes = self.tower.key.clone();
            let folder = self.home.join("state");
            let (tx, rx) = mpsc::channel();
            self.tower.write = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = persist_projects(&folder, &bytes);
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
        if self.tower.due.is_some() || self.tower.write.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

#[derive(Clone)]
pub(super) enum Action {
    Resize(String, u8),
    Row(u32, String, bool),
    Duplicate(u32),
    Delete(u32),
    Move(u32, u32),
    Add(AssemblyKind),
    Kind(usize, AssemblyKind),
    Pattern(usize, Vec<String>),
    Edit(usize, Option<usize>),
    Accessories(usize),
    Roof(usize, crate::catalog::exterior::Roof),
    RoofLayout(roofs::Layout),
    TopHandle(usize, bool),
    Base(usize, crate::catalog::exterior::Base),
}
impl App {
    pub(super) fn tower_action(
        &mut self,
        action: Action,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if !matches!(action, Action::Edit(..)) {
            self.close_inline_editor(ctx);
        }
        self.commit_member();
        let reset = self.tower.reset_inserts;
        match action {
            Action::Edit(i, d) => {
                self.tower_edit_slot(i, d, ctx);
                return Ok(());
            }
            Action::Accessories(i) => {
                self.select_member_in_combination(i, ctx);
                self.open_exterior();
                return Ok(());
            }
            _ => {}
        }
        let boundaries = matches!(
            action,
            Action::Add(_) | Action::Duplicate(_) | Action::Delete(_) | Action::Move(..)
        );
        let top_kind_changed = matches!(action, Action::Kind(..));
        let mut next = self.members.clone();
        let level;
        let mut reorder = None;
        let mut preview_reorder = None;
        match action {
            Action::Resize(width, depth) => {
                next = resize(&next, &self.catalog, &width, depth, reset)?;
                level = self.members[self.active_member].placement.level;
            }
            Action::Row(row, height, split) => {
                next = set_row(&next, &self.catalog, row, &height, split, reset)?;
                level = row;
            }
            Action::Duplicate(row) => {
                let copies: Vec<_> = next
                    .iter()
                    .filter(|m| m.placement.level == row)
                    .cloned()
                    .collect();
                for m in &mut next {
                    if m.placement.level > row {
                        m.placement.level += 1;
                    }
                }
                for mut m in copies {
                    m.placement.level = row + 1;
                    m.exterior.base = Default::default();
                    m.exterior.roof = Default::default();
                    m.exterior.top_handle = false;
                    m.exterior.pull_handle = false;
                    next.push(m);
                }
                level = row + 1;
            }
            Action::Delete(row) => {
                if rows(&next)?.len() <= 1 {
                    return Err("至少保留一层 / Keep at least one layer".into());
                }
                next.retain(|m| m.placement.level != row);
                for m in &mut next {
                    if m.placement.level > row {
                        m.placement.level -= 1;
                    }
                }
                level = row.saturating_sub(1);
            }
            Action::Move(from, to) => {
                if from != to && self.scene_key == "combination" {
                    let source_order = self
                        .pending_layer_reorder
                        .as_ref()
                        .map(|pending| &pending.sources);
                    if source_order.is_some()
                        || self
                            .scene
                            .member_placements
                            .iter()
                            .zip(&self.members)
                            .all(|(placement, member)| *placement == member.placement)
                            && self.scene.member_placements.len() == self.members.len()
                    {
                        let mut order: Vec<_> = (0..next.len()).collect();
                        order.sort_by_key(|&index| {
                            let p = next[index].placement;
                            (moved_level(p.level, from, to), p.column)
                        });
                        preview_reorder = Some(super::layer_reorder::Pending {
                            revision: self.preview.revision(),
                            sources: order
                                .iter()
                                .map(|&index| source_order.map_or(index, |sources| sources[index]))
                                .collect(),
                            lifted: order
                                .iter()
                                .map(|&index| next[index].placement.level == from)
                                .collect(),
                        });
                    }
                }
                for m in &mut next {
                    m.placement.level = moved_level(m.placement.level, from, to);
                }
                if from != to && !self.tower.card_positions.is_empty() {
                    reorder = Some(ReorderMotion {
                        start: ctx.input(|input| input.time),
                        previous_y: self
                            .tower
                            .card_positions
                            .iter()
                            .map(|(&level, &y)| (moved_level(level, from, to), y))
                            .collect(),
                    });
                }
                level = to;
            }
            Action::Add(kind) => {
                let all = rows(&next)?;
                let top = all.last().ok_or("先打开模型库 / Open library first")?;
                let mut additions: Vec<_> = top.members.iter().map(|i| next[*i].clone()).collect();
                for m in &mut additions {
                    m.placement.level = top.level + 1;
                    m.preset = fresh_like(&m.preset);
                    m.preset.assembly_kind = kind;
                    m.exterior = Default::default();
                    configure(&mut m.preset, &self.catalog, false)?;
                }
                level = top.level + 1;
                next.extend(additions);
            }
            Action::Kind(i, kind) => {
                let p = &mut next[i].preset;
                if kind != p.assembly_kind {
                    if !p.drawers.is_empty() {
                        let mut first = p.drawers[0].clone();
                        first.selection = p.selection.clone();
                        *p = first;
                    }
                    p.drawers.clear();
                    p.assembly_kind = kind;
                    configure(p, &self.catalog, reset)?;
                }
                level = next[i].placement.level;
            }
            Action::Pattern(i, heights) => {
                next[i].preset = drawer_pattern(
                    &next[i].preset,
                    &self.catalog,
                    &heights.iter().map(String::as_str).collect::<Vec<_>>(),
                    reset,
                )?;
                level = next[i].placement.level;
            }
            Action::Roof(i, roof) => {
                next[i].exterior.roof = roof;
                if roof != crate::catalog::exterior::Roof::Grid {
                    next[i].exterior.top_handle = false;
                }
                roofs::set_layout(&mut next, roofs::layout(&self.members)?)?;
                level = next[i].placement.level;
            }
            Action::RoofLayout(layout) => {
                roofs::set_layout(&mut next, layout)?;
                level = rows(&next)?.last().ok_or("Missing top layer")?.level;
            }
            Action::TopHandle(i, enabled) => {
                next[i].exterior.top_handle = enabled;
                if enabled {
                    next[i].exterior.roof = crate::catalog::exterior::Roof::Grid;
                }
                level = next[i].placement.level;
            }
            Action::Base(i, base) => {
                next[i].exterior.base = base;
                level = next[i].placement.level;
            }
            _ => return Ok(()),
        }
        // Boundary accessories belong to the tower, so move them with its ends.
        if boundaries {
            let old_rows = rows(&self.members)?;
            let last = next.iter().map(|m| m.placement.level).max().unwrap_or(0);
            let old_top = old_rows.last().unwrap();
            let old_bottom = &old_rows[0];
            for m in &mut next {
                m.exterior.base = Default::default();
                m.exterior.roof = Default::default();
                m.exterior.roof_width = None;
                m.exterior.top_handle = false;
                m.exterior.pull_handle = false;
            }
            for m in &mut next {
                let source = if m.placement.level == 0 {
                    Some(
                        &self.members[*old_bottom
                            .members
                            .get(m.placement.column as usize)
                            .unwrap_or(&old_bottom.members[0])],
                    )
                } else {
                    None
                };
                if let Some(source) = source {
                    m.exterior.base = source.exterior.base;
                }
                if m.placement.level == last {
                    let source = &self.members[*old_top
                        .members
                        .get(m.placement.column as usize)
                        .unwrap_or(&old_top.members[0])];
                    m.exterior.roof = source.exterior.roof;
                    m.exterior.top_handle = source.exterior.top_handle;
                    m.exterior.pull_handle = source.exterior.pull_handle;
                    if m.exterior.pull_handle {
                        m.exterior.backbone = true;
                        m.exterior.pull_extension = source.exterior.pull_extension;
                    }
                }
            }
        }
        if boundaries || top_kind_changed {
            roofs::reconcile(&self.members, &mut next)?;
        }
        let explosion = self.explosion;
        let dimensions = self.dimensions;
        self.tower_apply(next, level, ctx)?;
        if let Some(mut pending) = preview_reorder
            && !self.preview.is_current(pending.revision)
        {
            pending.revision = self.preview.revision();
            self.scene_key = "combination".into();
            self.explosion = explosion;
            self.dimensions = dimensions;
            self.pending_layer_reorder = Some(pending);
            self.preview.expedite(Instant::now());
        }
        self.tower.reorder = reorder;
        Ok(())
    }
    pub(super) fn tower_project_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        let mut action = None;
        ui.horizontal(|ui| {
            let controls_width = if z { 165. } else { 190. };
            let title_width = (ui.available_width() - controls_width).clamp(96., 440.);
            ui.scope(|ui| {
                ui.set_max_width(title_width);
                self.tower.title_ui(ui, z);
            });
            ui.add_space(6.);
            let projects = ui.menu_button(t(z, "项目", "Projects"), |ui| {
                ui.set_width(274.);
                ui.small(
                    RichText::new(format!(
                        "{} · {}",
                        t(z, "项目", "Projects"),
                        self.tower.projects.len()
                    ))
                    .weak(),
                );
                egui::ScrollArea::vertical()
                    .max_height(250.)
                    .show(ui, |ui| {
                        for (index, project) in self.tower.projects.iter().enumerate() {
                            let current = index == self.tower.current;
                            let name = if current {
                                &self.tower.name
                            } else {
                                &project.name
                            };
                            let summary = project.summary(z);
                            let row = format!("{name}\n{summary}");
                            if ui
                                .add_sized(
                                    [ui.available_width(), 47.],
                                    crate::ui_style::choice_button(current, row).wrap(),
                                )
                                .on_hover_text(name)
                                .clicked()
                            {
                                if !current {
                                    action = Some(ProjectAction::Switch(index));
                                }
                                ui.close();
                            }
                        }
                    });
                ui.separator();
                ui.menu_button(t(z, "＋ 新建", "＋ New"), |ui| {
                    self.new_project_menu(ui, &mut action)
                });
                if ui.button(t(z, "打开文件…", "Open file…")).clicked() {
                    action = Some(ProjectAction::Open);
                    ui.close();
                }
                if ui.button(t(z, "保存为文件…", "Save to file…")).clicked() {
                    action = Some(ProjectAction::Save);
                    ui.close();
                }
            });
            let create = ui.menu_button(t(z, "＋ 新建", "＋ New"), |ui| {
                self.new_project_menu(ui, &mut action)
            });
            if self.smoke && std::env::args().any(|a| a == "--smoke-project-menu") {
                let button = if self.smoke_stage == 0 {
                    create.response
                } else {
                    projects.response
                };
                let id = egui::Popup::default_response_id(&button);
                if !egui::Popup::is_id_open(ctx, id) {
                    egui::Popup::open_id(ctx, id);
                    ctx.request_repaint();
                }
            }
        });

        let Some(action) = action else {
            return;
        };
        egui::Popup::close_all(ctx);
        self.tower.finish_rename();
        let result = match action {
            ProjectAction::Switch(index) => {
                self.tower_capture();
                let project = self.tower.projects[index].clone();
                self.tower_apply_project(project, ctx)
                    .map(|()| self.tower.current = index)
            }
            ProjectAction::Starter => {
                self.tower_new(ctx);
                Ok(())
            }
            ProjectAction::Recipe(index) => self.tower_from_recipe(index, ctx),
            ProjectAction::Duplicate => self.tower_duplicate_project(ctx),
            ProjectAction::Open => {
                self.tower_capture();
                self.load_preset(ctx);
                Ok(())
            }
            ProjectAction::Save => {
                self.save_preset();
                Ok(())
            }
        };
        if let Err(error) = result {
            self.tower.error = error;
        }
    }

    fn new_project_menu(&self, ui: &mut egui::Ui, action: &mut Option<ProjectAction>) {
        use super::recipes::{Group, RECIPES};

        let z = self.zh;
        let ready = !self.busy && !self.catalog.root.as_os_str().is_empty();
        ui.set_width(260.);
        ui.small(RichText::new(t(z, "新建项目", "New project")).weak());
        project_menu_item(
            ui,
            ready,
            t(z, "从空白开始", "Start from scratch"),
            ProjectAction::Starter,
            action,
        )
        .on_hover_text(t(z, "从一层 S3 抽屉开始", "Start with one S3 drawer"));
        ui.separator();
        let height = (ui.ctx().content_rect().height() - 210.).clamp(180., 505.);
        egui::ScrollArea::vertical()
            .id_salt("new-project-recipes")
            .max_height(height)
            .show(ui, |ui| {
                for group in Group::ALL {
                    ui.add_space(4.);
                    ui.small(RichText::new(group.title(z)).weak());
                    for (index, recipe) in RECIPES
                        .iter()
                        .enumerate()
                        .filter(|(_, recipe)| recipe.group == group)
                    {
                        project_menu_item(
                            ui,
                            ready,
                            recipe.name(z),
                            ProjectAction::Recipe(index),
                            action,
                        )
                        .on_hover_text(recipe.description(z));
                    }
                }
            });
        ui.separator();
        project_menu_item(
            ui,
            ready && !self.members.is_empty(),
            t(z, "复制当前项目", "Duplicate current project"),
            ProjectAction::Duplicate,
            action,
        );
        project_menu_item(
            ui,
            ready,
            t(z, "打开项目文件…", "Open project file…"),
            ProjectAction::Open,
            action,
        );
    }

    pub(super) fn tower_preview_color_menu(&mut self, ui: &mut egui::Ui) {
        colors::preview_menu(self, ui);
    }

    pub(super) fn reveal_tower_colors(&mut self, ctx: &egui::Context) {
        if !self.combination_view || self.inline_edit.open {
            self.show_combination(ctx);
        }
        if self.gridfinity_open {
            self.close_bin_editor();
        }
        self.tab = 0;
        self.tower.print_list = false;
        self.tower.reveal_colors = true;
        ctx.request_repaint();
    }

    pub(super) fn tower_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        self.commit_member();
        self.tower_footer_panel(ui, ctx);
        let mut action = None;
        let reset_scroll = std::mem::take(&mut self.tower.scroll_to_top);
        let scroll = egui::ScrollArea::vertical()
            .id_salt(("tower-config-scroll", self.tower.print_list))
            .auto_shrink([false, false]);
        let scroll = if reset_scroll {
            scroll.vertical_scroll_offset(0.)
        } else {
            scroll
        };
        scroll.show(ui, |ui| {
            if self.members.is_empty() {
                ui.add_space(16.);
                ui.strong(t(z, "开始你的第一个项目", "Start your first project"));
                ui.label(RichText::new(t(z, "先关联本地模型库，再选择尺寸并添加抽屉层。", "Link a local model library, choose a size and add layers.")).weak());
                return;
            }
            if self.tower.print_list { self.tower_print_list(ui, ctx); return; }
            let all = match rows(&self.members) {
                Ok(rows) => rows,
                Err(error) => {
                    self.tower.scroll_to_layer = None;
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    ui.small(t(z, "原方案的标签、内部配置和装配位置完整保留。可新建层式项目，或选择原成员编辑。", "Original labels, inserts and placements are preserved. Start a layer project or edit an existing member."));
                    for (i, m) in self.members.iter().enumerate() {
                        if ui.button(&m.name).clicked() { action = Some(Action::Edit(i, None)); }
                    }
                    colors::show(self, ui);
                    return;
                }
            };
            let Some(first) = all.first() else { return; };
            if self.smoke && std::env::args().any(|a| a == "--smoke-color-groups") {
                for group in ["footprint", "layers", "modifications"] { layers::set_group(ui, group, false); }
                layers::set_group(ui, "colors", true);
            }
            let build_panel_smoke = self.smoke && std::env::args().any(|arg| arg == "--smoke-build-panel");
            if build_panel_smoke && self.smoke_stage == 4 {
                layers::set_group(ui, "footprint", false);
                layers::set_group(ui, "layers", false);
                layers::set_group(ui, "modifications", true);
                layers::set_group(ui, "colors", true);
            }
            let footprint = format!("{}{}", first.width, first.depth);
            if layers::group(ui, "footprint", t(z, "占地", "Footprint"), true, |ui| {
                ui.label(RichText::new(&footprint).weak());
            }) {
                let mut width = first.width.as_str();
                let mut depth = first.depth;
                layers::choice_row(ui, t(z, "宽度", "Width"), 160., &mut width, &[("S", "S"), ("M", "M"), ("L", "L"), ("XL", "XL")]);
                layers::choice_row(ui, t(z, "深度", "Depth"), 192., &mut depth, &[(3, "3"), (4, "4"), (5, "5"), (6, "6"), (7, "7")]);
                if width != first.width || depth != first.depth {
                    action = Some(Action::Resize(width.into(), depth));
                }
                let width_mm = crate::constraints::stacking::width(&first.width).unwrap_or(0) as f32 / 10.;
                ui.horizontal_wrapped(|ui| {
                    ui.small(RichText::new(if z {
                        format!("标称宽 {width_mm:.1} mm · 深 {} 格", first.depth)
                    } else {
                        format!("Nominal width {width_mm:.1} mm · {} depth units", first.depth)
                    }).weak());
                    ui.menu_button(t(z, "选项", "Options"), |ui| {
                        ui.checkbox(&mut self.tower.reset_inserts, t(z, "改规格时重置内部配置（保留标牌）", "Reset inserts on size changes (keep labels)"));
                    });
                });
                ui.add_space(6.);
            }
            ui.separator();
            if let Some(open) = self.tower.layer_group_request.take() {
                layers::set_group(ui, "layers", open);
            }
            if layers::group(ui, "layers", t(z, "层", "Layers"), true, |ui| {
                ui.label(RichText::new(format!("{} {}", all.len(), t(z, "层", "layers"))).weak());
            }) {
                self.tower_layers(ui, ctx, &all, &mut action);
            }
            ui.separator();
            let modifications = self.modification_summary();
            let show_empty_card = build_panel_smoke && self.smoke_stage == 1;
            if layers::group(ui, "modifications", t(z, "改装", "Modifications"), show_empty_card, |ui| {
                ui.add(egui::Label::new(RichText::new(modifications).weak()).truncate());
            }) {
                modifications::show(self, ui, ctx, &mut action);
            }
            ui.separator();
            colors::show(self, ui);
            ui.separator();
            self.workflow_undo_ui(ui,ctx);
            ui.add_space(12.);
        });
        if let Some(action) = action
            && let Err(e) = self.tower_action(action, ctx)
        {
            self.tower.error = e;
        }
    }
    fn tower_layers(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        all: &[crate::constraints::tower::Row],
        action: &mut Option<Action>,
    ) {
        let z = self.zh;
        let first = &all[0];
        let scroll_to_layer = self.tower.scroll_to_layer.take();
        roofs::show(self, ui, all.last().unwrap(), action);
        let origin_y = ui.cursor().min.y;
        let now = ctx.input(|input| input.time);
        let mut reorder = self.tower.reorder.take();
        let remaining = reorder.as_ref().map_or(0., |motion| motion.remaining(now));
        if remaining > 0. {
            ctx.request_repaint();
        } else {
            reorder = None;
        }
        self.tower.card_positions.clear();
        for row in all.iter().rev() {
            let expanded = self.tower.open_layer == Some(row.level);
            let all_boxes = row
                .members
                .iter()
                .all(|&i| self.members[i].preset.assembly_kind == AssemblyKind::Sustainer);
            let kind = if row
                .members
                .iter()
                .all(|&i| self.members[i].preset.assembly_kind == AssemblyKind::Drawer)
            {
                t(z, "抽屉", "Drawer")
            } else if all_boxes {
                t(z, "箱子", "Box")
            } else {
                t(z, "混合", "Mixed")
            };
            let size = row
                .members
                .iter()
                .map(|&i| format!("{}{}", self.members[i].preset.selection.width, row.depth))
                .collect::<Vec<_>>()
                .join(" + ");
            let height_mm =
                crate::constraints::AssemblyRules::installation_height(&row.height, all_boxes)
                    .unwrap_or_default();
            let destination = ui.available_rect_before_wrap();
            let y = destination.min.y - origin_y;
            let previous = reorder
                .as_ref()
                .and_then(|motion| motion.previous_y.get(&row.level))
                .copied()
                .unwrap_or(y);
            let offset = egui::vec2(0., (previous - y) * remaining);
            let mut card_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("tower-card-slot", row.level))
                    .max_rect(destination.translate(offset)),
            );
            let card = layers::layer(
                &mut card_ui,
                layers::Header {
                    level: row.level,
                    kind,
                    size: &size,
                    height: &row.height,
                    height_mm,
                    can_delete: all.len() > 1,
                    zh: z,
                },
                expanded,
                |ui| {
                    let mut height = row.height.clone();
                    let mut split = row.members.len() == 2;
                    if row.width != "XL" || split {
                        layers::choice_row(
                            ui,
                            t(z, "高度", "Height"),
                            126.,
                            &mut height,
                            &[
                                ("1F".into(), "1F"),
                                ("2F".into(), "2F"),
                                ("4F".into(), "4F"),
                            ],
                        );
                    } else {
                        ui.label(t(z, "高度 · 1F", "Height · 1F"));
                    }
                    if matches!(row.width.as_str(), "L" | "XL") {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(t(z, "组合方式", "Arrangement"));
                            let whole = ui.add_enabled_ui(row.width != "XL", |ui| {
                                ui.selectable_value(
                                    &mut split,
                                    false,
                                    format!("{} {}", t(z, "整块", "Whole"), row.width),
                                );
                            });
                            if row.width == "XL" {
                                whole.response.on_hover_text(t(
                                    z,
                                    "整块 XL 安装面待校准；可用 S＋M。",
                                    "Full XL seating datums need calibration; use S+M.",
                                ));
                            }
                            ui.selectable_value(
                                &mut split,
                                true,
                                if row.width == "L" { "S + S" } else { "S + M" },
                            );
                        });
                    }
                    if height != row.height || split != (row.members.len() == 2) {
                        *action = Some(Action::Row(row.level, height, split));
                    }
                    for (column, &index) in row.members.iter().enumerate() {
                        ui.push_id(index, |ui| {
                            if row.members.len() > 1 {
                                ui.small(if column == 0 {
                                    t(z, "左侧", "Left")
                                } else {
                                    t(z, "右侧", "Right")
                                });
                            }
                            self.tower_cell(ui, index, action);
                        });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        self.layer_motion_ui(ui, &row.members, ctx);
                        let checked_count = row
                            .members
                            .iter()
                            .filter(|i| self.member_selection.contains(i))
                            .count();
                        let mut checked = checked_count == row.members.len();
                        let mixed = checked_count > 0 && !checked;
                        if ui
                            .add(
                                egui::Checkbox::new(
                                    &mut checked,
                                    t(z, "勾选导出", "Include in export"),
                                )
                                .indeterminate(mixed),
                            )
                            .changed()
                        {
                            for &i in &row.members {
                                if checked {
                                    self.member_selection.insert(i);
                                } else {
                                    self.member_selection.remove(&i);
                                }
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if layers::icon_button(
                                ui,
                                layers::Icon::Down,
                                row.level > 0,
                                t(z, "下移", "Move down"),
                            )
                            .clicked()
                                && row.level > 0
                            {
                                *action = Some(Action::Move(row.level, row.level - 1));
                            }
                            if layers::icon_button(
                                ui,
                                layers::Icon::Up,
                                row.level + 1 < all.len() as u32,
                                t(z, "上移", "Move up"),
                            )
                            .clicked()
                                && row.level + 1 < all.len() as u32
                            {
                                *action = Some(Action::Move(row.level, row.level + 1));
                            }
                        });
                    });
                },
            );
            let visual_rect = card.response.rect;
            self.tower
                .card_positions
                .insert(row.level, visual_rect.min.y - origin_y);
            ui.advance_cursor_after_rect(card_ui.min_rect().translate(-offset));
            if scroll_to_layer == Some(row.level) {
                let mut target = visual_rect.translate(-offset);
                let visible_height = (ui.clip_rect().height() - 20.).max(40.);
                target.max.y = target.max.y.min(target.min.y + visible_height);
                ui.scroll_to_rect(target, None);
            }
            if let Some(open) = card.open {
                self.tower.open_layer = if open { Some(row.level) } else { None };
                if open && !expanded {
                    self.select_member_in_combination(row.members[0], ctx);
                }
            }
            if card.action.is_some() {
                *action = card.action;
            }
            if let Some(from) = card.response.dnd_release_payload::<u32>()
                && *from != row.level
            {
                *action = Some(Action::Move(*from, row.level));
            }
        }
        self.tower.reorder = reorder;
        ui.horizontal_wrapped(|ui| {
            if ui.button(t(z, "＋ 抽屉层", "＋ Drawer layer")).clicked() {
                *action = Some(Action::Add(AssemblyKind::Drawer));
            }
            if ui.button(t(z, "＋ 箱子层", "＋ Box layer")).clicked() {
                *action = Some(Action::Add(AssemblyKind::Sustainer));
            }
        });
        ui.add_space(8.);
        let base_empty = first
            .members
            .iter()
            .all(|&i| self.members[i].exterior.base == crate::catalog::exterior::Base::None);
        layers::section(
            ui,
            "tower-bases",
            if base_empty {
                t(z, "无底部", "No base")
            } else {
                t(z, "底部配置", "Base")
            },
            &format!("{}{}", first.width, first.depth),
            base_empty,
            false,
            |ui| {
                ui.set_min_width(ui.available_width());
                for (column, &i) in first.members.iter().enumerate() {
                    ui.push_id(("tower-base", i), |ui| {
                        if first.members.len() > 1 {
                            ui.small(format!(
                                "{} · {}",
                                if column == 0 {
                                    t(z, "左侧", "Left")
                                } else {
                                    t(z, "右侧", "Right")
                                },
                                self.members[i].preset.selection.width
                            ));
                        }
                        let mut base = self.members[i].exterior.base;
                        egui::ComboBox::from_id_salt("base")
                            .width(ui.available_width())
                            .selected_text(base_name(base, z))
                            .show_ui(ui, |ui| {
                                for value in [
                                    crate::catalog::exterior::Base::None,
                                    crate::catalog::exterior::Base::Plain,
                                    crate::catalog::exterior::Base::Trolley,
                                ] {
                                    ui.selectable_value(&mut base, value, base_name(value, z));
                                }
                            });
                        if base != self.members[i].exterior.base {
                            *action = Some(Action::Base(i, base));
                        }
                    });
                }
            },
        );
        ui.add_space(6.);
    }

    fn modification_summary(&self) -> String {
        use crate::catalog::exterior::Board;
        let z = self.zh;
        let has = |check: fn(&crate::catalog::exterior::Exterior) -> bool| {
            self.members.iter().any(|member| check(&member.exterior))
        };
        let summary = [
            (has(|e| e.backbone), t(z, "背板", "Backbone")),
            (
                has(|e| e.stationary_handle || e.pull_handle),
                t(z, "把手", "Handles"),
            ),
            (has(|e| e.board != Board::None), t(z, "洞洞板", "Pegboard")),
            (has(|e| e.wall_mount), t(z, "挂墙", "Wall mount")),
        ]
        .into_iter()
        .filter_map(|(enabled, name)| enabled.then_some(name))
        .collect::<Vec<_>>()
        .join(" · ");
        if summary.is_empty() {
            t(z, "无", "None").into()
        } else {
            summary
        }
    }

    fn tower_cell(&mut self, ui: &mut egui::Ui, index: usize, action: &mut Option<Action>) {
        let z = self.zh;
        let m = &self.members[index];
        let p = &m.preset;
        ui.vertical(|ui| {
            if p.assembly_kind == AssemblyKind::Drawer && p.selection.height != "1F" {
                let parts = if p.drawers.is_empty() {
                    vec![p.selection.height.clone()]
                } else {
                    p.drawers
                        .iter()
                        .map(|d| d.selection.height.clone())
                        .collect()
                };
                let mut choice = parts.join(" / ");
                let before = choice.clone();
                egui::ComboBox::from_id_salt("drawer-layout")
                    .width(132.)
                    .selected_text(&choice)
                    .show_ui(ui, |ui| {
                        let mut choices = vec![vec![p.selection.height.clone()]];
                        if p.selection.height == "2F" {
                            choices.push(vec!["1F".into(), "1F".into()]);
                        }
                        if p.selection.height == "4F" {
                            choices.extend([
                                vec!["2F".into(), "2F".into()],
                                vec!["1F".into(); 4],
                                vec!["2F".into(), "1F".into(), "1F".into()],
                                vec!["1F".into(), "2F".into(), "1F".into()],
                                vec!["1F".into(), "1F".into(), "2F".into()],
                            ]);
                        }
                        for pattern in choices {
                            let value = pattern.join(" / ");
                            ui.selectable_value(&mut choice, value.clone(), value);
                        }
                    });
                if choice != before {
                    *action = Some(Action::Pattern(
                        index,
                        choice.split(" / ").map(str::to_owned).collect(),
                    ));
                }
            }
            let cells: Vec<_> = if p.drawers.is_empty() {
                vec![(None, p)]
            } else {
                p.drawers
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (Some(i), p))
                    .collect()
            };
            for (drawer, p) in cells {
                ui.push_id(drawer, |ui| {
                    let interior = if p
                        .bom
                        .iter()
                        .any(|b| b.enabled && b.path.contains("/Gridfinity Baseplate/"))
                    {
                        t(z, "平底 · Gridfinity", "Flat · Gridfinity")
                    } else if p.divider_layout.enabled || !p.sustainer_dividers.items.is_empty() {
                        t(z, "分隔板", "Dividers")
                    } else {
                        t(z, "平底", "Flat")
                    };
                    let label = if p.label.active() {
                        p.label.text.replace('\n', " / ")
                    } else {
                        format!(
                            "{}{}-{}",
                            p.selection.width, p.selection.depth, p.selection.height
                        )
                    };
                    let selected = self.inline_edit.open
                        && self.inline_edit.target
                            == Some(crate::scene::Target {
                                member: index,
                                drawer,
                            });
                    let response = layers::surface(ui, "interior-summary", selected, false, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{}{}",
                                    p.selection.width, p.selection.depth
                                ))
                                .weak(),
                            );
                            ui.strong(&p.selection.height);
                            if let Some(d) = drawer {
                                ui.small(
                                    RichText::new(format!("{} {}", t(z, "抽屉", "Drawer"), d + 1))
                                        .weak(),
                                );
                            }
                        });
                        let kind = if p.assembly_kind == AssemblyKind::Drawer {
                            t(z, "抽屉", "Drawer")
                        } else {
                            t(z, "翻盖箱", "Box")
                        };
                        ui.label(RichText::new(format!("{kind} · {interior}")).weak());
                        ui.add(
                            egui::Label::new(RichText::new(format!("◇ {label}")).weak()).truncate(),
                        );
                    });
                    if response
                        .response
                        .on_hover_text(t(z, "编辑内部和标签", "Edit interior and label"))
                        .clicked()
                    {
                        *action = Some(Action::Edit(index, drawer));
                    }
                });
            }
        });
    }
    pub(super) fn tower_footer_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("tower-summary")
            .frame(crate::ui_style::panel_frame(
                ui.style(),
                egui::Margin {
                    top: 8,
                    ..Default::default()
                },
            ))
            .show_inside(ui, |ui| {
                ui.separator();
                self.tower_footer(ui, ctx);
            });
    }
    fn tower_print_items(&self) -> Vec<Bom> {
        self.members
            .iter()
            .flat_map(|m| {
                let mut items = m.preset.printing_bom();
                if let Ok(extras) = self.catalog.exterior_components(
                    &m.preset.selection,
                    &m.exterior,
                    m.preset.assembly_kind == AssemblyKind::Sustainer,
                ) {
                    items.extend(extras.into_iter().map(|c| c.bom));
                }
                items
            })
            .filter(|b| b.enabled)
            .collect()
    }

    pub(super) fn tower_file_count(&self) -> usize {
        self.tower_print_items()
            .into_iter()
            .map(|item| item.path)
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    fn tower_footer(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let z = self.zh;
        if !self.tower.error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.tower.error);
        }
        let items = self.tower_print_items();
        let files: std::collections::HashSet<_> = items.iter().map(|b| b.path.as_str()).collect();
        let copies: u32 = items.iter().map(|b| b.qty).sum();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.small(t(z, "总高", "Height"));
                if self.scene_key == "combination" && !self.scene.member_bounds.is_empty() {
                    let lo = self
                        .scene
                        .member_bounds
                        .iter()
                        .map(|(lo, _)| lo.y)
                        .fold(f32::INFINITY, f32::min);
                    let hi = self
                        .scene
                        .member_bounds
                        .iter()
                        .map(|(_, hi)| hi.y)
                        .fold(f32::NEG_INFINITY, f32::max);
                    ui.strong(format!("{:.1} mm", hi - lo));
                } else {
                    ui.strong("—");
                }
            });
            ui.vertical(|ui| {
                ui.small("STL");
                ui.strong(format!(
                    "{} {} / {} {}",
                    files.len(),
                    t(z, "种", "types"),
                    copies,
                    t(z, "份", "copies")
                ));
            });
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if self.tower.print_list {
                    t(z, "查看打印板", "Preview plates")
                } else {
                    t(z, "下一步：打印清单 →", "Next: print list →")
                })
                .clicked()
            {
                self.tab = 0;
                if self.tower.print_list {
                    self.preview_modes.mode = preview_modes::Mode::Plates;
                } else {
                    self.tower.print_list = true;
                }
            }
            if ui
                .add_enabled(
                    !items.is_empty(),
                    crate::ui_style::primary_button(t(z, "导出整个项目…", "Export project…")),
                )
                .clicked()
            {
                self.commit_member();
                self.export_scope = workflow::ExportScope::All;
                self.export_open = true;
            }
        });
        if self.busy || self.preview.has_pending() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.small(t(z, "预览更新中…", "Updating preview…"));
            });
        } else {
            ui.small(t(
                z,
                "装配位置为网格估算，接口与承载待实物核对。",
                "Mesh-based placement estimates; physical fit and load unverified.",
            ));
        }
    }
    fn tower_print_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        ui.heading(t(z, "打印清单", "Print list"));
        ui.small(t(
            z,
            "整个项目 · 自上而下核对。份数按源 STL 统计，标牌保留各自文字。",
            "Entire project · top to bottom. Copies count source STL files; labels keep their own text.",
        ));
        let mut next = self.members.clone();
        let mut changes = Vec::new();
        let mut edit_error = None;
        let mut accessories = None;
        let mut show_rows = |ui: &mut egui::Ui,
                             p: &mut Preset,
                             housing: Option<bool>,
                             target: crate::scene::Target| {
            let items = p
                .bom
                .iter_mut()
                .filter(|b| housing.is_none_or(|h| crate::catalog::tower::housing(&b.path) == h));
            let edits = print_list::rows(ui, items, &p.selection, z, None);
            if let Some((path, enabled, qty)) = edits.toggle {
                let inserts = (p.assembly_kind == AssemblyKind::Drawer)
                    .then_some((&mut p.divider_layout, &mut p.gridfinity));
                if let Err(e) =
                    print_list::set_accessory(&p.selection, &mut p.bom, inserts, path, enabled, qty)
                {
                    edit_error = Some(e);
                    return;
                }
            } else if !edits.changed {
                return;
            }
            match p.validate_member(&self.catalog) {
                Ok(()) => changes.push((target, p.clone())),
                Err(e) => edit_error = Some(e),
            }
        };
        let mut order: Vec<_> = (0..next.len()).collect();
        order.sort_by_key(|&i| {
            (
                std::cmp::Reverse(next[i].placement.level),
                next[i].placement.column,
            )
        });
        for index in order {
            let member = &mut next[index];
            ui.push_id(("print-member", index), |ui| {
                egui::CollapsingHeader::new(&member.name)
                    .default_open(index == self.active_member)
                    .show(ui, |ui| {
                        let stack = !member.preset.drawers.is_empty();
                        show_rows(
                            ui,
                            &mut member.preset,
                            stack.then_some(true),
                            crate::scene::Target {
                                member: index,
                                drawer: None,
                            },
                        );
                        for (i, p) in member.preset.drawers.iter_mut().enumerate() {
                            ui.collapsing(
                                format!("{} {} · {}", t(z, "抽屉", "Drawer"), i + 1, p.label.text),
                                |ui| {
                                    show_rows(
                                        ui,
                                        p,
                                        Some(false),
                                        crate::scene::Target {
                                            member: index,
                                            drawer: Some(i),
                                        },
                                    )
                                },
                            );
                        }
                        if let Ok(extras) = self.catalog.exterior_components(
                            &member.preset.selection,
                            &member.exterior,
                            member.preset.assembly_kind == AssemblyKind::Sustainer,
                        ) && !extras.is_empty()
                        {
                            ui.separator();
                            ui.small(t(
                                z,
                                "顶盖 / 底座 / 外部配件（随配置导出）",
                                "Covers / base / accessories (exported with configuration)",
                            ));
                            for extra in extras {
                                let b = extra.bom;
                                let name = crate::accessories::info(&b.path, z)
                                    .map_or_else(|| b.name().to_owned(), |m| m.name);
                                ui.label(format!("{name} × {}", b.qty));
                            }
                            if ui
                                .small_button(t(z, "修改配件…", "Edit accessories…"))
                                .clicked()
                            {
                                accessories = Some(index);
                            }
                        }
                    });
            });
        }
        if !changes.is_empty() {
            self.record_workflow();
            for (target, edited) in changes {
                print_list::apply_project_edit(&mut self.members, target, edited);
            }
            let active = crate::scene::Target {
                member: self.active_member,
                drawer: self.active_drawer,
            };
            if let Some(p) = inline_edit::target_preset(&self.members, active) {
                self.bom.clone_from(&p.bom);
                self.gridfinity.clone_from(&p.gridfinity);
                self.divider_layout.clone_from(&p.divider_layout);
            }
            self.tower.error.clear();
            self.assembly(ctx);
        }
        if let Some(e) = edit_error {
            self.tower.error = e;
        }
        if let Some(index) = accessories
            && let Err(error) = self.tower_action(Action::Accessories(index), ctx)
        {
            self.tower.error = error;
        }
    }
}
fn roof_name(value: crate::catalog::exterior::Roof, z: bool) -> &'static str {
    use crate::catalog::exterior::Roof;
    match value {
        Roof::None => t(z, "无附加顶盖", "No extra cover"),
        Roof::Table => t(z, "平顶", "Flat top"),
        Roof::Grid => t(z, "网格顶盖", "Grid top"),
        Roof::Bowl => t(z, "收纳托盘", "Tray top"),
        Roof::UnderTable => t(z, "桌下安装顶盖", "Under-table top"),
    }
}
fn base_name(value: crate::catalog::exterior::Base, z: bool) -> &'static str {
    use crate::catalog::exterior::Base;
    match value {
        Base::None => t(z, "无底座", "No base"),
        Base::Plain => t(z, "普通底座", "Plain base"),
        Base::Trolley => t(z, "双轮底座", "Trolley base"),
    }
}

impl App {
    pub(super) fn tower_imported_project(&mut self, name: String) {
        self.tower.rename_draft = None;
        self.tower.current = self.tower.projects.len();
        self.tower.name = name;
    }
}

impl App {
    pub(super) fn setup_tower_smoke(&mut self, ctx: &egui::Context) {
        if std::env::args().any(|a| a == "--smoke-color-groups") {
            if let Err(error) = self.setup_color_groups_smoke(ctx) {
                self.tower.error = error;
            }
            return;
        }
        self.tower.name = "工作台抽屉柜 · 层式配置".into();
        let base = self.member_snapshot();
        let mut members = Vec::new();
        for level in 0..3 {
            for (column, width) in ["S", "M"].iter().enumerate() {
                let mut p = fresh_like(&base);
                p.selection = Selection {
                    width: (*width).into(),
                    depth: 5,
                    height: if level == 2 { "1F" } else { "2F" }.into(),
                    hidden_key: false,
                };
                p.label.enabled = true;
                p.label.text = format!("{} {}", width, level + 1);
                let mut colors = crate::print_plate::Config {
                    preview_colors: true,
                    palette: [
                        [74, 104, 120],
                        [234, 151, 73],
                        [78, 156, 148],
                        [168, 184, 198],
                        [136, 109, 173],
                        [235, 235, 232],
                    ],
                    ..Default::default()
                };
                colors.set_part_slot(crate::label::PATH, 2);
                colors.set_text_slot(crate::label::PATH, 6);
                p.print_config = Some(colors);
                if let Err(e) = configure(&mut p, &self.catalog, false) {
                    self.tower.error = e;
                    return;
                }
                if level == 1 && column == 1 {
                    match drawer_pattern(&p, &self.catalog, &["1F", "1F"], false) {
                        Ok(mut split) => {
                            split.drawers[0].label.enabled = true;
                            split.drawers[0].label.text = "BIN A".into();
                            split.drawers[1].label.enabled = true;
                            split.drawers[1].label.text = "BIN B".into();
                            p = split;
                        }
                        Err(e) => {
                            self.tower.error = e;
                            return;
                        }
                    }
                }
                members.push(Member {
                    name: String::new(),
                    preset: p,
                    placement: crate::scene::Placement {
                        level,
                        column: column as u32,
                        offset: [0.; 3],
                    },
                    exterior: Default::default(),
                });
            }
        }
        if let Err(e) = self.tower_apply(members, 1, ctx) {
            self.tower.error = e;
        }
        self.tower_capture();
        self.smoke_stage = 0;
        self.smoke_frame = 0;
        self.screenshot_requested = false;
        if std::env::args().any(|a| a == "--smoke-build-panel") {
            self.tower.open_layer = Some(2);
        }
        if std::env::args().any(|a| a == "--smoke-project-menu") {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1050., 700.)));
        }
    }
    pub(super) fn tower_smoke(&mut self, ctx: &egui::Context) {
        if std::env::args().any(|a| a == "--smoke-color-groups") {
            self.color_groups_smoke(ctx);
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        let workspace_smoke = std::env::args().any(|a| a == "--smoke-workspace");
        let build_panel_smoke = std::env::args().any(|a| a == "--smoke-build-panel");
        let project_menu_smoke = std::env::args().any(|a| a == "--smoke-project-menu");
        let folder = self.home.join(if workspace_smoke {
            "output/workspace-ui"
        } else if build_panel_smoke {
            "output/build-panel-ui"
        } else if project_menu_smoke {
            "output/project-menu-ui"
        } else {
            "output/tower-workflow"
        });
        if workspace_smoke {
            let theme = if [1, 3, 5, 7, 13, 17].contains(&self.smoke_stage) {
                appearance::Appearance::Dark
            } else {
                appearance::Appearance::Light
            };
            if self.appearance != theme {
                self.appearance = theme;
                theme.apply(ctx);
            }
            if self.smoke_frame == 0 {
                let size = if [6, 7, 15, 16, 17].contains(&self.smoke_stage) {
                    egui::vec2(1050., 700.)
                } else {
                    egui::vec2(1440., 960.)
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
        }
        let error = if !self.tower.error.is_empty() {
            Some(self.tower.error.clone())
        } else if let Some(e) = &self.preview_modes.error {
            Some(e.clone())
        } else if self.smoke_start.elapsed() > Duration::from_secs(240) {
            Some(format!("Timeout: {}", self.status))
        } else if !self.busy && !self.preview.has_pending() && self.failed_preview.is_some() {
            Some(self.status.clone())
        } else {
            None
        };
        if let Some(error) = error {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("smoke-error.txt"), error);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.busy
            || self.preview.has_pending()
            || self.pending_layer_reorder.is_some()
            || self.scene.parts.is_empty()
        {
            return;
        }
        if self.preview_modes.mode == super::preview_modes::Mode::Plates
            && self.preview_modes.layout.is_none()
        {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 && !self.inline_edit.open {
            self.preview_mode_fit();
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
            let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                folder.join(format!("workflow-{}.png", self.smoke_stage)),
                &bytes,
                image.size[0] as u32,
                image.size[1] as u32,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
            if self.smoke_stage == 0
                && (rows(&self.members)?.len() != 3
                    || self
                        .scene
                        .parts
                        .iter()
                        .filter(|p| p.pose.path.contains("/Modules/"))
                        .count()
                        != 6)
            {
                return Err("Incorrect layer shells".into());
            }
            if build_panel_smoke
                && self.smoke_stage == 2
                && (self.tower.open_layer != Some(0)
                    || self.tower.print_list
                    || self.tower.scroll_to_layer.is_some()
                    || self.tower.layer_group_request.is_some()
                    || self.inline_edit.open)
            {
                return Err("Preview selection did not reveal its layer in the build panel".into());
            }
            if build_panel_smoke && self.smoke_stage == 3 {
                if self.tower.open_layer != Some(1) {
                    return Err("The moved layer did not stay expanded at its new position".into());
                }
                let motion = self
                    .layer_reorder
                    .as_ref()
                    .ok_or("No 3D layer reorder motion")?;
                let at_level = |level| {
                    self.scene
                        .member_placements
                        .iter()
                        .position(|p| p.column == 0 && p.level == level)
                        .ok_or("Missing animated smoke-test layer")
                };
                let lower = at_level(0)?;
                let upper = at_level(1)?;
                let start = motion.offsets(motion.start, 0., 0.);
                let mid = motion.offsets(motion.start + 0.2, 0., 0.);
                if start.get(lower).is_none_or(|v| v.y <= 0.)
                    || start.get(upper).is_none_or(|v| v.y >= 0.)
                    || mid
                        .get(upper)
                        .is_none_or(|v| v.z <= start[upper].z || v.y <= start[upper].y)
                    || !motion.offsets(motion.start + 1., 0., 0.).is_empty()
                {
                    return Err("The 3D preview did not interpolate both layers into place".into());
                }
            }
            if !build_panel_smoke && [2, 12, 15].contains(&self.smoke_stage) {
                if !self.combination_view
                    || self.scene_key != "combination"
                    || !self.inline_edit.open
                    || self.inline_edit.target
                        != Some(crate::scene::Target {
                            member: 3,
                            drawer: Some(1),
                        })
                    || self
                        .scene
                        .parts
                        .iter()
                        .filter(|p| p.pose.path.contains("/Modules/"))
                        .count()
                        != 6
                {
                    return Err(
                        "Inline editing replaced the full assembly or lost its subdrawer target"
                            .into(),
                    );
                }
                if self.members[3].preset.drawers[0].label.text != "BIN A" {
                    return Err("Subdrawer editing changed its sibling label".into());
                }
            }
            if !build_panel_smoke && self.smoke_stage == 2 && !self.label_open {
                return Err("Subdrawer label editor did not open".into());
            }
            if [3, 13, 16].contains(&self.smoke_stage) && self.inline_edit.open {
                return Err("Inline editor did not close when leaving configuration".into());
            }
            if [19, 20].contains(&self.smoke_stage) {
                let target = crate::scene::Target {
                    member: 3,
                    drawer: Some(if self.smoke_stage == 19 { 1 } else { 0 }),
                };
                if self.scene_key != "combination"
                    || !self.combination_view
                    || self.inline_edit.target != Some(target)
                    || self
                        .scene
                        .parts
                        .iter()
                        .filter(|p| p.pose.path.contains("/Modules/"))
                        .count()
                        != 6
                {
                    return Err("Path focus or target switch replaced the assembly scene".into());
                }
                if self.smoke_stage == 19
                    && !self
                        .selected
                        .and_then(|i| self.scene.parts.get(i))
                        .is_some_and(|p| target.contains(p) && p.pose.path == crate::label::PATH)
                {
                    return Err("Label focus selected the wrong repeated source instance".into());
                }
            }
            if build_panel_smoke
                && self.smoke_stage == 4
                && (!self.members[0].exterior.backbone
                    || !self
                        .scene
                        .parts
                        .iter()
                        .any(|part| part.pose.path.contains("Backbone")))
            {
                return Err("The inline rear modification did not load a backbone".into());
            }
            if !build_panel_smoke && self.smoke_stage == 4 {
                let layout = self.preview_modes.layout.as_ref().unwrap();
                let copies: usize = self
                    .members
                    .iter()
                    .flat_map(|m| m.preset.printing_bom())
                    .filter(|b| b.enabled)
                    .map(|b| b.qty as usize)
                    .sum();
                if layout.plates.iter().map(|p| p.copies).sum::<usize>() != copies {
                    return Err("Plate copies differ from shared-shell manufacturing BOM".into());
                }
                if !layout.parts.iter().flatten().any(|p| p.drawer == Some(1)) {
                    return Err("Missing independently editable drawer identity on plates".into());
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::write(folder.join("smoke-error.txt"), e);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.smoke_frame = 0;
        self.screenshot_requested = false;
        if build_panel_smoke || project_menu_smoke {
            if self.smoke_stage == 0 {
                if build_panel_smoke {
                    self.tower.layer_group_request = Some(false);
                }
                self.smoke_stage = 1;
            } else if build_panel_smoke && self.smoke_stage == 1 {
                let member = self
                    .members
                    .iter()
                    .position(|member| member.placement.level == 0);
                let part = member.and_then(|member| {
                    let target = crate::scene::Target {
                        member,
                        drawer: None,
                    };
                    self.scene
                        .parts
                        .iter()
                        .position(|part| target.contains(part))
                });
                if let Some(part) = part {
                    self.tower.print_list = true;
                    self.select_combination_part(part, ctx);
                    self.smoke_stage = 2;
                } else {
                    let _ = std::fs::write(
                        folder.join("smoke-error.txt"),
                        "Missing bottom-layer test part",
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else if build_panel_smoke && self.smoke_stage == 2 {
                match self.tower_action(Action::Move(0, 1), ctx) {
                    Ok(()) if self.tower.reorder.is_some() => self.smoke_stage = 3,
                    result => {
                        let error = result
                            .err()
                            .unwrap_or_else(|| "Layer move did not start a transition".into());
                        let _ = std::fs::write(folder.join("smoke-error.txt"), error);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            } else if build_panel_smoke && self.smoke_stage == 3 {
                let mut exterior = self.members[0].exterior.clone();
                exterior.backbone = true;
                match self.apply_exterior(0, exterior, ctx) {
                    Ok(()) => {
                        self.tower.scroll_to_top = true;
                        self.smoke_stage = 4;
                    }
                    Err(error) => {
                        let _ = std::fs::write(folder.join("smoke-error.txt"), error);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            } else {
                if build_panel_smoke {
                    let _ = std::fs::write(
                        folder.join("selection-result.txt"),
                        "PASS: preview selection and 3D layer reorder; inline backbone loaded; expanded modification and color cards captured.",
                    );
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        let result = match self.smoke_stage {
            0 => {
                self.preview_modes.mode = super::preview_modes::Mode::Layers;
                Ok(())
            }
            1 => {
                self.tower_edit_slot(3, Some(1), ctx);
                self.label.text = "BIN EDIT".into();
                self.label_open = true;
                self.assembly(ctx);
                Ok(())
            }
            2 => {
                self.label_open = false;
                self.show_combination(ctx);
                self.tower.print_list = true;
                Ok(())
            }
            3 => {
                self.tower.print_list = false;
                self.preview_modes.mode = super::preview_modes::Mode::Plates;
                Ok(())
            }
            4 => {
                self.preview_modes.mode = super::preview_modes::Mode::Assembly;
                self.tower_new(ctx);
                Ok(())
            }
            5 => {
                self.tower_capture();
                let project = self.tower.projects[0].clone();
                self.tower.current = 0;
                self.tower_apply_project(project, ctx)
            }
            6 => self.tower_action(Action::Duplicate(1), ctx),
            7 => {
                self.undo_workflow(ctx);
                Ok(())
            }
            8 => {
                self.preview_modes.mode = super::preview_modes::Mode::Exploded;
                self.explosion = 1.;
                self.camera.fit(&self.scene, self.explosion);
                Ok(())
            }
            9 => {
                self.explosion = 0.45;
                Ok(())
            }
            10 => {
                self.preview_modes.mode = super::preview_modes::Mode::Assembly;
                self.explosion = 0.;
                Ok(())
            }
            11 if workspace_smoke => {
                self.tower_edit_slot(3, Some(1), ctx);
                self.tower.show_parts = false;
                Ok(())
            }
            12 => {
                let overview = self.inline_edit.overview.clone();
                self.close_inline_editor(ctx);
                self.tower.print_list = true;
                if let Some((camera, ghost)) = overview {
                    if self.camera.center != camera.center
                        || self.camera.radius != camera.radius
                        || self.camera.zoom != camera.zoom
                        || self.camera.pan != camera.pan
                        || self.camera.yaw != camera.yaw
                        || self.camera.pitch != camera.pitch
                        || self.ghost != ghost
                    {
                        Err("Closing inline editing did not restore the overview camera".into())
                    } else {
                        Ok(())
                    }
                } else {
                    Err("Inline editing lost the overview camera".into())
                }
            }
            13 => {
                self.tower.show_parts = true;
                Ok(())
            }
            14 => {
                self.tower.show_parts = false;
                self.tower.print_list = false;
                self.tower_edit_slot(3, Some(1), ctx);
                self.zh = false;
                Ok(())
            }
            15 => {
                self.zh = true;
                self.show_combination(ctx);
                self.tower.print_list = true;
                Ok(())
            }
            16 => {
                self.tab = 1;
                Ok(())
            }
            17 => {
                self.tab = 0;
                Ok(())
            }
            18 if workspace_smoke => {
                self.tower.print_list = false;
                self.tower_edit_slot(3, Some(1), ctx);
                self.single(crate::label::PATH.into(), ctx);
                Ok(())
            }
            19 => {
                self.tower_edit_slot(3, Some(0), ctx);
                Ok(())
            }
            20 => {
                self.close_inline_editor(ctx);
                Ok(())
            }
            _ => {
                let result = (|| -> Result<(), String> {
                    if rows(&self.members)?.len() != 3 {
                        return Err("Undo did not restore layer count".into());
                    }
                    if self.members[3].preset.drawers[1].label.text != "BIN EDIT" {
                        return Err("Subdrawer text did not survive project switch and undo".into());
                    }
                    let saved = self.tower_snapshot();
                    let text = toml::to_string_pretty(&saved).map_err(|e| e.to_string())?;
                    let restored: CombinationPreset =
                        toml::from_str(&text).map_err(|e| e.to_string())?;
                    restored.validate(&self.catalog)?;
                    std::fs::write(folder.join("project.toml"), text).map_err(|e| e.to_string())?;
                    std::fs::write(folder.join("smoke-result.txt"),"PASS: one full assembly viewport during inline subdrawer editing, unchanged sibling label, target identity preserved, editor closed on navigation, English minimum-size floating editor, configuration / print-list navigation, optional properties, minimum-size project checklist and library, layer cards, common XL5 footprint, independent S+M cells, shared 2F housing / two 1F drawers, label editing, print plates, project switching, layer duplication, undo, full and partial combination explosion and TOML roundtrip. All source geometry remains read-only; assembly estimates unverified.").map_err(|e|e.to_string())
                })();
                if let Err(e) = result {
                    let _ = std::fs::write(folder.join("smoke-error.txt"), e);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        };
        if let Err(e) = result {
            self.tower.error = e;
        }
        self.smoke_stage += 1;
    }
}

fn persist_projects(folder: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(folder).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(folder.join("projects.toml"))
        .map_err(|e| e.to_string())?;
    Ok(())
}
impl App {
    pub(super) fn tower_flush(&mut self) {
        if self.smoke || !self.tower.loaded || self.members.is_empty() {
            return;
        }
        if let Some(write) = self.tower.write.take() {
            let _ = write.recv();
        }
        self.tower_capture();
        let projects = Projects {
            current: self.tower.current,
            items: self.tower.projects.clone(),
        };
        if let Ok(text) = toml::to_string(&projects) {
            let _ = persist_projects(&self.home.join("state"), text.as_bytes());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn catalog() -> Catalog {
        let empty = Catalog::default();
        let mut entries = std::collections::BTreeMap::new();
        for width in ["S", "M", "L", "XL"] {
            for depth in 3..=7 {
                for height in ["1F", "2F", "4F"] {
                    if width == "XL" && height != "1F" {
                        continue;
                    }
                    let s = Selection {
                        width: width.into(),
                        depth,
                        height: height.into(),
                        hidden_key: false,
                    };
                    for b in empty.bom(&s) {
                        entries
                            .entry(b.path.clone())
                            .or_insert(crate::catalog::Entry {
                                path: b.path.clone(),
                                name: b.name().into(),
                                category: 2,
                            });
                    }
                    for kind in ["Grid", "Table", "Bowl", "Under Table Mount"] {
                        let path = format!(
                            "{}/Roofs/{width}{depth} {kind} Roof.stl",
                            Catalog::prefix(&s)
                        );
                        entries
                            .entry(path.clone())
                            .or_insert(crate::catalog::Entry {
                                name: format!("{width}{depth} {kind} Roof.stl"),
                                path,
                                category: 1,
                            });
                    }
                }
            }
        }
        let path = "01. DRAWERS/07. Universal/Top Handle (Turn On Supports).stl".to_owned();
        entries.insert(
            path.clone(),
            crate::catalog::Entry {
                name: "Top Handle".into(),
                path,
                category: 1,
            },
        );
        Catalog {
            root: PathBuf::new(),
            entries: entries.into_values().collect(),
        }
    }
    fn preset(c: &Catalog, width: &str, height: &str) -> Preset {
        let selection = Selection {
            width: width.into(),
            height: height.into(),
            ..Default::default()
        };
        Preset {
            drawers: vec![],
            bom: c.bom(&selection),
            selection,
            assembly_kind: AssemblyKind::Drawer,
            sustainer_options: Default::default(),
            sustainer_angle: 0.,
            drawer_extension: 0.,
            sustainer_dividers: Default::default(),
            divider_layout: Default::default(),
            gridfinity: Default::default(),
            label: crate::label::Config {
                enabled: true,
                text: "KEEP".into(),
                ..Default::default()
            },
            print_config: Some(Default::default()),
        }
    }
    fn member(c: &Catalog, width: &str, height: &str, level: u32, column: u32) -> Member {
        Member {
            name: format!("{level}-{column}"),
            preset: preset(c, width, height),
            placement: crate::scene::Placement {
                level,
                column,
                offset: [0.; 3],
            },
            exterior: Default::default(),
        }
    }
    #[test]
    fn split_layers_and_global_resize_keep_independent_labels() {
        let c = catalog();
        let original = vec![member(&c, "L", "2F", 0, 0), member(&c, "L", "1F", 1, 0)];
        let split = set_row(&original, &c, 0, "2F", true, false).unwrap();
        assert_eq!(rows(&split).unwrap()[0].members.len(), 2);
        let mut changed = resize(&split, &c, "XL", 5, false).unwrap();
        assert_eq!(
            changed
                .iter()
                .map(|m| m.preset.selection.width.as_str())
                .collect::<Vec<_>>(),
            vec!["S", "M", "S", "M"]
        );
        assert!(changed.iter().all(|m| m.preset.selection.depth == 5));
        changed[1].preset.label.text = "RIGHT".into();
        assert_eq!(changed[0].preset.label.text, "KEEP");
        assert_eq!(original[0].preset.selection.width, "L");
    }
    #[test]
    fn whole_top_follows_footprint_across_layer_splits_resizes_and_roundtrips() {
        use crate::catalog::exterior::{Role, Roof};
        let c = catalog();
        let mut original = vec![member(&c, "L", "2F", 0, 0)];
        original[0].exterior.roof = Roof::Grid;
        original[0].exterior.top_handle = true;
        let split = set_row(&original, &c, 0, "2F", true, false).unwrap();
        let roof_paths = |members: &[Member]| {
            members
                .iter()
                .flat_map(|m| {
                    c.exterior_components(&m.preset.selection, &m.exterior, false)
                        .unwrap()
                })
                .filter(|p| p.role == Role::Roof)
                .map(|p| p.bom.path)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            roof_paths(&split),
            vec![format!(
                "{}/Roofs/L3 Grid Roof.stl",
                Catalog::prefix(&original[0].preset.selection)
            )]
        );
        assert_eq!(split[0].exterior.roof_width.as_deref(), Some("L"));
        assert!(split[0].exterior.top_handle);
        assert_eq!(split[1].exterior.roof, Roof::None);
        let merged = set_row(&split, &c, 0, "2F", false, false).unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].exterior.roof, Roof::Grid);
        assert!(merged[0].exterior.roof_width.is_none());
        let mut resized = resize(&split, &c, "XL", 5, false).unwrap();
        assert_eq!(roof_paths(&resized).len(), 1);
        assert!(roof_paths(&resized)[0].ends_with("/XL5 Grid Roof.stl"));
        roofs::set_layout(&mut resized, roofs::Layout::Apart).unwrap();
        assert_eq!(roof_paths(&resized).len(), 2);
        assert!(roof_paths(&resized)[0].ends_with("/S5 Grid Roof.stl"));
        assert!(roof_paths(&resized)[1].ends_with("/M5 Grid Roof.stl"));
        assert!(resized.iter().all(|m| m.exterior.top_handle));
        let restored: Vec<Member> = toml::from_str::<CombinationPreset>(
            &toml::to_string(&CombinationPreset {
                project_name: "Top test".into(),
                colors: Default::default(),
                preset: resized[0].preset.clone(),
                members: resized.clone(),
                active_member: 0,
                combination_gap: 0.,
            })
            .unwrap(),
        )
        .unwrap()
        .members;
        assert_eq!(roof_paths(&restored), roof_paths(&resized));
        assert!(roofs::layout(&restored).unwrap() == roofs::Layout::Apart);
        crate::app::combination::validate_layout(&restored, 0.).unwrap();
    }
    #[test]
    fn legacy_split_covers_keep_types_and_empty_split_is_a_preference_only() {
        use crate::catalog::exterior::Roof;
        let c = catalog();
        let mut original = vec![member(&c, "S", "2F", 0, 0), member(&c, "S", "2F", 0, 1)];
        original[0].exterior.roof = Roof::Table;
        original[1].exterior.roof = Roof::Bowl;
        let changed = resize(&original, &c, "XL", 4, false).unwrap();
        assert_eq!(changed[0].exterior.roof, Roof::Table);
        assert_eq!(changed[1].exterior.roof, Roof::Bowl);
        assert_eq!(
            changed[1]
                .exterior
                .roof_selection(&changed[1].preset.selection)
                .width,
            "M"
        );
        let mut empty = original;
        for m in &mut empty {
            m.exterior = Default::default();
        }
        roofs::set_layout(&mut empty, roofs::Layout::Apart).unwrap();
        assert!(roofs::layout(&empty).unwrap() == roofs::Layout::Apart);
        assert!(empty.iter().all(|m| m.exterior.is_empty()));
        assert!(empty.iter().all(|m| {
            c.exterior_components(&m.preset.selection, &m.exterior, false)
                .unwrap()
                .is_empty()
        }));
        let legacy: crate::catalog::exterior::Exterior = toml::from_str("roof = 'table'").unwrap();
        assert!(legacy.roof_width.is_none());
    }
    #[test]
    fn checked_jobs_keep_drawer_only_requests_and_attach_exterior_to_the_shell() {
        let c = catalog();
        let mut m = member(&c, "M", "2F", 0, 0);
        let mut child = preset(&c, "M", "1F");
        child.bom.retain(|item| crate::label::is_label(&item.path));
        assert_eq!(child.bom.len(), 1);
        child.bom[0].qty = 2;
        m.preset.drawers = vec![child.clone(), child];
        m.preset.drawers[0].bom[0].enabled = false;
        for item in &mut m.preset.bom {
            item.enabled = false;
        }
        let jobs = print_list::checked_jobs([(7, &m)], &c).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!((jobs[0].0, jobs[0].1.drawer), (7, Some(1)));
        assert_eq!(jobs[0].1.items[0].qty, 2);

        m.exterior.roof = crate::catalog::exterior::Roof::Table;
        let jobs = print_list::checked_jobs([(7, &m)], &c).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].1.drawer, None);
        assert!(jobs[0].1.items.iter().any(|b| b.path.contains("/Roofs/")));
        assert_eq!(jobs[1].1.items.len(), 1);
    }
    #[test]
    fn print_jobs_enforce_rails_and_cap_quantity_preserving_other_choices() {
        let c = catalog();
        let mut m = member(&c, "M", "2F", 0, 0);
        for item in &mut m.preset.bom {
            item.enabled = false;
            item.qty = 12;
        }
        let items = m.preset.printing_bom();
        let rail = items
            .iter()
            .find(|b| b.path.contains("/Geared Rails/"))
            .unwrap();
        assert_eq!((rail.enabled, rail.qty), (true, 1));
        let cap = items
            .iter()
            .find(|b| b.path.ends_with("/Facade Cap.stl"))
            .unwrap();
        assert_eq!((cap.enabled, cap.qty), (false, 5));
        let optional = items
            .iter()
            .find(|b| crate::label::is_label(&b.path))
            .unwrap();
        assert_eq!((optional.enabled, optional.qty), (false, 12));
        let jobs = print_list::checked_jobs([(0, &m)], &c).unwrap();
        assert_eq!(jobs[0].1.items.len(), 1);
        assert_eq!(jobs[0].1.items[0].path, rail.path);
    }
    #[test]
    fn printing_edits_change_only_the_target_drawers_print_fields() {
        let c = catalog();
        let mut m = member(&c, "M", "2F", 3, 1);
        m.name = "Custom name".into();
        m.placement.offset = [1., 2., 3.];
        m.preset.drawers = vec![preset(&c, "M", "1F"), preset(&c, "M", "1F")];
        let mut edit = m.preset.drawers[1].clone();
        edit.bom[0].qty = 7;
        edit.label.text = "unrelated edit".into();
        let mut expected = m.clone();
        expected.preset.drawers[1].bom[0].qty = 7;
        print_list::apply_project_edit(
            std::slice::from_mut(&mut m),
            crate::scene::Target {
                member: 0,
                drawer: Some(1),
            },
            edit,
        );
        assert_eq!(
            serde_json::to_value(m).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    #[test]
    fn multiple_drawers_print_one_shared_shell_and_keep_leaf_settings() {
        let c = catalog();
        let p = preset(&c, "M", "2F");
        let mut stacked = drawer_pattern(&p, &c, &["1F", "1F"], false).unwrap();
        stacked.drawers[0].label.text = "TOP".into();
        stacked.drawers[1].label.text = "BOTTOM".into();
        let jobs = stacked.print_jobs("M3");
        assert_eq!(jobs.len(), 3);
        let items = stacked.printing_bom();
        assert_eq!(
            items
                .iter()
                .filter(|b| b.path.contains("/Modules/"))
                .map(|b| b.qty)
                .sum::<u32>(),
            1
        );
        assert_eq!(
            items
                .iter()
                .filter(|b| crate::label::is_label(&b.path))
                .map(|b| b.qty)
                .sum::<u32>(),
            2
        );
        assert!(jobs[1..].iter().all(|j| {
            j.items
                .iter()
                .all(|b| !crate::catalog::tower::housing(&b.path))
        }));
        let text = toml::to_string(&stacked).unwrap();
        let restored: Preset = toml::from_str(&text).unwrap();
        restored.validate_member(&c).unwrap();
        assert_eq!(restored.drawers[0].label.text, "TOP");
        assert_eq!(restored.drawers[1].label.text, "BOTTOM");
    }
    #[test]
    fn reconfiguring_a_member_updates_each_child_footprint_and_preserves_drawer_details() {
        let catalog = catalog();
        let mut stacked =
            drawer_pattern(&preset(&catalog, "M", "2F"), &catalog, &["1F", "1F"], false).unwrap();
        stacked.drawers[0].label.text = "TOP".into();
        stacked.drawers[1].label.text = "BOTTOM".into();
        stacked.selection.width = "S".into();
        stacked.selection.depth = 5;
        configure(&mut stacked, &catalog, false).unwrap();
        for (child, text) in stacked.drawers.iter().zip(["TOP", "BOTTOM"]) {
            assert_eq!(child.label.text, text);
            assert_eq!(child.selection.width, "S");
            assert_eq!(child.selection.depth, 5);
            assert_eq!(child.selection.height, "1F");
            assert!(child.bom.iter().any(|b| b.path.ends_with("/S5 Box 1F.stl")));
        }
    }

    #[test]
    fn rejected_resize_is_atomic_and_nested_drawer_stacks_are_invalid() {
        let c = catalog();
        let original = vec![member(&c, "L", "2F", 0, 0)];
        let before = serde_json::to_vec(&original).unwrap();
        assert!(resize(&original, &c, "L", 8, false).is_err());
        assert_eq!(before, serde_json::to_vec(&original).unwrap());
        let mut invalid = drawer_pattern(&original[0].preset, &c, &["1F", "1F"], false).unwrap();
        invalid.drawers[0].drawers.push(preset(&c, "L", "1F"));
        assert!(invalid.validate_member(&c).is_err());
    }
    #[test]
    fn legacy_side_by_side_projects_are_preserved_without_automatic_conversion() {
        let c = catalog();
        let legacy = vec![
            member(&c, "S", "1F", 0, 0),
            member(&c, "M", "2F", 0, 1),
            member(&c, "S", "1F", 1, 0),
        ];
        let before = serde_json::to_vec(&legacy).unwrap();
        assert!(rows(&legacy).is_err());
        assert_eq!(before, serde_json::to_vec(&legacy).unwrap());
    }
    #[test]
    fn project_copy_keeps_nested_content_and_uses_a_unique_name() {
        let catalog = catalog();
        let mut member = member(&catalog, "M", "2F", 0, 0);
        member.preset = drawer_pattern(&member.preset, &catalog, &["1F", "1F"], false).unwrap();
        member.preset.drawers[1].label.text = "工具".into();
        let source = Project {
            name: "工作台".into(),
            saved: CombinationPreset {
                project_name: "工作台".into(),
                colors: Default::default(),
                preset: member.preset.clone(),
                members: vec![member],
                active_member: 0,
                combination_gap: 0.,
            },
        };
        let first = source.clone().duplicate(&[], true);
        let mut second = source.clone().duplicate(&[first], true);
        assert_eq!(second.name, "工作台 副本 2");
        assert_eq!(second.saved.project_name, second.name);
        assert_eq!(second.summary(true), "M3 · 1 层");
        let label = &mut second.saved.members[0].preset.drawers[1].label;
        assert_eq!(label.text, "工具");
        label.text = "更改".into();
        assert_eq!(source.saved.members[0].preset.drawers[1].label.text, "工具");
    }

    #[test]
    fn project_sessions_persist_current_project_and_leaf_labels() {
        let c = catalog();
        let a = member(&c, "S", "1F", 0, 0);
        let mut b = member(&c, "M", "2F", 0, 0);
        b.preset = drawer_pattern(&b.preset, &c, &["1F", "1F"], false).unwrap();
        b.preset.drawers[1].label.text = "B".into();
        let make = |name: &str, m: Member| Project {
            name: name.into(),
            saved: CombinationPreset {
                project_name: name.into(),
                colors: Default::default(),
                preset: m.preset.clone(),
                members: vec![m],
                active_member: 0,
                combination_gap: 0.,
            },
        };
        let mut projects = Projects {
            current: 1,
            items: vec![make("FIRST", a), make("SECOND", b)],
        };
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output/tower-project-tests");
        std::fs::create_dir_all(&out).unwrap();
        let dir = tempfile::tempdir_in(out).unwrap();
        persist_projects(dir.path(), toml::to_string(&projects).unwrap().as_bytes()).unwrap();
        let restored: Projects =
            toml::from_str(&std::fs::read_to_string(dir.path().join("projects.toml")).unwrap())
                .unwrap();
        assert_eq!(restored.current, 1);
        assert_eq!(
            restored.items[1].saved.members[0].preset.drawers[1]
                .label
                .text,
            "B"
        );
        projects.current = 0;
        persist_projects(dir.path(), toml::to_string(&projects).unwrap().as_bytes()).unwrap();
        let restored: Projects =
            toml::from_str(&std::fs::read_to_string(dir.path().join("projects.toml")).unwrap())
                .unwrap();
        assert_eq!(restored.current, 0);
        assert_eq!(restored.items.len(), 2);
    }
    #[test]
    #[ignore = "requires read-only VAULT_MODEL_ROOT"]
    fn layer_workflow_library_scene_and_export() {
        let root = PathBuf::from(crate::test_support::required_env("VAULT_MODEL_ROOT"));
        let catalog = Catalog::scan(root.clone()).unwrap();
        let mut m = member(&catalog, "M", "2F", 0, 0);
        m.preset = drawer_pattern(&m.preset, &catalog, &["1F", "1F"], false).unwrap();
        m.preset.drawers[0].label.text = "TOP".into();
        m.preset.drawers[1].label.text = "BOTTOM".into();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let scene =
            super::super::combination::load(&root, &home, &[m.clone()], 0., &cancel).unwrap();
        assert!(!scene.calibrated);
        assert_eq!(
            scene
                .parts
                .iter()
                .filter(|p| p.pose.path.contains("/Modules/"))
                .count(),
            1
        );
        assert_eq!(
            scene
                .parts
                .iter()
                .filter(|p| crate::label::is_label(&p.pose.path))
                .count(),
            2
        );
        let unique: std::collections::HashSet<_> = scene
            .parts
            .iter()
            .map(crate::constraints::PartId::of)
            .collect();
        assert_eq!(unique.len(), scene.parts.len());
        let box_y: Vec<_> = scene
            .parts
            .iter()
            .filter(|p| p.pose.path.contains("/Boxes/"))
            .map(|p| p.pose.position[1])
            .collect();
        assert_eq!(box_y.len(), 2);
        assert!((box_y[0] - box_y[1] - 48.).abs() < 0.01);
        let mut four = member(&catalog, "M", "4F", 0, 0);
        four.preset = drawer_pattern(&four.preset, &catalog, &["1F", "2F", "1F"], false).unwrap();
        let scene4 = super::super::combination::load(&root, &home, &[four], 0., &cancel).unwrap();
        assert_eq!(
            scene4
                .parts
                .iter()
                .filter(|p| p.pose.path.contains("/Modules/"))
                .count(),
            1
        );
        let bottoms: Vec<_> = scene4
            .parts
            .iter()
            .filter(|p| p.pose.path.contains("/Boxes/"))
            .map(|p| crate::constraints::world_bounds(p).0.y)
            .collect();
        assert_eq!(bottoms.len(), 3);
        assert!((bottoms[0] - bottoms[1] - 96.).abs() < 0.25);
        assert!((bottoms[1] - bottoms[2] - 48.).abs() < 0.25);
        let out = home.join("output/tower-workflow");
        std::fs::create_dir_all(&out).unwrap();
        let mut total = 0;
        for (i, job) in m.preset.print_jobs("M3").into_iter().enumerate() {
            let destination = out.join(format!("job-{i}.3mf"));
            crate::export::run_configured(
                &root,
                &job.items,
                &destination,
                &crate::export::Options {
                    format: crate::export::Format::ThreeMf,
                    plate: job.preset.print_config.clone().unwrap(),
                },
                &job.preset.label,
                Some((&home, &job.preset.gridfinity, &job.preset.selection)),
            )
            .unwrap();
            total += job
                .items
                .iter()
                .filter(|b| b.enabled)
                .map(|b| b.qty)
                .sum::<u32>();
        }
        std::fs::write(out.join("integration.txt"),format!("PASS: one shared M3 2F shell, two independent 1F drawers, 48 mm slot spacing, {} unique display identities, {total} manufacturing copies, three 3MF jobs. Assembly estimates remain unverified.",unique.len())).unwrap();
    }
}
