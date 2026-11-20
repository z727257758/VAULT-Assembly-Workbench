use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FilamentMaterial {
    PlaBasic,
    PlaMatte,
    PetgBasic,
    PetgMatte,
    PetgHf,
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
        }
    }

    pub fn family(self) -> &'static str {
        match self {
            Self::PlaBasic | Self::PlaMatte => "PLA",
            Self::PetgBasic | Self::PetgMatte | Self::PetgHf => "PETG",
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
const PETG_BASIC: [[u8; 3]; 12] = rgb([
    0xFF621A, 0xE80016, 0xFBCB00, 0x13AB48, 0x087CC0, 0x082996, 0x000000, 0xFFFFFF, 0xDDD3CA,
    0x603721, 0xA7AAAA, 0x525252,
]);
const PETG_MATTE: [[u8; 3]; 11] = rgb([
    0x090909, 0xFFFFFF, 0xC8C9CA, 0xEDA6B7, 0xA3A8DC, 0xB9DDE8, 0xAFDCA9, 0xB6D600, 0x8E9089,
    0x525252, 0xAA795F,
]);
const PETG_HF: [[u8; 3]; 14] = rgb([
    0x002D89, 0xC3000B, 0x000000, 0xA7AAAA, 0xFFFFFF, 0x525252, 0xFADEBB, 0xF85200, 0x00AE42,
    0xFFCF00, 0xB5CFA3, 0x8E9089, 0x6C4936, 0xB9D6EC,
]);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaterialLibrary(BTreeMap<FilamentMaterial, Vec<[u8; 3]>>);

impl MaterialLibrary {
    pub fn swatches(&self, material: FilamentMaterial) -> &[[u8; 3]] {
        self.0
            .get(&material)
            .map_or_else(|| material.default_swatches(), Vec::as_slice)
    }

    pub fn swatches_mut(&mut self, material: FilamentMaterial) -> &mut Vec<[u8; 3]> {
        self.0
            .entry(material)
            .or_insert_with(|| material.default_swatches().to_vec())
    }

    pub fn restore(&mut self, material: FilamentMaterial) {
        self.0.remove(&material);
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
