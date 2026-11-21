use super::{ColorTheme, DEFAULT_MATERIALS, DEFAULT_PALETTE, GroupAppearance, PaletteGroup};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::LazyLock,
};

mod bambu;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FilamentMaterial {
    PlaBasic,
    PlaMatte,
    PetgBasic,
    PetgMatte,
    PetgHf,
    Imported(u64),
}

impl FilamentMaterial {
    pub const ALL: [Self; 5] = [
        Self::PlaBasic,
        Self::PlaMatte,
        Self::PetgBasic,
        Self::PetgMatte,
        Self::PetgHf,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::PlaBasic => "PLA Basic",
            Self::PlaMatte => "PLA Matte",
            Self::PetgBasic => "PETG Basic",
            Self::PetgMatte => "PETG Matte",
            Self::PetgHf => "PETG HF",
            Self::Imported(_) => "Imported filament",
        }
    }

    pub fn family(self) -> &'static str {
        match self {
            Self::PlaBasic | Self::PlaMatte => "PLA",
            Self::PetgBasic | Self::PetgMatte | Self::PetgHf => "PETG",
            Self::Imported(_) => "Imported",
        }
    }

    pub fn profile_name(self, printer: super::Printer) -> &'static str {
        use super::Printer;
        match (self, printer) {
            (Self::PlaBasic, Printer::P1s) => "Bambu PLA Basic @BBL P1S 0.4 nozzle",
            (Self::PlaMatte, Printer::P1s) => "Bambu PLA Matte @BBL P1S 0.4 nozzle",
            (Self::PetgHf, Printer::P1s) => "Bambu PETG HF @BBL P1S 0.4 nozzle",
            (Self::PlaBasic, Printer::X1c) => "Bambu PLA Basic @BBL X1C",
            (Self::PlaMatte, Printer::X1c) => "Bambu PLA Matte @BBL X1C",
            (Self::PetgHf, Printer::X1c) => "Bambu PETG HF @BBL X1C",
            (Self::PetgBasic | Self::PetgMatte, Printer::P1s | Printer::X1c) => {
                "Bambu PETG Basic @BBL X1C"
            }
            (Self::PlaBasic, Printer::A1) => "Bambu PLA Basic @BBL A1",
            (Self::PlaMatte, Printer::A1) => "Bambu PLA Matte @BBL A1",
            (Self::PetgHf, Printer::A1) => "Bambu PETG HF @BBL A1",
            (Self::PetgBasic | Self::PetgMatte, Printer::A1) => "Bambu PETG Basic @BBL A1",
            (Self::PlaBasic, Printer::A1Mini) => "Bambu PLA Basic @BBL A1M",
            (Self::PlaMatte, Printer::A1Mini) => "Bambu PLA Matte @BBL A1M",
            (Self::PetgHf, Printer::A1Mini) => "Bambu PETG HF @BBL A1M",
            (Self::PetgBasic | Self::PetgMatte, Printer::A1Mini) => {
                "Bambu PETG Basic @BBL A1M 0.4 nozzle"
            }
            (Self::Imported(_), _) => "Imported filament",
        }
    }

    fn swatch_key(self) -> Cow<'static, str> {
        match self {
            Self::PlaBasic => "PlaBasic".into(),
            Self::PlaMatte => "PlaMatte".into(),
            Self::PetgBasic => "PetgBasic".into(),
            Self::PetgMatte => "PetgMatte".into(),
            Self::PetgHf => "PetgHf".into(),
            Self::Imported(id) => format!("imported-{id:016x}").into(),
        }
    }

    fn default_swatches(self) -> &'static [[u8; 3]] {
        // Workspace display swatches. These RGB values can be edited per material.
        match self {
            Self::PlaBasic => &PLA_BASIC,
            Self::PlaMatte => &PLA_MATTE,
            Self::PetgBasic => &PETG_BASIC,
            Self::PetgMatte => &PETG_MATTE,
            Self::PetgHf => &PETG_HF,
            Self::Imported(_) => &[],
        }
    }
}

