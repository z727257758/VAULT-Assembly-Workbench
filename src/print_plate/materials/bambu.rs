use super::{BambuPreset, FilamentMaterial, is_importable_option};
use crate::print_plate::Printer;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::File,
    io::{Read, Seek},
    path::{Component, Path, PathBuf},
};

const MAX_PRESETS: usize = 128;
const MAX_ARCHIVE_ENTRIES: usize = 512;
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_JSON_BYTES: usize = 16 * MAX_JSON_BYTES;

#[derive(Debug, PartialEq, Eq)]
struct RawPreset {
    name: String,
    inherits: Option<String>,
    material_type: Option<String>,
    printers: Option<Vec<Printer>>,
    options: BTreeMap<String, String>,
    color: Option<[u8; 3]>,
    filament_id: Option<String>,
    system: bool,
}

#[derive(Default)]
struct Batch {
    entries: BTreeMap<String, RawPreset>,
    json_bytes: usize,
}

pub(super) fn read_presets(
    paths: &[PathBuf],
    known: &[BambuPreset],
) -> Result<Vec<BambuPreset>, String> {
    if paths.len() > MAX_PRESETS {
        return Err(format!(
            "Select up to {MAX_PRESETS} Bambu preset files at a time"
        ));
    }
    let mut batch = Batch::default();
    for path in paths {
        let label = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        batch
            .read_file(path)
            .map_err(|error| format!("{label}: {error}"))?;
    }
    batch.resolve(known)
}

impl Batch {
    fn read_file(&mut self, path: &Path) -> Result<(), String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        match extension(path).as_str() {
            "json" => self.read_json(file),
            "zip" | "bbsflmt" | "bbscfg" => {
                if file.metadata().map_err(|e| e.to_string())?.len() > MAX_ARCHIVE_BYTES {
                    return Err("Preset archive exceeds 64 MiB".into());
                }
                self.read_archive(file)
            }
            _ => Err("Choose a Bambu Studio .json, .zip, .bbsflmt or .bbscfg file".into()),
        }
    }

    fn read_archive(&mut self, reader: impl Read + Seek) -> Result<(), String> {
        let mut archive = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            return Err(format!(
                "Preset archive exceeds {MAX_ARCHIVE_ENTRIES} entries"
            ));
        }
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(|e| e.to_string())?;
            if !entry.is_file() || extension(Path::new(entry.name())) != "json" {
                continue;
            }
            let name = entry.name().to_owned();
            self.read_json(entry)
                .map_err(|error| format!("{name}: {error}"))?;
        }
        Ok(())
    }

    fn read_json(&mut self, reader: impl Read) -> Result<(), String> {
        let limit = MAX_JSON_BYTES.min(MAX_TOTAL_JSON_BYTES - self.json_bytes);
        let mut bytes = Vec::new();
        reader
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > limit {
            return Err("Preset JSON exceeds the 1 MiB file or 16 MiB batch limit".into());
        }
        self.json_bytes += bytes.len();
        let value = parse_json(&bytes)?;
        let Some(raw) = RawPreset::parse(&value)? else {
            return Ok(());
        };
        if let Some(previous) = self.entries.get(&raw.name) {
            if previous != &raw {
                return Err(format!(
                    "Conflicting copies of filament preset \"{}\"",
                    raw.name
                ));
            }
        } else {
            if self.entries.len() >= MAX_PRESETS {
                return Err(format!(
                    "Import up to {MAX_PRESETS} filament presets at a time"
                ));
            }
            self.entries.insert(raw.name.clone(), raw);
        }
        Ok(())
    }

    fn resolve(self, known: &[BambuPreset]) -> Result<Vec<BambuPreset>, String> {
        if self.entries.is_empty() {
            return Err("No Bambu Studio filament presets found in the selected files".into());
        }
        let mut resolver = Resolver {
            entries: self.entries,
            known,
            resolved: BTreeMap::new(),
            active: BTreeSet::new(),
            factory: None,
        };
        let names: Vec<_> = resolver.entries.keys().cloned().collect();
        for name in names {
            resolver.get(&name)?;
        }
        Ok(resolver.resolved.into_values().collect())
    }
}

