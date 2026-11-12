use super::*;
mod layers;
mod roofs;
pub(super) struct State {
    pub name: String,
    pub show_parts: bool,
    pub print_list: bool,
    open_layer: Option<u32>,
    scroll_to_top: bool,
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
            show_parts: false,
            print_list: false,
            open_layer: Some(0),
            scroll_to_top: true,
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
pub(super) fn default_name() -> String {
    "工作台抽屉柜".into()
}
#[derive(Clone, Serialize, Deserialize)]
struct Project {
    name: String,
    saved: CombinationPreset,
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
        if self.drawers.is_empty() {
            return vec![PrintJob {
                name: name.into(),
                preset: self.clone(),
                items: self.bom.clone(),
                drawer: None,
            }];
        }
        let mut shell = self.clone();
        shell.drawers.clear();
        shell.gridfinity = Default::default();
        shell.label.enabled = false;
        let mut jobs = vec![PrintJob {
            name: format!("{name} · 模块"),
            items: crate::catalog::tower::housing_items(&self.bom),
            preset: shell,
            drawer: None,
        }];
        for (i, drawer) in self.drawers.iter().enumerate() {
            jobs.push(PrintJob {
                name: format!("{name} · 抽屉 {}", i + 1),
                preset: drawer.clone(),
                items: crate::catalog::tower::drawer_items(&drawer.bom),
                drawer: Some(i),
            });
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
    p.sustainer_options = p.sustainer_options.normalized(&p.selection);
    p.bom = if p.assembly_kind == AssemblyKind::Drawer {
        catalog.reconfigure(&p.selection, &p.bom)
    } else {
        catalog.reconfigure_sustainer(&p.selection, p.sustainer_options, &p.bom)?
    };
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
        if member >= self.members.len() {
            return;
        }
        self.commit_member();
        if self.member_edit_baseline.is_none() {
            self.member_edit_baseline = Some(self.workflow_snapshot());
        }
        self.restore_member(member);
        if let Some(drawer) = drawer
            && let Some(p) = self.members[member].preset.drawers.get(drawer).cloned()
        {
            self.restore_editor(p);
            self.active_drawer = Some(drawer);
        }
        self.preview_modes.mode = super::preview_modes::Mode::Assembly;
        self.tab = 0;
        self.tower.print_list = false;
        self.assembly(ctx);
    }
    fn tower_snapshot(&mut self) -> CombinationPreset {
        self.commit_member();
        CombinationPreset {
            project_name: self.tower.name.clone(),
            colors: self.global_colors.clone(),
            preset: self
                .members
                .get(self.active_member)
                .map(|m| m.preset.clone())
                .unwrap_or_else(|| self.member_snapshot()),
            members: self.members.clone(),
            active_member: self.active_member,
            combination_gap: self.combination_gap,
        }
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
    fn tower_apply_project(&mut self, project: Project, ctx: &egui::Context) -> Result<(), String> {
        project.saved.validate(&self.catalog)?;
        let saved = project.saved;
        self.tower.name = project.name;
        self.members = saved.members;
        self.active_member = saved.active_member;
        self.global_colors = saved.colors;
        self.combination_gap = saved.combination_gap;
        if self.members.is_empty() {
            self.restore_editor(saved.preset);
            self.active_drawer = None;
            self.ensure_members();
        }
        self.restore_member(self.active_member.min(self.members.len() - 1));
        self.combination_view = true;
        self.workflow_history.clear();
        self.member_edit_baseline = None;
        self.combination_view_state = None;
        self.member_selection.clear();
        self.scene_key.clear();
        self.tower.open_layer = Some(self.members[self.active_member].placement.level);
        self.tower.print_list = false;
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
        self.tower.current = self.tower.projects.len();
        self.tower.projects.push(project.clone());
        if let Err(e) = self.tower_apply_project(project, ctx) {
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
enum Action {
    Resize(String, u8),
    Row(u32, String, bool),
    Duplicate(u32),
    Delete(u32),
    Move(u32, u32),
    Add(AssemblyKind),
    Kind(usize, AssemblyKind),
    Pattern(usize, Vec<String>),
    Edit(usize, Option<usize>),
    Label(usize, Option<usize>),
    Accessories(usize),
    Roof(usize, crate::catalog::exterior::Roof),
    RoofLayout(roofs::Layout),
    TopHandle(usize, bool),
    Base(usize, crate::catalog::exterior::Base),
}
impl App {
    fn tower_action(&mut self, action: Action, ctx: &egui::Context) -> Result<(), String> {
        self.commit_member();
        let reset = self.tower.reset_inserts;
        match action {
            Action::Edit(i, d) | Action::Label(i, d) => {
                let label = matches!(action, Action::Label(..));
                self.tower_edit_slot(i, d, ctx);
                if label {
                    self.label_open = true;
                }
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
                for m in &mut next {
                    let l = m.placement.level;
                    m.placement.level = if l == from {
                        to
                    } else if from < to && l > from && l <= to {
                        l - 1
                    } else if from > to && l >= to && l < from {
                        l + 1
                    } else {
                        l
                    };
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
        self.tower_apply(next, level, ctx)
    }
    pub(super) fn tower_project_header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let z = self.zh;
        let mut choose = None;
        let mut new = false;
        let mut open = false;
        let mut save = false;
        let mut recipe = None;
        ui.horizontal(|ui| {
            ui.menu_button(t(z, "项目", "Project"), |ui| {
                ui.set_min_width(240.);
                for (i, p) in self.tower.projects.iter().enumerate() {
                    if ui
                        .selectable_label(i == self.tower.current, &p.name)
                        .clicked()
                    {
                        choose = Some(i);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(t(z, "＋ 新建项目", "＋ New project")).clicked() {
                    new = true;
                    ui.close();
                }
                if ui.button(t(z, "打开文件…", "Open file…")).clicked() {
                    open = true;
                    ui.close();
                }
                if ui.button(t(z, "保存为文件…", "Save to file…")).clicked() {
                    save = true;
                    ui.close();
                }
            });
            if ui
                .small_button("＋")
                .on_hover_text(t(z, "新建项目", "New project"))
                .clicked()
            {
                new = true;
            }
            ui.add_enabled_ui(
                !self.busy && !self.catalog.root.as_os_str().is_empty(),
                |ui| {
                    ui.menu_button(t(z, "预设", "Presets"), |ui| {
                        ui.set_max_width(340.);
                        ui.small(t(
                            z,
                            "从预设新建项目，保留当前项目。",
                            "Create a new project and keep the current one.",
                        ));
                        ui.separator();
                        for (index, item) in super::recipes::RECIPES.iter().enumerate() {
                            if ui
                                .button(item.name(z))
                                .on_hover_text(item.description(z))
                                .clicked()
                            {
                                recipe = Some(index);
                                ui.close();
                            }
                        }
                    });
                },
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.tower.name)
                    .hint_text(t(z, "项目名称", "Project name"))
                    .char_limit(80)
                    .desired_width(ui.available_width().min(320.)),
            );
        });
        if let Some(i) = choose {
            self.tower_capture();
            let p = self.tower.projects[i].clone();
            if let Err(e) = self.tower_apply_project(p, ctx) {
                self.tower.error = e;
            } else {
                self.tower.current = i;
            }
        }
        if new {
            self.tower_new(ctx);
        }
        if let Some(index) = recipe
            && let Err(error) = self.tower_from_recipe(index, ctx)
        {
            self.tower.error = error;
        }
        if open {
            self.tower_capture();
            self.load_preset(ctx);
        }
        if save {
            self.save_preset();
        }
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
                    ui.colored_label(ui.visuals().hyperlink_color, error);
                    ui.small(t(z, "原方案的标签、内部配置和装配位置完整保留。可新建层式项目，或选择原成员编辑。", "Original labels, inserts and placements are preserved. Start a layer project or edit an existing member."));
                    for (i, m) in self.members.iter().enumerate() {
                        if ui.button(&m.name).clicked() { action = Some(Action::Edit(i, None)); }
                    }
                    return;
                }
            };
            let Some(first) = all.first() else { return; };
            let mut width = first.width.clone();
            let mut depth = first.depth;
            ui.horizontal(|ui| {
                ui.strong(t(z, "整体尺寸", "Overall size"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(t(z, "选项", "Options"), |ui| {
                        ui.checkbox(&mut self.tower.reset_inserts, t(z, "改规格时重置内部配置（保留标牌）", "Reset inserts on size changes (keep labels)"));
                    });
                });
            });
            ui.small(t(z, "宽度", "Width"));
            appearance::size_choices(ui, &mut width, &[("S".into(), "S"), ("M".into(), "M"), ("L".into(), "L"), ("XL".into(), "XL")]);
            ui.small(t(z, "深度", "Depth"));
            appearance::size_choices(ui, &mut depth, &[(3,"3"), (4,"4"), (5,"5"), (6,"6"), (7,"7")]);
            if width != first.width || depth != first.depth { action = Some(Action::Resize(width, depth)); }
            ui.small(format!("{} {:.1} mm · {} {}", t(z, "标称宽度", "Nominal width"), crate::constraints::stacking::width(&first.width).unwrap_or(0) as f32 / 10., first.depth, t(z, "格深度", "depth units")));
            ui.add_space(12.);
            let top = all.last().unwrap();
            roofs::show(self, ui, top, &mut action);
            ui.add_space(8.);
            ui.horizontal(|ui| {
                ui.strong(t(z, "层", "Layers"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(RichText::new(format!("{} {}",all.len(),t(z,"层","layers"))).weak()); });
            });
            for row in all.iter().rev() {
                let expanded = self.tower.open_layer == Some(row.level);
                let kind = if row.members.iter().all(|&i| self.members[i].preset.assembly_kind == AssemblyKind::Drawer) {
                    t(z,"抽屉","Drawer")
                } else if row.members.iter().all(|&i| self.members[i].preset.assembly_kind == AssemblyKind::Sustainer) {
                    t(z,"箱子","Box")
                } else { t(z,"混合","Mixed") };
                let size = row.members.iter().map(|&i|format!("{}{}",self.members[i].preset.selection.width,row.depth)).collect::<Vec<_>>().join(" + ");
                let card = layers::layer(ui, layers::Header {level:row.level,kind,size:&size,height:&row.height,can_delete:all.len()>1,zh:z}, expanded, |ui| {
                    let mut height=row.height.clone();
                    let mut split=row.members.len()==2;
                    ui.horizontal(|ui| {
                        ui.label(t(z,"高度","Height"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.allocate_ui_with_layout(egui::vec2(180_f32.min(ui.available_width()),36.),egui::Layout::left_to_right(egui::Align::Center),|ui| {
                                if row.width != "XL" || split { appearance::size_choices(ui,&mut height,&[("1F".into(),"1F"),("2F".into(),"2F"),("4F".into(),"4F")]); }
                                else {ui.label("1F");}
                            });
                        });
                    });
                    if matches!(row.width.as_str(),"L"|"XL") {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(t(z,"组合方式","Arrangement"));
                            let whole=ui.add_enabled_ui(row.width!="XL",|ui| { ui.selectable_value(&mut split,false,format!("{} {}",t(z,"整块","Whole"),row.width)); });
                            if row.width=="XL" { whole.response.on_hover_text(t(z,"整块 XL 安装面待校准；可用 S＋M。","Full XL seating datums need calibration; use S+M.")); }
                            ui.selectable_value(&mut split,true,if row.width=="L" {"S + S"} else {"S + M"});
                        });
                    }
                    if height!=row.height || split!=(row.members.len()==2) { action=Some(Action::Row(row.level,height,split)); }
                    for (column, &index) in row.members.iter().enumerate() {
                        ui.push_id(index, |ui| {
                            if row.members.len() > 1 { ui.small(if column==0 {t(z,"左侧","Left")} else {t(z,"右侧","Right")}); }
                            self.tower_cell(ui,index,&mut action);
                        });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        let checked_count=row.members.iter().filter(|i|self.member_selection.contains(i)).count();
                        let mut checked=checked_count==row.members.len();
                        let mixed=checked_count>0 && !checked;
                        if ui.add(egui::Checkbox::new(&mut checked,t(z,"勾选导出","Include in export")).indeterminate(mixed)).changed() {
                            for &i in &row.members {if checked {self.member_selection.insert(i);} else {self.member_selection.remove(&i);}}
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if layers::icon_button(ui,layers::Icon::Down,row.level>0,t(z,"下移","Move down")).clicked() && row.level>0 {action=Some(Action::Move(row.level,row.level-1));}
                            if layers::icon_button(ui,layers::Icon::Up,row.level+1<all.len() as u32,t(z,"上移","Move up")).clicked() && row.level+1<all.len() as u32 {action=Some(Action::Move(row.level,row.level+1));}
                        });
                    });
                });
                if let Some(open) = card.open {
                    self.tower.open_layer=if open {Some(row.level)} else {None};
                    if open && !expanded {self.select_member_in_combination(row.members[0],ctx);}
                }
                if card.action.is_some() {action=card.action;}
                if let Some(from)=card.response.dnd_release_payload::<u32>() && *from!=row.level {action=Some(Action::Move(*from,row.level));}
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button(t(z,"＋ 抽屉层","＋ Drawer layer")).clicked() { action=Some(Action::Add(AssemblyKind::Drawer)); }
                if ui.button(t(z,"＋ 箱子层","＋ Box layer")).clicked() { action=Some(Action::Add(AssemblyKind::Sustainer)); }
            });
            ui.add_space(8.);
            let base_empty=first.members.iter().all(|&i|self.members[i].exterior.base==crate::catalog::exterior::Base::None);
            layers::section(ui,"tower-bases",if base_empty {t(z,"无底部","No base")} else {t(z,"底部配置","Base")},&format!("{}{}",first.width,first.depth),base_empty,false,|ui| {
                ui.set_min_width(ui.available_width());
                for (column, &i) in first.members.iter().enumerate() { ui.push_id(("tower-base",i),|ui| {
                    if first.members.len()>1 { ui.small(format!("{} · {}", if column==0 {t(z,"左侧","Left")} else {t(z,"右侧","Right")}, self.members[i].preset.selection.width)); }
                    let mut base=self.members[i].exterior.base;
                    egui::ComboBox::from_id_salt("base").width(ui.available_width()).selected_text(base_name(base,z)).show_ui(ui,|ui| {
                        for value in [crate::catalog::exterior::Base::None,crate::catalog::exterior::Base::Plain,crate::catalog::exterior::Base::Trolley] { ui.selectable_value(&mut base,value,base_name(value,z)); }
                    });
                    if base!=self.members[i].exterior.base { action=Some(Action::Base(i,base)); }
                }); }
            });
            self.workflow_undo_ui(ui,ctx);
            ui.add_space(12.);
        });
        if let Some(action) = action
            && let Err(e) = self.tower_action(action, ctx)
        {
            self.tower.error = e;
        }
    }
    fn tower_cell(&mut self, ui: &mut egui::Ui, index: usize, action: &mut Option<Action>) {
        let z = self.zh;
        let m = &self.members[index];
        let p = &m.preset;
        egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .inner_margin(10.)
            .corner_radius(8.)
            .show(ui, |ui| {
                ui.strong(format!(
                    "{}{} {}",
                    p.selection.width, p.selection.depth, p.selection.height
                ));
                ui.horizontal_wrapped(|ui| {
                    for (kind, label) in [
                        (AssemblyKind::Drawer, t(z, "抽屉", "Drawer")),
                        (AssemblyKind::Sustainer, t(z, "箱子", "Box")),
                    ] {
                        if ui
                            .selectable_label(p.assembly_kind == kind, label)
                            .clicked()
                            && kind != p.assembly_kind
                        {
                            *action = Some(Action::Kind(index, kind));
                        }
                    }
                });
                if p.assembly_kind == AssemblyKind::Drawer {
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
                        .width(ui.available_width().max(80.))
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
                        ui.separator();
                        if let Some(d) = drawer {
                            ui.strong(format!(
                                "{} {} · {}",
                                t(z, "抽屉", "Drawer"),
                                d + 1,
                                p.selection.height
                            ));
                        }
                        let interior = if !p.gridfinity.bins.is_empty() {
                            t(z, "Gridfinity 收纳", "Gridfinity bins")
                        } else if p.divider_layout.enabled || !p.sustainer_dividers.items.is_empty()
                        {
                            t(z, "收纳隔板", "Dividers")
                        } else {
                            t(z, "无内部配置", "No inserts")
                        };
                        if ui
                            .button(format!("{} · {interior}…", t(z, "内部", "Interior")))
                            .clicked()
                        {
                            *action = Some(Action::Edit(index, drawer));
                        }
                        let label = if p.label.active() {
                            p.label.text.replace('\n', " / ")
                        } else {
                            t(z, "空白", "Blank").into()
                        };
                        if ui
                            .add(
                                egui::Button::new(format!("{} · {label}…", t(z, "标牌", "Label")))
                                    .truncate(),
                            )
                            .clicked()
                        {
                            *action = Some(Action::Label(index, drawer));
                        }
                    });
                }
                if ui
                    .small_button(t(z, "背板 / 把手 / 配件…", "Back / handles / accessories…"))
                    .clicked()
                {
                    *action = Some(Action::Accessories(index));
                }
            });
    }
    pub(super) fn tower_footer_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("tower-summary")
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin {
                        top: 12,
                        ..Default::default()
                    }),
            )
            .show_inside(ui, |ui| {
                ui.separator();
                self.tower_footer(ui, ctx);
            });
    }
    fn tower_footer(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let z = self.zh;
        if !self.tower.error.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, &self.tower.error);
        }
        let items: Vec<_> = self
            .members
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
            .collect();
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
                    egui::Button::new(
                        RichText::new(t(z, "导出整个项目…", "Export project…"))
                            .color(Color32::WHITE),
                    )
                    .fill(appearance::BRAND)
                    .min_size(egui::vec2(140., 36.)),
                )
                .clicked()
            {
                self.commit_member();
                self.export_scope = workflow::ExportScope::All;
                self.export_options.format = crate::export::Format::ThreeMf;
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
        let mut changed = false;
        let mut accessories = None;
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
                        for b in &mut member.preset.bom {
                            if stack && !crate::catalog::tower::housing(&b.path) {
                                continue;
                            }
                            ui.horizontal_wrapped(|ui| {
                                let name = crate::accessories::info(&b.path, z)
                                    .map_or_else(|| b.name().to_owned(), |m| m.name);
                                changed |= ui.checkbox(&mut b.enabled, name).changed();
                                changed |= ui
                                    .add(egui::DragValue::new(&mut b.qty).range(1..=100))
                                    .changed();
                            });
                        }
                        for (i, p) in member.preset.drawers.iter_mut().enumerate() {
                            ui.collapsing(
                                format!("{} {} · {}", t(z, "抽屉", "Drawer"), i + 1, p.label.text),
                                |ui| {
                                    for b in &mut p.bom {
                                        if crate::catalog::tower::housing(&b.path) {
                                            continue;
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            let name = crate::accessories::info(&b.path, z)
                                                .map_or_else(|| b.name().to_owned(), |m| m.name);
                                            changed |= ui.checkbox(&mut b.enabled, name).changed();
                                            if !b.path.starts_with(crate::gridfinity::PREFIX) {
                                                changed |= ui
                                                    .add(
                                                        egui::DragValue::new(&mut b.qty)
                                                            .range(1..=100),
                                                    )
                                                    .changed();
                                            }
                                        });
                                    }
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
        if changed
            && let Err(e) =
                self.tower_apply(next, self.members[self.active_member].placement.level, ctx)
        {
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
        self.tower.current = self.tower.projects.len();
        self.tower.name = name;
    }
}

impl App {
    pub(super) fn setup_tower_smoke(&mut self, ctx: &egui::Context) {
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
                let mut colors = p.print_config.clone().unwrap_or_default();
                colors.preview_colors = true;
                colors.palette = [
                    [74, 104, 120],
                    [234, 151, 73],
                    [78, 156, 148],
                    [168, 184, 198],
                    [136, 109, 173],
                    [235, 235, 232],
                ];
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
    }
    pub(super) fn tower_smoke(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));
        let workspace_smoke = std::env::args().any(|a| a == "--smoke-workspace");
        let folder = self.home.join(if workspace_smoke {
            "output/workspace-ui"
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
        if self.busy || self.preview.has_pending() || self.scene.parts.is_empty() {
            return;
        }
        if self.preview_modes.mode == super::preview_modes::Mode::Plates
            && self.preview_modes.layout.is_none()
        {
            return;
        }
        self.smoke_frame += 1;
        if self.smoke_frame == 5 {
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
            if self.smoke_stage == 2 && !self.label_open {
                return Err("Subdrawer label editor did not open".into());
            }
            if self.smoke_stage == 4 {
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
                self.tower.print_list = true;
                Ok(())
            }
            13 => {
                self.tower.show_parts = true;
                Ok(())
            }
            14 => {
                self.tower.show_parts = false;
                self.tower.print_list = false;
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
                    std::fs::write(folder.join("smoke-result.txt"),"PASS: consistent configuration / print-list navigation, single-drawer editor and checklist, optional properties, English minimum-size editor, minimum-size project checklist and library, layer cards, common XL5 footprint, independent S+M cells, shared 2F housing / two 1F drawers, label editing, print list, print plates, project switching, layer duplication, undo, full and partial combination explosion, closed-view return and TOML roundtrip. All source geometry remains read-only; assembly estimates unverified.").map_err(|e|e.to_string())
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