const fn rgb<const N: usize>(hex: [u32; N]) -> [[u8; 3]; N] {
    let mut result = [[0; 3]; N];
    let mut index = 0;
    while index < N {
        let [_, r, g, b] = hex[index].to_be_bytes();
        result[index] = [r, g, b];
        index += 1;
    }
    result
}
const PLA_BASIC: [[u8; 3]; 30] = rgb([
    0xFFFFFF, 0xF7E7DE, 0xD3D5D6, 0xA7AAAA, 0x8E9089, 0x5B6579, 0x525252, 0x000000, 0xED0087,
    0xF34974, 0xF45371, 0x9B2035, 0xC12C1C, 0xFF6911, 0xFF9715, 0xE3BB63, 0xFFC900, 0xF3ED28,
    0xB9CE00, 0x3F8D42, 0x00AE42, 0x00AEB7, 0x0086D6, 0x0059B6, 0x0A2581, 0x5D41B7, 0x47295B,
    0x827E48, 0x9E412B, 0x725033,
]);
const PLA_MATTE: [[u8; 3]; 17] = rgb([
    0xFFFFFF, 0xCDC8B9, 0xF6DA72, 0xFAA46B, 0xE8A7BB, 0xB493D3, 0x8F347E, 0xC53258, 0xBB4546,
    0xBFDF88, 0x8AB6B2, 0xA5D2E5, 0x587799, 0x685851, 0x989999, 0x525252, 0x191919,
]);
// Bambu Studio 02.05.00.66, BBL/filament/filaments_color_codes.json, GFG00.
const BAMBU_PETG_BASIC: [[u8; 3]; 7] = rgb([
    0x000000, 0xFFFFFF, 0x7F7E83, 0xFCE300, 0xD6001C, 0x001489, 0x4F2C1D,
]);
// Classic solid colors, including the Ice Pink entry listed in the full color table.
// Source: http://www.aliz3d.com/product/2 (Chinese and English color-code tables).
const ALIZ_PETG: [(u32, &str, &str, &str); 30] = [
    (0x7354A0, "紫罗兰", "Violet", "AEB7500"),
    (0xFF5B00, "橙色", "Orange", "AEB2500"),
    (0xFED100, "黄色", "Yellow", "AEB3500"),
    (0x00A3E0, "青色", "Cyan", "AEB5500"),
    (0xD72F8B, "品红色", "Magenta", "AEB1500"),
    (0x111111, "黑色", "Black", "AEB8900"),
    (0xFFFFFF, "白色", "White", "AEB8000"),
    (0x54301A, "咖啡色", "Coffee Color", "AEB1800"),
    (0xE2231A, "大红", "Bright Red", "AEB1501"),
    (0xC41230, "朱砂红", "Cinnabar Red", "AEB1401"),
    (0x001489, "克莱因蓝", "Klein Blue", "AEB6700"),
    (0x0055B8, "蓝色", "Blue", "AEB6600"),
    (0x8B2332, "酒红色", "Wine Red", "AEB1700"),
    (0x5B6770, "蓝灰色", "Gray Blue", "AEB6601"),
    (0x6C4F3D, "棕色", "Brown", "AEB1600"),
    (0x0072CE, "湖蓝色", "Lake Blue", "AEB6500"),
    (0xF1B2DC, "樱花粉", "Cherry Blossom Pink", "AEB1200"),
    (0xFEECD2, "肤色", "Skin Color", "AEB3100"),
    (0x60C659, "柠檬绿", "Lemon Green", "AEB4400"),
    (0x2AD2C9, "薄荷绿", "Mint Green", "AEB4500"),
    (0xA5ACAF, "浅灰色", "Light Gray", "AEB8300"),
    (0x00843D, "绿色", "Green", "AEB4502"),
    (0x556B2F, "橄榄绿", "Olive Green", "AEB4501"),
    (0x3A4B30, "墨绿色", "Dark Green", "AEB4600"),
    (0x53565A, "铁灰色", "Iron Gray", "AEB8700"),
    (0xFFFCF7, "奶白色", "Milky White", "AEB8001"),
    (0x213123, "军绿色", "Army Green", "AEB4700"),
    (0xFFEDAE, "米黄色", "Beige", "AEB3200"),
    (0x97999B, "冷灰色", "Cold Gray", "AEB8400"),
    (0xF7C8D5, "冰粉色", "Ice Pink", "AEB1300"),
];

