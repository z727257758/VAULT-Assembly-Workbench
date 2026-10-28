use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub category: usize,
}
#[derive(Clone, Default)]
pub struct Catalog {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Selection {
    pub width: String,
    pub depth: u8,
    pub height: String,
    pub hidden_key: bool,
}
impl Default for Selection {
    fn default() -> Self {
        Self {
            width: "S".into(),
            depth: 3,
            height: "1F".into(),
            hidden_key: false,
        }
    }
}
/// Stable family key for parallel print placement; unknown parts group only with copies.
pub fn print_alignment_family(name: &str) -> &str {
    // The external regression fixture appends the copy number to display names.
    let name = name
        .rsplit_once(" #")
        .filter(|(_, n)| n.parse::<u32>().is_ok())
        .map_or(name, |(base, _)| base);
    match name {
        "x4 Runner Guide.stl" | "x4 Runner Slide.stl" => "@runner-guide-slide",
        _ => name, // Other elongated parts align with copies of the same part.
    }
}
/// Exposed screw recesses; the label covers one additional recess.
pub fn facade_cap_count(s: &Selection) -> u32 {
    match (s.width.as_str(), s.height.as_str()) {
        ("S", "1F") => 1,
        ("S", "2F" | "4F") | ("M" | "L", "1F") => 3,
        ("M" | "L", "2F" | "4F") => 5,
        _ => 1, // Preserve the existing estimate for unsupported split XL facades.
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Bom {
    pub path: String,
    pub qty: u32,
    pub group: String,
    pub enabled: bool,
}
impl Bom {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}
/// Standard unsplit drawer modules have two lower clip seats, or four on L.
pub fn module_clip_count(path: &str) -> Option<usize> {
    let segments: Vec<_> = path.split('/').collect();
    let ["01. DRAWERS", family, size, "Modules", file] = segments.as_slice() else {
        return None;
    };
    let (width, count) = match *family {
        "01. S Families" => ("S", 2),
        "02. M Families" => ("M", 2),
        "03. L Families" => ("L", 4),
        _ => return None,
    };
    if !(3..=7).any(|depth| *size == format!("{width}{depth}")) {
        return None;
    }
    ["1F", "2F", "4F"]
        .into_iter()
        .any(|height| *file == format!("{size} Module {height}.stl"))
        .then_some(count)
}

impl Catalog {
    pub fn scan(root: PathBuf) -> Result<Self, String> {
        fn visit(root: &Path, dir: &Path, out: &mut Vec<Entry>) -> Result<(), String> {
            for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
                let e = entry.map_err(|e| e.to_string())?;
                let t = e.file_type().map_err(|e| e.to_string())?;
                if t.is_symlink() {
                    continue;
                }
                let p = e.path();
                if t.is_dir() {
                    visit(root, &p, out)?;
                } else if t.is_file() {
                    let ext = p
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if ["stl", "pdf", "png", "jpg", "jpeg"].contains(&ext.as_str()) {
                        let rel = p
                            .strip_prefix(root)
                            .map_err(|e| e.to_string())?
                            .to_string_lossy()
                            .replace('\\', "/");
                        let category = if rel.starts_with("01. DRAWERS/") {
                            1
                        } else if rel.starts_with("02. SUSTAINERS/") {
                            2
                        } else if rel.starts_with("03. MODIFICATIONS/") {
                            3
                        } else {
                            4
                        };
                        out.push(Entry {
                            name: e.file_name().to_string_lossy().into(),
                            path: rel,
                            category,
                        });
                    }
                }
            }
            Ok(())
        }
        let mut entries = Vec::new();
        visit(&root, &root, &mut entries)?;
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self { root, entries })
    }
    pub fn resolve(&self, path: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|e| e.path.eq_ignore_ascii_case(path))
            .map(|e| e.path.clone())
    }
    pub fn stl_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.path.to_lowercase().ends_with(".stl"))
            .count()
    }
    pub fn prefix(s: &Selection) -> String {
        let family = match s.width.as_str() {
            "S" => "01. S Families",
            "M" => "02. M Families",
            "L" => "03. L Families",
            _ => "05. XL Families",
        };
        format!("01. DRAWERS/{family}/{}{}", s.width, s.depth)
    }
    pub fn bom(&self, s: &Selection) -> Vec<Bom> {
        let prefix = Self::prefix(s);
        let wd = format!("{}{}", s.width, s.depth);
        let uni = "01. DRAWERS/07. Universal";
        let mut result = Vec::new();
        let mut add = |path: String, qty: u32, group: &str, enabled: bool| {
            let path = self.resolve(&path).unwrap_or(path);
            result.push(Bom {
                path,
                qty,
                group: group.into(),
                enabled,
            });
        };
        add(
            format!("{prefix}/Modules/{wd} Module {}.stl", s.height),
            1,
            "Structure",
            true,
        );
        if s.width == "XL" {
            for e in &self.entries {
                if e.path.starts_with(&format!("{prefix}/Boxes/")) && e.name.ends_with(".stl") {
                    add(e.path.clone(), 1, "Drawer", true);
                }
            }
        } else {
            add(
                format!("{prefix}/Boxes/{wd} Box {}.stl", s.height),
                1,
                "Drawer",
                true,
            );
            let family = prefix.rsplit_once('/').unwrap().0;
            add(
                format!(
                    "{family}/Facades/{}{} (Turn On Supports).stl",
                    s.width, s.height
                ),
                1,
                "Facade",
                true,
            );
        }
        add(
            format!("{uni}/Geared Rails/{} Rails.stl", s.width),
            1,
            "Geared rails",
            true,
        );
        let xl = if s.width == "XL" { " XL" } else { "" };
        let folder = if s.width == "XL" { "/XL Runners" } else { "" };
        for kind in ["Guide", "Slide"] {
            add(
                format!("{uni}/Runners{folder}/x{} Runner {kind}{xl}.stl", s.depth),
                2,
                "Runners",
                true,
            );
        }
        for (name, qty) in [
            ("Runners/Runner Stopper.stl", 2),
            ("Latch.stl", if s.width == "L" { 2 } else { 1 }),
            ("Springs.stl", 1),
            ("Key Cap.stl", 1),
            ("Facade Cap.stl", facade_cap_count(s)),
            ("Label.stl", 1),
        ] {
            add(format!("{uni}/{name}"), qty, "Mechanism", true);
        }
        for name in if s.hidden_key {
            vec![
                "Hidden Key (Turn On Supports).stl",
                "Hidden Key Part 2 (Turn on Supports).stl",
            ]
        } else {
            vec!["Key (Turn On Supports).stl"]
        } {
            add(format!("{uni}/{name}"), 1, "Key", true);
        }
        for e in &self.entries {
            if e.path
                .starts_with(&format!("{prefix}/Gridfinity Baseplate/"))
                && e.name.ends_with(".stl")
            {
                add(e.path.clone(), 1, "Optional baseplate", false);
            }
        }
        result
    }
    /// Keep quantities and compatible accessories when changing drawer options.
    pub fn reconfigure(&self, s: &Selection, old: &[Bom]) -> Vec<Bom> {
        let mut result = self.bom(s);
        for item in &mut result {
            if let Some(saved) = old.iter().find(|b| b.path == item.path) {
                if item.group != "Geared rails" && !item.path.ends_with("/Facade Cap.stl") {
                    item.qty = saved.qty;
                }
                if item.group != "Geared rails" {
                    item.enabled = saved.enabled;
                }
            }
        }
        let extras = self.accessories(s);
        for item in old {
            if !result.iter().any(|b| b.path == item.path)
                && extras.iter().any(|e| e.path == item.path)
            {
                result.push(item.clone());
            }
        }
        result
    }
    pub fn set_accessory(bom: &mut Vec<Bom>, path: String, enabled: bool, qty: u32) {
        let required = path.contains("/Geared Rails/");
        let enabled = enabled || required;
        let qty = if required { 1 } else { qty };
        if enabled {
            for item in bom.iter_mut() {
                if crate::accessories::conflicts(&path, &item.path) {
                    item.enabled = false;
                }
            }
        }
        if let Some(item) = bom.iter_mut().find(|b| b.path == path) {
            item.enabled = enabled;
            item.qty = qty.clamp(1, 100);
        } else {
            bom.push(Bom {
                path,
                qty: qty.clamp(1, 100),
                group: "Optional accessory".into(),
                enabled,
            });
        }
    }
    pub fn accessories(&self, s: &Selection) -> Vec<Entry> {
        let prefix = format!("{}/Gridfinity Baseplate/", Self::prefix(s));
        self.entries
            .iter()
            .filter(|e| {
                e.name.ends_with(".stl")
                    && (e.path.starts_with(&prefix)
                        || e.path.contains("/Grid Dividers/")
                            && crate::accessories::divider_matches(&e.path, s))
            })
            .cloned()
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn facade_cap_quantity_tracks_width_height_and_migrates_old_presets() {
        let c = Catalog::default();
        let mut old = Vec::new();
        for (width, height, count) in [
            ("S", "1F", 1),
            ("M", "1F", 3),
            ("L", "2F", 5),
            ("S", "4F", 3),
            ("M", "4F", 5),
            ("S", "1F", 1),
        ] {
            let s = Selection {
                width: width.into(),
                height: height.into(),
                ..Default::default()
            };
            let bom = c.reconfigure(&s, &old);
            assert_eq!(
                bom.iter()
                    .find(|b| b.name() == "Facade Cap.stl")
                    .unwrap()
                    .qty,
                count
            );
            old = bom;
            old.iter_mut()
                .find(|b| b.name() == "Facade Cap.stl")
                .unwrap()
                .qty = 1;
        }
    }
    #[test]
    fn legacy_roof_and_base_are_ignored_and_removed() {
        let c = Catalog::default();
        let s:Selection=toml::from_str("width='S'\ndepth=3\nheight='1F'\nhidden_key=false\nroof='old/Roofs/Roof.stl'\nbase='03. MODIFICATIONS/Base/S/S3 Base.stl'").unwrap();
        let old = vec![
            Bom {
                path: "old/Roofs/Roof.stl".into(),
                qty: 1,
                group: "Roof".into(),
                enabled: true,
            },
            Bom {
                path: "03. MODIFICATIONS/Base/S/S3 Base.stl".into(),
                qty: 1,
                group: "Base".into(),
                enabled: true,
            },
        ];
        assert!(
            c.reconfigure(&s, &old)
                .iter()
                .all(|b| b.group != "Roof" && b.group != "Base")
        );
        let saved = toml::to_string(&s).unwrap();
        assert!(!saved.contains("roof") && !saved.contains("base"));
    }
    #[test]
    fn xl_only_uses_real_split_boxes() {
        let c = Catalog {
            root: PathBuf::new(),
            entries: vec![Entry {
                path: "01. DRAWERS/05. XL Families/XL6/Boxes/XL6 Box 1F Part 1.stl".into(),
                name: "XL6 Box 1F Part 1.stl".into(),
                category: 1,
            }],
        };
        let s = Selection {
            width: "XL".into(),
            depth: 6,
            ..Default::default()
        };
        let b = c.bom(&s);
        assert!(b.iter().any(|b| b.name().contains("Part 1")));
        assert!(!b.iter().any(|b| b.path.contains("Facades")));
        assert!(
            b.iter()
                .find(|b| b.group == "Geared rails")
                .unwrap()
                .enabled
        );
    }
}
