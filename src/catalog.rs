pub mod colors;
pub mod exterior;
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
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SustainerBody {
    #[default]
    Flat,
    Dividers,
    Sealed,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SustainerOptions {
    pub body: SustainerBody,
    pub split_roof: bool,
}
impl SustainerOptions {
    pub fn normalized(mut self, s: &Selection) -> Self {
        if matches!(s.width.as_str(), "S" | "M") {
            self.body = SustainerBody::Flat;
        }
        self.split_roof &= s.depth >= 6;
        self
    }
    pub fn key(self, s: &Selection) -> String {
        let options = self.normalized(s);
        format!(
            "sustainer-{}{}-{}-{:?}-{}",
            s.width,
            s.depth,
            s.height,
            options.body,
            if options.split_roof { "split" } else { "whole" }
        )
        .to_lowercase()
    }
}

/// Supported combinations in this package, including both cover choices.
pub fn sustainer_configurations() -> Vec<(Selection, SustainerOptions)> {
    let mut result = Vec::new();
    for width in ["S", "M", "L", "XL"] {
        for depth in 3..=7 {
            let heights: &[&str] = if width == "XL" {
                &["1F"]
            } else {
                &["1F", "2F", "4F"]
            };
            let bodies: &[SustainerBody] = if matches!(width, "L" | "XL") {
                &[
                    SustainerBody::Flat,
                    SustainerBody::Dividers,
                    SustainerBody::Sealed,
                ]
            } else {
                &[SustainerBody::Flat]
            };
            for height in heights {
                for &body in bodies {
                    for split_roof in if depth >= 6 {
                        vec![false, true]
                    } else {
                        vec![false]
                    } {
                        result.push((
                            Selection {
                                width: width.into(),
                                depth,
                                height: (*height).into(),
                                hidden_key: false,
                            },
                            SustainerOptions { body, split_roof },
                        ));
                    }
                }
            }
        }
    }
    result
}

// The supplied package mixes Roof/Roofs, whitespace and apostrophes. Match the
// complete directory-qualified name, preserve its original spelling, reject ambiguity.
fn sustainer_key(path: &str) -> String {
    path.chars()
        .filter(|c| !c.is_whitespace() && *c != '\'')
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .replace("/roofs/", "/roof/")
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
    pub fn sustainer_divider(
        &self,
        s: &Selection,
        options: SustainerOptions,
        axis: crate::sustainer_dividers::Axis,
        lane: usize,
    ) -> Result<String, String> {
        use crate::sustainer_dividers::Axis;
        let o = options.normalized(s);
        let name = match (s.width.as_str(), o.body, axis) {
            ("S" | "M", _, Axis::Width) => {
                format!("{} Dividers/{}{} Divider.stl", s.width, s.width, s.height)
            }
            ("L", SustainerBody::Dividers, Axis::Width) => {
                format!("L Width Dividers/L-{} Width Divider.stl", s.height)
            }
            ("L", SustainerBody::Dividers, Axis::Depth) => format!(
                "L Depth Dividers/L{}-{} Depth Divider.stl",
                s.depth, s.height
            ),
            ("XL", SustainerBody::Dividers, Axis::Width) => "XL Width Divider.stl".into(),
            ("XL", SustainerBody::Dividers, Axis::Depth) => {
                format!("XL Depth Dividers/XL{} Depth Divider.stl", s.depth)
            }
            ("L", SustainerBody::Sealed, Axis::Width) => {
                format!("L Seal Dividers/L-{} Seal Divider.stl", s.height)
            }
            ("XL", SustainerBody::Sealed, Axis::Width) => format!(
                "XL Seal Divider ({}).stl",
                if lane == 1 { "Center" } else { "Sides" }
            ),
            _ => return Err("当前箱体不支持此类隔板".into()),
        };
        self.sustainer_resolve(&format!("{}{name}", crate::sustainer_dividers::PREFIX))
    }

    pub fn reconfigure_sustainer(
        &self,
        s: &Selection,
        options: SustainerOptions,
        old: &[Bom],
    ) -> Result<Vec<Bom>, String> {
        let mut bom = self.sustainer_bom(s, options)?;
        let small = matches!(s.width.as_str(), "S" | "M");
        let old_small = old
            .iter()
            .find(|b| b.path.contains("/Modules/"))
            .map(|b| b.path.contains("/01. S Families/") || b.path.contains("/02. M Families/"));
        for b in &mut bom {
            if let Some(saved) = old.iter().find(|p| p.path == b.path) {
                b.enabled = saved.enabled;
                if b.group != "Roof connector"
                    && (!matches!(b.group.as_str(), "Locks" | "Stacking latches")
                        || old_small.is_none_or(|old| old == small))
                {
                    b.qty = saved.qty;
                }
            }
        }
        Ok(bom)
    }
    fn sustainer_resolve(&self, expected: &str) -> Result<String, String> {
        let key = sustainer_key(expected);
        let mut matches = self
            .entries
            .iter()
            .filter(|e| sustainer_key(&e.path) == key);
        let found = matches
            .next()
            .ok_or_else(|| format!("Missing Sustainer component: {expected}"))?;
        if matches.next().is_some() {
            return Err(format!("Ambiguous Sustainer component: {expected}"));
        }
        Ok(found.path.clone())
    }
    pub fn sustainer_bom(
        &self,
        s: &Selection,
        options: SustainerOptions,
    ) -> Result<Vec<Bom>, String> {
        if !["S", "M", "L", "XL"].contains(&s.width.as_str())
            || !(3..=7).contains(&s.depth)
            || !["1F", "2F", "4F"].contains(&s.height.as_str())
            || s.width == "XL" && s.height != "1F"
        {
            return Err("Invalid Sustainer size".into());
        }
        let options = options.normalized(s);
        let family = match s.width.as_str() {
            "S" => "01. S Families",
            "M" => "02. M Families",
            "L" => "03. L Families",
            _ => "04. XL Families",
        };
        let base = format!("02. SUSTAINERS/{family}");
        let size = format!("{}{}", s.width, s.depth);
        let dir = format!("{base}/{size}");
        let universal = "02. SUSTAINERS/05. Universal";
        let mut bom = Vec::new();
        let mut add = |path: String, qty, group: &str| -> Result<(), String> {
            bom.push(Bom {
                path: self.sustainer_resolve(&path)?,
                qty,
                group: group.into(),
                enabled: true,
            });
            Ok(())
        };
        let variant = if matches!(s.width.as_str(), "L" | "XL") {
            match options.body {
                SustainerBody::Flat => " Flat",
                SustainerBody::Dividers => " Dividers",
                SustainerBody::Sealed => " Sealed",
            }
        } else {
            ""
        };
        add(
            format!("{dir}/Modules/{size} {}{variant} Module.stl", s.height),
            1,
            "Structure",
        )?;
        if options.split_roof {
            for part in [1, 2] {
                add(format!("{dir}/Roof/{size} Roof Part {part}.stl"), 1, "Roof")?;
            }
            add(
                format!("{dir}/Roof/Connector For Split Roof.stl"),
                match s.width.as_str() {
                    "S" => 2,
                    "M" => 3,
                    "L" => 4,
                    _ => 5,
                },
                "Roof connector",
            )?;
        } else {
            let suffix = if s.depth >= 6 { " Whole" } else { "" };
            add(format!("{dir}/Roof/{size} Roof{suffix}.stl"), 1, "Roof")?;
        }
        // Use the family facade shown by the naming diagram; Universal/Facades
        // contains different meshes and is not an interchangeable duplicate.
        let facade = if s.width == "XL" {
            base.clone()
        } else {
            format!("{base}/Facades")
        };
        add(
            format!("{facade}/{}{} Facade.stl", s.width, s.height),
            1,
            "Facade",
        )?;
        let small = matches!(s.width.as_str(), "S" | "M");
        let bottom = match (small, s.height.as_str()) {
            (true, "1F") => "S-M 1F Hinge",
            (true, _) => "S-M 2-4F Hinge",
            (false, "1F") => "L-XL 1F Hinge",
            (false, _) => "L 2-4F Hinge",
        };
        let top = if small {
            format!("{} Hinge Top", s.width)
        } else {
            "L-XL Hinge Top".into()
        };
        add(
            format!("{universal}/Hinges/{bottom}.stl"),
            1,
            "Lower hinges",
        )?;
        add(format!("{universal}/Hinges/{top}.stl"), 1, "Upper hinges")?;
        add(
            format!(
                "{universal}/Rods/{} Rods.stl",
                if small { "S-M" } else { "L-XL" }
            ),
            1,
            "Hinge rods",
        )?;
        let locks = if small { 1 } else { 2 };
        for name in ["LOCK HOLDER", "LOCK INNER PART", "1 SET OF PINS FOR 1 LOCK"] {
            add(format!("{universal}/Locks/{name}.stl"), locks, "Locks")?;
        }
        add(
            format!(
                "{universal}/Locks/LOCK OUTER PART {}.stl",
                if s.height == "1F" { "1F" } else { "2-4F" }
            ),
            locks,
            "Locks",
        )?;
        // The shared underside stacking latch is stored in the drawer Universal folder.
        add(
            crate::sustainer::STACKING_LATCH.into(),
            if small { 1 } else { 2 },
            "Stacking latches",
        )?;
        add(format!("{universal}/Label for Sustainers.stl"), 1, "Label")?;
        Ok(bom)
    }
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
    fn sustainer_matching_is_directory_scoped_and_rejects_ambiguous_names() {
        let mut c = Catalog::default();
        for path in [
            "02. SUSTAINERS/02. M Families/M3/Modules/M3 1F Module .stl",
            "01. DRAWERS/02. M Families/M3/Modules/M3 Module 1F.stl",
        ] {
            c.entries.push(Entry {
                path: path.into(),
                name: path.rsplit('/').next().unwrap().into(),
                category: 2,
            });
        }
        let expected = "02. SUSTAINERS/02. M Families/M3/Modules/M3 1F Module.stl";
        assert!(
            c.sustainer_resolve(expected)
                .unwrap()
                .ends_with("Module .stl")
        );
        c.entries.push(Entry {
            path: expected.into(),
            name: "M3 1F Module.stl".into(),
            category: 2,
        });
        assert!(
            c.sustainer_resolve(expected)
                .unwrap_err()
                .contains("Ambiguous")
        );
        let s = Selection::default();
        let options = SustainerOptions {
            body: SustainerBody::Sealed,
            split_roof: true,
        };
        assert_eq!(options.normalized(&s), SustainerOptions::default());
        assert!(options.key(&s).starts_with("sustainer-s3-1f-"));
    }
    #[test]
    fn sustainer_library_all_sizes_variants_and_print_counts() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let mut configurations = 0;
        for width in ["S", "M", "L", "XL"] {
            for depth in 3..=7 {
                for height in if width == "XL" {
                    vec!["1F"]
                } else {
                    vec!["1F", "2F", "4F"]
                } {
                    for body in if matches!(width, "S" | "M") {
                        vec![SustainerBody::Flat]
                    } else {
                        vec![
                            SustainerBody::Flat,
                            SustainerBody::Dividers,
                            SustainerBody::Sealed,
                        ]
                    } {
                        for split_roof in if depth < 6 {
                            vec![false]
                        } else {
                            vec![false, true]
                        } {
                            let s = Selection {
                                width: width.into(),
                                depth,
                                height: height.into(),
                                hidden_key: false,
                            };
                            let options = SustainerOptions { body, split_roof };
                            let bom = c.sustainer_bom(&s, options).unwrap();
                            configurations += 1;
                            assert!(bom.iter().all(|b| (b.path.starts_with("02. SUSTAINERS/")
                                || b.path == crate::sustainer::STACKING_LATCH)
                                && c.root.join(&b.path).is_file()));
                            assert_eq!(
                                bom.iter()
                                    .find(|b| b.path == crate::sustainer::STACKING_LATCH)
                                    .unwrap()
                                    .qty,
                                if matches!(width, "S" | "M") { 1 } else { 2 }
                            );
                            assert_eq!(bom.iter().filter(|b| b.group == "Structure").count(), 1);
                            assert_eq!(bom.iter().filter(|b| b.group == "Facade").count(), 1);
                            assert_eq!(
                                bom.iter().filter(|b| b.group == "Roof").count(),
                                if split_roof { 2 } else { 1 }
                            );
                            assert_eq!(
                                bom.iter().filter(|b| b.group == "Roof connector").count(),
                                usize::from(split_roof)
                            );
                            if split_roof {
                                let expected = match width {
                                    "S" => 2,
                                    "M" => 3,
                                    "L" => 4,
                                    _ => 5,
                                };
                                assert_eq!(
                                    bom.iter()
                                        .find(|b| b.group == "Roof connector")
                                        .unwrap()
                                        .qty,
                                    expected
                                );
                                let mut legacy = bom.clone();
                                legacy
                                    .iter_mut()
                                    .find(|b| b.group == "Roof connector")
                                    .unwrap()
                                    .qty = 1;
                                assert_eq!(
                                    c.reconfigure_sustainer(&s, options, &legacy)
                                        .unwrap()
                                        .iter()
                                        .find(|b| b.group == "Roof connector")
                                        .unwrap()
                                        .qty,
                                    expected
                                );
                            }
                            assert_eq!(
                                bom.iter()
                                    .find(|b| b.name() == "LOCK HOLDER.stl")
                                    .unwrap()
                                    .qty,
                                if matches!(width, "S" | "M") { 1 } else { 2 }
                            );
                            let mut saved = bom.clone();
                            saved.last_mut().unwrap().qty = 3;
                            saved.last_mut().unwrap().enabled = false;
                            let restored = c.reconfigure_sustainer(&s, options, &saved).unwrap();
                            assert_eq!(restored.last().unwrap().qty, 3);
                            assert!(!restored.last().unwrap().enabled);
                        }
                    }
                }
            }
        }
        assert_eq!(configurations, 126);
        let old = c
            .sustainer_bom(&Selection::default(), Default::default())
            .unwrap();
        let large = Selection {
            width: "L".into(),
            ..Default::default()
        };
        let next = c
            .reconfigure_sustainer(&large, Default::default(), &old)
            .unwrap();
        assert_eq!(
            next.iter()
                .find(|b| b.path == crate::sustainer::STACKING_LATCH)
                .unwrap()
                .qty,
            2
        );
        let legacy: Vec<_> = old
            .iter()
            .filter(|b| b.path != crate::sustainer::STACKING_LATCH)
            .cloned()
            .collect();
        assert_eq!(
            c.reconfigure_sustainer(&Selection::default(), Default::default(), &legacy)
                .unwrap()
                .iter()
                .find(|b| b.path == crate::sustainer::STACKING_LATCH)
                .unwrap()
                .qty,
            1
        );
        assert_eq!(
            next.iter()
                .find(|b| b.name() == "LOCK HOLDER.stl")
                .unwrap()
                .qty,
            2
        );
    }
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
