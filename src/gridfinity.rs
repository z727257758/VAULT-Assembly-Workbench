//! Gridfinity Rebuilt adapter: upstream geometry, local layouts, no source STL writes.
use crate::{
    catalog::{Bom, Selection},
    constraints::{self, Joint, PartId, Relation},
    mesh::Mesh,
    scene::{Part, Pose, Scene},
};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
pub const PREFIX: &str = "generated/gridfinity/";
pub const GROUP: &str = "Gridfinity bins";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bin {
    pub id: u32,
    pub cell: [u32; 2],
    pub span: [u32; 2],
    pub auto_height: bool,
    pub height_units: u32,
    pub tab: bool,
    pub detachable_label: bool,
    // Presets predating this field retain their original flat interior.
    #[serde(default)]
    pub scoop: bool,
    pub label: crate::label::Config,
}
impl Default for Bin {
    fn default() -> Self {
        Self {
            id: 1,
            cell: [0, 0],
            span: [1, 1],
            auto_height: true,
            height_units: 3,
            tab: true,
            detachable_label: false,
            scoop: true,
            label: crate::label::Config {
                size: 4.,
                text: String::new(),
                ..Default::default()
            },
        }
    }
}
impl Bin {
    pub fn label_path(&self) -> String {
        format!("{PREFIX}Cullenect-{}.stl", self.id)
    }
    pub fn detachable(&self) -> bool {
        self.tab && self.detachable_label
    }
    pub fn path(&self) -> String {
        format!("{PREFIX}Gridfinity-{}.stl", self.id)
    }
}
/// Explicit batch fields; identity, placement and individual text are never copied.
#[derive(Clone, Debug)]
pub struct BatchFields {
    pub span: bool,
    pub height: bool,
    pub scoop: bool,
    pub tab: bool,
    pub lettering: bool,
}
impl Default for BatchFields {
    fn default() -> Self {
        Self {
            span: false,
            height: true,
            scoop: true,
            tab: true,
            lettering: false,
        }
    }
}
impl BatchFields {
    pub fn any(&self) -> bool {
        self.span || self.height || self.scoop || self.tab || self.lettering
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub baseplate: String,
    pub bins: Vec<Bin>,
}
pub fn grid(s: &Selection) -> Result<[u32; 2], String> {
    let x = match s.width.as_str() {
        "S" => 3,
        "M" => 5,
        "L" => 6,
        _ => return Err("Gridfinity 编辑暂支持 S/M/L 非分体抽屉".into()),
    };
    if !(3..=7).contains(&s.depth) {
        return Err("无效抽屉深度".into());
    }
    Ok([x, s.depth as u32])
}
pub fn grid_for(s: &Selection, path: &str) -> Result<[u32; 2], String> {
    let mut g = grid(s)?;
    if path.contains("Handle") {
        g[0] = match s.width.as_str() {
            "S" => 2,
            "M" => 4,
            _ => g[0],
        };
    }
    Ok(g)
}
pub fn max_units(s: &Selection) -> Result<u32, String> {
    // Drawer internal heights measured from the source floor to its rim.
    // Rebuilt measures 7*U + 3.55 mm overall. Text stays below the stacking rim.
    // Reserve 1.3 mm seating offset and at least 1.5 mm above the actual rim.
    let clear = match s.height.as_str() {
        "1F" => 34.5_f32,
        "2F" => 82.5,
        "4F" => 178.5,
        _ => return Err("无效抽屉层高".into()),
    };
    Ok(((clear - 1.3 - 3.55 - 1.5) / 7.).floor() as u32)
}
pub fn units(b: &Bin, s: &Selection) -> Result<u32, String> {
    Ok(if b.auto_height {
        max_units(s)?
    } else {
        b.height_units
    })
}
impl Layout {
    /// Apply to a candidate layout and commit only if every target remains valid.
    pub fn apply_batch(
        &mut self,
        source: u32,
        targets: &[u32],
        fields: &BatchFields,
        s: &Selection,
    ) -> Result<usize, String> {
        let template = self
            .bins
            .iter()
            .find(|b| b.id == source)
            .cloned()
            .ok_or("来源盒子不存在")?;
        if !fields.any() {
            return Err("请勾选要批量同步的参数".into());
        }
        let mut next = self.clone();
        let mut count = 0;
        for id in targets
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
        {
            if id == source {
                continue;
            }
            let b = next
                .bins
                .iter_mut()
                .find(|b| b.id == id)
                .ok_or("目标盒子不存在")?;
            if fields.span {
                b.span = template.span;
            }
            if fields.height {
                b.auto_height = template.auto_height;
                b.height_units = template.height_units;
            }
            if fields.scoop {
                b.scoop = template.scoop;
            }
            if fields.tab {
                b.tab = template.tab;
                b.detachable_label = template.detachable_label;
            }
            if fields.lettering {
                let text = std::mem::take(&mut b.label.text);
                let enabled = b.label.enabled;
                b.label = template.label.clone();
                b.label.text = text;
                b.label.enabled = enabled;
            }
            if b.tab && b.label.active() {
                b.label.validate().map_err(|e| format!("盒子 {id}：{e}"))?;
                if b.detachable() && b.label.depth > 0.4 {
                    return Err(format!(
                        "盒子 {id}：可拆标牌刻深超过 0.4 mm；请先单独调低刻深，或勾选文字样式同步"
                    ));
                }
            }
            count += 1;
        }
        next.validate(s)?;
        *self = next;
        Ok(count)
    }
    pub fn validate(&self, s: &Selection) -> Result<(), String> {
        if self.bins.is_empty() {
            return Ok(());
        }
        let g = grid_for(s, &self.baseplate)?;
        let max = max_units(s)?;
        if self.bins.len() > 42 {
            return Err("最多 42 个盒子".into());
        }
        for (i, b) in self.bins.iter().enumerate() {
            if b.id == 0 || self.bins[..i].iter().any(|p| p.id == b.id) {
                return Err("盒子编号重复或无效".into());
            }
            if (0..2).any(|a| b.span[a] == 0 || b.span[a] > g[a] || b.cell[a] > g[a] - b.span[a]) {
                return Err("盒子超出底板网格".into());
            }
            if self.bins[..i].iter().any(|a| {
                (0..2)
                    .all(|k| a.cell[k] < b.cell[k] + b.span[k] && b.cell[k] < a.cell[k] + a.span[k])
            }) {
                return Err("盒子占用的网格重叠".into());
            }
            if units(b, s)? < 3 || units(b, s)? > max {
                return Err(format!("盒高需为 3–{max} U（含标签台及顶部净空）"));
            }
            if b.tab && b.label.active() {
                b.label.validate()?;
                if b.detachable() && b.label.depth > 0.4 {
                    return Err("Detachable label text depth must be at most 0.4 mm".into());
                }
            }
        }
        Ok(())
    }
    pub fn add(&mut self, s: &Selection, template: Option<Bin>) -> Result<u32, String> {
        let g = grid_for(s, &self.baseplate)?;
        let mut b = template.unwrap_or_default();
        b.id = self
            .bins
            .iter()
            .map(|b| b.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("盒子编号溢出")?;
        for y in 0..g[1] {
            for x in 0..g[0] {
                b.cell = [x, y];
                let mut next = self.clone();
                next.bins.push(b.clone());
                if next.validate(s).is_ok() {
                    *self = next;
                    return Ok(b.id);
                }
            }
        }
        Err("没有可容纳此盒子的空位".into())
    }
    pub fn sync_bom(&self, bom: &mut Vec<Bom>) {
        bom.retain(|b| !b.path.starts_with(PREFIX));
        for b in &self.bins {
            bom.push(Bom {
                path: b.path(),
                qty: 1,
                enabled: true,
                group: GROUP.into(),
            });
            if b.detachable() {
                bom.push(Bom {
                    path: b.label_path(),
                    qty: 1,
                    enabled: true,
                    group: GROUP.into(),
                });
            }
        }
    }
}
fn find_executable(
    home: &Path,
    configured: Option<PathBuf>,
    system: Vec<PathBuf>,
) -> Option<PathBuf> {
    [
        "tools/openscad/openscad.com",
        "tools/openscad/openscad-2021.01/openscad.com",
        "openscad/openscad.com",
    ]
    .into_iter()
    .map(|p| home.join(p))
    .chain(configured.map(|p| if p.is_absolute() { p } else { home.join(p) }))
    .chain(system)
    .find(|p| p.is_file())
}
pub fn executable(home: &Path) -> Option<PathBuf> {
    let system = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| PathBuf::from(p).join("OpenSCAD/openscad.com"))
        .chain(std::env::var_os("PATH").into_iter().flat_map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("openscad.com"))
                .collect::<Vec<_>>()
        }))
        .collect();
    find_executable(
        home,
        std::env::var_os("VAULT_OPENSCAD").map(PathBuf::from),
        system,
    )
}

