//! Print-bed packing and Bambu project settings. No assembly transforms.
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
mod materials;
mod printer;
pub use materials::{BambuPreset, FilamentMaterial, MaterialLibrary};
pub use printer::Printer;
pub const BIN_COLOR_KEY: &str = "generated/gridfinity/@bin-color";
// The Bambu Studio 02.05 project importer/UI uses EnforcerBlockerType::ExtruderMax.
const MAX_PROJECT_SLOTS: u32 = 32;
pub const LABEL_COLOR_KEY: &str = "generated/gridfinity/@label-color";
const TEXT_ROLE: &str = "text";
const DEFAULT_TEXT_SLOT: u32 = 6;
fn generated_color_type(path: &str) -> Option<(&'static str, u32)> {
    let file = path
        .strip_prefix(crate::gridfinity::PREFIX)?
        .strip_suffix(".stl")?;
    let (key, id) = if let Some(id) = file.strip_prefix("Gridfinity-") {
        (BIN_COLOR_KEY, id)
    } else {
        (LABEL_COLOR_KEY, file.strip_prefix("Cullenect-")?)
    };
    let id = id.parse::<u32>().ok()?;
    (id > 0).then_some((key, id))
}
/// Generated bins and removable labels each share an assignment across all IDs.
pub fn color_key(path: &str) -> &str {
    generated_color_type(path).map_or(path, |(key, _)| key)
}
fn assignment(map: &std::collections::BTreeMap<String, u32>, path: &str) -> Option<u32> {
    let key = color_key(path);
    map.get(key).copied().or_else(|| {
        if key == path {
            return None;
        }
        // Older presets stored one assignment per ID. Use the lowest configured
        // ID consistently until the next edit consolidates the whole type.
        map.iter()
            .filter_map(|(p, slot)| {
                generated_color_type(p)
                    .filter(|(family, _)| *family == key)
                    .map(|(_, id)| (id, *slot))
            })
            .min_by_key(|(id, _)| *id)
            .map(|(_, slot)| slot)
    })
}
fn set_assignment(map: &mut std::collections::BTreeMap<String, u32>, path: &str, slot: u32) {
    let key = color_key(path).to_owned();
    map.retain(|p, _| color_key(p) != key);
    if slot != 0 {
        map.insert(key, slot);
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub printer: Printer,
    pub follow_global_colors: bool,
    pub role_filaments: std::collections::BTreeMap<String, u32>,
    pub component_filaments: std::collections::BTreeMap<String, u32>,
    pub component_text_filaments: std::collections::BTreeMap<String, u32>,
    pub margin: f32,
    pub brim: f32,
    pub gap: f32,
    pub rotate: bool,
    pub split: bool,
    pub center: bool,
    pub flat_text_face_down: bool,
    pub layer: f32,
    pub walls: u32,
    pub infill: u32,
    pub textured: bool,
    pub support_by_name: bool,
    pub module_clip_supports: bool,
    pub filament: u32,
    pub palette: [[u8; 3]; 6],
    #[serde(default = "legacy_materials")]
    pub materials: [FilamentMaterial; 6],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_filaments: Vec<GroupAppearance>,
    pub part_filaments: std::collections::BTreeMap<String, u32>,
    pub preview_colors: bool,
    pub text_filaments: std::collections::BTreeMap<String, u32>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            printer: Printer::default(),
            follow_global_colors: false,
            role_filaments: default_role_slots(),
            component_filaments: Default::default(),
            component_text_filaments: Default::default(),
            margin: 0.5,
            brim: 0.,
            gap: 6.,
            rotate: true,
            split: true,
            center: false,
            flat_text_face_down: true,
            layer: 0.2,
            walls: 2,
            infill: 15,
            textured: true,
            support_by_name: true,
            module_clip_supports: true,
            filament: 1,
            palette: DEFAULT_PALETTE,
            materials: DEFAULT_MATERIALS,
            extra_filaments: Vec::new(),
            part_filaments: Default::default(),
            preview_colors: false,
            text_filaments: Default::default(),
        }
    }
}
impl Config {
    fn brim_extent(&self) -> Result<f32, String> {
        if self.brim == 0. {
            return Ok(0.);
        }
        #[derive(Deserialize)]
        struct Brim {
            brim_object_gap: String,
        }
        let preset: Brim =
            serde_json::from_str(self.printer.template()).map_err(|e| e.to_string())?;
        let gap: f32 = preset
            .brim_object_gap
            .parse()
            .map_err(|_| "无效 Brim 间隙")?;
        if !gap.is_finite() || gap < 0. {
            return Err("无效 Brim 间隙".into());
        }
        Ok(self.brim + gap)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.margin.is_finite()
            || !(0.0..=30.).contains(&self.margin)
            || !self.brim.is_finite()
            || !(0.0..=10.).contains(&self.brim)
            || !self.gap.is_finite()
            || !(0.0..=30.).contains(&self.gap)
            || !self.layer.is_finite()
            || !(0.08..=0.28).contains(&self.layer)
            || !(1..=10).contains(&self.walls)
            || self.infill > 100
            || self.slot_count() > MAX_PROJECT_SLOTS
            || !(1..=self.slot_count()).contains(&self.filament)
            || self
                .part_filaments
                .values()
                .chain(self.text_filaments.values())
                .chain(self.role_filaments.values())
                .chain(self.component_filaments.values())
                .chain(self.component_text_filaments.values())
                .any(|slot| !(1..=self.slot_count()).contains(slot))
        {
            return Err(format!("无效的 {} 打印或摆盘参数", self.printer.name()));
        }
        Ok(())
    }
    pub fn part_assignment(&self, path: &str) -> u32 {
        assignment(&self.part_filaments, path).unwrap_or(0)
    }
    pub fn text_assignment(&self, path: &str) -> u32 {
        assignment(&self.text_filaments, path).unwrap_or(0)
    }
    pub fn set_part_slot(&mut self, path: &str, slot: u32) {
        set_assignment(&mut self.part_filaments, path, slot);
    }
    pub fn set_text_slot(&mut self, path: &str, slot: u32) {
        set_assignment(&mut self.text_filaments, path, slot);
    }
    pub fn slot(&self, path: &str) -> u32 {
        assignment(&self.part_filaments, path)
            .or_else(|| {
                self.component_filaments
                    .get(&crate::catalog::colors::component(path))
                    .copied()
            })
            .or_else(|| {
                self.role_filaments
                    .get(crate::catalog::colors::role(path))
                    .copied()
            })
            .unwrap_or(self.filament)
            .clamp(1, self.slot_count())
    }
    pub fn text_slot(&self, path: &str) -> u32 {
        assignment(&self.text_filaments, path)
            .or_else(|| {
                self.component_text_filaments
                    .get(&crate::catalog::colors::component(path))
                    .copied()
            })
            .or_else(|| self.role_filaments.get(TEXT_ROLE).copied())
            .unwrap_or_else(|| self.slot(path))
            .clamp(1, self.slot_count())
    }
    pub fn slot_color(&self, slot: u32) -> [f32; 4] {
        let [r, g, b] = self.appearance(slot).rgb;
        [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
    }
    pub fn color_hex(&self, slot: u32) -> String {
        let [r, g, b] = self.appearance(slot).rgb;
        format!("#{r:02X}{g:02X}{b:02X}")
    }
    pub fn color(&self, path: &str) -> [f32; 4] {
        self.slot_color(self.slot(path))
    }
    pub fn slot_count(&self) -> u32 {
        (self.palette.len() + self.extra_filaments.len()) as u32
    }
    pub fn appearance(&self, slot: u32) -> GroupAppearance {
        slot_appearance(&self.palette, &self.materials, &self.extra_filaments, slot)
    }
    pub fn set_appearance(&mut self, slot: u32, appearance: GroupAppearance) {
        write_slot(
            &mut self.palette,
            &mut self.materials,
            &mut self.extra_filaments,
            slot,
            appearance,
        );
    }
    pub fn project_json(&self) -> Result<Vec<u8>, String> {
        self.project_json_with_materials(&MaterialLibrary::default())
    }

    pub fn project_json_with_materials(
        &self,
        library: &MaterialLibrary,
    ) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut d: serde_json::Value =
            serde_json::from_str(self.printer.template()).map_err(|e| e.to_string())?;
        let process_parent = d["print_settings_id"].clone();
        if d["inherits_group"].is_array() {
            d["inherits_group"][0] = process_parent.clone();
        }
        for (key, value) in [
            ("layer_height", self.layer.to_string()),
            ("wall_loops", self.walls.to_string()),
            ("sparse_infill_density", format!("{}%", self.infill)),
            (
                "curr_bed_type",
                if self.textured {
                    "Textured PEI Plate"
                } else {
                    "High Temp Plate"
                }
                .into(),
            ),
            ("print_sequence", "by layer".into()),
            ("enable_prime_tower", "0".into()),
            (
                "brim_type",
                if self.brim > 0. {
                    "outer_only"
                } else {
                    "no_brim"
                }
                .into(),
            ),
            ("brim_width", self.brim.to_string()),
            ("skirt_loops", "0".into()),
            ("enable_support", "0".into()),
            (
                "print_settings_id",
                format!(
                    "VAULT {}",
                    self.printer.name().trim_start_matches("Bambu Lab ")
                ),
            ),
        ] {
            d[key] = serde_json::Value::String(value);
        }
        let count = self.slot_count();
        if count > 6 {
            materials::extend_project_slots(&mut d, count as usize)?;
        }
        let colors: Vec<_> = (1..=count).map(|slot| self.color_hex(slot)).collect();
        d["filament_colour"] = serde_json::json!(colors);
        d["filament_multi_colour"] = serde_json::json!(colors);
        let profiles: std::collections::BTreeMap<
            String,
            std::collections::BTreeMap<String, String>,
        > = serde_json::from_str(self.printer.filament_overrides()).map_err(|e| e.to_string())?;
        for slot in 1..=count {
            let material = self.appearance(slot).material;
            let preset = library.for_export(material, self.printer, slot)?;
            let base = preset.map_or(material, |preset| preset.base);
            let i = (slot - 1) as usize;
            if base != FilamentMaterial::PetgBasic && base != FilamentMaterial::PetgMatte {
                let profile = profiles
                    .get(base.name())
                    .ok_or_else(|| format!("Missing embedded profile for {}", base.name()))?;
                for (key, value) in profile {
                    d[key][i] = serde_json::Value::String(value.clone());
                }
            }
            if let Some(preset) = preset {
                let parent = preset.parent.as_ref().map_or_else(
                    || d["filament_settings_id"][i].clone(),
                    |name| serde_json::json!(name),
                );
                if !d["inherits_group"].is_array() {
                    d["inherits_group"] = serde_json::json!(vec![""; count as usize + 2]);
                    d["inherits_group"][0] = process_parent.clone();
                }
                d["inherits_group"][slot as usize] = parent;
                d["filament_settings_id"][i] = serde_json::json!(preset.name);
                if let Some(id) = &preset.filament_id {
                    d["filament_ids"][i] = serde_json::json!(id);
                }
                let mut changed = Vec::new();
                for (key, value) in &preset.options {
                    if materials::is_importable_option(key) {
                        d[key][i] = serde_json::json!(value);
                        changed.push(key.as_str());
                    }
                }
                d["different_settings_to_system"][slot as usize] =
                    serde_json::json!(changed.join(";"));
            }
        }
        d["default_filament_profile"][0] = d["filament_settings_id"][0].clone();
        d["wipe_tower_x"] = serde_json::json!(["0"]);
        d["wipe_tower_y"] = serde_json::json!(["0"]);
        serde_json::to_vec_pretty(&d).map_err(|e| e.to_string())
    }
}