static PETG_BASIC: std::sync::LazyLock<Vec<[u8; 3]>> = std::sync::LazyLock::new(|| {
    let mut swatches = BAMBU_PETG_BASIC.to_vec();
    for (hex, _, _, _) in ALIZ_PETG {
        let [color] = rgb([hex]);
        if !swatches.contains(&color) {
            swatches.push(color);
        }
    }
    swatches
});

impl FilamentMaterial {
    pub(crate) fn aliz_swatch_label(self, [r, g, b]: [u8; 3], zh: bool) -> Option<String> {
        if self != Self::PetgBasic {
            return None;
        }
        let hex = u32::from_be_bytes([0, r, g, b]);
        let &(_, chinese, english, code) = ALIZ_PETG.iter().find(|entry| entry.0 == hex)?;
        let name = if zh { chinese } else { english };
        Some(format!("ALIZ {name} · {code} · #{hex:06X}"))
    }
}
const PETG_MATTE: [[u8; 3]; 11] = rgb([
    0x090909, 0xFFFFFF, 0xC8C9CA, 0xEDA6B7, 0xA3A8DC, 0xB9DDE8, 0xAFDCA9, 0xB6D600, 0x8E9089,
    0x525252, 0xAA795F,
]);
const PETG_HF: [[u8; 3]; 14] = rgb([
    0x002D89, 0xC3000B, 0x000000, 0xA7AAAA, 0xFFFFFF, 0x525252, 0xFADEBB, 0xF85200, 0x00AE42,
    0xFFCF00, 0xB5CFA3, 0x8E9089, 0x6C4936, 0xB9D6EC,
]);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BambuPreset {
    pub name: String,
    pub base: FilamentMaterial,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub printers: Vec<super::Printer>,
    pub options: BTreeMap<String, String>,
    pub color: Option<[u8; 3]>,
    pub filament_id: Option<String>,
}

impl BambuPreset {
    pub fn material(&self) -> FilamentMaterial {
        // A preset keeps its slot identity when reimported. Keep the value within TOML's integer range.
        let hash = self
            .name
            .bytes()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        FilamentMaterial::Imported(hash & i64::MAX as u64)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialLibrary {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    presets: Vec<BambuPreset>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    hidden_materials: BTreeSet<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    hidden_swatches: BTreeMap<String, BTreeSet<[u8; 3]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_appearances: Option<[GroupAppearance; 6]>,
    #[serde(flatten)]
    swatches: BTreeMap<String, Vec<[u8; 3]>>,
}

impl MaterialLibrary {
    pub fn choices(&self) -> impl Iterator<Item = FilamentMaterial> + '_ {
        FilamentMaterial::ALL
            .into_iter()
            .chain(self.presets.iter().map(BambuPreset::material))
    }

    pub fn visible_choices(&self) -> impl Iterator<Item = FilamentMaterial> + '_ {
        self.choices().filter(|material| !self.is_hidden(*material))
    }

    pub fn is_hidden(&self, material: FilamentMaterial) -> bool {
        self.hidden_materials
            .contains(material.swatch_key().as_ref())
    }

    pub fn set_hidden(&mut self, material: FilamentMaterial, hidden: bool) {
        let key = material.swatch_key();
        if hidden {
            self.hidden_materials.insert(key.into_owned());
        } else {
            self.hidden_materials.remove(key.as_ref());
        }
    }

    pub fn is_swatch_hidden(&self, material: FilamentMaterial, rgb: [u8; 3]) -> bool {
        self.hidden_swatches
            .get(material.swatch_key().as_ref())
            .is_some_and(|colors| colors.contains(&rgb))
    }