impl RawPreset {
    fn parse(value: &Value) -> Result<Option<Self>, String> {
        let object = value
            .as_object()
            .ok_or("Expected a Bambu Studio preset JSON object")?;
        let kind = first_text(value.get("type"));
        let is_filament = if let Some(kind) = kind {
            kind.eq_ignore_ascii_case("filament")
        } else {
            !object.contains_key("printer_settings_id")
                && !object.contains_key("print_settings_id")
                && (object.contains_key("filament_settings_id")
                    || object.contains_key("filament_type"))
        };
        if !is_filament {
            return Ok(None);
        }

        let name = first_text(value.get("name"))
            .or_else(|| first_text(value.get("filament_settings_id")))
            .filter(|name| name.chars().count() <= 160 && !name.chars().any(char::is_control))
            .ok_or("Filament preset requires a name of up to 160 characters")?;
        let variant_column = standard_variant_column(value.get("filament_extruder_variant"))
            .map_err(|error| format!("{name}: {error}"))?;
        let printers = value
            .get("compatible_printers")
            .map(compatible_printers)
            .transpose()
            .map_err(|error| format!("{name}: {error}"))?;
        if printers.as_ref().is_none_or(Vec::is_empty)
            && first_text(value.get("compatible_printers_condition")).is_some()
        {
            return Err(format!(
                "{name}: Choose a preset that lists its compatible 0.4 mm printers"
            ));
        }
        let options = object
            .iter()
            .filter(|(key, _)| is_importable_option(key))
            .filter_map(|(key, field)| {
                let field = match field {
                    Value::Array(values) if values.len() > 1 => values.get(variant_column)?,
                    Value::Array(values) => values.first()?,
                    scalar => scalar,
                };
                option_value(key, field).map(|value| (key.clone(), value))
            })
            .collect();
        Ok(Some(Self {
            name: name.to_owned(),
            inherits: first_text(value.get("inherits")).map(str::to_owned),
            material_type: first_text(value.get("filament_type")).map(str::to_owned),
            printers,
            options,
            color: ["default_filament_colour", "filament_colour"]
                .into_iter()
                .filter_map(|key| first_text(value.get(key)))
                .find_map(rgb),
            filament_id: first_text(value.get("filament_id"))
                .or_else(|| first_text(value.get("filament_ids")))
                .map(str::to_owned),
            system: first_text(value.get("from"))
                .is_some_and(|source| source.eq_ignore_ascii_case("system")),
        }))
    }
}

struct Resolver<'a> {
    entries: BTreeMap<String, RawPreset>,
    known: &'a [BambuPreset],
    resolved: BTreeMap<String, BambuPreset>,
    active: BTreeSet<String>,
    factory: Option<FactoryCatalog>,
}

impl Resolver<'_> {
    fn get(&mut self, name: &str) -> Result<BambuPreset, String> {
        if let Some(preset) = self.resolved.get(name) {
            return Ok(preset.clone());
        }
        if !self.active.insert(name.to_owned()) {
            return Err(format!(
                "Circular filament preset inheritance at \"{name}\""
            ));
        }
        let parent_name = self.entries[name].inherits.clone();
        let parent = match parent_name.as_deref() {
            Some(parent) if self.entries.contains_key(parent) => {
                let mut resolved = self.get(parent)?;
                if self.entries[parent].system && factory_material(parent).is_none() {
                    resolved.parent = Some(parent.to_owned());
                }
                Some(resolved)
            }
            Some(parent) => {
                if let Some(preset) = self.known.iter().find(|preset| preset.name == parent) {
                    Some(preset.clone())
                } else if factory_material(parent).is_some()
                    || bundled_template(&self.entries[name], parent)
                {
                    None
                } else {
                    self.factory
                        .get_or_insert_with(FactoryCatalog::installed)
                        .get(parent)?
                }
            }
            None => None,
        };
        let raw = &self.entries[name];
        let preset = resolve_preset(raw, parent).map_err(|error| format!("{name}: {error}"))?;
        self.active.remove(name);
        self.resolved.insert(name.to_owned(), preset.clone());
        Ok(preset)
    }
}