const DEFAULT_PALETTE: [[u8; 3]; 6] = [
    [167, 170, 170],
    [248, 82, 0],
    [82, 82, 82],
    [0, 0, 0],
    [142, 144, 137],
    [246, 246, 243],
];

const DEFAULT_MATERIALS: [FilamentMaterial; 6] = [
    FilamentMaterial::PlaBasic,
    FilamentMaterial::PetgHf,
    FilamentMaterial::PlaBasic,
    FilamentMaterial::PlaBasic,
    FilamentMaterial::PlaBasic,
    FilamentMaterial::PlaBasic,
];
fn legacy_materials() -> [FilamentMaterial; 6] {
    [FilamentMaterial::PetgBasic; 6]
}
fn default_role_slots() -> std::collections::BTreeMap<String, u32> {
    crate::catalog::colors::ROLES
        .iter()
        .zip(1..)
        .map(|((role, _, _), slot)| ((*role).to_owned(), slot))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorGroup {
    pub name: String,
    pub components: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub slot: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_appearance: Option<GroupAppearance>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaletteGroup {
    Role(&'static str),
    Text,
    Custom(usize),
}

impl PaletteGroup {
    fn default_slot(self) -> Option<usize> {
        match self {
            Self::Role(role) => crate::catalog::colors::ROLES
                .iter()
                .position(|(key, _, _)| *key == role)
                .map(|index| index + 1),
            Self::Text => Some(DEFAULT_TEXT_SLOT as usize),
            Self::Custom(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupAppearance {
    pub rgb: [u8; 3],
    pub material: FilamentMaterial,
}

fn slot_appearance(
    palette: &[[u8; 3]; 6],
    materials: &[FilamentMaterial; 6],
    extra: &[GroupAppearance],
    slot: u32,
) -> GroupAppearance {
    let i = (slot as usize).clamp(1, palette.len() + extra.len()) - 1;
    if i < palette.len() {
        GroupAppearance {
            rgb: palette[i],
            material: materials[i],
        }
    } else {
        extra[i - palette.len()]
    }
}

fn write_slot(
    palette: &mut [[u8; 3]; 6],
    materials: &mut [FilamentMaterial; 6],
    extra: &mut [GroupAppearance],
    slot: u32,
    appearance: GroupAppearance,
) {
    let i = (slot - 1) as usize;
    if i < palette.len() {
        palette[i] = appearance.rgb;
        materials[i] = appearance.material;
    } else {
        extra[i - palette.len()] = appearance;
    }
}

/// Color-only data: applying a theme must not overwrite plate or support settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorTheme {
    #[serde(default)]
    grouping_version: u8,
    pub palette: [[u8; 3]; 6],
    pub materials: [FilamentMaterial; 6],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_filaments: Vec<GroupAppearance>,
    pub filament: u32,
    #[serde(default = "default_role_slots")]
    pub roles: std::collections::BTreeMap<String, u32>,
    pub role_overrides: std::collections::BTreeMap<String, String>,
    pub component_text: std::collections::BTreeMap<String, u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<ColorGroup>,
}
impl Default for ColorTheme {
    fn default() -> Self {
        let mut roles = default_role_slots();
        roles.insert(TEXT_ROLE.into(), DEFAULT_TEXT_SLOT);
        Self {
            grouping_version: 1,
            palette: DEFAULT_PALETTE,
            materials: DEFAULT_MATERIALS,
            extra_filaments: Vec::new(),
            filament: 1,
            roles,
            role_overrides: Default::default(),
            component_text: Default::default(),
            groups: Vec::new(),
        }
    }
}
impl ColorTheme {
    pub fn legacy_default() -> Self {
        Self {
            roles: default_role_slots(),
            ..Self::default()
        }
    }

    pub fn load<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let theme = Self::deserialize(de)?;
        if theme.grouping_version == 0 {
            return Ok(Self::legacy_default());
        }
        theme.validate().map_err(serde::de::Error::custom)?;
        Ok(theme)
    }
    pub fn role_slot(&self, role: &str) -> u32 {
        self.roles.get(role).copied().unwrap_or(self.filament)
    }
    pub fn slot_count(&self) -> u32 {
        (self.palette.len() + self.extra_filaments.len()) as u32
    }
    pub fn appearance(&self, slot: u32) -> GroupAppearance {
        slot_appearance(&self.palette, &self.materials, &self.extra_filaments, slot)
    }
    pub fn text_color_enabled(&self) -> bool {
        self.roles.contains_key(TEXT_ROLE)
    }
    pub fn set_text_color_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if !enabled {
            self.roles.remove(TEXT_ROLE);
        } else if !self.text_color_enabled() {
            self.set_group_appearance(PaletteGroup::Text, Self::default_text_appearance())?;
        }
        Ok(())
    }
    fn default_text_appearance() -> GroupAppearance {
        slot_appearance(&DEFAULT_PALETTE, &DEFAULT_MATERIALS, &[], DEFAULT_TEXT_SLOT)
    }
    fn palette_group_slot(&self, group: PaletteGroup) -> u32 {
        match group {
            PaletteGroup::Role(role) => self.role_slot(role),
            PaletteGroup::Text => self
                .roles
                .get(TEXT_ROLE)
                .copied()
                .unwrap_or(DEFAULT_TEXT_SLOT),
            PaletteGroup::Custom(i) => self.groups[i].slot,
        }
    }
    pub fn group_appearance(&self, group: PaletteGroup) -> GroupAppearance {
        if group == PaletteGroup::Text && !self.text_color_enabled() {
            Self::default_text_appearance()
        } else {
            self.appearance(self.palette_group_slot(group))
        }
    }

    /// Detach a shared slot when an edit would recolor another group or lettering.
    pub fn set_group_appearance(
        &mut self,
        group: PaletteGroup,
        appearance: GroupAppearance,
    ) -> Result<(), String> {
        self.check_group(group)?;
        let old_slot = self.palette_group_slot(group);
        let previous = self.appearance(old_slot);
        if previous == appearance && (group != PaletteGroup::Text || self.text_color_enabled()) {
            return Ok(());
        }
        let mut protected = std::collections::BTreeSet::from([self.filament]);
        protected.extend(self.component_text.values().copied());
        let edited_role = match group {
            PaletteGroup::Role(role) => Some(role),
            PaletteGroup::Text => Some(TEXT_ROLE),
            PaletteGroup::Custom(_) => None,
        };
        for (role, slot) in &self.roles {
            if edited_role != Some(role.as_str()) {
                protected.insert(*slot);
            }
        }
        for (i, custom) in self.groups.iter().enumerate() {
            if group != PaletteGroup::Custom(i) {
                protected.insert(custom.slot);
            }
        }
        let slots = 1..=self.slot_count();
        let slot = if let Some(existing) = slots.clone().find(|&s| self.appearance(s) == appearance)
        {
            existing
        } else if !protected.contains(&old_slot) {
            old_slot
        } else if let Some(free) = slots.into_iter().find(|s| !protected.contains(s)) {
            free
        } else {
            if self.slot_count() >= MAX_PROJECT_SLOTS {
                return Err("当前工程的 32 个耗材槽都已使用，请选择现有的材料和颜色组合。 / All 32 project filaments are in use. Choose an existing material and color.".into());
            }
            self.extra_filaments.push(appearance);
            self.slot_count()
        };
        write_slot(
            &mut self.palette,
            &mut self.materials,
            &mut self.extra_filaments,
            slot,
            appearance,
        );
        match group {
            PaletteGroup::Role(role) => {
                self.roles.insert(role.to_owned(), slot);
            }
            PaletteGroup::Text => {
                self.roles.insert(TEXT_ROLE.into(), slot);
            }
            PaletteGroup::Custom(i) => {
                self.groups[i].initial_appearance.get_or_insert(previous);
                self.groups[i].slot = slot;
            }
        }
        Ok(())
    }
    pub fn trim_unused_slots(&mut self) {
        let last = self
            .roles
            .values()
            .chain(self.component_text.values())
            .copied()
            .chain(self.groups.iter().map(|group| group.slot))
            .fold(self.filament, u32::max);
        self.extra_filaments
            .truncate((last as usize).saturating_sub(self.palette.len()));
    }
    fn check_group(&self, group: PaletteGroup) -> Result<(), String> {
        let exists = match group {
            PaletteGroup::Role(role) => crate::catalog::colors::ROLES
                .iter()
                .any(|(id, _, _)| *id == role),
            PaletteGroup::Text => true,
            PaletteGroup::Custom(i) => i < self.groups.len(),
        };
        exists
            .then_some(())
            .ok_or_else(|| "颜色分组不存在 / Color group not found".into())
    }
    pub fn reset_group_appearance(&mut self, group: PaletteGroup) -> Result<(), String> {
        self.check_group(group)?;
        let original = match group {
            PaletteGroup::Role(role) => slot_appearance(
                &DEFAULT_PALETTE,
                &DEFAULT_MATERIALS,
                &[],
                default_role_slots()[role],
            ),
            PaletteGroup::Text if !self.text_color_enabled() => return Ok(()),
            PaletteGroup::Text => Self::default_text_appearance(),
            PaletteGroup::Custom(i) => self.groups[i]
                .initial_appearance
                .unwrap_or_else(|| self.group_appearance(group)),
        };
        self.set_group_appearance(group, original)
    }
    pub fn role_for(&self, path: &str) -> &str {
        self.role_overrides
            .get(&crate::catalog::colors::component(path))
            .map(String::as_str)
            .unwrap_or_else(|| crate::catalog::colors::role(path))
    }
    pub fn group_slot(&self, path: &str) -> u32 {
        let component = crate::catalog::colors::component(path);
        self.groups
            .iter()
            .find(|g| g.components.contains(&component))
            .map_or_else(|| self.role_slot(self.role_for(path)), |g| g.slot)
    }
    pub fn validate(&self) -> Result<(), String> {
        let valid_role = |key: &str| {
            crate::catalog::colors::ROLES
                .iter()
                .any(|(role, _, _)| *role == key)
        };
        if self.grouping_version != 1
            || self.slot_count() > MAX_PROJECT_SLOTS
            || !(1..=self.slot_count()).contains(&self.filament)
            || self
                .role_overrides
                .iter()
                .any(|(key, role)| key.is_empty() || !valid_role(role))
            || self
                .component_text
                .iter()
                .any(|(key, slot)| key.is_empty() || !(1..=self.slot_count()).contains(slot))
            || self.roles.iter().any(|(key, slot)| {
                !(1..=self.slot_count()).contains(slot) || (key != TEXT_ROLE && !valid_role(key))
            })
        {
            return Err("无效的整体配色 / Invalid overall colors".into());
        }
        let mut assigned = std::collections::BTreeSet::new();
        let mut names: std::collections::BTreeSet<_> = crate::catalog::colors::ROLES
            .iter()
            .flat_map(|(_, cn, en)| [cn.to_lowercase(), en.to_lowercase()])
            .collect();
        if self.groups.len() > 64
            || self.groups.iter().any(|group| {
                !(1..=self.slot_count()).contains(&group.slot)
                    || group.name.trim().is_empty()
                    || !names.insert(group.name.trim().to_lowercase())
                    || group.name.chars().count() > 48
                    || group.components.is_empty()
                    || group
                        .components
                        .iter()
                        .any(|key| key.is_empty() || !assigned.insert(key))
            })
        {
            return Err("无效的配色分组 / Invalid color groups".into());
        }
        Ok(())
    }
    pub fn apply(&self, c: &mut Config) {
        c.palette = self.palette;
        c.materials = self.materials;
        c.extra_filaments.clone_from(&self.extra_filaments);
        c.filament = self.filament;
        c.role_filaments = self.roles.clone();
        c.component_filaments = self
            .role_overrides
            .iter()
            .map(|(key, role)| (key.clone(), self.role_slot(role)))
            .collect();
        for group in &self.groups {
            c.component_filaments
                .extend(group.components.iter().map(|key| (key.clone(), group.slot)));
        }
        c.component_text_filaments = self.component_text.clone();
        c.part_filaments.clear();
        c.text_filaments.clear();
        c.preview_colors = true;
    }
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}
impl Rect {
    const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    fn corners(self) -> [Vec2; 4] {
        [
            Vec2::new(self.x, self.y),
            Vec2::new(self.right(), self.y),
            Vec2::new(self.right(), self.top()),
            Vec2::new(self.x, self.top()),
        ]
    }
    fn expanded(self, padding: f32) -> Self {
        Self {
            x: self.x - padding,
            y: self.y - padding,
            w: self.w + 2. * padding,
            h: self.h + 2. * padding,
        }
    }
    fn right(self) -> f32 {
        self.x + self.w
    }
    fn top(self) -> f32 {
        self.y + self.h
    }
    fn intersects(self, b: Self) -> bool {
        self.x < b.right() - 0.0001
            && b.x < self.right() - 0.0001
            && self.y < b.top() - 0.0001
            && b.y < self.top() - 0.0001
    }
    fn contains(self, b: Self) -> bool {
        self.x <= b.x && self.y <= b.y && self.right() >= b.right() && self.top() >= b.top()
    }
}
// Bambu's preview checks the convex hull of ALL extrusion paths against the
// front-left exclusion, even when every individual object avoids that corner.
fn extend_hull(hull: &[Vec2], rect: Rect) -> Vec<Vec2> {
    let mut points = hull.to_vec();
    points.extend(rect.corners());
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    let mut result = Vec::<Vec2>::new();
    for p in &points {
        while result.len() >= 2 {
            let n = result.len();
            if (result[n - 1] - result[n - 2]).perp_dot(*p - result[n - 1]) > 0. {
                break;
            }
            result.pop();
        }
        result.push(*p);
    }
    let lower = result.len();
    for p in points.iter().rev().skip(1) {
        while result.len() > lower {
            let n = result.len();
            if (result[n - 1] - result[n - 2]).perp_dot(*p - result[n - 1]) > 0. {
                break;
            }
            result.pop();
        }
        result.push(*p);
    }
    result.pop();
    result
}
fn hull_intersects(hull: &[Vec2], rect: Rect) -> bool {
    if hull.is_empty() {
        return false;
    }
    let corners = rect.corners();
    let edges = hull.iter().zip(hull.iter().cycle().skip(1)).map(|(a, b)| {
        let d = *b - *a;
        Vec2::new(-d.y, d.x).normalize_or_zero()
    });
    for axis in [Vec2::X, Vec2::Y].into_iter().chain(edges) {
        if axis == Vec2::ZERO {
            continue;
        }
        let range = |points: &[Vec2]| {
            points
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                    let v = p.dot(axis);
                    (lo.min(v), hi.max(v))
                })
        };
        let (a, b) = range(hull);
        let (c, d) = range(&corners);
        if b <= c + 0.0001 || d <= a + 0.0001 {
            return false;
        }
    }
    true
}
fn subtract(free: &mut Vec<Rect>, used: Rect) {
    let mut next = Vec::new();
    for &r in free.iter() {
        if !r.intersects(used) {
            next.push(r);
            continue;
        }
        if used.x > r.x {
            next.push(Rect {
                w: used.x - r.x,
                ..r
            });
        }
        if used.right() < r.right() {
            next.push(Rect {
                x: used.right(),
                w: r.right() - used.right(),
                ..r
            });
        }
        if used.y > r.y {
            next.push(Rect {
                h: used.y - r.y,
                ..r
            });
        }
        if used.top() < r.top() {
            next.push(Rect {
                y: used.top(),
                h: r.top() - used.top(),
                ..r
            });
        }
    }
    *free = next
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            !next
                .iter()
                .enumerate()
                .any(|(j, b)| i != &j && b.contains(**r) && (!r.contains(*b) || j < *i))
        })
        .map(|(_, r)| *r)
        .collect();
}
const PRIME_LINE_CLEARANCE: f32 = 2.;
impl Printer {
    /// Startup extrusion paths, including optional flow calibration, in nozzle
    /// coordinates from the Bambu Studio 02.05.00.66 machine start presets.
    fn start_lines(self) -> &'static [Rect] {
        const P1S: &[Rect] = &[
            Rect::new(18., 1., 222., 0.5),
            Rect::new(239.5, 1., 0.5, 10.),
        ];
        // Both nozzle-load lines, right return, and the optical calibration pattern.
        const X1C: &[Rect] = &[
            Rect::new(18., 1., 222., 1.5),
            Rect::new(239., 1., 1., 11.),
            Rect::new(35., 1.5, 196., 10.5),
        ];
        const A1: &[Rect] = &[Rect::new(108., -0.5, 45., 1.5)];
        const A1_MINI: &[Rect] = &[Rect::new(68., -4., 45., 1.5)];
        match self {
            Self::P1s => P1S,
            Self::X1c => X1C,
            Self::A1 => A1,
            Self::A1Mini => A1_MINI,
        }
    }
}