    pub fn set_swatch_hidden(&mut self, material: FilamentMaterial, rgb: [u8; 3], hidden: bool) {
        let key = material.swatch_key();
        if hidden && self.swatches(material).contains(&rgb) {
            self.hidden_swatches
                .entry(key.into_owned())
                .or_default()
                .insert(rgb);
        } else if let Some(colors) = self.hidden_swatches.get_mut(key.as_ref()) {
            colors.remove(&rgb);
            if colors.is_empty() {
                self.hidden_swatches.remove(key.as_ref());
            }
        }
    }

    pub fn visible_swatches(
        &self,
        material: FilamentMaterial,
    ) -> impl Iterator<Item = [u8; 3]> + '_ {
        let key = material.swatch_key();
        let hidden = self.hidden_materials.contains(key.as_ref());
        let hidden_swatches = self.hidden_swatches.get(key.as_ref());
        self.swatches(material).iter().copied().filter(move |rgb| {
            !hidden && hidden_swatches.is_none_or(|colors| !colors.contains(rgb))
        })
    }

    pub fn default_appearance(&self, group: PaletteGroup) -> Option<GroupAppearance> {
        let index = group.default_slot()? - 1;
        Some(
            self.default_appearances
                .map_or_else(|| factory_appearance(index), |defaults| defaults[index]),
        )
    }

    pub fn set_default_appearance(&mut self, group: PaletteGroup, appearance: GroupAppearance) {
        if let Some(slot) = group.default_slot() {
            let factory = std::array::from_fn(factory_appearance);
            let mut defaults = self.default_appearances.unwrap_or(factory);
            defaults[slot - 1] = appearance;
            self.default_appearances = (defaults != factory).then_some(defaults);
        }
    }

    pub fn reset_default_appearances(&mut self) {
        self.default_appearances = None;
    }

    pub fn default_theme(&self) -> ColorTheme {
        let mut theme = ColorTheme::default();
        if let Some(defaults) = self.default_appearances {
            theme.palette = defaults.map(|default| default.rgb);
            theme.materials = defaults.map(|default| default.material);
        }
        theme
    }

    pub fn preset(&self, material: FilamentMaterial) -> Option<&BambuPreset> {
        if matches!(material, FilamentMaterial::Imported(_)) {
            self.presets
                .iter()
                .find(|preset| preset.material() == material)
        } else {
            None
        }
    }

    pub(crate) fn density(&self, material: FilamentMaterial) -> Option<f64> {
        let preset = self.preset(material);
        if let Some(density) = preset.and_then(|p| p.options.get("filament_density")) {
            return density
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0.);
        }
        // Densities in the bundled printer templates and filament overrides.
        match preset.map_or(material, |p| p.base) {
            FilamentMaterial::PlaBasic => Some(1.26),
            FilamentMaterial::PlaMatte => Some(1.32),
            FilamentMaterial::PetgBasic | FilamentMaterial::PetgMatte => Some(1.25),
            FilamentMaterial::PetgHf => Some(1.28),
            FilamentMaterial::Imported(_) => None,
        }
    }

    pub fn name(&self, material: FilamentMaterial) -> &str {
        self.preset(material)
            .map_or(material.name(), |preset| &preset.name)
    }

    pub fn family(&self, material: FilamentMaterial) -> &str {
        self.preset(material).map_or(material.family(), |preset| {
            preset
                .options
                .get("filament_type")
                .map_or(preset.base.family(), String::as_str)
        })
    }

    pub fn profile_name(&self, material: FilamentMaterial, printer: super::Printer) -> &str {
        self.preset(material)
            .map_or(material.profile_name(printer), |preset| &preset.name)
    }

    pub(super) fn for_export(
        &self,
        material: FilamentMaterial,
        printer: super::Printer,
        slot: u32,
    ) -> Result<Option<&BambuPreset>, String> {
        if !matches!(material, FilamentMaterial::Imported(_)) {
            return Ok(None);
        }
        let preset = self.preset(material).ok_or_else(|| {
            format!("耗材槽 {slot} 缺少导入预设，请在设置中重新导入 / Slot {slot} needs its imported filament preset")
        })?;
        if !preset.printers.is_empty() && !preset.printers.contains(&printer) {
            return Err(format!(
                "耗材槽 {slot} 的预设与 {} 不兼容 / Slot {slot} preset is incompatible with {}: {}",
                printer.name(),
                printer.name(),
                preset.name
            ));
        }
        Ok(Some(preset))
    }

    pub fn read_bambu_presets(&self, paths: &[PathBuf]) -> Result<Vec<BambuPreset>, String> {
        bambu::read_presets(paths, &self.presets)
    }

    pub fn import_presets(&mut self, presets: Vec<BambuPreset>) -> Result<usize, String> {
        let mut next = self.presets.clone();
        let count = presets.len();
        for preset in presets {
            if let Some(previous) = next
                .iter_mut()
                .find(|old| old.material() == preset.material())
            {
                if previous.name != preset.name {
                    return Err(format!(
                        "耗材预设标识冲突 / Filament preset ID collision: {}",
                        preset.name
                    ));
                }
                *previous = preset;
            } else {
                next.push(preset);
            }
        }
        next.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.name.cmp(&b.name))
        });
        self.presets = next;
        Ok(count)
    }

    fn default_swatches(&self, material: FilamentMaterial) -> &[[u8; 3]] {
        self.preset(material).map_or_else(
            || material.default_swatches(),
            |preset| {
                preset
                    .color
                    .as_ref()
                    .map_or_else(|| preset.base.default_swatches(), std::slice::from_ref)
            },
        )
    }

    pub fn swatches(&self, material: FilamentMaterial) -> &[[u8; 3]] {
        self.swatches
            .get(material.swatch_key().as_ref())
            .map_or_else(|| self.default_swatches(material), Vec::as_slice)
    }

    pub fn swatches_mut(&mut self, material: FilamentMaterial) -> &mut Vec<[u8; 3]> {
        let key = material.swatch_key();
        if !self.swatches.contains_key(key.as_ref()) {
            let defaults = self.default_swatches(material).to_vec();
            self.swatches.insert(key.clone().into_owned(), defaults);
        }
        self.swatches.get_mut(key.as_ref()).unwrap()
    }

    pub fn restore(&mut self, material: FilamentMaterial) {
        let key = material.swatch_key();
        self.swatches.remove(key.as_ref());
        self.hidden_swatches.remove(key.as_ref());
    }
}