fn script(home: &Path, b: &Bin, s: &Selection) -> Result<String, String> {
    let lib = home
        .join("vendor/gridfinity-rebuilt/src/core")
        .to_string_lossy()
        .replace('\\', "/");
    let h = units(b, s)? as f32 * 7.;
    let mut scad = format!(
        "use <{lib}/bin.scad>\nuse <{lib}/cutouts.scad>\n$fa=6; $fs=0.5;\nb=new_bin([{},{}],{h},include_lip=true);\nmodule box() {{ bin_render(b) compartment_cutter(bin_get_infill_size_mm(b)-[0.6,0.6,0],tab_width={},tab_angle=270{}); }}\n",
        b.span[0],
        b.span[1],
        if b.tab { 38 } else { 0 },
        if b.scoop { ",scoop_percent=1" } else { "" }
    );
    scad += "box();\n";
    Ok(scad)
}
#[cfg(test)]
fn generate_body(home: &Path, b: &Bin, s: &Selection) -> Result<Arc<Mesh>, String> {
    generate_body_cancel(home, b, s, None)
}
fn generate_body_cancel(
    home: &Path,
    b: &Bin,
    s: &Selection,
    cancel: Option<&crate::preview::Cancel>,
) -> Result<Arc<Mesh>, String> {
    let body = script(home, b, s)?;
    let hash = body.bytes().fold(14695981039346656037_u64, |h, c| {
        (h ^ c as u64).wrapping_mul(1099511628211)
    });
    let dir = home.join("state/gridfinity-cache/bodies");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let output = dir.join(format!("rebuilt-910e22d-v1-{hash:016x}.stl"));
    if let Some(mesh) = cached(&output) {
        return Ok(mesh);
    }
    let _permit = claim_body(&output, cancel)?;
    crate::preview::check(cancel)?;
    if let Some(mesh) = cached(&output) {
        return Ok(mesh);
    }
    if !output.is_file() {
        let exe = executable(home)
            .ok_or("未找到 OpenSCAD，请运行 tools/setup-gridfinity.ps1 或设置 VAULT_OPENSCAD")?;
        let work = tempfile::Builder::new()
            .prefix("generate-")
            .tempdir_in(&dir)
            .map_err(|e| e.to_string())?;
        let src = work.path().join("bin.scad");
        let mesh = work.path().join("bin.stl");
        let log = work.path().join("render.log");
        std::fs::write(&src, body).map_err(|e| e.to_string())?;
        let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
        let mut command = std::process::Command::new(exe);
        command
            .current_dir(home)
            .arg("--export-format")
            .arg("binstl")
            .arg("-o")
            .arg(&mesh)
            .arg(&src)
            .stdout(std::process::Stdio::null())
            .stderr(file);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|e| format!("OpenSCAD: {e}"))?;
        let start = Instant::now();
        loop {
            if let Err(e) = crate::preview::check(cancel) {
                terminate_generator(&mut child);
                return Err(e);
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() || !mesh.is_file() {
                    return Err(format!(
                        "Gridfinity 生成失败：{}",
                        std::fs::read_to_string(log)
                            .unwrap_or_default()
                            .chars()
                            .take(1800)
                            .collect::<String>()
                    ));
                }
                break;
            }
            if start.elapsed() > Duration::from_secs(180) {
                terminate_generator(&mut child);
                return Err("Gridfinity 生成超过 180 秒，请减少跨格尺寸".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Mesh::load(&mesh)?;
        // Publish the completed cache file atomically. Concurrent preview/export may finish together.
        if let Err(e) = std::fs::hard_link(&mesh, &output)
            && e.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(e.to_string());
        }
    }
    let mesh = Mesh::load(&output)?;
    remember(&output, mesh.clone());
    Ok(mesh)
}
fn terminate_generator(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // OpenSCAD's console launcher may own a GUI executable; stop only this job's tree.
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
// Global bound includes concurrent preview/export. A key has only one producer.
static BODY_JOBS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Default::default()));
struct BodyPermit(PathBuf);
impl Drop for BodyPermit {
    fn drop(&mut self) {
        BODY_JOBS.lock().unwrap().remove(&self.0);
    }
}
fn claim_body(path: &Path, cancel: Option<&crate::preview::Cancel>) -> Result<BodyPermit, String> {
    loop {
        crate::preview::check(cancel)?;
        {
            let mut jobs = BODY_JOBS.lock().unwrap();
            if jobs.len() < 2 && !jobs.contains(path) {
                jobs.insert(path.to_owned());
                return Ok(BodyPermit(path.to_owned()));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn socket_key(home: &Path, b: &Bin, s: &Selection) -> Result<PathBuf, String> {
    let hash = script(home, b, s)?
        .bytes()
        .fold(14695981039346656037_u64, |h, c| {
            (h ^ c as u64).wrapping_mul(1099511628211)
        });
    Ok(home
        .join("state/gridfinity-cache")
        .join(format!("native-cullenect-v1-{hash:016x}.stl")))
}
fn preview_mesh(home: &Path, b: &Bin, s: &Selection) -> Result<Arc<Mesh>, String> {
    if b.detachable() {
        return Ok(cached(&socket_key(home, b, s)?).unwrap_or(proxy(b, s)?));
    }
    let body = script(home, b, s)?;
    let (key, dir, prefix) = if b.tab && b.label.active() {
        (
            format!(
                "rust-lettering-v1\n{}\n{}",
                body,
                toml::to_string(&b.label).map_err(|e| e.to_string())?
            ),
            "lettering",
            "",
        )
    } else {
        (body, "bodies", "rebuilt-910e22d-v1-")
    };
    let hash = key.bytes().fold(14695981039346656037_u64, |h, c| {
        (h ^ c as u64).wrapping_mul(1099511628211)
    });
    let path = home
        .join("state/gridfinity-cache")
        .join(dir)
        .join(format!("{prefix}{hash:016x}.stl"));
    if let Some(mesh) = cached(&path) {
        return Ok(mesh);
    }
    proxy(b, s)
}
// Display-only open box. Never cached or used by export.
fn proxy(b: &Bin, s: &Selection) -> Result<Arc<Mesh>, String> {
    let x = b.span[0] as f32 * 42. - 0.5;
    let z = b.span[1] as f32 * 42. - 0.5;
    let h = units(b, s)? as f32 * 7. + 3.55;
    let mut vertices = Vec::new();
    let mut block = |lo: Vec3, hi: Vec3| {
        let points = [
            Vec3::new(lo.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, lo.z),
            Vec3::new(hi.x, hi.y, lo.z),
            Vec3::new(lo.x, hi.y, lo.z),
            Vec3::new(lo.x, lo.y, hi.z),
            Vec3::new(hi.x, lo.y, hi.z),
            Vec3::new(hi.x, hi.y, hi.z),
            Vec3::new(lo.x, hi.y, hi.z),
        ];
        for [a, b, c] in [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [3, 7, 6],
            [3, 6, 2],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
        ] {
            let normal = (points[b] - points[a])
                .cross(points[c] - points[a])
                .normalize()
                .to_array();
            for i in [a, b, c] {
                vertices.push(crate::mesh::Vertex {
                    position: points[i].to_array(),
                    normal,
                    shade: 1.,
                });
            }
        }
    };
    let min = Vec3::new(-x / 2., -h / 2., -z / 2.);
    let max = -min;
    block(min, Vec3::new(max.x, min.y + 5., max.z));
    block(min, Vec3::new(min.x + 1.2, max.y, max.z));
    block(Vec3::new(max.x - 1.2, min.y, min.z), max);
    block(min, Vec3::new(max.x, max.y, min.z + 1.2));
    block(Vec3::new(min.x, min.y, max.z - 1.2), max);
    if b.tab {
        block(
            Vec3::new(min.x + 1.2, max.y - 5., max.z - 16.),
            Vec3::new(max.x - 1.2, max.y - 3.5, max.z - 1.2),
        );
    }
    Ok(Arc::new(Mesh {
        vertices,
        min,
        max,
        size: Vec3::new(x, z, h),
    }))
}

// A bounded shared cache keeps unrelated boxes and blank bodies available during typing.
type MeshCache = std::collections::VecDeque<(PathBuf, Arc<Mesh>)>;
static MESH_CACHE: std::sync::LazyLock<std::sync::Mutex<MeshCache>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Default::default()));
fn cached(path: &Path) -> Option<Arc<Mesh>> {
    let mut cache = MESH_CACHE.lock().ok()?;
    let i = cache.iter().position(|(p, _)| p == path)?;
    let entry = cache.remove(i)?;
    let mesh = entry.1.clone();
    cache.push_back(entry);
    Some(mesh)
}
fn remember(path: &Path, mesh: Arc<Mesh>) {
    if let Ok(mut cache) = MESH_CACHE.lock() {
        cache.retain(|(p, _)| p != path);
        cache.push_back((path.to_path_buf(), mesh));
        while cache.len() > 16
            || cache.iter().map(|(_, m)| m.vertices.len()).sum::<usize>() > 2_000_000
        {
            cache.pop_front();
        }
    }
}
#[cfg(test)]
pub fn generate(home: &Path, b: &Bin, s: &Selection) -> Result<Arc<Mesh>, String> {
    generate_cancel(home, b, s, None)
}
fn generate_cancel(
    home: &Path,
    b: &Bin,
    s: &Selection,
    cancel: Option<&crate::preview::Cancel>,
) -> Result<Arc<Mesh>, String> {
    crate::preview::check(cancel)?;
    if b.detachable() {
        let path = socket_key(home, b, s)?;
        if let Some(mesh) = cached(&path) {
            return Ok(mesh);
        }
        let body = generate_body_cancel(home, b, s, cancel)?;
        let (offset, top) = label_seat(b, s, &body)?;
        let mesh = crate::cullenect::socket(&body, offset, top)?;
        remember(&path, mesh.clone());
        return Ok(mesh);
    }
    // Validate text before potentially starting an expensive new body render.
    let polygons = if b.tab && b.label.active() {
        b.label.validate()?;
        let polygons = crate::label::text_polygons(&b.label)?;
        if polygons
            .iter()
            .flatten()
            .any(|p| p.x.abs() > 17. || p.y.abs() > 5.)
        {
            return Err("盒子标签文字超出 34 × 10 mm 安全区，请缩小字号".into());
        }
        Some(polygons)
    } else {
        None
    };
    let Some(polygons) = polygons else {
        return generate_body_cancel(home, b, s, cancel);
    };
    let key = format!(
        "rust-lettering-v1\n{}\n{}",
        script(home, b, s)?,
        toml::to_string(&b.label).map_err(|e| e.to_string())?
    );
    let hash = key.bytes().fold(14695981039346656037_u64, |h, c| {
        (h ^ c as u64).wrapping_mul(1099511628211)
    });
    let dir = home.join("state/gridfinity-cache/lettering");
    let path = dir.join(format!("{hash:016x}.stl"));
    if let Some(mesh) = cached(&path) {
        return Ok(mesh);
    }
    let body = generate_body_cancel(home, b, s, cancel)?;
    crate::preview::check(cancel)?;
    let top = body.min.y + units(b, s)? as f32 * 7. - 1.2;
    let tab_y = -(b.span[1] as f32 * 42. - 0.5) / 2. + 1.25 + 15.85 / 2.;
    let result = if path.is_file() {
        Mesh::load(&path)?
    } else {
        let mesh =
            crate::label::apply_text(&body, &b.label, &polygons, [0., tab_y as f64], top as f64)?;
        if (mesh.min - body.min).abs().max_element() > 0.005
            || (mesh.max - body.max).abs().max_element() > 0.005
        {
            return Err("标签文字改变了盒体安装边界，停止生成".into());
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let staging = tempfile::Builder::new()
            .prefix("lettering-")
            .tempdir_in(&dir)
            .map_err(|e| e.to_string())?;
        let temp = staging.path().join("bin.stl");
        crate::label::write_stl(&mesh, &temp)?;
        if let Err(e) = std::fs::hard_link(&temp, &path)
            && e.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(e.to_string());
        }
        mesh
    };
    // STL stores geometry only; restore the same text shading after a cache reload.
    let mut mesh = (*result).clone();
    for tri in mesh.vertices.chunks_exact_mut(3) {
        let c = tri.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / 3.;
        let lettering = c.x.abs() < 19.01
            && (-c.z - tab_y).abs() < 6.01
            && match b.label.style {
                crate::label::Style::Raised => {
                    c.y > top + 0.001 && c.y <= top + b.label.depth + 0.001
                }
                crate::label::Style::Engraved => {
                    (c.y - (top - b.label.depth)).abs() < 0.001 && tri[0].normal[1] > 0.9
                }
            };
        for v in tri {
            v.shade = if lettering { 0.45 } else { 1. };
        }
    }
    let mesh = Arc::new(mesh);
    remember(&path, mesh.clone());
    Ok(mesh)
}
pub(crate) fn label_seat(b: &Bin, s: &Selection, mesh: &Mesh) -> Result<(f32, f32), String> {
    Ok((
        -(b.span[1] as f32 * 42. - 0.5) / 2. + 1.25 + 15.85 / 2.,
        mesh.min.y + units(b, s)? as f32 * 7. - 1.2,
    ))
}
pub fn generate_many(
    home: &Path,
    bins: &[&Bin],
    s: &Selection,
    cancel: Option<&crate::preview::Cancel>,
) -> Result<Vec<Arc<Mesh>>, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut keys = Vec::new();
    let mut unique = Vec::new();
    let mut indices = Vec::new();
    for bin in bins {
        let mut key = script(home, bin, s)?;
        key += if bin.detachable() {
            "\ncullenect-v1"
        } else {
            ""
        };
        if bin.tab && !bin.detachable() && bin.label.active() {
            key += &toml::to_string(&bin.label).map_err(|e| e.to_string())?;
        }
        let index = if let Some(i) = keys.iter().position(|k| k == &key) {
            i
        } else {
            keys.push(key);
            unique.push(*bin);
            unique.len() - 1
        };
        indices.push(index);
    }
    if unique.is_empty() {
        return Ok(Vec::new());
    }
    let next = AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..unique.len().min(2) {
            let next = &next;
            let unique = &unique;
            let tx = tx.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= unique.len() {
                        break;
                    }
                    let result = generate_cancel(home, unique[i], s, cancel);
                    if tx.send((i, result)).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(tx);
    let mut results = vec![None; unique.len()];
    for (i, result) in rx {
        results[i] = Some(result);
    }
    let meshes = results
        .into_iter()
        .map(|r| r.ok_or_else(|| "盒子生成线程异常".to_string())?)
        .collect::<Result<Vec<_>, String>>()?;
    Ok(indices.into_iter().map(|i| meshes[i].clone()).collect())
}
// Recover socket centers from opposing vertical faces, independent of handles or asymmetric bounds.
fn centers(mesh: &Mesh, axis: usize, count: usize) -> Result<Vec<f32>, String> {
    let mut levels = BTreeMap::<i32, f32>::new();
    for tri in mesh.vertices.chunks_exact(3) {
        let a = Vec3::from(tri[0].position);
        let b = Vec3::from(tri[1].position);
        let c = Vec3::from(tri[2].position);
        let n = (b - a).cross(c - a);
        let area = n.length();
        if area > 0.001
            && n[axis].abs() > area * 0.999
            && (a.y + b.y + c.y) / 3. < mesh.min.y + 4.01
        {
            *levels.entry((a[axis] * 100.).round() as i32).or_default() += area;
        }
    }
    let positions: Vec<f32> = levels
        .into_iter()
        .filter(|(_, area)| *area > 1.)
        .map(|(v, _)| v as f32 / 100.)
        .collect();
    for &lo in &positions {
        let first = lo + 18.85;
        let candidate: Vec<f32> = (0..count).map(|i| first + i as f32 * 42.).collect();
        if candidate.iter().all(|c| {
            [-18.85, 18.85]
                .iter()
                .all(|offset| positions.iter().any(|p| (*p - *c - *offset).abs() < 0.04))
        }) {
            return Ok(candidate);
        }
    }
    Err("无法识别底板的 42 mm 标准孔位，未生成估算装配".into())
}
#[cfg(test)]
pub fn apply(
    scene: &mut Scene,
    home: &Path,
    layout: &Layout,
    s: &Selection,
    items: &[Bom],
) -> Result<(), String> {
    apply_preview(scene, home, layout, s, items, false, None)
}
pub fn apply_preview(
    scene: &mut Scene,
    home: &Path,
    layout: &Layout,
    s: &Selection,
    items: &[Bom],
    draft: bool,
    cancel: Option<&crate::preview::Cancel>,
) -> Result<(), String> {
    layout.validate(s)?;
    let selected: Vec<_> = layout
        .bins
        .iter()
        .filter(|b| items.iter().any(|i| i.enabled && i.path == b.path()))
        .collect();
    if selected.is_empty()
        && !layout
            .bins
            .iter()
            .any(|b| b.detachable() && items.iter().any(|i| i.enabled && i.path == b.label_path()))
    {
        return Ok(());
    }
    let plate = scene
        .parts
        .iter()
        .position(|p| p.pose.path.contains("/Gridfinity Baseplate/"));
    let g = grid_for(s, &layout.baseplate)?;
    let geometry = if scene.assembly {
        let p = plate.ok_or("Gridfinity 盒子需要收纳底板")?;
        if layout.baseplate != scene.parts[p].pose.path {
            return Err("盒子布局与当前底板不一致".into());
        }
        Some((
            centers(&scene.parts[p].mesh, 0, g[0] as usize)?,
            centers(&scene.parts[p].mesh, 2, g[1] as usize)?,
        ))
    } else {
        None
    };
    let meshes = if draft {
        selected.iter().map(|b| preview_mesh(home, b, s)).collect()
    } else {
        generate_many(home, &selected, s, cancel)
    };
    for (b, mesh) in selected.into_iter().zip(meshes?) {
        let index = scene.parts.len();
        let size = mesh.size;
        scene.parts.push(Part {
            mesh: mesh.clone(),
            pose: Pose {
                path: b.path(),
                instance: 0,
                body: None,
                position: [0.; 3],
                rotation: [0.; 3],
                travel: [0.; 3],
                travel_frame: None,
            },
            color: [0.78, 0.65, 0.40, 1.],
            visible: true,
            source_size: size,
            source_bodies: 1,
        });
        if let (Some(p), Some((xs, zs))) = (plate, &geometry) {
            let position = [
                xs[(g[0] - b.cell[0] - b.span[0]) as usize] + (b.span[0] - 1) as f32 * 21.,
                scene.parts[p].mesh.min.y + 1.3 - mesh.min.y,
                zs[(g[1] - b.cell[1] - b.span[1]) as usize] + (b.span[1] - 1) as f32 * 21.,
            ];
            constraints::add(
                scene,
                Relation {
                    parent: PartId::of(&scene.parts[p]),
                    child: PartId::of(&scene.parts[index]),
                    joint: Joint::Fixed {
                        position,
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    label: "Gridfinity bin / measured baseplate sockets".into(),
                },
            )?;
        }
    }
    for b in layout.bins.iter().filter(|b| b.detachable()) {
        if !items.iter().any(|i| {
            i.enabled && (i.path == b.label_path() || (!scene.assembly && i.path == b.path()))
        }) {
            continue;
        }
        let parent = scene.parts.iter().position(|p| p.pose.path == b.path());
        let (offset, top) = if let Some(parent) = parent {
            label_seat(b, s, &scene.parts[parent].mesh)?
        } else {
            (0., 1.2)
        };
        let mesh = crate::cullenect::label(&b.label)?;
        let index = scene.parts.len();
        scene.parts.push(Part {
            source_size: mesh.size,
            mesh,
            pose: Pose {
                path: b.label_path(),
                instance: 0,
                body: None,
                position: [0., top - 1.2, -offset],
                rotation: [0.; 3],
                travel: [0.; 3],
                travel_frame: None,
            },
            color: [0.9, 0.9, 0.9, 1.],
            visible: true,
            source_bodies: 1,
        });
        if let Some(parent) = parent {
            constraints::add(
                scene,
                Relation {
                    parent: PartId::of(&scene.parts[parent]),
                    child: PartId::of(&scene.parts[index]),
                    joint: Joint::Fixed {
                        position: [0., top - 1.2, -offset],
                        rotation: Quat::IDENTITY.to_array(),
                    },
                    label: "Cullenect / nominal dimensions, print fit unverified".into(),
                },
            )?;
        }
    }
    if scene.assembly {
        crate::explosion::reset(scene)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn batch_layout() -> Layout {
        Layout {
            bins: (0..3)
                .map(|i| Bin {
                    id: i + 1,
                    cell: [i, 0],
                    label: crate::label::Config {
                        enabled: i != 2,
                        text: format!("Part {i}"),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn batch_parameters_preserve_text_identity_and_unselected_bins() {
        let mut layout = batch_layout();
        layout.bins[0].auto_height = false;
        layout.bins[0].height_units = 3;
        layout.bins[0].scoop = false;
        layout.bins[0].detachable_label = true;
        layout.bins[0].label.depth = 0.2;
        layout.bins[0].label.font = crate::label::Font::ArialBold;
        let before = layout.clone();
        let fields = BatchFields {
            lettering: true,
            ..Default::default()
        };
        assert_eq!(
            layout
                .apply_batch(1, &[1, 2, 2], &fields, &Selection::default())
                .unwrap(),
            1
        );
        assert_eq!(layout.bins[0], before.bins[0]);
        assert_eq!(layout.bins[2], before.bins[2]);
        let target = &layout.bins[1];
        assert_eq!(
            (target.id, target.cell, target.span),
            (before.bins[1].id, before.bins[1].cell, before.bins[1].span)
        );
        assert_eq!(target.label.text, before.bins[1].label.text);
        assert_eq!(target.label.enabled, before.bins[1].label.enabled);
        assert!(target.detachable());
        assert_eq!(target.label.depth, 0.2);
        assert!(!target.auto_height);
        assert!(!target.scoop);
        layout
            .apply_batch(1, &[2, 3], &fields, &Selection::default())
            .unwrap();
        assert!(!layout.bins[2].label.enabled);
        assert_eq!(layout.bins[2].label.text, before.bins[2].label.text);
        let round: Layout = toml::from_str(&toml::to_string(&layout).unwrap()).unwrap();
        assert_eq!(round, layout);
    }
    #[test]
    fn batch_rejects_size_conflicts_and_incompatible_labels_atomically() {
        let s = Selection::default();
        let mut layout = batch_layout();
        layout.bins[0].span = [1, 2];
        layout.bins[2].cell = [2, 2];
        let before = layout.clone();
        assert!(
            layout
                .apply_batch(
                    1,
                    &[2, 3],
                    &BatchFields {
                        span: true,
                        ..Default::default()
                    },
                    &s
                )
                .is_err()
        );
        assert_eq!(layout, before);
        layout.bins[0].span = [2, 1];
        layout.bins[1].cell = [0, 1];
        layout.bins[2].cell = [1, 1];
        let before = layout.clone();
        assert!(
            layout
                .apply_batch(
                    1,
                    &[2, 3],
                    &BatchFields {
                        span: true,
                        ..Default::default()
                    },
                    &s
                )
                .is_err()
        );
        assert_eq!(layout, before);
        let mut layout = batch_layout();
        layout.bins[0].detachable_label = true;
        layout.bins[0].label.depth = 0.2;
        let before = layout.clone();
        assert!(
            layout
                .apply_batch(1, &[3, 2], &BatchFields::default(), &s)
                .is_err()
        );
        assert_eq!(layout, before);
        assert!(
            layout
                .apply_batch(1, &[3, 99], &BatchFields::default(), &s)
                .is_err()
        );
        assert_eq!(layout, before);
    }
    #[test]
    fn batch_geometry_does_not_copy_individual_label_settings() {
        let mut layout = batch_layout();
        layout.bins[0].scoop = false;
        layout.bins[0].label.size = 7.;
        layout.bins[0].label.offset = [1., 2.];
        let labels: Vec<_> = layout.bins.iter().map(|b| b.label.clone()).collect();
        layout
            .apply_batch(1, &[2, 3], &BatchFields::default(), &Selection::default())
            .unwrap();
        assert_eq!(
            labels,
            layout
                .bins
                .iter()
                .map(|b| b.label.clone())
                .collect::<Vec<_>>()
        );
        assert!(layout.bins.iter().all(|b| !b.scoop));
    }
    #[test]
    fn four_unit_bins_fit_real_one_floor_drawers() {
        if std::env::var_os("VAULT_GRIDFINITY_TEST").is_none() {
            return;
        }
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let home = Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalog = crate::catalog::Catalog::scan(root.into()).unwrap();
        for width in ["S", "M", "L"] {
            let selection = Selection {
                width: width.into(),
                height: "1F".into(),
                depth: 3,
                ..Default::default()
            };
            assert_eq!(max_units(&selection).unwrap(), 4);
            let plates: Vec<_> = catalog
                .accessories(&selection)
                .into_iter()
                .filter(|e| e.path.contains("/Gridfinity Baseplate/"))
                .collect();
            assert!(!plates.is_empty());
            for plate in plates {
                for detachable in [false, true] {
                    let mut items = catalog.bom(&selection);
                    crate::catalog::Catalog::set_accessory(&mut items, plate.path.clone(), true, 1);
                    let mut scene =
                        Scene::load(&catalog.root, &items, "4U clearance".into(), true).unwrap();
                    let layout = Layout {
                        baseplate: plate.path.clone(),
                        bins: vec![Bin {
                            detachable_label: detachable,
                            label: crate::label::Config {
                                enabled: true,
                                text: "M3".into(),
                                font: crate::label::Font::ArialBold,
                                size: 4.,
                                depth: 0.4,
                                ..Default::default()
                            },
                            ..Default::default()
                        }],
                    };
                    layout.sync_bom(&mut items);
                    apply_preview(&mut scene, home, &layout, &selection, &items, false, None)
                        .unwrap();
                    let drawer = scene
                        .parts
                        .iter()
                        .find(|p| p.pose.path.contains("/Boxes/"))
                        .unwrap();
                    let ceiling = drawer
                        .pose
                        .matrix(0.)
                        .transform_point3(Vec3::new(0., drawer.mesh.max.y, 0.))
                        .y;
                    for part in scene
                        .parts
                        .iter()
                        .filter(|p| p.pose.path.starts_with(PREFIX))
                    {
                        let top = part
                            .mesh
                            .vertices
                            .iter()
                            .map(|v| {
                                part.pose
                                    .matrix(0.)
                                    .transform_point3(Vec3::from(v.position))
                                    .y
                            })
                            .fold(f32::NEG_INFINITY, f32::max);
                        assert!(
                            ceiling - top >= 1.5,
                            "{width} {} gap {}",
                            plate.path,
                            ceiling - top
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn detachable_real_geometry_and_export() {
        if std::env::var_os("VAULT_GRIDFINITY_TEST").is_none() {
            return;
        }
        let home = Path::new(env!("CARGO_MANIFEST_DIR"));
        let selection = Selection::default();
        let bin = Bin {
            detachable_label: true,
            label: crate::label::Config {
                enabled: true,
                text: "M3".into(),
                font: crate::label::Font::ArialBold,
                size: 4.,
                depth: 0.2,
                ..Default::default()
            },
            ..Default::default()
        };
        let body = generate(home, &bin, &selection).unwrap();
        let solid = crate::label::solid(&body).unwrap();
        assert_eq!(
            solid.decompose().len(),
            1,
            "socket must remain attached to the bin"
        );
        let original = generate_body(home, &bin, &selection).unwrap();
        assert!((body.min - original.min).abs().max_element() < 0.001);
        assert!((body.max - original.max).abs().max_element() < 0.001);
        let (offset, top) = label_seat(&bin, &selection, &body).unwrap();
        let label = crate::cullenect::label(&bin.label).unwrap();
        let installed =
            crate::label::solid(&label)
                .unwrap()
                .translate(manifold_rust::linalg::Vec3::new(
                    0.,
                    offset as f64,
                    top as f64 - 1.2,
                ));
        assert!(
            solid.intersection(&installed).volume().abs() < 0.001,
            "nominal seated geometry must not overlap"
        );
        let layout = Layout {
            bins: vec![bin.clone()],
            ..Default::default()
        };
        let mut bom = Vec::new();
        layout.sync_bom(&mut bom);
        assert_eq!(bom.len(), 2);
        let mut scene = Scene::default();
        apply_preview(&mut scene, home, &layout, &selection, &bom, false, None).unwrap();
        assert_eq!(scene.parts.len(), 2);
        let mut changed = bin.clone();
        changed.label.text = "M4".into();
        assert!(Arc::ptr_eq(
            &body,
            &generate(home, &changed, &selection).unwrap()
        ));
        let out = home.join("output/cullenect-validation");
        std::fs::create_dir_all(&out).unwrap();
        let temp = tempfile::tempdir_in(&out).unwrap();
        let root = temp.path().join("empty-library");
        std::fs::create_dir_all(&root).unwrap();
        for (format, name) in [
            (crate::export::Format::Stl, "parts"),
            (crate::export::Format::ThreeMf, "parts.3mf"),
        ] {
            let dest = temp.path().join(name);
            let mut options = crate::export::Options {
                format,
                ..Default::default()
            };
            options.plate.text_filaments.insert(bin.label_path(), 2);
            crate::export::run_configured(
                &root,
                &bom,
                &dest,
                &options,
                &Default::default(),
                Some((home, &layout, &selection)),
            )
            .unwrap();
            if format == crate::export::Format::Stl {
                assert_eq!(std::fs::read_dir(dest).unwrap().count(), 2);
            } else {
                use std::io::Read;
                let mut zip = zip::ZipArchive::new(std::fs::File::open(dest).unwrap()).unwrap();
                let mut model = String::new();
                zip.by_name("3D/3dmodel.model")
                    .unwrap()
                    .read_to_string(&mut model)
                    .unwrap();
                assert_eq!(model.matches("<item ").count(), 2);
                let mut settings = String::new();
                zip.by_name("Metadata/model_settings.config")
                    .unwrap()
                    .read_to_string(&mut settings)
                    .unwrap();
                assert!(settings.contains("Cullenect-1.stl"));
                assert_eq!(settings.matches("<part ").count(), 3);
                assert_eq!(settings.matches("key=\"extruder\" value=\"2\"").count(), 1);
            }
        }
        crate::label::write_stl(&body, &out.join("Gridfinity-1-with-socket.stl")).unwrap();
        crate::label::write_stl(&label, &out.join("Cullenect-1-M3.stl")).unwrap();
    }
    #[test]
    fn detachable_old_presets_and_label_only_preview() {
        assert!(!toml::from_str::<Bin>("id=1").unwrap().detachable());
        let bin = Bin {
            detachable_label: true,
            ..Default::default()
        };
        let layout = Layout {
            bins: vec![bin.clone()],
            ..Default::default()
        };
        let items = vec![Bom {
            path: bin.label_path(),
            qty: 1,
            enabled: true,
            group: GROUP.into(),
        }];
        let mut scene = Scene::default();
        apply_preview(
            &mut scene,
            Path::new("missing-generator"),
            &layout,
            &Selection::default(),
            &items,
            false,
            None,
        )
        .unwrap();
        assert_eq!(scene.parts.len(), 1);
        assert_eq!(scene.parts[0].pose.path, bin.label_path());
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/cullenect-validation");
        std::fs::create_dir_all(&out).unwrap();
        let temp = tempfile::tempdir_in(out).unwrap();
        let root = temp.path().join("library");
        std::fs::create_dir_all(&root).unwrap();
        crate::export::run_design(
            &root,
            &items,
            &temp.path().join("label-only"),
            crate::export::Format::Stl,
            &Default::default(),
            Some((
                Path::new("missing-generator"),
                &layout,
                &Selection::default(),
            )),
        )
        .unwrap();
    }
    #[test]
    fn relative_generator_search_prefers_bundled_and_ignores_working_directory() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("application");
        let local = home.join("tools/openscad/openscad.com");
        let legacy = home.join("tools/openscad/openscad-2021.01/openscad.com");
        let custom = home.join("custom/openscad.com");
        let system = temp.path().join("system/openscad.com");
        for p in [&legacy, &custom, &system] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, []).unwrap();
        }
        let configured = Some(PathBuf::from("custom/openscad.com"));
        assert_eq!(
            find_executable(&home, configured.clone(), vec![system.clone()]),
            Some(legacy.clone())
        );
        std::fs::write(&local, []).unwrap();
        assert_eq!(
            find_executable(&home, configured.clone(), vec![system.clone()]),
            Some(local.clone())
        );
        std::fs::remove_file(local).unwrap();
        std::fs::remove_file(legacy).unwrap();
        assert_eq!(
            find_executable(&home, configured.clone(), vec![system.clone()]),
            Some(custom.clone())
        );
        std::fs::remove_file(custom).unwrap();
        assert_eq!(
            find_executable(&home, configured, vec![system.clone()]),
            Some(system)
        );
        assert!(find_executable(&home, None, vec![]).is_none());
    }
    #[test]
    fn real_parallel_generation_cancellation_and_cache() {
        if std::env::var_os("VAULT_GRIDFINITY_BENCH").is_none() {
            return;
        }
        fn copy_tree(src: &Path, dst: &Path) {
            std::fs::create_dir_all(dst).unwrap();
            for item in std::fs::read_dir(src).unwrap() {
                let item = item.unwrap();
                let to = dst.join(item.file_name());
                if item.path().is_dir() {
                    copy_tree(&item.path(), &to);
                } else {
                    std::fs::copy(item.path(), to).unwrap();
                }
            }
        }
        let real = Path::new(env!("CARGO_MANIFEST_DIR"));
        let temp = tempfile::tempdir_in(real.join("target")).unwrap();
        let home = temp.path();
        copy_tree(&real.join("vendor"), &home.join("vendor"));
        let s = Selection::default();
        let a = Bin {
            scoop: false,
            ..Default::default()
        };
        let b = Bin {
            scoop: true,
            ..Default::default()
        };
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let start = Instant::now();
        std::thread::scope(|scope| {
            let task = scope.spawn(|| generate_cancel(home, &a, &s, Some(&cancel)));
            std::thread::sleep(Duration::from_millis(500));
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            assert!(task.join().unwrap().err().unwrap().contains("取消"));
        });
        let cancelled = start.elapsed();
        assert!(cancelled < Duration::from_secs(5));
        let start = Instant::now();
        let meshes = generate_many(home, &[&a, &b, &a, &b], &s, None).unwrap();
        let cold = start.elapsed();
        assert!(Arc::ptr_eq(&meshes[0], &meshes[2]));
        assert!(Arc::ptr_eq(&meshes[1], &meshes[3]));
        for mesh in &meshes {
            assert!(crate::label::solid(mesh).unwrap().volume() > 0.);
        }
        let start = Instant::now();
        let again = generate_many(home, &[&a, &b], &s, None).unwrap();
        let warm = start.elapsed();
        assert!(Arc::ptr_eq(&again[0], &meshes[0]));
        std::fs::write(real.join("output/gridfinity-generation-timing.txt"),
            format!("cancel: {cancelled:?}\ncold two distinct bodies (four instances): {cold:?}\nwarm: {warm:?}\n")).unwrap();
    }
    #[test]
    fn bounded_jobs_deduplicate_and_waiters_cancel() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let first = claim_body(&a, None).unwrap();
        let second = claim_body(&b, None).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let cancel = &cancel;
            let a = &a;
            scope.spawn(move || {
                tx.send(claim_body(a, Some(cancel)).is_err()).unwrap();
            });
            assert!(rx.recv_timeout(Duration::from_millis(60)).is_err());
            cancel.store(true, Ordering::Relaxed);
            assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap());
        });
        let c = dir.path().join("c");
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let _permit = claim_body(&c, None).unwrap();
                tx.send(()).unwrap();
            });
            assert!(rx.recv_timeout(Duration::from_millis(60)).is_err());
            drop(second);
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
        });
        drop(first);
        assert!(claim_body(&a, None).is_ok());
    }
    #[test]
    fn draft_needs_no_generator_and_never_enters_exact_cache() {
        let dir = tempfile::tempdir().unwrap();
        let s = Selection::default();
        let b = Bin::default();
        let draft = preview_mesh(dir.path(), &b, &s).unwrap();
        assert!(draft.vertices.len() < 300);
        assert!(
            draft
                .vertices
                .iter()
                .all(|v| Vec3::from(v.position).is_finite())
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(generate(dir.path(), &b, &s).is_err());
        assert!(generate_many(dir.path(), &[], &s, None).unwrap().is_empty());
    }
    #[test]
    fn layout_bounds_overlap_height_and_roundtrip() {
        let s = Selection::default();
        let mut layout = Layout::default();
        let id = layout.add(&s, None).unwrap();
        let other = layout.add(&s, Some(layout.bins[0].clone())).unwrap();
        assert_ne!(id, other);
        assert_ne!(layout.bins[0].cell, layout.bins[1].cell);
        let saved = toml::to_string(&layout).unwrap();
        assert_eq!(layout, toml::from_str(&saved).unwrap());
        layout.bins[1].cell = layout.bins[0].cell;
        assert!(layout.validate(&s).is_err());
        layout.bins.pop();
        layout.bins[0].span = [4, 1];
        assert!(layout.validate(&s).is_err());
        layout.bins[0].span = [1, 1];
        for (height, expected) in [("1F", 4), ("2F", 10), ("4F", 24)] {
            let selection = Selection {
                height: height.into(),
                ..s.clone()
            };
            assert_eq!(units(&layout.bins[0], &selection).unwrap(), expected);
        }
        layout.bins[0].auto_height = false;
        layout.bins[0].height_units = 4;
        assert!(layout.validate(&s).is_ok());
        layout.bins[0].height_units = 5;
        assert!(layout.validate(&s).is_err());
        let mut bom = vec![];
        layout.sync_bom(&mut bom);
        assert_eq!(bom.len(), 1);
        layout.bins.clear();
        layout.sync_bom(&mut bom);
        assert!(bom.is_empty());
    }
    #[test]
    fn lettering_reuses_body_without_generator_and_preserves_geometry() {
        if std::env::var_os("VAULT_GRIDFINITY_TEST").is_none() {
            return;
        }
        let real = Path::new(env!("CARGO_MANIFEST_DIR"));
        let selection = Selection::default();
        let mut bin = Bin::default();
        let blank = generate_body(real, &bin, &selection).unwrap();
        // This isolated workspace has neither a generator nor any library scripts.
        // Only the blank STL is copied; every new text must be processed entirely in Rust.
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let body_script = script(home, &bin, &selection).unwrap();
        let hash = body_script.bytes().fold(14695981039346656037_u64, |h, c| {
            (h ^ c as u64).wrapping_mul(1099511628211)
        });
        let dir = home.join("state/gridfinity-cache/bodies");
        std::fs::create_dir_all(&dir).unwrap();
        let body_path = dir.join(format!("rebuilt-910e22d-v1-{hash:016x}.stl"));
        crate::label::write_stl(&blank, &body_path).unwrap();
        let original = std::fs::read(&body_path).unwrap();
        let base_volume = crate::label::solid(&blank).unwrap().volume();
        let mut timings = String::new();
        for (text, style) in [
            ("电阻\nA1", crate::label::Style::Raised),
            ("电容\nB8", crate::label::Style::Engraved),
            ("螺丝", crate::label::Style::Raised),
        ] {
            bin.label = crate::label::Config {
                enabled: true,
                text: text.into(),
                style,
                size: 4.,
                ..Default::default()
            };
            let start = Instant::now();
            let mesh = generate(home, &bin, &selection).unwrap();
            let elapsed = start.elapsed();
            let volume = crate::label::solid(&mesh).unwrap().volume();
            assert!(if style == crate::label::Style::Raised {
                volume > base_volume
            } else {
                volume < base_volume
            });
            assert!((mesh.min - blank.min).abs().max_element() < 0.001);
            assert!((mesh.max - blank.max).abs().max_element() < 0.001);
            let again = generate(home, &bin, &selection).unwrap();
            assert!(Arc::ptr_eq(&mesh, &again));
            // Exercise the disk-cache path, including its reconstructed text shading.
            if let Ok(mut cache) = MESH_CACHE.lock() {
                cache.retain(|(p, _)| !p.starts_with(home));
            }
            let reload = generate(home, &bin, &selection).unwrap();
            assert!((crate::label::solid(&reload).unwrap().volume() - volume).abs() < 0.02);
            assert_eq!(
                mesh.vertices.iter().filter(|v| v.shade < 1.).count(),
                reload.vertices.iter().filter(|v| v.shade < 1.).count()
            );
            assert_eq!(original, std::fs::read(&body_path).unwrap());
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
            timings += &format!(
                "{:?} {:?}: {:.1} ms\n",
                style,
                text,
                elapsed.as_secs_f64() * 1000.
            );
        }
        let reference = script(home, &bin, &selection).unwrap();
        bin.label.text = "换字".into();
        bin.label.depth = 0.7;
        bin.label.size = 5.;
        bin.label.font = crate::label::Font::Arial;
        assert_eq!(reference, script(home, &bin, &selection).unwrap());
        bin.span = [2, 1];
        assert_ne!(reference, script(home, &bin, &selection).unwrap());
        bin.span = [1, 1];
        bin.tab = false;
        assert_ne!(reference, script(home, &bin, &selection).unwrap());
        println!("{timings}");
        std::fs::write(real.join("output/gridfinity-lettering-timing.txt"), timings).unwrap();
    }
    #[test]
    fn scoop_persists_and_changes_body_cache_without_changing_text_behavior() {
        assert!(Bin::default().scoop);
        assert!(!toml::from_str::<Bin>("scoop = false").unwrap().scoop);
        assert!(toml::from_str::<Bin>("scoop = true").unwrap().scoop);
        let mut bin: Bin = toml::from_str("id = 7\n").unwrap();
        assert!(!bin.scoop);
        let selection = Selection::default();
        let home = Path::new(".");
        let plain = script(home, &bin, &selection).unwrap();
        bin.scoop = true;
        let curved = script(home, &bin, &selection).unwrap();
        assert_ne!(plain, curved);
        assert!(curved.contains("scoop_percent=1"));
        assert_eq!(
            bin,
            toml::from_str::<Bin>(&toml::to_string(&bin).unwrap()).unwrap()
        );
        bin.label.text = "New text".into();
        assert_eq!(curved, script(home, &bin, &selection).unwrap());
        bin.scoop = false;
        assert_eq!(plain, script(home, &bin, &selection).unwrap());
    }

    #[test]
    fn scoop_geometry_keeps_mounting_bounds_and_adds_material() {
        if std::env::var_os("VAULT_GRIDFINITY_TEST").is_none() {
            return;
        }
        let home = Path::new(env!("CARGO_MANIFEST_DIR"));
        let selection = Selection::default();
        let mut bin = Bin {
            scoop: false,
            ..Default::default()
        };
        let plain = generate(home, &bin, &selection).unwrap();
        bin.scoop = true;
        let curved = generate(home, &bin, &selection).unwrap();
        assert!((plain.min - curved.min).length() < 0.005);
        assert!((plain.max - curved.max).length() < 0.005);
        assert!(
            crate::label::solid(&curved).unwrap().volume()
                > crate::label::solid(&plain).unwrap().volume()
        );
        bin.scoop = false;
        assert!(Arc::ptr_eq(
            &plain,
            &generate(home, &bin, &selection).unwrap()
        ));
    }

    #[test]
    fn actual_socket_centers_all_standard_variants() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let catalog = crate::catalog::Catalog::scan(root.into()).unwrap();
        for width in ["S", "M", "L"] {
            for depth in 3..=7 {
                let selection = Selection {
                    width: width.into(),
                    depth,
                    ..Default::default()
                };
                for entry in catalog
                    .accessories(&selection)
                    .iter()
                    .filter(|e| e.path.contains("/Gridfinity Baseplate/"))
                {
                    let g = grid_for(&selection, &entry.path).unwrap();
                    let mesh = Mesh::load(&catalog.root.join(&entry.path)).unwrap();
                    for (axis, n) in [(0, g[0]), (2, g[1])] {
                        let c = centers(&mesh, axis, n as usize)
                            .unwrap_or_else(|e| panic!("{} axis {axis}: {e}", entry.path));
                        assert!(c.windows(2).all(|p| (p[1] - p[0] - 42.).abs() < 0.001));
                    }
                }
            }
        }
    }
    #[test]
    fn rebuilt_geometry_export_and_mates() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        if std::env::var_os("VAULT_GRIDFINITY_TEST").is_none() {
            return;
        }
        let home = Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalog = crate::catalog::Catalog::scan(root.into()).unwrap();
        let selection = Selection::default();
        let mut layout = Layout {
            baseplate: String::new(),
            bins: vec![Bin {
                label: crate::label::Config {
                    enabled: true,
                    text: "收纳\nA8".into(),
                    size: 4.,
                    ..Default::default()
                },
                ..Default::default()
            }],
        };
        let out = home.join("output/gridfinity");
        std::fs::create_dir_all(&out).unwrap();
        let mut bom = catalog.bom(&selection);
        let plate = catalog
            .accessories(&selection)
            .into_iter()
            .find(|e| e.path.ends_with("S3 Baseplate.stl"))
            .unwrap()
            .path;
        crate::catalog::Catalog::set_accessory(&mut bom, plate.clone(), true, 1);
        layout.baseplate = plate.clone();
        layout.sync_bom(&mut bom);
        let original = std::fs::read(catalog.root.join(&plate)).unwrap();
        for style in [crate::label::Style::Raised, crate::label::Style::Engraved] {
            layout.bins[0].label.style = style;
            let mesh = generate(home, &layout.bins[0], &selection).unwrap();
            assert!((mesh.size.x - 41.5).abs() < 0.05);
            assert!(mesh.size.z > 24. && mesh.size.z < 25.5);
            let originals: Vec<_> = bom
                .iter()
                .filter(|b| !b.path.starts_with(PREFIX))
                .cloned()
                .collect();
            let mut scene =
                Scene::load(&catalog.root, &originals, "Gridfinity".into(), true).unwrap();
            apply(&mut scene, home, &layout, &selection, &bom).unwrap();
            let p = scene
                .parts
                .iter()
                .position(|p| p.pose.path == plate)
                .unwrap();
            let b = scene
                .parts
                .iter()
                .position(|p| p.pose.path == layout.bins[0].path())
                .unwrap();
            let crossings = crate::collision::audit_pair(&scene, p, b);
            assert!(
                crossings.is_empty(),
                "baseplate crossing count: {}",
                crossings.len()
            );
            let relative =
                scene.parts[p].pose.matrix(0.).inverse() * scene.parts[b].pose.matrix(0.);
            scene.parts[p].pose.position[0] += 12.;
            constraints::solve(&mut scene).unwrap();
            assert!(
                (scene.parts[p].pose.matrix(0.).inverse() * scene.parts[b].pose.matrix(0.))
                    .abs_diff_eq(relative, 0.001)
            );
            let mut changed = scene.clone();
            let mut moved = layout.clone();
            moved.bins[0].cell = [1, 1];
            changed.parts.retain(|p| !p.pose.path.starts_with(PREFIX));
            changed
                .relations
                .retain(|r| !r.child.path.starts_with(PREFIX));
            apply(&mut changed, home, &moved, &selection, &bom).unwrap();
            changed.retain_assembly_state(&scene);
            let new_relative =
                changed.parts[p].pose.matrix(0.).inverse() * changed.parts[b].pose.matrix(0.);
            assert!((new_relative.w_axis.x - relative.w_axis.x + 42.).abs() < 0.01);
            let items: Vec<_> = bom
                .iter()
                .filter(|b| b.path.starts_with(PREFIX))
                .cloned()
                .collect();
            crate::export::run_design(
                &catalog.root,
                &items,
                &out.join(format!("bin-{style:?}.3mf")),
                crate::export::Format::ThreeMf,
                &Default::default(),
                Some((home, &layout, &selection)),
            )
            .unwrap();
            let top = mesh.min.y + units(&layout.bins[0], &selection).unwrap() as f32 * 7. - 1.2;
            let tab_y = -(layout.bins[0].span[1] as f32 * 42. - 0.5) / 2. + 1.25 + 15.85 / 2.;
            let regions = crate::label::color_regions(
                &mesh,
                &layout.bins[0].label,
                [0., tab_y as f64],
                top as f64,
            )
            .unwrap();
            let a = crate::label::solid(&regions[0]).unwrap();
            let b = crate::label::solid(&regions[1]).unwrap();
            assert!(a.intersection(&b).volume().abs() < 0.01);
            assert!(
                (a.volume() + b.volume() - crate::label::solid(&mesh).unwrap().volume()).abs()
                    < 0.1
            );
            let mut options = crate::export::Options::default();
            options
                .plate
                .text_filaments
                .insert(layout.bins[0].path(), 2);
            options.plate.center = true;
            crate::export::run_configured(
                &catalog.root,
                &items,
                &out.join(format!("bin-{style:?}-colors.3mf")),
                &options,
                &Default::default(),
                Some((home, &layout, &selection)),
            )
            .unwrap();
            crate::label::write_stl(&mesh, &out.join(format!("bin-{style:?}.stl"))).unwrap();
        }
        assert_eq!(original, std::fs::read(catalog.root.join(plate)).unwrap());
        layout.bins[0].span = [2, 1];
        layout.bins[0].label.enabled = false;
        let mesh = generate(home, &layout.bins[0], &selection).unwrap();
        assert!((mesh.size.x - 83.5).abs() < 0.05);
        crate::label::write_stl(&mesh, &out.join("bin-2x1.stl")).unwrap();
        for height in ["2F", "4F"] {
            let selection = Selection {
                height: height.into(),
                ..Default::default()
            };
            for span in if height == "2F" {
                vec![[1, 1], [2, 1]]
            } else {
                vec![[1, 1]]
            } {
                let bin = Bin {
                    span,
                    label: crate::label::Config {
                        enabled: span == [1, 1],
                        text: "收纳\nA8".into(),
                        size: 4.,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mesh = generate(home, &bin, &selection).unwrap();
                let actual = mesh.size.z;
                let ceiling = if height == "2F" { 82.5 } else { 178.5 };
                assert!(actual + 1.3 < ceiling - 2.);
                assert!(
                    (actual - (max_units(&selection).unwrap() as f32 * 7. + 3.55)).abs() < 0.05
                );
                crate::label::write_stl(
                    &mesh,
                    &out.join(format!("bin-{height}-{}x{}.stl", span[0], span[1])),
                )
                .unwrap();
            }
        }
    }
}