struct PackingBed {
    bounds: Rect,
    corner: Option<Rect>,
    exclusions: Vec<Rect>,
}
fn usable_bed(c: &Config, offset: Vec2) -> Rect {
    // Both the model and the G-code (model minus nozzle offset) must fit.
    let size = Vec2::from_array(c.printer.bed_size());
    let min = offset.max(Vec2::ZERO) + Vec2::splat(c.margin);
    let max = size + offset.min(Vec2::ZERO) - Vec2::splat(c.margin);
    Rect::new(min.x, min.y, max.x - min.x, max.y - min.y)
}
impl PackingBed {
    fn new(c: &Config) -> Self {
        let offset = Vec2::from_array(c.printer.nozzle_offset());
        let both_frames = |r: Rect| {
            Rect::new(
                r.x + offset.x.min(0.),
                r.y + offset.y.min(0.),
                r.w + offset.x.abs(),
                r.h + offset.y.abs(),
            )
        };
        let [w, h] = c.printer.excluded_corner();
        let corner =
            (w > 0. && h > 0.).then(|| both_frames(Rect::new(0., 0., w + c.margin, h + c.margin)));
        let mut exclusions = Vec::new();
        if let Some(r) = corner {
            exclusions.push(Rect::new(r.x, r.y, r.w + c.gap, r.h + c.gap));
        }
        exclusions.extend(
            c.printer
                .start_lines()
                .iter()
                .map(|&r| both_frames(r).expanded(PRIME_LINE_CLEARANCE + c.margin + c.gap)),
        );
        Self {
            bounds: usable_bed(c, offset),
            corner,
            exclusions,
        }
    }

