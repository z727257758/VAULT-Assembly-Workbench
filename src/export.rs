//! Export original manufacturing geometry, never the display-only assembly meshes.
use crate::{catalog::Bom, mesh::Mesh};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Stl,
    #[default]
    #[serde(alias = "Bambu")]
    ThreeMf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub format: Format,
    pub plate: crate::print_plate::Config,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::ThreeMf,
            plate: Default::default(),
        }
    }
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn xml(s: &str) -> String {
    s.chars()
        .filter(|c| matches!(*c as u32, 9 | 10 | 13) || *c >= ' ')
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace(char::from(39), "&apos;")
}
fn sources(root: &Path, items: &[Bom], destination: &Path) -> Result<Vec<(Bom, PathBuf)>, String> {
    let root = root.canonicalize().map_err(err)?;
    let parent = destination
        .parent()
        .ok_or("Invalid export destination")?
        .canonicalize()
        .map_err(err)?;
    if parent.starts_with(&root)
        || destination
            .canonicalize()
            .is_ok_and(|p| p.starts_with(&root))
    {
        return Err("不能导出到原始模型库目录".into());
    }
    let mut result = Vec::new();
    for b in items.iter().filter(|b| b.enabled) {
        if b.qty == 0 || b.qty > 100 {
            return Err(format!("无效份数：{}", b.name()));
        }
        let source = root
            .join(&b.path)
            .canonicalize()
            .map_err(|e| format!("{}: {e}", b.name()))?;
        if !source.starts_with(&root)
            || !source.is_file()
            || !b.path.to_lowercase().ends_with(".stl")
        {
            return Err(format!("无效模型路径：{}", b.path));
        }
        result.push((b.clone(), source));
    }
    Ok(result)
}
#[cfg(test)]
pub fn run(
    root: &Path,
    items: &[Bom],
    destination: &Path,
    format: Format,
) -> Result<PathBuf, String> {
    run_with_label(root, items, destination, format, &Default::default())
}
#[cfg(test)]
pub fn run_with_label(
    root: &Path,
    items: &[Bom],
    destination: &Path,
    format: Format,
    label: &crate::label::Config,
) -> Result<PathBuf, String> {
    run_design(root, items, destination, format, label, None)
}
#[cfg(test)]
pub fn run_design(
    root: &Path,
    items: &[Bom],
    destination: &Path,
    format: Format,
    label: &crate::label::Config,
    gridfinity: Option<(
        &Path,
        &crate::gridfinity::Layout,
        &crate::catalog::Selection,
    )>,
) -> Result<PathBuf, String> {
    run_configured(
        root,
        items,
        destination,
        &Options {
            format,
            ..Default::default()
        },
        label,
        gridfinity,
    )
}
pub fn run_configured(
    root: &Path,
    items: &[Bom],
    destination: &Path,
    options: &Options,
    label: &crate::label::Config,
    gridfinity: Option<(
        &Path,
        &crate::gridfinity::Layout,
        &crate::catalog::Selection,
    )>,
) -> Result<PathBuf, String> {
    let format = options.format;
    if format == Format::ThreeMf {
        options.plate.validate()?;
    }
    label.validate()?;
    let originals: Vec<_> = items
        .iter()
        .filter(|b| !b.path.starts_with(crate::gridfinity::PREFIX))
        .cloned()
        .collect();
    let mut inputs = sources(root, &originals, destination)?;
    let mut generated = HashMap::new();
    if let Some((home, layout, selection)) = gridfinity {
        layout.validate(selection)?;
        let mut bins = Vec::new();
        let mut generated_items = Vec::new();
        for item in items
            .iter()
            .filter(|b| b.enabled && b.path.starts_with(crate::gridfinity::PREFIX))
        {
            let bin = layout
                .bins
                .iter()
                .find(|b| b.path() == item.path || (b.detachable() && b.label_path() == item.path))
                .ok_or("生成盒子的配置缺失")?;
            if item.qty != 1 {
                return Err("盒子份数由布局管理".into());
            }
            if bin.detachable() && bin.label_path() == item.path {
                generated.insert(item.path.clone(), crate::cullenect::label(&bin.label)?);
                inputs.push((item.clone(), PathBuf::new()));
                continue;
            }
            bins.push(bin);
            generated_items.push(item);
        }
        let meshes = crate::gridfinity::generate_many(home, &bins, selection, None)?;
        for (item, mesh) in generated_items.into_iter().zip(meshes) {
            generated.insert(item.path.clone(), mesh);
            inputs.push((item.clone(), PathBuf::new()));
        }
    } else if items
        .iter()
        .any(|b| b.enabled && b.path.starts_with(crate::gridfinity::PREFIX))
    {
        return Err("缺少 Gridfinity 布局".into());
    }
    if inputs.is_empty() {
        return Err("打印清单中没有已选模型".into());
    }

    let parent = destination.parent().ok_or("Invalid export destination")?;
    if format == Format::Stl {
        if destination.exists() {
            return Err("目标文件夹已存在，请选择新名称".into());
        }
        let staging = tempfile::Builder::new()
            .prefix(".vault-export-")
            .tempdir_in(parent)
            .map_err(err)?;
        for (i, (item, source)) in inputs.iter().enumerate() {
            let generated = if let Some(mesh) = generated.get(&item.path) {
                Some(mesh.clone())
            } else if item.path == crate::label::PATH && label.active() {
                Some(crate::label::generate(&Mesh::load(source)?, label)?)
            } else {
                None
            };
            for copy in 1..=item.qty {
                let name = format!("{:03}-{:03}-{}", i + 1, copy, item.name());
                if let Some(mesh) = &generated {
                    crate::label::write_stl(mesh, &staging.path().join(name))?;
                } else {
                    std::fs::copy(source, staging.path().join(name)).map_err(err)?;
                }
            }
        }
        std::fs::rename(staging.path(), destination).map_err(err)?;
    } else {
        let mut staging = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
        write_3mf(
            staging.as_file_mut(),
            &inputs,
            label,
            &generated,
            &options.plate,
            gridfinity.map(|(_, layout, selection)| (layout, selection)),
        )?;
        staging.as_file().sync_all().map_err(err)?;
        staging.persist(destination).map_err(err)?;
    }
    Ok(destination.to_path_buf())
}
fn write_3mf(
    file: &mut File,
    inputs: &[(Bom, PathBuf)],
    label: &crate::label::Config,
    generated: &HashMap<String, std::sync::Arc<Mesh>>,
    plate: &crate::print_plate::Config,
    gridfinity: Option<(&crate::gridfinity::Layout, &crate::catalog::Selection)>,
) -> Result<(), String> {
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, content) in [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/><Default Extension="config" ContentType="application/octet-stream"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#,
        ),
    ] {
        zip.start_file(name, options).map_err(err)?;
        zip.write_all(content.as_bytes()).map_err(err)?;
    }
    zip.start_file("3D/3dmodel.model", options).map_err(err)?;
    writeln!(zip,r#"<?xml version="1.0" encoding="UTF-8"?><model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><metadata name="Application">BambuStudio-02.05.00.66</metadata><metadata name="BambuStudio:3mfVersion">1</metadata><metadata name="VAULT:Generator">VAULT 装配工作台 · P1S exporter</metadata><resources>"#).map_err(err)?;
    let mut sizes = Vec::new();
    let mut resources = Vec::new();
    let mut material_ids = Vec::<Vec<(usize, u32)>>::new();
    for (item, source) in inputs {
        let mesh = if let Some(mesh) = generated.get(&item.path) {
            mesh.clone()
        } else {
            Mesh::load(source).map_err(|e| format!("{}: {e}", item.name()))?
        };
        let mesh = if item.path == crate::label::PATH {
            crate::label::generate(&mesh, label)?
        } else {
            mesh
        };
        sizes.push(mesh.size);
        let base_slot = plate.slot(&item.path);
        let text_slot = plate.text_slot(&item.path);
        let text_spec = if item.path == crate::label::PATH && label.active() {
            let top = if label.style == crate::label::Style::Raised {
                mesh.max.y - label.depth
            } else {
                mesh.max.y
            };
            Some((label.clone(), [0., 0.], top as f64))
        } else if let Some((layout, selection)) = gridfinity {
            if let Some(bin) = layout.bins.iter().find(|b| {
                b.tab
                    && b.label.active()
                    && ((!b.detachable() && b.path() == item.path)
                        || (b.detachable() && b.label_path() == item.path))
            }) {
                if bin.detachable() {
                    Some((bin.label.clone(), [0., 0.], 1.2))
                } else {
                    let top =
                        mesh.min.y + crate::gridfinity::units(bin, selection)? as f32 * 7. - 1.2;
                    let tab_y = -(bin.span[1] as f32 * 42. - 0.5) / 2. + 1.25 + 15.85 / 2.;
                    Some((bin.label.clone(), [0., tab_y as f64], top as f64))
                }
            } else {
                None
            }
        } else {
            None
        };
        let meshes = if text_slot != base_slot
            && let Some((config, offset, top)) = text_spec
        {
            crate::label::color_regions(&mesh, &config, offset, top)?
        } else {
            vec![mesh.clone()]
        };
        let mut ids = Vec::new();
        for (i, region) in meshes.into_iter().enumerate() {
            let slot = if i == 0 { base_slot } else { text_slot };
            ids.push((resources.len() + 1, slot));
            resources.push((
                region,
                mesh.min.y,
                format!("{}{}", item.name(), if i == 0 { "" } else { " · 文字" }),
            ));
        }
        material_ids.push(ids);
    }
    for (index, (mesh, floor, name)) in resources.iter().enumerate() {
        // Weld by exact source coordinates, preserving triangle order and disconnected bodies.
        let mut map = HashMap::<[u32; 3], usize>::new();
        let mut vertices = Vec::<[f32; 3]>::new();
        let mut triangles = Vec::with_capacity(mesh.vertices.len());
        for v in &mesh.vertices {
            let [x, y, z] = v.position;
            let point = [x, -z, y - floor]; // undo viewer axis mapping, put source bottom at Z=0
            let key = point.map(|v| if v == 0. { 0 } else { v.to_bits() });
            let id = *map.entry(key).or_insert_with(|| {
                let id = vertices.len();
                vertices.push(point);
                id
            });
            triangles.push(id);
        }
        writeln!(
            zip,
            r#"<object id="{}" type="model" name="{}"><mesh><vertices>"#,
            index + 1,
            xml(name)
        )
        .map_err(err)?;
        for [x, y, z] in vertices {
            writeln!(zip, r#"<vertex x="{x}" y="{y}" z="{z}"/>"#).map_err(err)?;
        }
        writeln!(zip, "</vertices><triangles>").map_err(err)?;
        for t in triangles.chunks_exact(3) {
            writeln!(
                zip,
                r#"<triangle v1="{}" v2="{}" v3="{}"/>"#,
                t[0], t[1], t[2]
            )
            .map_err(err)?;
        }
        writeln!(zip, "</triangles></mesh></object>").map_err(err)?;
    }
    let parts: Vec<_> = inputs
        .iter()
        .zip(&sizes)
        .map(|((item, _), size)| (item.name().to_string(), *size, item.qty))
        .collect();
    let materials: Vec<Vec<u32>> = material_ids
        .iter()
        .map(|ids| ids.iter().map(|(_, slot)| *slot).collect())
        .collect();
    let plate_groups: Vec<_> = inputs
        .iter()
        .map(|(item, _)| {
            if gridfinity.is_some_and(|(layout, _)| {
                layout
                    .bins
                    .iter()
                    .any(|b| b.detachable() && b.label_path() == item.path)
            }) {
                crate::print_plate::PlateGroup::RemovableLabel
            } else {
                crate::print_plate::PlateGroup::Standard
            }
        })
        .collect();
    let placements =
        crate::print_plate::arrange_grouped_materials(&parts, &materials, &plate_groups, plate)?;
    let plate_count = placements.iter().map(|p| p.plate + 1).max().unwrap_or(0);
    let mut config = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><config>");
    // Each copy gets a wrapper object: Bambu's plate map stores one instance per object ID.
    for (i, p) in placements.iter().enumerate() {
        let object = resources.len() + i + 1;
        let name = xml(&format!("{} #{}", inputs[p.input].0.name(), p.copy + 1));
        writeln!(
            zip,
            r#"<object id="{object}" type="model" name="{name}"><components>"#
        )
        .map_err(err)?;
        for (part, _) in &material_ids[p.input] {
            writeln!(zip, r#"<component objectid="{part}"/>"#).map_err(err)?;
        }
        writeln!(zip, "</components></object>").map_err(err)?;
        config += &format!(
            r#"<object id="{object}"><metadata key="name" value="{name}"/><metadata key="extruder" value="{}"/>"#,
            plate.slot(&inputs[p.input].0.path)
        );
        if plate.support_by_name
            && inputs[p.input]
                .0
                .path
                .to_ascii_lowercase()
                .contains("turn on supports")
        {
            config += r#"<metadata key="enable_support" value="1"/>"#;
        }
        for (part, slot) in &material_ids[p.input] {
            let part_name = xml(&resources[*part - 1].2);
            config += &format!(
                r#"<part id="{part}" subtype="normal_part"><metadata key="name" value="{part_name}"/><metadata key="extruder" value="{slot}"/><metadata key="matrix" value="1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"/></part>"#
            );
        }
        config += "</object>";
    }
    writeln!(zip, "</resources><build>").map_err(err)?;
    for (i, p) in placements.iter().enumerate() {
        let object = resources.len() + i + 1;
        let [ox, oy] = crate::print_plate::plate_origin(p.plate, plate_count);
        let (x, y) = (ox + p.x, oy + p.y);
        let rotation = if p.rotated {
            "0 1 0 -1 0 0 0 0 1"
        } else {
            "1 0 0 0 1 0 0 0 1"
        };
        writeln!(
            zip,
            r#"<item objectid="{object}" transform="{rotation} {x} {y} 0" printable="1"/>"#
        )
        .map_err(err)?;
    }
    writeln!(zip, "</build></model>").map_err(err)?;
    for index in 0..plate_count {
        let item = placements.iter().find(|p| p.plate == index).unwrap();
        let mut slots = materials[item.input].clone();
        slots.sort_unstable();
        slots.dedup();
        let colors = slots
            .iter()
            .map(|s| format!("{s} {}", plate.color_hex(*s)))
            .collect::<Vec<_>>()
            .join(" + ");
        let name = xml(&format!(
            "{}{} {colors} · {}",
            if plate_groups[item.input] == crate::print_plate::PlateGroup::RemovableLabel {
                "可拆标牌 · "
            } else {
                ""
            },
            if slots.len() > 1 { "多色" } else { "颜色" },
            index + 1
        ));
        config += &format!(
            r#"<plate><metadata key="plater_id" value="{}"/><metadata key="plater_name" value="{}"/><metadata key="locked" value="false"/>"#,
            index + 1,
            name
        );
        for (i, _p) in placements
            .iter()
            .enumerate()
            .filter(|(_, p)| p.plate == index)
        {
            let object = resources.len() + i + 1;
            config += &format!(
                r#"<model_instance><metadata key="object_id" value="{object}"/><metadata key="instance_id" value="0"/><metadata key="identify_id" value="{}"/></model_instance>"#,
                i + 1
            );
        }
        config += "</plate>";
    }
    config += "</config>";
    zip.start_file("Metadata/model_settings.config", options)
        .map_err(err)?;
    zip.write_all(config.as_bytes()).map_err(err)?;
    zip.start_file("Metadata/project_settings.config", options)
        .map_err(err)?;
    zip.write_all(&plate.project_json()?).map_err(err)?;
    zip.finish().map_err(err)?.flush().map_err(err)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    const TRI: &str = "solid t
facet normal 0 0 1
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid t";
    #[test]
    fn removable_labels_export_on_dedicated_plates() {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/cullenect-validation");
        std::fs::create_dir_all(&out).unwrap();
        let temp = tempfile::tempdir_in(&out).unwrap();
        let root = temp.path().join("library");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("tall.stl"), TRI).unwrap();
        for multicolor in [false, true] {
            let bin = crate::gridfinity::Bin {
                detachable_label: true,
                label: crate::label::Config {
                    enabled: multicolor,
                    text: "M3".into(),
                    font: crate::label::Font::ArialBold,
                    size: 4.,
                    depth: 0.2,
                    ..Default::default()
                },
                ..Default::default()
            };
            let layout = crate::gridfinity::Layout {
                bins: vec![bin.clone()],
                ..Default::default()
            };
            let items = vec![
                Bom {
                    path: "tall.stl".into(),
                    qty: 2,
                    enabled: true,
                    group: "test".into(),
                },
                Bom {
                    path: bin.label_path(),
                    qty: 1,
                    enabled: true,
                    group: crate::gridfinity::GROUP.into(),
                },
            ];
            let mut options = Options::default();
            if multicolor {
                options.plate.text_filaments.insert(bin.label_path(), 2);
            }
            let destination = temp.path().join(format!("separate-{multicolor}.3mf"));
            run_configured(
                &root,
                &items,
                &destination,
                &options,
                &Default::default(),
                Some((Path::new("no-generator"), &layout, &Default::default())),
            )
            .unwrap();
            let mut zip = zip::ZipArchive::new(File::open(destination).unwrap()).unwrap();
            let mut settings = String::new();
            zip.by_name("Metadata/model_settings.config")
                .unwrap()
                .read_to_string(&mut settings)
                .unwrap();
            assert_eq!(settings.matches("<plate>").count(), 2);
            let plates: Vec<_> = settings
                .split("<plate>")
                .skip(1)
                .map(|p| p.split("</plate>").next().unwrap())
                .collect();
            let label_plate = plates.iter().find(|p| p.contains("可拆标牌")).unwrap();
            assert_eq!(label_plate.matches("<model_instance>").count(), 1);
            let other_plate = plates.iter().find(|p| !p.contains("可拆标牌")).unwrap();
            assert_eq!(other_plate.matches("<model_instance>").count(), 2);
            let label_object = settings
                .split("<object id=\"")
                .skip(1)
                .find(|o| {
                    o.split("</object>")
                        .next()
                        .unwrap()
                        .contains("Cullenect-1.stl")
                })
                .unwrap();
            let id = label_object.split('"').next().unwrap();
            let membership = format!("key=\"object_id\" value=\"{id}\"");
            assert!(label_plate.contains(&membership));
            assert!(!other_plate.contains(&membership));
            assert_eq!(
                label_object
                    .split("</object>")
                    .next()
                    .unwrap()
                    .matches("<part ")
                    .count(),
                if multicolor { 2 } else { 1 }
            );
        }
    }
    #[test]
    fn three_mf_keeps_color_ids_and_separate_plate_memberships() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        std::fs::create_dir(&root).unwrap();
        let mut bom = Vec::new();
        let mut options = Options::default();
        for (name, slot) in [("red.stl", 1), ("blue.stl", 2)] {
            std::fs::write(root.join(name), TRI).unwrap();
            bom.push(Bom {
                path: name.into(),
                qty: 2,
                enabled: true,
                group: "Test".into(),
            });
            options.plate.part_filaments.insert(name.into(), slot);
        }
        options.plate.palette[0] = [240, 20, 30];
        options.plate.palette[1] = [10, 100, 220];
        let target = temp.path().join("colors.3mf");
        run_configured(&root, &bom, &target, &options, &Default::default(), None).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(target).unwrap()).unwrap();
        let mut config = String::new();
        zip.by_name("Metadata/model_settings.config")
            .unwrap()
            .read_to_string(&mut config)
            .unwrap();
        assert_eq!(config.matches("<plate>").count(), 2);
        assert_eq!(config.matches("<model_instance>").count(), 4);
        assert_eq!(config.matches("key=\"extruder\" value=\"1\"").count(), 4);
        assert_eq!(config.matches("key=\"extruder\" value=\"2\"").count(), 4);
        assert!(config.contains("#F0141E"));
        assert!(config.contains("#0A64DC"));
        let json: serde_json::Value =
            serde_json::from_reader(zip.by_name("Metadata/project_settings.config").unwrap())
                .unwrap();
        assert_eq!(json["filament_colour"][0], "#F0141E");
        assert_eq!(json["filament_colour"][1], "#0A64DC");
    }
    #[test]
    fn legacy_open_bambu_migrates_without_launch_and_settings_roundtrip() {
        let o: Options =
            toml::from_str("format = \"Bambu\"\nbambu_executable = \"old.exe\"").unwrap();
        assert_eq!(o.format, Format::ThreeMf);
        let c = crate::print_plate::Config {
            layer: 0.16,
            walls: 3,
            infill: 22,
            filament: 4,
            textured: false,
            ..Default::default()
        };
        let settings: serde_json::Value =
            serde_json::from_slice(&c.project_json().unwrap()).unwrap();
        assert_eq!(settings["printer_model"], "Bambu Lab P1S");
        assert_eq!(settings["layer_height"], "0.16");
        assert_eq!(settings["wall_loops"], "3");
        assert_eq!(settings["sparse_infill_density"], "22%");
        assert_eq!(settings["curr_bed_type"], "High Temp Plate");
        assert_eq!(settings["enable_prime_tower"], "0");
        let o = Options {
            plate: c,
            ..Default::default()
        };
        let loaded: Options = toml::from_str(&toml::to_string(&o).unwrap()).unwrap();
        assert_eq!(loaded.plate.filament, 4);
    }
    #[test]
    fn original_copies_package_instances_and_source_protection() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("A & B.stl"), TRI).unwrap();
        let bom = vec![Bom {
            path: "A & B.stl".into(),
            qty: 2,
            group: "test".into(),
            enabled: true,
        }];
        let dir = temp.path().join("stls");
        run(&root, &bom, &dir, Format::Stl).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        for f in std::fs::read_dir(&dir).unwrap() {
            assert_eq!(std::fs::read(f.unwrap().path()).unwrap(), TRI.as_bytes());
        }
        let target = temp.path().join("test.3mf");
        run(&root, &bom, &target, Format::ThreeMf).unwrap();
        let mut z = zip::ZipArchive::new(File::open(&target).unwrap()).unwrap();
        assert_eq!(z.len(), 5);
        let mut model = String::new();
        z.by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model)
            .unwrap();
        assert_eq!(model.matches("<object ").count(), 3);
        assert_eq!(model.matches("<item ").count(), 2);
        assert!(model.contains("A &amp; B.stl"));
        assert!(run(&root, &bom, &root.join("bad.3mf"), Format::ThreeMf).is_err());
        let original = std::fs::read(&target).unwrap();
        let mut invalid = bom.clone();
        invalid.push(Bom {
            path: "missing.stl".into(),
            ..bom[0].clone()
        });
        assert!(run(&root, &invalid, &target, Format::ThreeMf).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), original);
        std::fs::write(root.join("invalid.stl"), "not a mesh").unwrap();
        invalid[1].path = "invalid.stl".into();
        assert!(run(&root, &invalid, &target, Format::ThreeMf).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), original);
        invalid[1].path = "../outside.stl".into();
        std::fs::write(temp.path().join("outside.stl"), TRI).unwrap();
        assert!(run(&root, &invalid, &target, Format::ThreeMf).is_err());

        assert_eq!(
            std::fs::read(root.join("A & B.stl")).unwrap(),
            TRI.as_bytes()
        );
    }
    #[test]
    fn real_text_multimaterial_export() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output/export-smoke");
        std::fs::create_dir_all(&out).unwrap();
        let mut options = Options::default();
        options.plate.center = true;
        options
            .plate
            .text_filaments
            .insert(crate::label::PATH.into(), 2);
        options.plate.palette[0] = [240, 240, 240];
        options.plate.palette[1] = [20, 20, 20];
        let item = Bom {
            path: crate::label::PATH.into(),
            qty: 2,
            enabled: true,
            group: "Label".into(),
        };
        for style in [crate::label::Style::Raised, crate::label::Style::Engraved] {
            let label = crate::label::Config {
                enabled: true,
                text: "收纳\nA8".into(),
                style,
                ..Default::default()
            };
            let dest = out.join(format!("VAULT-text-{style:?}.3mf"));
            run_configured(
                Path::new(&root),
                std::slice::from_ref(&item),
                &dest,
                &options,
                &label,
                None,
            )
            .unwrap();
            let mut z = zip::ZipArchive::new(File::open(dest).unwrap()).unwrap();
            let mut settings = String::new();
            z.by_name("Metadata/model_settings.config")
                .unwrap()
                .read_to_string(&mut settings)
                .unwrap();
            assert_eq!(settings.matches("<plate>").count(), 1);
            assert_eq!(settings.matches("<part ").count(), 4);
            assert_eq!(settings.matches(r#"key="extruder" value="2""#).count(), 2);
            let mut model = String::new();
            z.by_name("3D/3dmodel.model")
                .unwrap()
                .read_to_string(&mut model)
                .unwrap();
            assert_eq!(model.matches("<component ").count(), 4);
            assert_eq!(model.matches("<item ").count(), 2);
        }
    }
    #[test]
    fn real_library_3mf() {
        let Ok(root) = std::env::var("VAULT_EXPORT_TEST_ROOT") else {
            return;
        };
        let c = crate::catalog::Catalog::scan(root.into()).unwrap();
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output/export-smoke");
        std::fs::create_dir_all(&out).unwrap();
        for width in ["S", "M"] {
            let selection = crate::catalog::Selection {
                width: width.into(),
                depth: if width == "M" { 4 } else { 3 },
                ..Default::default()
            };
            let bom = c.bom(&selection);
            if width == "M" {
                let mut options = Options::default();
                options.plate.palette[0] = [190, 190, 190];
                options.plate.palette[1] = [255, 120, 20];
                for item in &bom {
                    if item.path.contains("/Facades/") || item.path.ends_with("/Facade Cap.stl") {
                        options.plate.part_filaments.insert(item.path.clone(), 2);
                    }
                }
                run_configured(
                    &c.root,
                    &bom,
                    &out.join("VAULT-M4-colors.3mf"),
                    &options,
                    &Default::default(),
                    None,
                )
                .unwrap();
            }

            run(
                &c.root,
                &bom,
                &out.join(format!("VAULT-{width}{}.3mf", selection.depth)),
                Format::ThreeMf,
            )
            .unwrap();
        }
    }
}