fn resolve_preset(raw: &RawPreset, parent: Option<BambuPreset>) -> Result<BambuPreset, String> {
    let factory_name = factory_material(&raw.name);
    let factory_parent = raw.inherits.as_deref().and_then(factory_material);
    let explicit_type = raw
        .material_type
        .as_deref()
        .map(|kind| {
            family(kind).ok_or_else(|| {
                format!("Unsupported filament type \"{kind}\". Import a PLA or PETG preset.")
            })
        })
        .transpose()?;
    if parent.is_none()
        && factory_parent.is_none()
        && let Some(name) = &raw.inherits
        && !bundled_template(raw, name)
    {
        return Err(format!(
            "Missing parent preset \"{name}\". Select its JSON too, install the Bambu Studio system preset, or import a complete PLA/PETG preset."
        ));
    }
    let base = parent
        .as_ref()
        .map(|preset| preset.base)
        .or_else(|| factory_name.map(|(material, _)| material))
        .or_else(|| factory_parent.map(|(material, _)| material))
        .or(explicit_type)
        .ok_or("The preset must specify PLA or PETG, or inherit a supported Bambu filament")?;
    if explicit_type.is_some_and(|material| material.family() != base.family()) {
        return Err("The filament type differs from the inherited material family".into());
    }
    let printers = if let Some(printers) = &raw.printers {
        printers.clone()
    } else if let Some(parent) = &parent {
        parent.printers.clone()
    } else if let Some((material, variant)) = factory_name.or(factory_parent) {
        factory_printers(material, variant)?
    } else {
        Vec::new()
    };
    let mut options = parent
        .as_ref()
        .map(|preset| preset.options.clone())
        .unwrap_or_default();
    options.extend(raw.options.clone());
    let external_parent = parent.as_ref().and_then(|preset| preset.parent.clone());
    if external_parent.is_some()
        || (parent.is_none() && factory_name.is_none() && factory_parent.is_none())
        || raw
            .material_type
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("PETG-CF"))
    {
        require_thermal_options(&options)?;
    }
    Ok(BambuPreset {
        name: raw.name.clone(),
        base,
        printers,
        options,
        color: raw
            .color
            .or_else(|| parent.as_ref().and_then(|preset| preset.color)),
        filament_id: raw
            .filament_id
            .clone()
            .or_else(|| parent.and_then(|preset| preset.filament_id)),
        parent: external_parent,
    })
}