fn factory_appearance(index: usize) -> GroupAppearance {
    GroupAppearance {
        rgb: DEFAULT_PALETTE[index],
        material: DEFAULT_MATERIALS[index],
    }
}

// Each listed option and each filament_* array in the project templates contains
// one value per filament. Printer, nozzle, process and plate arrays have their
// own dimensions and stay independent of the number of filament slots.
const FILAMENT_OPTIONS: &str = "
    activate_air_filtration additional_cooling_fan_speed chamber_temperatures circle_compensation_speed
    close_fan_the_first_x_layers complete_print_exhaust_fan_speed cool_plate_temp cool_plate_temp_initial_layer
    cooling_perimeter_transition_distance cooling_slowdown_logic counter_coef_1 counter_coef_2 counter_coef_3
    counter_limit_max counter_limit_min default_filament_colour diameter_limit during_print_exhaust_fan_speed
    enable_overhang_bridge_fan enable_pressure_advance eng_plate_temp eng_plate_temp_initial_layer
    fan_cooling_layer_time fan_max_speed fan_min_speed first_x_layer_fan_speed full_fan_speed_layer
    hole_coef_1 hole_coef_2 hole_coef_3 hole_limit_max hole_limit_min hot_plate_temp hot_plate_temp_initial_layer
    impact_strength_z long_retractions_when_ec no_slow_down_for_cooling_on_outwalls nozzle_temperature
    nozzle_temperature_initial_layer nozzle_temperature_range_high nozzle_temperature_range_low overhang_fan_speed
    overhang_fan_threshold overhang_threshold_participating_cooling override_process_overhang_speed pre_start_fan_time
    pressure_advance reduce_fan_stop_start_freq required_nozzle_HRC retraction_distances_when_ec slow_down_for_layer_cooling
    slow_down_layer_time slow_down_min_speed supertack_plate_temp supertack_plate_temp_initial_layer
    temperature_vitrification textured_plate_temp textured_plate_temp_initial_layer volumetric_speed_coefficients";