    fn hull_blocked(&self, hull: &[Vec2]) -> bool {
        self.corner
            .is_some_and(|corner| hull_intersects(hull, corner))
    }
}
fn empty_plate(c: &Config, bed: &PackingBed) -> Vec<Rect> {
    let mut free = vec![bed.bounds.expanded(c.gap / 2.)];
    for zone in &bed.exclusions {
        // Packed rectangles already include half the part gap on every side.
        // Contract obstacles by that amount to match the physical-footprint
        // checks used after centering, without counting the gap twice.
        subtract(&mut free, zone.expanded(-c.gap / 2.));
    }
    free
}
#[derive(Clone, Debug)]
pub struct Placement {
    pub input: usize,
    pub copy: u32,
    pub plate: usize,
    pub x: f32,
    pub y: f32,
    pub rotated: bool,
    pub width: f32,
    pub depth: f32,
}
impl Placement {
    fn footprint(&self) -> Rect {
        Rect {
            x: self.x - self.width / 2.,
            y: self.y - self.depth / 2.,
            w: self.width,
            h: self.depth,
        }
    }
}
fn parallel_family(name: &str, size: Vec3) -> Option<&str> {
    if size.x.max(size.y) < 3. * size.x.min(size.y) {
        return None;
    }
    Some(crate::catalog::print_alignment_family(name))
}
// Optimize within existing plates only: this preference must never increase
// plate count or change material groups. A group is a parallel, end-aligned band.
// If the entire band cannot fit safely, preserve the valid MaxRects result.
fn align_parallel_groups(
    parts: &[(String, Vec3, u32)],
    placed: &mut [Placement],
    c: &Config,
    bed: &PackingBed,
) {
    let mut groups = std::collections::BTreeMap::<(usize, &str), Vec<usize>>::new();
    for (i, p) in placed.iter().enumerate() {
        if let Some(family) = parallel_family(&parts[p.input].0, parts[p.input].1) {
            groups.entry((p.plate, family)).or_default().push(i);
        }
    }
    for ((plate, _), mut ids) in groups {
        if ids.len() < 2 {
            continue;
        }
        // Longest first, stable source/copy tie-breaks. Both source orientations
        // are normalized by long axis; equal rotated flags are not required.
        ids.sort_by(|&a, &b| {
            placed[b]
                .width
                .max(placed[b].depth)
                .total_cmp(&placed[a].width.max(placed[a].depth))
                .then((placed[a].input, placed[a].copy).cmp(&(placed[b].input, placed[b].copy)))
        });
        let members: std::collections::BTreeSet<_> = ids.iter().copied().collect();
        let fixed: Vec<_> = placed
            .iter()
            .enumerate()
            .filter(|(i, p)| p.plate == plate && !members.contains(i))
            .map(|(_, p)| p.footprint())
            .collect();
        let mut free = empty_plate(c, bed);
        let mut fixed_hull = Vec::new();
        for r in &fixed {
            subtract(&mut free, r.expanded(c.gap / 2.));
            fixed_hull = extend_hull(&fixed_hull, *r);
        }
        let long = ids
            .iter()
            .map(|&i| placed[i].width.max(placed[i].depth))
            .fold(0., f32::max);
        let band = ids
            .iter()
            .map(|&i| placed[i].width.min(placed[i].depth))
            .sum::<f32>()
            + c.gap * (ids.len() - 1) as f32;
        let mut best: Option<((f32, f32), Rect, bool)> = None;
        for horizontal in [true, false] {
            if !c.rotate
                && ids.iter().any(|&i| {
                    let s = parts[placed[i].input].1;
                    (s.x >= s.y) != horizontal
                })
            {
                continue;
            }
            let (w, h) = if horizontal {
                (long, band)
            } else {
                (band, long)
            };
            for r in &free {
                if w + c.gap > r.w + 0.0001 || h + c.gap > r.h + 0.0001 {
                    continue;
                }
                let rect = Rect {
                    x: r.x + c.gap / 2.,
                    y: r.y + c.gap / 2.,
                    w,
                    h,
                };
                let hull = extend_hull(&fixed_hull, rect);
                if bed.hull_blocked(&hull) {
                    continue;
                }
                let min = hull
                    .iter()
                    .copied()
                    .fold(Vec2::splat(f32::INFINITY), Vec2::min);
                let max = hull
                    .iter()
                    .copied()
                    .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
                let score = ((max.x - min.x) * (max.y - min.y), (r.w - w).min(r.h - h));
                if best.as_ref().is_none_or(|(old, _, _)| score < *old) {
                    best = Some((score, rect, horizontal));
                }
            }
        }
        if let Some((_, rect, horizontal)) = best {
            let mut along_band = 0.;
            for i in ids {
                let p = &mut placed[i];
                let long = p.width.max(p.depth);
                let short = p.width.min(p.depth);
                (p.width, p.depth) = if horizontal {
                    (long, short)
                } else {
                    (short, long)
                };
                p.x = rect.x + p.width / 2. + if horizontal { 0. } else { along_band };
                p.y = rect.y + p.depth / 2. + if horizontal { along_band } else { 0. };
                let s = parts[p.input].1;
                p.rotated = (s.x >= s.y) != horizontal;
                along_band += short + c.gap;
            }
        }
    }
}
pub fn arrange(parts: &[(String, Vec3, u32)], c: &Config) -> Result<Vec<Placement>, String> {
    c.validate()?;
    let mut jobs = Vec::new();
    for (i, (name, size, qty)) in parts.iter().enumerate() {
        if !size.is_finite()
            || size.x <= 0.
            || size.y <= 0.
            || size.z < 0.
            || *qty == 0
            || *qty > 100
        {
            return Err(format!("{name}：尺寸或份数无效"));
        }
        if size.z > c.printer.height() + 0.001 {
            return Err(format!(
                "{name}：高度 {:.1} mm 超出 {} 的 {} mm 限高",
                size.z,
                c.printer.name(),
                c.printer.height()
            ));
        }
        for copy in 0..*qty {
            jobs.push((i, copy));
        }
    }
    if jobs.len() > 1000 {
        return Err("单次导出最多 1000 份零件".into());
    }
    jobs.sort_by(|a, b| {
        (parts[b.0].1.x * parts[b.0].1.y).total_cmp(&(parts[a.0].1.x * parts[a.0].1.y))
    });
    let bed = PackingBed::new(c);
    if bed.bounds.w <= 0. || bed.bounds.h <= 0. {
        return Err("喷嘴偏移与边距超出可用打印区域".into());
    }
    let initial_free = empty_plate(c, &bed);
    let mut plates = vec![initial_free.clone()];
    let mut hulls = vec![Vec::<Vec2>::new()];
    let brim = c.brim_extent()?;
    let mut placed = Vec::new();
    for (input, copy) in jobs {
        let raw_size = parts[input].1;
        let size = Vec3::new(raw_size.x + 2. * brim, raw_size.y + 2. * brim, raw_size.z);
        let mut candidate = None;
        loop {
            for (plate, free) in plates.iter().enumerate() {
                for r in free {
                    for rotated in [false, true] {
                        if rotated && !c.rotate {
                            continue;
                        }
                        let (w, h) = if rotated {
                            (size.y + c.gap, size.x + c.gap)
                        } else {
                            (size.x + c.gap, size.y + c.gap)
                        };
                        if w > r.w + 0.0001 || h > r.h + 0.0001 {
                            continue;
                        }
                        let score = (r.w - w).min(r.h - h);
                        if candidate.as_ref().is_none_or(|(old, _, _, _)| score < *old) {
                            let candidate_rect = Rect {
                                x: r.x,
                                y: r.y,
                                w,
                                h,
                            };
                            let hull =
                                extend_hull(&hulls[plate], candidate_rect.expanded(-c.gap / 2.));
                            if bed.hull_blocked(&hull) {
                                continue;
                            }
                            candidate = Some((
                                score,
                                plate,
                                Rect {
                                    x: r.x,
                                    y: r.y,
                                    w,
                                    h,
                                },
                                rotated,
                            ));
                        }
                    }
                }
                if candidate.is_some() {
                    break;
                }
            }
            if candidate.is_some() {
                break;
            }
            if placed
                .iter()
                .all(|p: &Placement| p.plate != plates.len() - 1)
            {
                return Err(format!(
                    "{}：{:.1} × {:.1} mm，无法放入 {} 可用区域（含边距、喷嘴偏移和擦料线避让）",
                    parts[input].0,
                    size.x,
                    size.y,
                    c.printer.name()
                ));
            }
            if !c.split {
                return Err("当前零件无法放入单个打印盘，请开启自动分盘".into());
            }
            if plates.len() >= 36 {
                return Err("导出超过 36 个打印盘，请减少零件份数".into());
            }
            plates.push(initial_free.clone());
            hulls.push(Vec::new());
        }
        let (_, plate, r, rotated) = candidate.unwrap();
        subtract(&mut plates[plate], r);
        hulls[plate] = extend_hull(&hulls[plate], r.expanded(-c.gap / 2.));
        placed.push(Placement {
            input,
            copy,
            plate,
            x: r.x + r.w / 2.,
            y: r.y + r.h / 2.,
            rotated,
            width: r.w - c.gap,
            depth: r.h - c.gap,
        });
    }
    align_parallel_groups(parts, &mut placed, c, &bed);
    // Groups may have moved; centering must use their new hulls.
    for (plate, hull) in hulls.iter_mut().enumerate() {
        hull.clear();
        for p in placed.iter().filter(|p| p.plate == plate) {
            *hull = extend_hull(hull, p.footprint());
        }
    }
    if c.center {
        for (plate, hull) in hulls.iter().enumerate() {
            let group: Vec<_> = placed.iter().filter(|p| p.plate == plate).collect();
            if group.is_empty() {
                continue;
            }
            let min_x = group
                .iter()
                .map(|p| p.x - p.width / 2.)
                .fold(f32::INFINITY, f32::min);
            let max_x = group
                .iter()
                .map(|p| p.x + p.width / 2.)
                .fold(f32::NEG_INFINITY, f32::max);
            let min_y = group
                .iter()
                .map(|p| p.y - p.depth / 2.)
                .fold(f32::INFINITY, f32::min);
            let max_y = group
                .iter()
                .map(|p| p.y + p.depth / 2.)
                .fold(f32::NEG_INFINITY, f32::max);
            let [width, depth] = c.printer.bed_size();
            let dx = (width - (min_x + max_x)) / 2.;
            let dy = (depth - (min_y + max_y)) / 2.;
            let safe = group.iter().all(|p| {
                let r = Rect {
                    x: p.x + dx - p.width / 2.,
                    y: p.y + dy - p.depth / 2.,
                    w: p.width,
                    h: p.depth,
                };
                bed.bounds.expanded(0.0001).contains(r)
                    && bed.exclusions.iter().all(|zone| !r.intersects(*zone))
            });
            // Rigid translation preserves spacing and brim; keep valid packing if blocked.
            let shifted_hull: Vec<_> = hull.iter().map(|v| *v + Vec2::new(dx, dy)).collect();
            if safe && !bed.hull_blocked(&shifted_hull) {
                for p in placed.iter_mut().filter(|p| p.plate == plate) {
                    p.x += dx;
                    p.y += dy;
                }
            }
        }
    }
    // Stable source/copy order, independent of packing order.
    placed.sort_by_key(|p| (p.input, p.copy));
    // Recompute from the final placements, including any centering. Fail closed
    // if later changes to the packer violate the slicer's whole-plate rule.
    for plate in 0..plates.len() {
        let mut hull = Vec::new();
        for p in placed.iter().filter(|p| p.plate == plate) {
            let r = Rect {
                x: p.x - p.width / 2.,
                y: p.y - p.depth / 2.,
                w: p.width,
                h: p.depth,
            };
            if !bed.bounds.expanded(0.001).contains(r)
                || bed.exclusions.iter().any(|zone| r.intersects(*zone))
            {
                return Err(format!(
                    "第 {} 盘 {}：最终打印占地越过边界或避让区域",
                    plate + 1,
                    parts[p.input].0
                ));
            }
            hull = extend_hull(&hull, r);
        }
        if bed.hull_blocked(&hull) {
            return Err(format!("第 {} 盘：整盘轮廓凸包进入左前禁放区", plate + 1));
        }
    }
    for (i, p) in placed.iter().enumerate() {
        for q in &placed[i + 1..] {
            if p.plate == q.plate
                && p.footprint()
                    .expanded(c.gap / 2.)
                    .intersects(q.footprint().expanded(c.gap / 2.))
            {
                return Err(format!(
                    "第 {} 盘：{} 与 {} 的最终占地间距不足",
                    p.plate + 1,
                    parts[p.input].0,
                    parts[q.input].0
                ));
            }
        }
    }
    Ok(placed)
}
/// Each filament/color slot owns its own plates, even when two RGB values match.
/// Keeping slots separate preserves their potentially different material settings.
#[cfg(test)]
pub fn arrange_colored(
    parts: &[(String, Vec3, u32)],
    paths: &[String],
    c: &Config,
) -> Result<Vec<Placement>, String> {
    let materials: Vec<_> = paths.iter().map(|p| vec![c.slot(p)]).collect();
    arrange_materials(parts, &materials, c)
}
/// Physical print groups never share plates, even with identical material sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlateGroup {
    Standard,
    RemovableLabel,
}
/// A multi-material object is indivisible; group plates by the complete material set.
#[cfg(test)]
pub fn arrange_materials(
    parts: &[(String, Vec3, u32)],
    materials: &[Vec<u32>],
    c: &Config,
) -> Result<Vec<Placement>, String> {
    arrange_grouped_materials(
        parts,
        materials,
        &vec![PlateGroup::Standard; parts.len()],
        c,
    )
}
/// Keep detachable labels on dedicated plates, then split each group by material set.
pub fn arrange_grouped_materials(
    parts: &[(String, Vec3, u32)],
    materials: &[Vec<u32>],
    plate_groups: &[PlateGroup],
    c: &Config,
) -> Result<Vec<Placement>, String> {
    c.validate()?;
    if parts.len() != materials.len() || parts.len() != plate_groups.len() {
        return Err("颜色配置与零件数量不一致".into());
    }
    if parts.iter().map(|p| u64::from(p.2)).sum::<u64>() > 1000 {
        return Err("单次导出最多 1000 份零件".into());
    }
    let mut all = Vec::new();
    let mut offset = 0;
    let mut groups = std::collections::BTreeMap::<(PlateGroup, Vec<u32>), Vec<usize>>::new();
    for (i, slots) in materials.iter().enumerate() {
        let mut key = slots.clone();
        key.sort_unstable();
        key.dedup();
        if key.is_empty() || key.iter().any(|s| !(1..=c.slot_count()).contains(s)) {
            return Err("无效耗材组合".into());
        }
        groups.entry((plate_groups[i], key)).or_default().push(i);
    }
    for ((_plate_group, slots), ids) in groups {
        let group: Vec<_> = ids.iter().map(|&i| parts[i].clone()).collect();
        let mut layout = arrange(&group, c).map_err(|e| format!("耗材组合 {slots:?}：{e}"))?;
        let count = layout.iter().map(|p| p.plate + 1).max().unwrap_or(0);
        for p in &mut layout {
            p.input = ids[p.input];
            p.plate += offset;
        }
        all.extend(layout);
        offset += count;
        if offset > 36 {
            return Err("分色后超过 36 个打印盘，请减少份数".into());
        }
    }
    all.sort_by_key(|p| (p.input, p.copy));
    Ok(all)
}
impl Printer {
    pub fn plate_origin(self, plate: usize, count: usize) -> [f32; 2] {
        let columns = (count.max(1) as f32).sqrt().ceil() as usize;
        let [width, depth] = self.bed_size();
        [
            (plate % columns) as f32 * (width * 1.2),
            -((plate / columns) as f32) * (depth * 1.2),
        ]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runner_plate_forms_a_parallel_aligned_compact_band() {
        let parts = vec![
            ("x4 Runner Guide.stl".into(), Vec3::new(27., 185.5, 9.4), 1),
            (
                "x4 Runner Slide.stl".into(),
                Vec3::new(18.221, 160., 8.979),
                2,
            ),
        ];
        for center in [false, true] {
            for brim in [0., 2.] {
                let c = Config {
                    center,
                    brim,
                    ..Default::default()
                };
                let result = arrange(&parts, &c).unwrap();
                assert_eq!(result.len(), 3);
                assert!(result.iter().all(|p| p.plate == 0));
                assert_machine_safe(&result, &c);
                let horizontal = result[0].width > result[0].depth;
                let anchor = result[0].footprint();
                for p in &result {
                    assert_eq!(p.width > p.depth, horizontal);
                    let r = p.footprint();
                    assert!(
                        (if horizontal {
                            r.x - anchor.x
                        } else {
                            r.y - anchor.y
                        })
                        .abs()
                            < 0.001
                    );
                }
                let min = result
                    .iter()
                    .flat_map(|p| p.footprint().corners())
                    .fold(Vec2::splat(f32::INFINITY), Vec2::min);
                let max = result
                    .iter()
                    .flat_map(|p| p.footprint().corners())
                    .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
                let size = max - min;
                assert!(
                    (size.max_element() - (185.5 + 2. * c.brim_extent().unwrap())).abs() < 0.001
                );
                assert!(
                    (size.min_element()
                        - (27. + 2. * 18.221 + 12. + 6. * c.brim_extent().unwrap()))
                    .abs()
                        < 0.001
                );
            }
        }
    }
    #[test]
    fn parallel_preference_uses_long_axes_and_honors_rotation_lock() {
        let parts = vec![
            ("x4 Runner Guide.stl".into(), Vec3::new(27., 185.5, 9.4), 1),
            (
                "x4 Runner Slide.stl".into(),
                Vec3::new(160., 18.221, 8.979),
                2,
            ),
        ];
        let enabled = arrange(&parts, &Config::default()).unwrap();
        assert!(enabled.iter().all(|p| p.plate == 0));
        assert!(
            enabled
                .iter()
                .all(|p| (p.width > p.depth) == (enabled[0].width > enabled[0].depth))
        );
        assert_ne!(enabled[0].rotated, enabled[1].rotated);
        let c = Config {
            rotate: false,
            ..Default::default()
        };
        let locked = arrange(&parts, &c).unwrap();
        assert!(locked.iter().all(|p| !p.rotated));
        assert_machine_safe(&locked, &c);
        let mut unchanged = locked.clone();
        align_parallel_groups(&parts, &mut unchanged, &c, &PackingBed::new(&c));
        for (a, b) in unchanged.iter().zip(&locked) {
            assert_eq!(
                (a.plate, a.x, a.y, a.rotated),
                (b.plate, b.x, b.y, b.rotated)
            );
        }
    }
    #[test]
    fn oversized_parallel_band_preserves_existing_mixed_layout() {
        let parts = vec![("long rail".into(), Vec3::new(180., 30., 5.), 8)];
        let c = Config::default();
        let mut placed: Vec<_> = (0..8)
            .map(|copy| {
                let (x, y, width, depth, rotated) = if copy < 6 {
                    (90.5, 51.5 + copy as f32 * 36., 180., 30., false)
                } else {
                    (201.5 + (copy - 6) as f32 * 36., 126.5, 30., 180., true)
                };
                Placement {
                    input: 0,
                    copy,
                    plate: 0,
                    x,
                    y,
                    width,
                    depth,
                    rotated,
                }
            })
            .collect();
        assert_machine_safe(&placed, &c);
        let before = placed.clone();
        // A parallel band is 282 mm wide; the mixed orientation layout fits.
        align_parallel_groups(&parts, &mut placed, &c, &PackingBed::new(&c));
        for (a, b) in before.iter().zip(&placed) {
            assert_eq!(
                (a.x, a.y, a.rotated, a.plate),
                (b.x, b.y, b.rotated, b.plate)
            );
        }
    }
    #[test]
    fn parallel_groups_keep_plates_materials_and_unrelated_parts() {
        let parts = vec![
            ("x4 Runner Guide.stl".into(), Vec3::new(27., 185.5, 9.4), 4),
            (
                "x4 Runner Slide.stl".into(),
                Vec3::new(160., 18.221, 8.979),
                8,
            ),
            ("unrelated".into(), Vec3::new(100., 100., 10.), 3),
        ];
        let c = Config::default();
        let mut result = arrange(&parts, &c).unwrap();
        let before = result.clone();
        align_parallel_groups(&parts, &mut result, &c, &PackingBed::new(&c));
        for (a, b) in before.iter().zip(&result) {
            assert_eq!((a.input, a.copy, a.plate), (b.input, b.copy, b.plate));
            if a.input == 2 {
                assert_eq!((a.x, a.y, a.rotated), (b.x, b.y, b.rotated));
            }
        }
        assert_machine_safe(&result, &c);
        let separate = arrange_materials(&parts[..2], &[vec![1], vec![2]], &c).unwrap();
        assert!(separate.iter().all(|a| {
            separate
                .iter()
                .all(|b| a.plate != b.plate || a.input == b.input)
        }));
        assert!(parallel_family("square", Vec3::new(20., 20., 2.)).is_none());
        assert_ne!(
            parallel_family("unrelated A", Vec3::new(100., 10., 2.)),
            parallel_family("unrelated B", Vec3::new(100., 10., 2.))
        );
    }
    #[test]
    fn individually_clear_parts_can_fail_bambu_whole_plate_hull() {
        // The user's plate 4: the baseplate is left of the cap. Neither part
        // touches the corner, but their connecting convex hull does.
        let base = Rect {
            x: 0.5,
            y: 36.5,
            w: 176.,
            h: 214.699,
        };
        let cap = Rect {
            x: 24.5,
            y: 12.,
            w: 37.799,
            h: 13.172,
        };
        let forbidden = Rect {
            x: 0.,
            y: 0.,
            w: 18.,
            h: 30.,
        };
        assert!(!base.intersects(forbidden) && !cap.intersects(forbidden));
        assert!(hull_intersects(
            &extend_hull(&extend_hull(&[], base), cap),
            forbidden
        ));
        let moved_cap = Rect { x: 182.5, ..cap };
        assert!(!hull_intersects(
            &extend_hull(&extend_hull(&[], base), moved_cap),
            forbidden
        ));
        let parts = vec![
            ("baseplate".into(), Vec3::new(214.699, 176., 4.), 2),
            ("cap".into(), Vec3::new(37.799, 13.172, 11.673), 2),
            ("bin".into(), Vec3::new(41.5, 83.5, 31.55), 4),
            ("facade".into(), Vec3::new(9.84, 9.84, 3.8), 6),
        ];
        for center in [false, true] {
            for rotate in [false, true] {
                for brim in [0., 2.] {
                    let c = Config {
                        center,
                        rotate,
                        brim,
                        ..Default::default()
                    };
                    let placements = arrange(&parts, &c).unwrap();
                    assert_machine_safe(&placements, &c);
                }
            }
        }
    }
    #[test]
    fn brim_gap_is_part_of_print_footprint() {
        let c = Config {
            brim: 2.,
            ..Default::default()
        };
        let placements = arrange(&[("brimmed".into(), Vec3::new(10., 10., 1.), 1)], &c).unwrap();
        assert!((placements[0].width - 14.2).abs() < 0.001);
        assert_eq!(Config::default().brim_extent().unwrap(), 0.);
    }
    fn assert_machine_safe(layout: &[Placement], c: &Config) {
        let offset = Vec2::from_array(c.printer.nozzle_offset());
        let [width, depth] = c.printer.bed_size();
        let [corner_w, corner_h] = c.printer.excluded_corner();
        for p in layout {
            for origin in [Vec2::ZERO, offset] {
                let r = Rect {
                    x: p.x - p.width / 2. - origin.x,
                    y: p.y - p.depth / 2. - origin.y,
                    w: p.width,
                    h: p.depth,
                };
                assert!(
                    r.x >= c.margin - 0.001
                        && r.y >= c.margin - 0.001
                        && r.right() <= width - c.margin + 0.001
                        && r.top() <= depth - c.margin + 0.001,
                    "Unsafe model/machine footprint: {p:?}, offset {origin:?}"
                );
                if corner_w > 0. && corner_h > 0. {
                    assert!(!r.intersects(Rect::new(0., 0., corner_w, corner_h)));
                }
                // Independent machine-coordinate check includes the real purge
                // segments and fixed clearance, including model Brim in r.
                for line in c.printer.start_lines() {
                    assert!(
                        !r.intersects(line.expanded(2.)),
                        "Footprint overlaps start purge line: {p:?}, frame {origin:?}"
                    );
                }
            }
        }
        for plate in 0..layout.iter().map(|p| p.plate + 1).max().unwrap_or(0) {
            let mut hull = Vec::new();
            for p in layout.iter().filter(|p| p.plate == plate) {
                hull = extend_hull(
                    &hull,
                    Rect {
                        x: p.x - p.width / 2.,
                        y: p.y - p.depth / 2.,
                        w: p.width,
                        h: p.depth,
                    },
                );
            }
            assert!(
                !PackingBed::new(c).hull_blocked(&hull),
                "Unsafe plate hull: {plate}"
            );
        }
    }
    #[test]
    fn purge_exclusions_follow_embedded_start_extrusions() {
        let profile: serde_json::Value =
            serde_json::from_str(include_str!("../config/export/p1s-0.4.json")).unwrap();
        let start = profile["machine_start_gcode"]
            .as_str()
            .unwrap()
            .split(";===== nozzle load line")
            .nth(1)
            .unwrap()
            .split("M400")
            .next()
            .unwrap();
        let mut position: Option<Vec2> = None;
        let mut count = 0;
        for line in start
            .lines()
            .filter(|l| l.starts_with("G0 ") || l.starts_with("G1 "))
        {
            let mut end = position.unwrap_or(Vec2::splat(f32::NAN));
            let mut extrusion = 0.;
            for token in line.split(';').next().unwrap().split_whitespace().skip(1) {
                if let Ok(value) = token[1..].parse::<f32>() {
                    match token.as_bytes()[0] {
                        b'X' => end.x = value,
                        b'Y' => end.y = value,
                        b'E' => extrusion = value,
                        _ => {}
                    }
                }
            }
            if let Some(previous) = position
                && extrusion > 0.
                && previous != end
            {
                let min = previous.min(end);
                let size = (end - previous).abs();
                let path = Rect {
                    x: min.x,
                    y: min.y,
                    w: size.x,
                    h: size.y,
                };
                assert!(
                    Printer::P1s
                        .start_lines()
                        .iter()
                        .any(|zone| zone.contains(path)),
                    "Start extrusion changed; update purge exclusions: {line}"
                );
                count += 1;
            }
            assert!(end.is_finite());
            position = Some(end);
        }
        assert_eq!(count, 4);
    }
    #[test]
    fn every_plate_avoids_purge_with_rotation_brim_and_centering() {
        let parts = vec![
            ("large".into(), Vec3::new(180., 180., 20.), 5),
            ("labels".into(), Vec3::new(36., 11., 1.2), 30),
            ("wide".into(), Vec3::new(120., 24., 5.), 5),
        ];
        for center in [false, true] {
            for rotate in [false, true] {
                for gap in [0., 6., 30.] {
                    for brim in [0., 2.] {
                        let c = Config {
                            center,
                            rotate,
                            gap,
                            brim,
                            margin: 0.,
                            ..Default::default()
                        };
                        let layout = arrange(&parts, &c).unwrap();
                        assert!(layout.iter().any(|p| p.plate > 0));
                        assert_machine_safe(&layout, &c);
                    }
                }
            }
        }
    }
    #[test]
    fn active_machine_controls_height_fit_and_plate_origin() {
        let parts = |width, height| vec![("module".into(), Vec3::new(width, 20., height), 1)];
        for (printer, max_height) in [
            (Printer::P1s, 250.),
            (Printer::X1c, 250.),
            (Printer::A1, 256.),
            (Printer::A1Mini, 180.),
        ] {
            let c = Config {
                printer,
                center: true,
                ..Config::default()
            };
            let p = arrange(&parts(50., max_height), &c).unwrap();
            let [width, depth] = printer.bed_size();
            assert_eq!((p[0].x, p[0].y), (width / 2., depth / 2.));
            assert!(
                arrange(&parts(50., max_height + 1.), &c)
                    .unwrap_err()
                    .contains(printer.name())
            );
            assert_eq!(
                arrange(&parts(181., 10.), &c).is_ok(),
                printer != Printer::A1Mini
            );

            let placements =
                arrange(&[("bins".into(), Vec3::new(140., 140., 25.), 3)], &c).unwrap();
            assert!(placements.iter().any(|p| p.plate > 0));
            assert_machine_safe(&placements, &c);
        }
        let origin = Vec2::from_array(Printer::A1Mini.plate_origin(3, 4));
        assert!(origin.abs_diff_eq(Vec2::new(216., -216.), 0.001));
    }
    #[test]
    fn machine_start_exclusions_follow_each_printers_front_edge() {
        let obstacles = |printer| {
            PackingBed::new(&Config {
                printer,
                margin: 0.,
                gap: 0.,
                ..Default::default()
            })
        };
        let covers = |bed: &PackingBed, x, y| {
            bed.exclusions
                .iter()
                .any(|r| r.contains(Rect::new(x, y, 0., 0.)))
        };
        let p1s = obstacles(Printer::P1s);
        let x1c = obstacles(Printer::X1c);
        let a1 = obstacles(Printer::A1);
        let mini = obstacles(Printer::A1Mini);
        assert!(covers(&p1s, 2., 2.) && covers(&x1c, 2., 2.));
        assert!(!covers(&a1, 2., 2.) && !covers(&mini, 2., 2.));
        assert!(covers(&x1c, 180., 12.) && !covers(&p1s, 180., 12.));
        assert!(covers(&a1, 125., 1.) && !covers(&mini, 125., 1.));
        assert!(covers(&mini, 88., -2.5) && !covers(&a1, 88., -2.5));
    }
    #[test]
    fn centering_cannot_move_a_wide_part_onto_the_purge_return() {
        let parts = vec![("wide".into(), Vec3::new(207., 218., 10.), 1)];
        let c = Config {
            rotate: false,
            ..Default::default()
        };
        let original = arrange(&parts, &c).unwrap();
        let centered = arrange(
            &parts,
            &Config {
                center: true,
                ..c.clone()
            },
        )
        .unwrap();
        // Centering gives X=24.5..231.5 and Y=19..237: clear of the front-left
        // keep-out, but intersects the buffered right-hand purge return.
        assert_eq!(
            (original[0].x, original[0].y),
            (centered[0].x, centered[0].y)
        );
        assert_machine_safe(&centered, &c);
    }
    #[test]
    fn m4_packing_reserves_nozzle_offset_before_rotation_and_centering() {
        let parts = vec![
            (
                "M4 Module 1F".into(),
                Vec3::new(254.173, 57.009, 188.101),
                1,
            ),
            ("M4 Baseplate".into(), Vec3::new(214.699, 176., 4.), 1),
            ("Facade Cap".into(), Vec3::new(9.84, 9.84, 3.8), 3),
            ("Label".into(), Vec3::new(47.857, 20.627, 2.), 1),
            ("Key Cap".into(), Vec3::new(37.799, 13.172, 11.673), 1),
        ];
        for center in [false, true] {
            for margin in [0., 0.5] {
                let c = Config {
                    center,
                    margin,
                    ..Default::default()
                };
                let layout = arrange(&parts, &c).unwrap();
                assert_machine_safe(&layout, &c);
                assert!(
                    !layout[0].rotated,
                    "254 mm module cannot fit along offset Y axis"
                );
                assert!(layout.iter().any(|p| p.plate > 0));
                assert!(arrange(&parts, &Config { split: false, ..c }).is_err());
            }
        }
        let c = Config {
            rotate: false,
            center: true,
            brim: 2.,
            margin: 0.5,
            ..Default::default()
        };
        let layout = arrange(&[("brimmed".into(), Vec3::new(90., 100., 10.), 8)], &c).unwrap();
        assert_machine_safe(&layout, &c);
    }
    #[test]
    fn nozzle_envelope_handles_both_offset_signs() {
        let c = Config::default();
        let area = usable_bed(&c, Vec2::new(-3., 2.));
        assert_eq!(
            (area.x, area.y, area.right(), area.top()),
            (0.5, 2.5, 252.5, 255.5)
        );
    }
    /// Optional external regression fixture: dimensions only, no model library copy.
    #[test]
    #[ignore = "requires VAULT_PACKING_FIXTURE"]
    fn repack_external_plate_fixture() {
        let path = crate::test_support::required_env("VAULT_PACKING_FIXTURE");
        #[derive(Deserialize)]
        struct Fixture {
            config: Config,
            plates: Vec<Vec<(String, Vec3, u32)>>,
        }
        let fixture: Fixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let mut result = Vec::new();
        let mut offset = 0;
        for (old_plate, parts) in fixture.plates.iter().enumerate() {
            let placements = arrange(parts, &fixture.config).unwrap();
            assert_machine_safe(&placements, &fixture.config);
            let count = placements.iter().map(|p| p.plate + 1).max().unwrap_or(0);
            for p in placements {
                result.push(serde_json::json!({"old_plate":old_plate,"input":p.input,
                    "plate":p.plate+offset,"x":p.x,"y":p.y,"rotated":p.rotated}));
            }
            offset += count;
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("output/packing-boundary");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            out.join("fixed-layout.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn generated_type_colors_follow_future_ids_and_keep_text_separate() {
        let a = crate::gridfinity::Bin::default();
        let b = crate::gridfinity::Bin {
            id: 2,
            ..Default::default()
        };
        let future = crate::gridfinity::Bin {
            id: 99,
            ..Default::default()
        };
        let mut c = Config::default();
        c.set_part_slot(&a.path(), 3);
        c.set_part_slot(&a.label_path(), 4);
        c.set_text_slot(&b.label_path(), 2);
        for bin in [&a, &b, &future] {
            assert_eq!(c.slot(&bin.path()), 3);
            assert_eq!(c.slot(&bin.label_path()), 4);
            assert_eq!(c.text_slot(&bin.label_path()), 2);
            assert_eq!(c.text_slot(&bin.path()), 3);
            assert_eq!(c.color(&bin.label_path()), c.slot_color(4));
        }
        c.set_part_slot(crate::label::PATH, 6);
        let round: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(round.slot(&future.path()), 3);
        assert_eq!(round.text_slot(&future.label_path()), 2);
        assert_eq!(round.slot(crate::label::PATH), 6);
        c.set_part_slot(&b.label_path(), 0);
        assert_eq!(c.slot(&a.label_path()), c.role_filaments["accessory"]);
        assert_eq!(c.text_slot(&a.label_path()), 2);
        c.set_text_slot(&future.label_path(), 0);
        assert_eq!(c.text_slot(&a.label_path()), c.slot(&a.label_path()));
        assert_eq!(c.slot(&a.path()), 3);
    }
    #[test]
    fn old_generated_assignments_converge_by_id_and_reset_without_resurrection() {
        let mut c = Config::default();
        let first = "generated/gridfinity/Cullenect-2.stl";
        let later = "generated/gridfinity/Cullenect-10.stl";
        c.part_filaments.insert(later.into(), 5);
        c.part_filaments.insert(first.into(), 3);
        c.part_filaments.insert("other.stl".into(), 6);
        assert_eq!(c.part_assignment(first), 3);
        assert_eq!(c.part_assignment(later), 3);
        c.set_part_slot(later, 4);
        assert_eq!(c.slot(first), 4);
        assert!(!c.part_filaments.contains_key(first));
        assert!(!c.part_filaments.contains_key(later));
        assert_eq!(c.part_filaments.get(LABEL_COLOR_KEY), Some(&4));
        c.set_part_slot(first, 0);
        assert_eq!(c.slot(later), c.role_filaments["accessory"]);
        assert_eq!(c.slot("other.stl"), 6);
        assert_eq!(
            color_key("library/Gridfinity-2.stl"),
            "library/Gridfinity-2.stl"
        );
        assert_eq!(
            color_key("generated/gridfinity/Cullenect-custom.stl"),
            "generated/gridfinity/Cullenect-custom.stl"
        );
    }
    #[test]
    fn labels_have_dedicated_plates_even_with_identical_materials() {
        use PlateGroup::{RemovableLabel, Standard};
        let c = Config {
            center: true,
            ..Default::default()
        };
        let parts = vec![
            ("bin".into(), Vec3::new(42., 42., 60.), 1),
            ("label A".into(), Vec3::new(36., 11., 1.4), 2),
            ("label B".into(), Vec3::new(36., 11., 1.4), 1),
            ("other tall part".into(), Vec3::new(42., 42., 80.), 1),
        ];
        for slots in [vec![1], vec![1, 2]] {
            let materials = vec![slots; 4];
            let layout = arrange_grouped_materials(
                &parts,
                &materials,
                &[Standard, RemovableLabel, RemovableLabel, Standard],
                &c,
            )
            .unwrap();
            assert_eq!(layout.len(), 5);
            let label_plate = layout.iter().find(|p| p.input == 1).unwrap().plate;
            let standard_plate = layout.iter().find(|p| p.input == 0).unwrap().plate;
            assert_ne!(label_plate, standard_plate);
            for p in &layout {
                assert_eq!(
                    p.plate,
                    if p.input == 1 || p.input == 2 {
                        label_plate
                    } else {
                        standard_plate
                    }
                );
            }
        }
        assert!(arrange_grouped_materials(&parts, &vec![vec![1]; 4], &[Standard], &c).is_err());
        let parts = vec![
            ("bin".into(), Vec3::new(230., 200., 60.), 1),
            ("label overflow".into(), Vec3::new(230., 200., 1.2), 2),
        ];
        let out =
            arrange_grouped_materials(&parts, &[vec![1], vec![1]], &[Standard, RemovableLabel], &c)
                .unwrap();
        assert_eq!(out.len(), 3);
        assert_ne!(out[0].plate, out[1].plate);
        assert_ne!(out[0].plate, out[2].plate);
        assert_ne!(out[1].plate, out[2].plate);
    }
    #[test]
    fn material_sets_keep_text_with_body_and_separate_single_color() {
        let c = Config::default();
        let parts = vec![("a".into(), Vec3::splat(20.), 1); 4];
        let out =
            arrange_materials(&parts, &[vec![1, 2], vec![2, 1], vec![1], vec![2]], &c).unwrap();
        assert_eq!(out[0].plate, out[1].plate);
        assert_ne!(out[0].plate, out[2].plate);
        assert_ne!(out[0].plate, out[3].plate);
        let mut c = c;
        c.text_filaments.insert("label".into(), 6);
        let round: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(round.text_slot("label"), 6);
        assert_eq!(round.text_slot("plain"), round.slot("plain"));
        c.text_filaments.insert("bad".into(), 7);
        assert!(c.validate().is_err());
    }
    #[test]
    fn centering_preserves_spacing_and_avoids_keepout() {
        let mut c = Config {
            center: true,
            brim: 2.,
            ..Default::default()
        };
        let one = arrange(&[("small".into(), Vec3::new(20., 30., 10.), 1)], &c).unwrap();
        assert!((one[0].x - 128.).abs() < 0.001 && (one[0].y - 128.).abs() < 0.001);
        let parts = vec![("small".into(), Vec3::new(20., 30., 10.), 3)];
        let centered = arrange(&parts, &c).unwrap();
        c.center = false;
        let original = arrange(&parts, &c).unwrap();
        let dx = centered[0].x - original[0].x;
        let dy = centered[0].y - original[0].y;
        for (a, b) in centered.iter().zip(&original) {
            assert!((a.x - b.x - dx).abs() < 0.001 && (a.y - b.y - dy).abs() < 0.001);
            assert_eq!(a.plate, b.plate);
            assert_eq!(a.rotated, b.rotated);
        }
        c.brim = 0.;
        c.rotate = false;
        let large = vec![("large".into(), Vec3::new(220., 200., 10.), 1)];
        let original = arrange(&large, &c).unwrap();
        c.center = true;
        let fallback = arrange(&large, &c).unwrap();
        assert_eq!(
            (fallback[0].x, fallback[0].y),
            (original[0].x, original[0].y)
        );
        let paths = vec!["a".into(), "b".into()];
        c.part_filaments.insert("b".into(), 2);
        let colored = arrange_colored(
            &[
                ("a".into(), Vec3::splat(20.), 1),
                ("b".into(), Vec3::splat(30.), 1),
            ],
            &paths,
            &c,
        )
        .unwrap();
        assert_ne!(colored[0].plate, colored[1].plate);
        assert!(
            colored
                .iter()
                .all(|p| (p.x - 128.).abs() < 0.001 && (p.y - 128.).abs() < 0.001)
        );
    }
    #[test]
    fn color_groups_never_mix_and_keep_copies_overflow_and_slot_settings() {
        let paths = vec!["a".into(), "b".into(), "c".into()];
        let parts = vec![
            ("a".into(), Vec3::new(200., 200., 20.), 2),
            ("b".into(), Vec3::new(30., 30., 10.), 2),
            ("c".into(), Vec3::new(10., 10., 10.), 1),
        ];
        let mut c = Config::default();
        c.part_filaments.insert("b".into(), 2);
        c.part_filaments.insert("c".into(), 2);
        c.palette[1] = [32, 120, 210];
        let layout = arrange_colored(&parts, &paths, &c).unwrap();
        assert_eq!(layout.len(), 5);
        let count = layout.iter().map(|p| p.plate + 1).max().unwrap();
        assert_eq!(count, 3);
        for p in &layout {
            for q in &layout {
                if p.plate == q.plate {
                    assert_eq!(c.slot(&paths[p.input]), c.slot(&paths[q.input]));
                }
            }
        }
        assert!(
            arrange_colored(
                &parts,
                &paths,
                &Config {
                    split: false,
                    ..c.clone()
                }
            )
            .is_err()
        );
        let small = vec![
            ("a".into(), Vec3::new(10., 10., 10.), 1),
            ("b".into(), Vec3::new(10., 10., 10.), 1),
        ];
        c.palette[1] = c.palette[0];
        let split = arrange_colored(
            &small,
            &paths[..2],
            &Config {
                split: false,
                ..c.clone()
            },
        )
        .unwrap();
        assert_ne!(split[0].plate, split[1].plate);
        let restored: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(restored.slot("b"), 2);
        assert_eq!(restored.color("b"), c.color("b"));
        c.palette[1] = [32, 120, 210];
        let json: serde_json::Value = serde_json::from_slice(&c.project_json().unwrap()).unwrap();
        assert_eq!(json["filament_colour"][1], "#2078D2");
        c.part_filaments.insert("bad".into(), 7);
        assert!(c.validate().is_err());
    }
    #[test]
    fn rotation_fits_narrow_free_strip_and_invalid_settings_fail() {
        let parts = vec![
            ("large".into(), Vec3::new(200., 213., 10.), 1),
            ("strip".into(), Vec3::new(100., 30., 10.), 1),
        ];
        let config = Config {
            margin: 3.,
            brim: 0.,
            split: false,
            ..Default::default()
        };
        let layout = arrange(&parts, &config).unwrap();
        assert!(layout[1].rotated);
        assert!(
            arrange(
                &parts,
                &Config {
                    rotate: false,
                    ..config
                }
            )
            .is_err()
        );
        assert!(
            Config {
                gap: f32::NAN,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn p1s_multiple_plates_clearance_keepout_and_bounds() {
        let c = Config::default();
        let result = arrange(
            &[
                ("box".into(), Vec3::new(120., 100., 40.), 7),
                ("small".into(), Vec3::new(20., 30., 5.), 8),
            ],
            &c,
        )
        .unwrap();
        assert!(result.iter().any(|p| p.plate > 0));
        assert_eq!(result.len(), 15);
        assert_machine_safe(&result, &c);
        for (i, p) in result.iter().enumerate() {
            let r = Rect {
                x: p.x - p.width / 2.,
                y: p.y - p.depth / 2.,
                w: p.width,
                h: p.depth,
            };
            assert!(
                r.x >= c.margin - 0.001
                    && r.y >= c.margin - 0.001
                    && r.right() <= 256. - c.margin + 0.001
                    && r.top() <= 256. - c.margin + 0.001
            );
            assert!(!r.intersects(Rect {
                x: 0.,
                y: 0.,
                w: 18.,
                h: 28.
            }));
            for q in &result[i + 1..] {
                if p.plate == q.plate {
                    assert!(
                        !Rect {
                            x: r.x - c.gap / 2.,
                            y: r.y - c.gap / 2.,
                            w: r.w + c.gap,
                            h: r.h + c.gap
                        }
                        .intersects(Rect {
                            x: q.x - q.width / 2. - c.gap / 2.,
                            y: q.y - q.depth / 2. - c.gap / 2.,
                            w: q.width + c.gap,
                            h: q.depth + c.gap
                        })
                    );
                }
            }
        }
        assert!(arrange(&[("oversize".into(), Vec3::new(300., 10., 5.), 1)], &c).is_err());
        assert!(arrange(&[("tall".into(), Vec3::new(10., 10., 251.), 1)], &c).is_err());
        assert!(
            arrange(
                &[("box".into(), Vec3::new(200., 200., 40.), 2)],
                &Config { split: false, ..c }
            )
            .is_err()
        );
        assert_eq!(c.printer.plate_origin(3, 7), [0., -307.2]);
        let wide = vec![("M4".into(), Vec3::new(254.2, 57., 188.), 1)];
        assert!(arrange(&wide, &Config::default()).is_ok());
        assert!(
            arrange(
                &wide,
                &Config {
                    margin: 3.,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod overall_color_tests {
    use super::*;

    #[test]
    fn project_text_color_preserves_old_defaults_and_label_overrides() {
        #[derive(Deserialize)]
        struct Saved {
            #[serde(deserialize_with = "ColorTheme::load")]
            colors: ColorTheme,
        }
        let mut config = Config::default();
        ColorTheme::default().apply(&mut config);
        assert_eq!(config.slot(crate::label::PATH), 3);
        assert_eq!(config.text_slot(crate::label::PATH), DEFAULT_TEXT_SLOT);

        let mut colors = toml::from_str::<Saved>(
            "[colors]\ngrouping_version = 1\n[colors.roles]\nbody = 1\nfunctional = 2\naccessory = 6\ndivider = 4\ngridfinity = 5\n[colors.component_text]\nexisting = 3\n",
        )
        .unwrap()
        .colors;
        colors.palette[5] = [20, 40, 60];
        assert!(!colors.text_color_enabled());
        colors.apply(&mut config);
        assert_eq!(
            config.text_slot(crate::label::PATH),
            config.slot(crate::label::PATH)
        );

        let original_body = colors.group_appearance(PaletteGroup::Role("accessory"));
        colors.set_text_color_enabled(true).unwrap();
        assert_eq!(colors.roles[TEXT_ROLE], 7);
        let new = GroupAppearance {
            rgb: [227, 190, 60],
            material: FilamentMaterial::PetgHf,
        };
        colors
            .set_group_appearance(PaletteGroup::Text, new)
            .unwrap();
        colors
            .component_text
            .insert(crate::catalog::colors::component(crate::label::PATH), 2);
        let mut colors: ColorTheme = toml::from_str(&toml::to_string(&colors).unwrap()).unwrap();
        assert_eq!(
            colors.group_appearance(PaletteGroup::Role("accessory")),
            original_body
        );
        colors.apply(&mut config);
        for path in [
            crate::label::SUSTAINER_PATH,
            "generated/gridfinity/Gridfinity-9.stl",
            "generated/gridfinity/Cullenect-9.stl",
        ] {
            assert_eq!(config.appearance(config.text_slot(path)), new);
        }
        assert_eq!(config.text_slot(crate::label::PATH), 2);
        config.set_text_slot(crate::label::PATH, 4);
        assert_eq!(config.text_slot(crate::label::PATH), 4);
        colors.reset_group_appearance(PaletteGroup::Text).unwrap();
        assert_eq!(
            colors.group_appearance(PaletteGroup::Text),
            ColorTheme::default_text_appearance()
        );
        colors.set_text_color_enabled(false).unwrap();
        colors.trim_unused_slots();
        colors.validate().unwrap();
        colors.apply(&mut config);
        assert_eq!(
            config.text_slot(crate::label::SUSTAINER_PATH),
            config.slot(crate::label::SUSTAINER_PATH)
        );
        assert_eq!(config.text_slot(crate::label::PATH), 2);
    }

    #[test]
    fn card_edits_detach_shared_slots_and_restore_custom_groups_after_reopening() {
        let mut theme = ColorTheme::default();
        let body = PaletteGroup::Role("body");
        let guides = PaletteGroup::Custom(0);
        let path = "01. DRAWERS/07. Universal/Runners/x3 Runner Guide.stl";
        let label = crate::label::PATH;
        let original = theme.group_appearance(body);
        theme.groups.push(ColorGroup {
            name: "导轨".into(),
            components: [crate::catalog::colors::component(path)].into(),
            slot: theme.role_slot("body"),
            initial_appearance: None,
        });
        theme
            .component_text
            .insert(crate::catalog::colors::component(label), 6);
        let purple = GroupAppearance {
            rgb: [113, 76, 164],
            material: FilamentMaterial::PetgHf,
        };
        theme.set_group_appearance(guides, purple).unwrap();
        assert_eq!(theme.group_appearance(body), original);
        assert_eq!(theme.group_slot(path), 7);
        assert_eq!(theme.slot_count(), 7);
        let mut c = Config::default();
        theme.apply(&mut c);
        assert_eq!(c.appearance(c.slot(path)), purple);
        assert_eq!(c.text_slot(label), 6);
        assert_eq!(c.appearance(6).rgb, DEFAULT_PALETTE[5]);
        arrange_materials(&[(path.into(), Vec3::splat(10.), 1)], &[vec![7]], &c).unwrap();
        let mut saved: ColorTheme = toml::from_str(&toml::to_string(&theme).unwrap()).unwrap();
        saved.reset_group_appearance(guides).unwrap();
        assert_eq!(saved.group_appearance(guides), original);
        assert_eq!(saved.group_appearance(body), original);
        saved.trim_unused_slots();
        assert_eq!(saved.slot_count(), 6);
        saved.validate().unwrap();
    }

    #[test]
    fn full_project_palette_can_share_existing_colors_or_edit_a_dedicated_slot() {
        let mut theme = ColorTheme::default();
        for slot in 1..=MAX_PROJECT_SLOTS {
            if slot > 6 {
                theme.extra_filaments.push(GroupAppearance {
                    rgb: [slot as u8, 24, 198],
                    material: FilamentMaterial::PlaBasic,
                });
            }
            theme.groups.push(ColorGroup {
                name: format!("Group {slot}"),
                components: [format!("component-{slot}")].into(),
                slot,
                initial_appearance: None,
            });
        }
        let gray = theme.appearance(3);
        let new = GroupAppearance {
            rgb: [103, 21, 126],
            material: FilamentMaterial::PlaBasic,
        };
        let body = PaletteGroup::Role("body");
        let original = theme.clone();
        assert!(theme.set_group_appearance(body, new).is_err());
        assert_eq!(theme, original);
        theme.set_group_appearance(body, gray).unwrap();
        theme
            .set_group_appearance(PaletteGroup::Custom(31), new)
            .unwrap();
        assert_eq!(theme.slot_count(), MAX_PROJECT_SLOTS);
        assert_eq!(theme.group_appearance(body), gray);
        let mut config = Config::default();
        theme.apply(&mut config);
        config.validate().unwrap();
        config.extra_filaments.push(new);
        assert!(config.project_json().is_err());
    }

    #[test]
    fn component_colors_persist_cross_sizes_and_allow_local_overrides() {
        let small = "01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F.stl";
        let large = "01. DRAWERS/03. L Families/L7/Modules/L7 Module 4F.stl";
        let mut theme = ColorTheme::default();
        theme.roles.insert("body".into(), 2);
        theme
            .role_overrides
            .insert(crate::catalog::colors::component(small), "accessory".into());
        theme.role_overrides.insert(
            crate::catalog::colors::component(crate::label::PATH),
            "divider".into(),
        );
        theme
            .component_text
            .insert(crate::catalog::colors::component(crate::label::PATH), 6);
        let theme: ColorTheme = toml::from_str(&toml::to_string(&theme).unwrap()).unwrap();
        theme.validate().unwrap();
        let mut config = Config {
            layer: 0.12,
            gap: 8.,
            ..Default::default()
        };
        theme.apply(&mut config);
        assert_eq!(config.slot(large), 3);
        assert_eq!(config.slot(crate::label::PATH), 4);
        assert_eq!(config.text_slot(crate::label::PATH), 6);
        assert_eq!(
            config.slot("02. SUSTAINERS/02. M Families/M4/Roof/M4 Roof.stl"),
            2
        );
        assert_eq!((config.layer, config.gap), (0.12, 8.));
        config.set_part_slot(large, 5);
        config.set_text_slot(crate::label::PATH, 1);
        let mut saved: Config = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(saved.slot(large), 5);
        assert_eq!(saved.slot(small), 3);
        assert_eq!(saved.text_slot(crate::label::PATH), 1);
        saved.set_part_slot(large, 0);
        saved.set_text_slot(crate::label::PATH, 0);
        assert_eq!(saved.slot(large), 3);
        assert_eq!(saved.text_slot(crate::label::PATH), 6);
        #[derive(Deserialize)]
        struct Saved {
            #[serde(deserialize_with = "ColorTheme::load")]
            colors: ColorTheme,
        }
        let old = toml::from_str::<Saved>("[colors]\nfilament = 2\n[colors.roles]\nshell = 4")
            .unwrap()
            .colors;
        old.apply(&mut saved);
        assert_eq!(saved.slot(large), 1);
        assert!(saved.component_filaments.is_empty());
        assert!(saved.component_text_filaments.is_empty());
        let mut invalid = theme.clone();
        invalid
            .role_overrides
            .insert("part".into(), "unknown".into());
        assert!(invalid.validate().is_err());
        invalid = theme;
        invalid.component_text.insert("bad".into(), 0);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn role_colors_cross_sizes_preserve_print_settings_and_allow_overrides() {
        let theme = ColorTheme {
            palette: [[10, 20, 30]; 6],
            filament: 2,
            roles: [
                ("body".into(), 3),
                ("accessory".into(), 4),
                ("text".into(), 5),
                ("gridfinity".into(), 6),
            ]
            .into(),
            ..Default::default()
        };
        theme.validate().unwrap();
        let mut c = Config {
            margin: 4.,
            gap: 8.,
            layer: 0.16,
            walls: 4,
            infill: 25,
            follow_global_colors: true,
            ..Default::default()
        };
        c.set_part_slot("old.stl", 1);
        theme.apply(&mut c);
        assert_eq!(
            (c.margin, c.gap, c.layer, c.walls, c.infill),
            (4., 8., 0.16, 4, 25)
        );
        assert!(c.follow_global_colors);
        for path in [
            "01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F.stl",
            "02. SUSTAINERS/02. M Families/M4/Modules/M4 1F Module.stl",
            "02. SUSTAINERS/02. M Families/M4/Roof/M4 Roof.stl",
        ] {
            assert_eq!(c.slot(path), 3, "{path}");
        }
        assert_eq!(c.slot(crate::label::PATH), 4);
        assert_eq!(c.slot(crate::label::SUSTAINER_PATH), 4);
        assert_eq!(c.text_slot(crate::label::PATH), 5);
        assert_eq!(c.slot("generated/gridfinity/Gridfinity-9.stl"), 6);
        assert_eq!(c.slot("generated/gridfinity/Cullenect-9.stl"), 4);
        c.set_part_slot(crate::label::PATH, 1);
        c.set_text_slot(crate::label::PATH, 2);
        assert_eq!(c.slot(crate::label::PATH), 1);
        assert_eq!(c.text_slot(crate::label::PATH), 2);
        let loaded: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert!(loaded.follow_global_colors);
        assert_eq!(loaded.slot(crate::label::PATH), 1);
        let legacy: Config = toml::from_str("filament = 3").unwrap();
        assert!(!legacy.follow_global_colors);
        assert_eq!(legacy.slot(crate::label::PATH), 3);
    }
    #[test]
    fn invalid_global_color_rules_are_rejected() {
        let mut c = ColorTheme::default();
        c.roles.insert("body".into(), 7);
        assert!(c.validate().is_err());
        c.roles.clear();
        c.roles.insert("unknown".into(), 1);
        assert!(c.validate().is_err());
    }
}