fn require_thermal_options(options: &BTreeMap<String, String>) -> Result<(), String> {
    for key in [
        "filament_type",
        "nozzle_temperature",
        "nozzle_temperature_initial_layer",
        "textured_plate_temp",
        "hot_plate_temp",
        "filament_flow_ratio",
        "filament_max_volumetric_speed",
    ] {
        if !options.contains_key(key) {
            return Err(format!(
                "Incomplete filament preset: missing {key}. Import a complete Bambu Studio preset."
            ));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct FactoryManifest {
    filament_list: Vec<FactoryEntry>,
}

#[derive(Deserialize)]
struct FactoryEntry {
    name: String,
    sub_path: PathBuf,
}

struct FactoryCatalog {
    directories: Vec<BTreeMap<String, PathBuf>>,
}

impl FactoryCatalog {
    fn installed() -> Self {
        let roots = [
            env::var_os("APPDATA")
                .map(|path| PathBuf::from(path).join("BambuStudio").join("system")),
            env::var_os("ProgramFiles").map(|path| {
                PathBuf::from(path)
                    .join("Bambu Studio")
                    .join("resources")
                    .join("profiles")
            }),
        ];
        Self {
            directories: roots
                .into_iter()
                .flatten()
                .filter_map(|root| Self::index(&root).ok())
                .collect(),
        }
    }

    fn index(root: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
        let manifest = read_bounded_json(&root.join("BBL.json"), 4 * MAX_JSON_BYTES)?;
        let manifest: FactoryManifest =
            serde_json::from_value(manifest).map_err(|e| e.to_string())?;
        Ok(manifest
            .filament_list
            .into_iter()
            .filter(|entry| {
                entry.sub_path.starts_with("filament")
                    && entry
                        .sub_path
                        .components()
                        .all(|c| matches!(c, Component::Normal(_)))
                    && extension(&entry.sub_path) == "json"
            })
            .map(|entry| (entry.name, root.join("BBL").join(entry.sub_path)))
            .collect())
    }

    fn get(&self, name: &str) -> Result<Option<BambuPreset>, String> {
        let Some(paths) = self
            .directories
            .iter()
            .find(|paths| paths.contains_key(name))
        else {
            return Ok(None);
        };
        let options = Self::collect(paths, name, &mut Vec::new())
            .map_err(|error| format!("Bambu Studio factory preset \"{name}\": {error}"))?;
        let raw = RawPreset::parse(&Value::Object(options))?
            .ok_or("The installed parent is not a Bambu Studio filament preset")?;
        require_thermal_options(&raw.options)?;
        let base = raw
            .material_type
            .as_deref()
            .and_then(family)
            .ok_or("Choose an installed PLA, PETG, or PETG-CF factory parent")?;
        Ok(Some(BambuPreset {
            name: raw.name,
            base,
            printers: raw.printers.unwrap_or_default(),
            options: raw.options,
            color: raw.color,
            filament_id: raw.filament_id,
            parent: Some(name.to_owned()),
        }))
    }

    fn collect(
        paths: &BTreeMap<String, PathBuf>,
        name: &str,
        active: &mut Vec<String>,
    ) -> Result<Map<String, Value>, String> {
        if active.len() >= 16 || active.iter().any(|ancestor| ancestor == name) {
            return Err("Factory preset inheritance is circular or too deep".into());
        }
        let path = paths
            .get(name)
            .ok_or_else(|| format!("Missing factory ancestor \"{name}\""))?;
        let Value::Object(definition) = read_bounded_json(path, MAX_JSON_BYTES)? else {
            return Err("Expected a factory preset JSON object".into());
        };
        if first_text(definition.get("name")) != Some(name)
            || first_text(definition.get("type")) != Some("filament")
        {
            return Err("Factory manifest and filament preset do not match".into());
        }
        active.push(name.to_owned());
        let mut options = match first_text(definition.get("inherits")) {
            Some(parent) => Self::collect(paths, parent, active)?,
            None => Map::new(),
        };
        if let Some(includes) = definition.get("include") {
            let includes = includes
                .as_array()
                .ok_or("Invalid factory filament include list")?;
            for include in includes {
                let include = include.as_str().ok_or("Invalid factory filament include")?;
                let mut template = Self::collect(paths, include, active)?;
                // Includes add reusable extruder defaults without replacing preset identity.
                template.retain(|key, _| {
                    is_importable_option(key) || key == "filament_extruder_variant"
                });
                options.extend(template);
            }
        }
        options.extend(definition);
        active.pop();
        Ok(options)
    }
}

fn read_bounded_json(path: &Path, limit: usize) -> Result<Value, String> {
    let file = File::open(path).map_err(|error| error.kind().to_string())?;
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.kind().to_string())?;
    if bytes.len() > limit {
        return Err("Factory JSON exceeds its size limit".into());
    }
    parse_json(&bytes)
}

fn parse_json(bytes: &[u8]) -> Result<Value, String> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn family(kind: &str) -> Option<FilamentMaterial> {
    match kind.to_ascii_uppercase().as_str() {
        "PLA" | "FDM_FILAMENT_PLA" => Some(FilamentMaterial::PlaBasic),
        "PETG" | "PETG-CF" | "FDM_FILAMENT_PET" => Some(FilamentMaterial::PetgBasic),
        _ => None,
    }
}

fn factory_material(name: &str) -> Option<(FilamentMaterial, &str)> {
    let (label, variant) = name.split_once('@').unwrap_or((name, ""));
    let label = label.trim();
    let label = label.strip_prefix("Bambu ")?;
    let material = FilamentMaterial::ALL.into_iter().find(|material| {
        *material != FilamentMaterial::PetgMatte && label.eq_ignore_ascii_case(material.name())
    })?;
    Some((material, variant.trim()))
}

fn bundled_template(raw: &RawPreset, parent: &str) -> bool {
    factory_material(&raw.name).is_some_and(|(material, _)| {
        matches!(parent, "fdm_filament_pla" | "fdm_filament_pet")
            && family(parent).is_some_and(|base| base.family() == material.family())
    })
}

fn factory_printers(material: FilamentMaterial, variant: &str) -> Result<Vec<Printer>, String> {
    use FilamentMaterial::PetgBasic;
    use Printer::{A1, A1Mini, P1s, X1c};
    if variant.is_empty() || variant.eq_ignore_ascii_case("base") {
        return Ok(Vec::new());
    }
    let variant = variant.to_ascii_lowercase();
    let printers = match variant.as_str() {
        "bbl p1s" | "bbl p1s 0.4 nozzle" => vec![P1s],
        "bbl a1" | "bbl a1 0.4 nozzle" => vec![A1],
        "bbl a1m" | "bbl a1m 0.4 nozzle" => vec![A1Mini],
        "bbl x1c" | "bbl x1c 0.4 nozzle" if material == PetgBasic => vec![P1s, X1c],
        "bbl x1c" | "bbl x1c 0.4 nozzle" => vec![X1c],
        _ => return Err(no_supported_printer()),
    };
    Ok(printers)
}

fn compatible_printers(field: &Value) -> Result<Vec<Printer>, String> {
    let names = if let Some(names) = field.as_array() {
        names
            .iter()
            .map(|name| name.as_str().ok_or("Invalid compatible_printers entry"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![field.as_str().ok_or("Invalid compatible_printers value")?]
    };
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let printers: Vec<_> = Printer::ALL
        .into_iter()
        .filter(|p| {
            let qualified = format!("{} 0.4 nozzle", p.name());
            names
                .iter()
                .any(|name| name.trim().eq_ignore_ascii_case(&qualified))
        })
        .collect();
    if printers.is_empty() {
        return Err(no_supported_printer());
    }
    Ok(printers)
}

fn no_supported_printer() -> String {
    "Choose a preset for a 0.4 mm P1S, A1, A1 mini or X1 Carbon nozzle".into()
}

fn standard_variant_column(field: Option<&Value>) -> Result<usize, &'static str> {
    let Some(field) = field else {
        return Ok(0);
    };
    let variants = if let Some(variants) = field.as_array() {
        variants
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
    } else {
        field.as_str().into_iter().collect()
    };
    if variants.is_empty() {
        return Ok(0);
    }
    variants
        .iter()
        .position(|variant| variant.trim().eq_ignore_ascii_case("Direct Drive Standard"))
        .ok_or("Choose a profile with a Direct Drive Standard filament variant")
}

fn first_text(value: Option<&Value>) -> Option<&str> {
    let value = value?;
    let value = value.as_str().or_else(|| value.get(0)?.as_str())?.trim();
    (!value.is_empty() && !value.eq_ignore_ascii_case("nil")).then_some(value)
}

fn option_value(key: &str, value: &Value) -> Option<String> {
    match value {
        // In Studio's nullable fields, nil explicitly restores the machine setting.
        Value::String(value) if value.trim().eq_ignore_ascii_case("nil") => matches!(
            key,
            "filament_retraction_length"
                | "filament_retraction_speed"
                | "filament_retraction_minimum_travel"
                | "filament_deretraction_speed"
                | "filament_retract_restart_extra"
                | "filament_retract_when_changing_layer"
                | "filament_retract_before_wipe"
                | "filament_wipe"
                | "filament_wipe_distance"
                | "filament_z_hop"
                | "filament_z_hop_types"
                | "filament_long_retractions_when_cut"
                | "filament_retraction_distances_when_cut"
                | "filament_enable_overhang_speed"
                | "filament_bridge_speed"
                | "filament_overhang_1_4_speed"
                | "filament_overhang_2_4_speed"
                | "filament_overhang_3_4_speed"
                | "filament_overhang_4_4_speed"
                | "filament_overhang_totally_speed"
                | "override_process_overhang_speed"
        )
        .then(|| "nil".into()),
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(u8::from(*value).to_string()),
        _ => None,
    }
}

fn rgb(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let [_, r, g, b] = u32::from_str_radix(hex, 16).ok()?.to_be_bytes();
    Some([r, g, b])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Cursor, Write};

    fn add(batch: &mut Batch, value: Value) {
        batch.read_json(Cursor::new(value.to_string())).unwrap();
    }

    #[test]
    fn imports_studio_saved_user_diff_and_resolves_factory_compatibility() {
        let mut batch = Batch::default();
        add(
            &mut batch,
            json!({
                "name": "Custom PETG",
                "filament_settings_id": ["Custom PETG"],
                "from": "User",
                "inherits": "Bambu PETG Basic @BBL X1C",
                "filament_extruder_variant": ["Direct Drive Standard", "Direct Drive High Flow"],
                "filament_flow_ratio": ["0.96", "1.01"],
                "nozzle_temperature": [248, 275],
                "default_filament_colour": ["#AABBCC"],
                "filament_id": "GFG00",
                "machine_start_gcode": "ignored"
            }),
        );
        let preset = batch.resolve(&[]).unwrap().pop().unwrap();
        assert_eq!(preset.base, FilamentMaterial::PetgBasic);
        assert_eq!(preset.printers, [Printer::P1s, Printer::X1c]);
        assert_eq!(preset.color, Some([170, 187, 204]));
        assert_eq!(preset.filament_id.as_deref(), Some("GFG00"));
        assert_eq!(preset.options["filament_flow_ratio"], "0.96");
        assert_eq!(preset.options["nozzle_temperature"], "248");
        assert!(!preset.options.contains_key("machine_start_gcode"));
    }

    #[test]
    fn reads_a_nested_bundle_and_inherits_only_missing_values() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, value) in [
            (
                "bundle_structure.json",
                json!({"bundle_type": "filament config bundle"}),
            ),
            (
                "machine/ignored.json",
                json!({"type": "machine", "name": "ignored"}),
            ),
            (
                "Bambu Lab/Z Parent.JSON",
                json!({
                    "filament_settings_id": ["Z Parent"],
                    "inherits": "Bambu PLA Basic @BBL A1",
                    "nozzle_temperature": ["213"],
                    "filament_flow_ratio": ["0.98"],
                    "filament_retraction_length": ["1.2"],
                    "filament_ids": ["GFA00"]
                }),
            ),
            (
                "Bambu Lab/A Child.json",
                json!({
                    "filament_settings_id": ["A Child"],
                    "inherits": "Z Parent",
                    "compatible_printers": ["Bambu Lab A1 mini 0.4 nozzle", "Bambu Lab A1 0.6 nozzle"],
                    "filament_flow_ratio": ["nil", "1.04"],
                    "filament_retraction_length": ["nil"],
                    "nozzle_temperature": ["221"]
                }),
            ),
        ] {
            archive
                .start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(value.to_string().as_bytes()).unwrap();
        }
        let mut batch = Batch::default();
        batch.read_archive(archive.finish().unwrap()).unwrap();
        let presets = batch.resolve(&[]).unwrap();
        assert_eq!(presets.len(), 2);
        let child = &presets[0];
        assert_eq!(child.name, "A Child");
        assert_eq!(child.printers, [Printer::A1Mini]);
        assert_eq!(child.options["nozzle_temperature"], "221");
        assert_eq!(child.options["filament_flow_ratio"], "0.98");
        assert_eq!(child.options["filament_retraction_length"], "nil");
        assert_eq!(child.filament_id.as_deref(), Some("GFA00"));
    }

    #[test]
    fn rejects_missing_parents_cycles_and_incompatible_nozzles() {
        let mut batch = Batch::default();
        add(
            &mut batch,
            json!({"type":"filament", "name":"Child", "inherits":"Missing"}),
        );
        assert!(batch.resolve(&[]).unwrap_err().contains("Missing parent"));
        let mut batch = Batch::default();
        add(
            &mut batch,
            json!({"type":"filament", "name":"Cycle", "inherits":"Cycle"}),
        );
        assert!(batch.resolve(&[]).unwrap_err().contains("Circular"));
        assert!(
            RawPreset::parse(&json!({
                "type": "filament",
                "name": "0.6 only",
                "filament_type": ["PLA"],
                "compatible_printers": ["Bambu Lab A1 0.6 nozzle"]
            }))
            .unwrap_err()
            .contains("0.4 mm")
        );
        for unsupported in [
            json!(null),
            json!([""]),
            json!(["Bambu Lab H2D 0.4 nozzle"]),
        ] {
            assert!(
                RawPreset::parse(&json!({
                    "type": "filament",
                    "name": "Unsupported settings",
                    "filament_type": ["PLA"],
                    "compatible_printers": unsupported
                }))
                .is_err()
            );
        }
        assert!(
            RawPreset::parse(&json!({
                "type": "filament",
                "name": "Conditional settings",
                "filament_type": ["PLA"],
                "compatible_printers_condition": "printer_model==\"H2D\""
            }))
            .is_err()
        );
    }

    #[test]
    fn standalone_preset_requires_core_settings() {
        let mut batch = Batch::default();
        add(
            &mut batch,
            json!({"type": "filament", "name": "Example", "filament_type": ["PLA"]}),
        );
        assert!(
            batch
                .resolve(&[])
                .unwrap_err()
                .contains("missing nozzle_temperature")
        );
        assert!(factory_material("Bambu PETG Matte @BBL X1C").is_none());
    }

    #[test]
    #[ignore = "requires Bambu Studio factory profiles"]
    fn installed_bambu_factory_ancestors_resolve_generic_filaments() {
        let mut batch = Batch::default();
        for (name, parent) in [
            ("Example Generic PETG", "Generic PETG"),
            ("Example Generic PETG-CF", "Generic PETG-CF @BBL X1C"),
            ("Example Generic PLA", "Generic PLA"),
        ] {
            add(
                &mut batch,
                json!({
                    "name": name,
                    "filament_settings_id": [name],
                    "inherits": parent,
                    "fan_max_speed": ["37"]
                }),
            );
        }
        let presets = batch.resolve(&[]).unwrap();
        assert_eq!(presets.len(), 3);
        for (preset, (parent, kind)) in presets.iter().zip([
            ("Generic PETG", "PETG"),
            ("Generic PETG-CF @BBL X1C", "PETG-CF"),
            ("Generic PLA", "PLA"),
        ]) {
            assert_eq!(preset.parent.as_deref(), Some(parent));
            assert_eq!(preset.options["filament_type"], kind);
            assert_eq!(preset.options["fan_max_speed"], "37");
            assert!(preset.printers.contains(&Printer::P1s));
            assert!(preset.printers.contains(&Printer::X1c));
            require_thermal_options(&preset.options).unwrap();
        }
    }
}