pub(super) fn is_importable_option(key: &str) -> bool {
    static COLUMNS: LazyLock<BTreeSet<String>> = LazyLock::new(|| {
        let templates: Vec<Value> = super::Printer::ALL
            .into_iter()
            .map(|printer| {
                serde_json::from_str(printer.template())
                    .expect("embedded printer template is valid")
            })
            .collect();
        templates[0]
            .as_object()
            .unwrap()
            .keys()
            .filter(|key| {
                (key.starts_with("filament_")
                    || FILAMENT_OPTIONS
                        .split_whitespace()
                        .any(|option| option == *key))
                    && !matches!(
                        key.as_str(),
                        "default_filament_colour"
                            | "filament_colour"
                            | "filament_colour_type"
                            | "filament_multi_colour"
                            | "filament_extruder_variant"
                            | "filament_ids"
                            | "filament_settings_id"
                            | "filament_map"
                            | "filament_nozzle_map"
                            | "filament_self_index"
                            | "filament_volume_map"
                    )
                    && templates.iter().all(|template| {
                        template[*key]
                            .as_array()
                            .is_some_and(|column| column.len() == 6)
                    })
            })
            .cloned()
            .collect()
    });
    COLUMNS.contains(key)
}

pub(super) fn extend_project_slots(project: &mut Value, count: usize) -> Result<(), String> {
    const ORIGINAL: usize = 6;
    let fields = project.as_object_mut().ok_or("Invalid printer template")?;
    for (key, value) in fields {
        let Some(entries) = value.as_array_mut() else {
            continue;
        };
        if key.starts_with("filament_") || FILAMENT_OPTIONS.split_whitespace().any(|k| k == key) {
            if entries.len() != ORIGINAL {
                return Err(format!("Unexpected filament column {key}"));
            }
            entries.resize(count, entries[0].clone());
        }
    }
    project["filament_self_index"] = json!((1..=count).map(|i| i.to_string()).collect::<Vec<_>>());
    // Bambu stores one unload/load pair per filament and a directed NxN matrix.
    let vector = project["flush_volumes_vector"]
        .as_array_mut()
        .ok_or("Invalid flush vector")?;
    let pair = vector.get(..2).ok_or("Missing flush defaults")?.to_vec();
    for _ in ORIGINAL..count {
        vector.extend_from_slice(&pair);
    }
    let matrix = project["flush_volumes_matrix"]
        .as_array()
        .filter(|m| m.len() == ORIGINAL * ORIGINAL)
        .ok_or("Invalid flush matrix")?;
    let new_transition = matrix
        .iter()
        .filter_map(|v| v.as_str()?.parse::<u32>().ok())
        .max()
        .ok_or("Missing flush volume")?
        .to_string();
    let mut expanded = Vec::with_capacity(count * count);
    for from in 0..count {
        for to in 0..count {
            let volume = if from < ORIGINAL && to < ORIGINAL {
                matrix[from * ORIGINAL + to].clone()
            } else if from == to {
                json!("0")
            } else {
                json!(new_transition)
            };
            expanded.push(volume);
        }
    }
    project["flush_volumes_matrix"] = Value::Array(expanded);
    // Bambu records modified settings for the process, each filament, and printer.
    let changed = project["different_settings_to_system"]
        .as_array_mut()
        .ok_or("Missing profile differences")?;
    let printer = changed.pop().ok_or("Missing printer profile differences")?;
    changed.resize(count + 1, json!(""));
    changed.push(printer);
    if let Some(parents) = project
        .get_mut("inherits_group")
        .and_then(Value::as_array_mut)
    {
        let printer = parents.pop().ok_or("Missing printer preset parent")?;
        parents.resize(count + 1, json!(""));
        parents.push(printer);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_visibility_and_new_project_defaults_roundtrip_without_recoloring_saved_slots() {
        let mut library: MaterialLibrary =
            toml::from_str("PlaBasic = [[1, 2, 3], [4, 5, 6]]").unwrap();
        let original = library.default_theme();
        let body = PaletteGroup::Role("body");
        let text = PaletteGroup::Text;
        let new_body = GroupAppearance {
            rgb: [4, 5, 6],
            material: FilamentMaterial::PlaBasic,
        };
        let new_text = GroupAppearance {
            rgb: [235, 207, 128],
            material: FilamentMaterial::PlaMatte,
        };
        library.set_default_appearance(body, new_body);
        library.set_default_appearance(text, new_text);
        library.set_swatch_hidden(FilamentMaterial::PlaBasic, [1, 2, 3], true);
        library.set_hidden(FilamentMaterial::PlaMatte, true);

        let mut restored: MaterialLibrary =
            toml::from_str(&toml::to_string(&library).unwrap()).unwrap();
        assert_eq!(restored, library);
        assert_eq!(
            restored.swatches(FilamentMaterial::PlaBasic),
            &[[1, 2, 3], [4, 5, 6]]
        );
        assert_eq!(
            restored
                .visible_swatches(FilamentMaterial::PlaBasic)
                .collect::<Vec<_>>(),
            [[4, 5, 6]]
        );
        assert_eq!(
            restored
                .visible_swatches(FilamentMaterial::PlaMatte)
                .count(),
            0
        );
        assert!(
            !restored
                .visible_choices()
                .any(|material| material == FilamentMaterial::PlaMatte)
        );
        assert_eq!(original.group_appearance(body), factory_appearance(0));
        let theme = restored.default_theme();
        assert_eq!(theme.role_slot("body"), 1);
        assert_eq!(theme.slot_count(), 6);
        assert_eq!(theme.group_appearance(body), new_body);
        assert_eq!(theme.group_appearance(text), new_text);
        assert_eq!(
            theme.group_appearance(PaletteGroup::Role("functional")),
            original.group_appearance(PaletteGroup::Role("functional"))
        );

        restored.set_hidden(FilamentMaterial::PlaBasic, true);
        assert_eq!(
            restored
                .visible_swatches(FilamentMaterial::PlaBasic)
                .count(),
            0
        );
        restored.set_hidden(FilamentMaterial::PlaBasic, false);
        assert!(restored.is_swatch_hidden(FilamentMaterial::PlaBasic, [1, 2, 3]));
        restored.set_swatch_hidden(FilamentMaterial::PlaBasic, [1, 2, 3], false);
        assert_eq!(
            restored
                .visible_swatches(FilamentMaterial::PlaBasic)
                .count(),
            2
        );
        restored.reset_default_appearances();
        assert_eq!(restored.default_theme(), original);
    }

    #[test]
    fn imported_slot_and_legacy_swatches_roundtrip() {
        let mut library: MaterialLibrary = toml::from_str("PlaBasic = [[9, 8, 7]]").unwrap();
        let preset = BambuPreset {
            name: "Workshop PLA @ P1S".into(),
            base: FilamentMaterial::PlaBasic,
            parent: None,
            printers: vec![crate::print_plate::Printer::P1s],
            options: [("filament_flow_ratio".into(), "0.96".into())].into(),
            color: Some([1, 2, 3]),
            filament_id: None,
        };
        let material = preset.material();
        library.import_presets(vec![preset.clone()]).unwrap();
        assert_eq!(library.swatches(material), &[[1, 2, 3]]);
        library.swatches_mut(material).push([4, 5, 6]);
        let saved: MaterialLibrary = toml::from_str(&toml::to_string(&library).unwrap()).unwrap();
        assert_eq!(saved.preset(material), Some(&preset));
        assert_eq!(saved.swatches(material), &[[1, 2, 3], [4, 5, 6]]);
        assert_eq!(saved.swatches(FilamentMaterial::PlaBasic), &[[9, 8, 7]]);

        let mut config = crate::print_plate::Config::default();
        config.materials[1] = material;
        let config: crate::print_plate::Config =
            toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(config.appearance(2).material, material);
    }

    #[test]
    fn petg_basic_swatches_include_the_deduplicated_aliz_catalog() {
        let material = FilamentMaterial::PetgBasic;
        let library = MaterialLibrary::default();
        let swatches = library.swatches(material);
        assert_eq!(swatches.len(), 35);
        assert!(swatches.starts_with(&BAMBU_PETG_BASIC));
        for (hex, _, _, _) in ALIZ_PETG {
            let [color] = rgb([hex]);
            assert_eq!(
                swatches.iter().filter(|&&swatch| swatch == color).count(),
                1
            );
        }
        assert_eq!(
            material
                .aliz_swatch_label([0xF7, 0xC8, 0xD5], true)
                .as_deref(),
            Some("ALIZ 冰粉色 · AEB1300 · #F7C8D5")
        );
    }

    #[test]
    fn material_swatches_roundtrip_independently_and_can_be_restored() {
        let mut library = MaterialLibrary::default();
        let defaults = library.clone();
        let color = [172, 29, 84];
        let colors = library.swatches_mut(FilamentMaterial::PlaMatte);
        colors.remove(0);
        colors[0] = color;
        colors.push([1, 2, 3]);
        library.swatches_mut(FilamentMaterial::PetgMatte).clear();
        let mut saved: MaterialLibrary =
            toml::from_str(&toml::to_string(&library).unwrap()).unwrap();
        assert_eq!(saved.swatches(FilamentMaterial::PlaMatte)[0], color);
        assert_eq!(
            saved.swatches(FilamentMaterial::PetgBasic),
            defaults.swatches(FilamentMaterial::PetgBasic)
        );
        assert!(saved.swatches(FilamentMaterial::PetgMatte).is_empty());
        saved.restore(FilamentMaterial::PetgMatte);
        saved.restore(FilamentMaterial::PlaMatte);
        assert_eq!(saved, defaults);
    }

    #[test]
    fn extra_slots_extend_material_columns_and_flush_matrix_independently_of_machine_settings() {
        use crate::print_plate::{Config, GroupAppearance};
        let mut config = Config::default();
        let original: Value = serde_json::from_slice(&config.project_json().unwrap()).unwrap();
        config.extra_filaments.extend([
            GroupAppearance {
                rgb: [43, 75, 81],
                material: FilamentMaterial::PlaMatte,
            },
            GroupAppearance {
                rgb: [124, 29, 36],
                material: FilamentMaterial::PetgMatte,
            },
        ]);
        let project: Value = serde_json::from_slice(&config.project_json().unwrap()).unwrap();
        for (key, value) in original.as_object().unwrap() {
            if let Some(column) = value.as_array().filter(|a| a.len() == 6) {
                let new = project[key].as_array().unwrap();
                assert_eq!(new.len(), 8, "{key}");
                assert_eq!(&new[..6], column, "{key}");
            }
        }
        assert_eq!(project["filament_self_index"][7], "8");
        assert_eq!(
            project["filament_settings_id"][6],
            FilamentMaterial::PlaMatte.profile_name(config.printer)
        );
        assert_eq!(project["filament_ids"][6], "GFA01");
        assert_eq!(
            project["filament_settings_id"][7],
            FilamentMaterial::PetgBasic.profile_name(config.printer)
        );
        assert_eq!(project["filament_colour"][6], "#2B4B51");
        assert_eq!(project["filament_colour"][7], "#7C1D24");
        assert_eq!(
            project["flush_volumes_vector"].as_array().unwrap().len(),
            16
        );
        let matrix = project["flush_volumes_matrix"].as_array().unwrap();
        assert_eq!(matrix.len(), 64);
        for from in 0..8 {
            for to in 0..8 {
                if from < 6 && to < 6 {
                    assert_eq!(
                        matrix[from * 8 + to],
                        original["flush_volumes_matrix"][from * 6 + to]
                    );
                } else {
                    assert_eq!(matrix[from * 8 + to] == "0", from == to);
                }
            }
        }
        assert_eq!(
            project["different_settings_to_system"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
        for field in [
            "extruder_offset",
            "nozzle_diameter",
            "nozzle_type",
            "printable_area",
            "machine_max_speed_x",
        ] {
            assert_eq!(project[field], original[field], "{field}");
        }
    }
}
