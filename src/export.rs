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
fn build_orientation(rotated: bool, face_down: bool) -> &'static str {
    // Rotate 180 degrees about print X, then optionally 90 degrees about Z.
    // Both are proper rotations: never mirror the lettering or triangle winding.
    match (rotated, face_down) {
        (false, false) => "1 0 0 0 1 0 0 0 1",
        (true, false) => "0 1 0 -1 0 0 0 0 1",
        (false, true) => "1 0 0 0 -1 0 0 0 -1",
        (true, true) => "0 1 0 1 0 0 0 0 -1",
    }
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
    writeln!(zip,r#"<?xml version="1.0" encoding="UTF-8"?><model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><metadata name="Application">BambuStudio-02.05.00.66</metadata><metadata name="BambuStudio:3mfVersion">1</metadata><metadata name="BambuStudio:FdmSupportsPaintingVersion">0</metadata><metadata name="VAULT:Generator">VAULT 装配工作台 · P1S exporter</metadata><resources>"#).map_err(err)?;
    let mut sizes = Vec::new();
    let mut resources = Vec::new();
    let mut material_ids = Vec::<Vec<(usize, u32)>>::new();
    let mut local_supports = Vec::new();
    let mut face_down = Vec::new();
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
        let standalone_label = item.path == crate::label::PATH
            || gridfinity.is_some_and(|(layout, _)| {
                layout
                    .bins
                    .iter()
                    .any(|b| b.detachable() && b.label_path() == item.path)
            });
        face_down.push(
            plate.flat_text_face_down
                && standalone_label
                && text_spec
                    .as_ref()
                    .is_some_and(|(config, _, _)| config.style == crate::label::Style::Flat),
        );
        let meshes = if text_slot != base_slot
            && let Some((config, offset, top)) = text_spec
        {
            crate::label::color_regions(&mesh, &config, offset, top)?
        } else {
            vec![mesh.clone()]
        };
        // Pack the final manufacturing vertices, including text/material
        // regions, rather than trusting cached sizes or a centered local origin.
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        for vertex in meshes.iter().flat_map(|m| &m.vertices) {
            let p = glam::Vec3::from(vertex.position);
            if !p.is_finite() {
                return Err(format!("{}：导出网格包含无效坐标", item.name()));
            }
            min = min.min(p);
            max = max.max(p);
        }
        let extent = max - min;
        sizes.push(glam::Vec3::new(extent.x, extent.z, extent.y));
        let origin = glam::Vec3::new((min.x + max.x) / 2., min.y, (min.z + max.z) / 2.);
        let mut ids = Vec::new();
        let mut painted = false;
        for (i, region) in meshes.into_iter().enumerate() {
            let slot = if i == 0 { base_slot } else { text_slot };
            let support_mask = if plate.module_clip_supports {
                crate::catalog::module_clip_count(&item.path)
                    .map(|count| crate::print_support::module_clips(&region, count))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            painted |= support_mask.iter().any(|&marked| marked);
            ids.push((resources.len() + 1, slot));
            resources.push((
                region,
                origin,
                format!("{}{}", item.name(), if i == 0 { "" } else { " · 文字" }),
                support_mask,
            ));
        }
        material_ids.push(ids);
        local_supports.push(painted);
    }
    for (index, (mesh, origin, name, support_mask)) in resources.iter().enumerate() {
        // Weld by exact source coordinates, preserving triangle order and disconnected bodies.
        let mut map = HashMap::<[u32; 3], usize>::new();
        let mut vertices = Vec::<[f32; 3]>::new();
        let mut triangles = Vec::with_capacity(mesh.vertices.len());
        for v in &mesh.vertices {
            let [x, y, z] = v.position;
            let point = [x - origin.x, origin.z - z, y - origin.y]; // centered XY, bottom at Z=0
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
        for (index, t) in triangles.chunks_exact(3).enumerate() {
            // Bambu TriangleSelector: unsplit ENFORCER (1 << 2) = hex 4.
            let paint = if support_mask.get(index) == Some(&true) {
                r#" paint_supports="4""#
            } else {
                ""
            };
            writeln!(
                zip,
                r#"<triangle v1="{}" v2="{}" v3="{}"{paint}/>"#,
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
        if local_supports[p.input] {
            config += r#"<metadata key="enable_support" value="1"/><metadata key="support_type" value="tree(manual)"/><metadata key="support_on_build_plate_only" value="1"/>"#;
        } else if plate.support_by_name
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
        let rotation = build_orientation(p.rotated, face_down[p.input]);
        let z = if face_down[p.input] {
            sizes[p.input].z
        } else {
            0.
        };
        writeln!(
            zip,
            r#"<item objectid="{object}" transform="{rotation} {x} {y} {z}" printable="1"/>"#
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
    fn face_down_keeps_integral_bin_upright() {
        if std::env::var_os("VAULT_GRIDFINITY_TEST").is_none() {
            return;
        }
        let home = Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = home.join("output/face-down");
        std::fs::create_dir_all(&out).unwrap();
        let bin = crate::gridfinity::Bin {
            label: crate::label::Config {
                enabled: true,
                text: "B8".into(),
                font: crate::label::Font::ArialBold,
                size: 4.,
                depth: 0.4,
                style: crate::label::Style::Flat,
                ..Default::default()
            },
            ..Default::default()
        };
        let selection = Default::default();
        let mesh = crate::gridfinity::generate(home, &bin, &selection).unwrap();
        let item = Bom {
            path: bin.path(),
            qty: 1,
            enabled: true,
            group: "Test".into(),
        };
        let generated = [(item.path.clone(), mesh)].into_iter().collect();
        let layout = crate::gridfinity::Layout {
            bins: vec![bin],
            ..Default::default()
        };
        let mut c = crate::print_plate::Config {
            flat_text_face_down: true,
            ..Default::default()
        };
        c.set_part_slot(&item.path, 3);
        c.set_text_slot(&item.path, 1);
        let path = out.join("Gridfinity-flat-bin-upright.3mf");
        write_3mf(
            &mut File::create(&path).unwrap(),
            &[(item, PathBuf::new())],
            &Default::default(),
            &generated,
            &c,
            Some((&layout, &selection)),
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut model = String::new();
        zip.by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model)
            .unwrap();
        let t: Vec<f32> = model
            .split("<item ")
            .nth(1)
            .unwrap()
            .split("transform=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(t[8], 1.);
        assert_eq!(t[11], 0.);
    }
    #[test]
    fn face_down_rotations_are_proper_for_both_packing_orientations() {
        for rotated in [false, true] {
            for down in [false, true] {
                let m: Vec<f32> = build_orientation(rotated, down)
                    .split_whitespace()
                    .map(|v| v.parse().unwrap())
                    .collect();
                let rotation = glam::Mat3::from_cols_array(&m.try_into().unwrap());
                assert!((rotation.determinant() - 1.).abs() < 0.0001);
                let normal = rotation * glam::Vec3::Z;
                assert_eq!(normal, if down { -glam::Vec3::Z } else { glam::Vec3::Z });
                let expected = if down {
                    glam::Vec3::new(2., -3., -5.)
                } else {
                    glam::Vec3::new(2., 3., 5.)
                };
                let expected = if rotated {
                    glam::Vec3::new(-expected.y, expected.x, expected.z)
                } else {
                    expected
                };
                assert_eq!(rotation * glam::Vec3::new(2., 3., 5.), expected);
            }
        }
        let old: crate::print_plate::Config = toml::from_str("rotate = false").unwrap();
        assert!(old.flat_text_face_down);
        let disabled = crate::print_plate::Config {
            flat_text_face_down: false,
            ..old
        };
        let restored: crate::print_plate::Config =
            toml::from_str(&toml::to_string(&disabled).unwrap()).unwrap();
        assert!(!restored.flat_text_face_down);
    }
    fn resource_points(xml: &str) -> Vec<glam::Vec3> {
        xml.split("<vertex ")
            .skip(1)
            .map(|s| {
                let coordinate = |key: &str| {
                    s.split_once(&format!("{key}=\""))
                        .unwrap()
                        .1
                        .split('"')
                        .next()
                        .unwrap()
                        .parse::<f32>()
                        .unwrap()
                };
                glam::Vec3::new(coordinate("x"), coordinate("y"), coordinate("z"))
            })
            .collect()
    }
    #[test]
    fn flat_label_copies_flip_together_without_changing_resources() {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/face-down");
        std::fs::create_dir_all(&out).unwrap();
        let bin = crate::gridfinity::Bin {
            detachable_label: true,
            label: crate::label::Config {
                enabled: true,
                text: "收纳 B8".into(),
                style: crate::label::Style::Flat,
                size: 5.,
                depth: 0.4,
                ..Default::default()
            },
            ..Default::default()
        };
        let layout = crate::gridfinity::Layout {
            bins: vec![bin.clone()],
            ..Default::default()
        };
        let item = Bom {
            path: bin.label_path(),
            qty: 2,
            enabled: true,
            group: "Test".into(),
        };
        let generated = [(
            item.path.clone(),
            crate::cullenect::label(&bin.label).unwrap(),
        )]
        .into_iter()
        .collect();
        let mut c = crate::print_plate::Config {
            center: true,
            ..Default::default()
        };
        c.set_part_slot(&item.path, 3);
        c.set_text_slot(&item.path, 1);
        let inputs = vec![(item, PathBuf::new())];
        let mut original_resources = None;
        for down in [false, true] {
            for rotate in [false, true] {
                c.flat_text_face_down = down;
                c.rotate = rotate;
                let path = out.join(format!("Cullenect-flat-down-{down}-rotate-{rotate}.3mf"));
                write_3mf(
                    &mut File::create(&path).unwrap(),
                    &inputs,
                    &Default::default(),
                    &generated,
                    &c,
                    Some((&layout, &Default::default())),
                )
                .unwrap();
                let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
                let mut model = String::new();
                zip.by_name("3D/3dmodel.model")
                    .unwrap()
                    .read_to_string(&mut model)
                    .unwrap();
                let resources = model.split("<build>").next().unwrap();
                if let Some(original) = &original_resources {
                    assert_eq!(original, resources);
                } else {
                    original_resources = Some(resources.to_owned());
                }
                let ink = model
                    .split("<object id=\"2\"")
                    .nth(1)
                    .unwrap()
                    .split("</object>")
                    .next()
                    .unwrap();
                let ink = resource_points(ink);
                let all = resource_points(resources);
                let items: Vec<_> = model.split("<item ").skip(1).collect();
                assert_eq!(items.len(), 2);
                for item in items {
                    let t: Vec<f32> = item
                        .split("transform=\"")
                        .nth(1)
                        .unwrap()
                        .split('"')
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .map(|v| v.parse().unwrap())
                        .collect();
                    let rotation = glam::Mat3::from_cols_array(&t[..9].try_into().unwrap());
                    let position = glam::Vec3::new(t[9], t[10], t[11]);
                    let height_range = |points: &[glam::Vec3]| {
                        points
                            .iter()
                            .map(|v| (rotation * *v + position).z)
                            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), z| {
                                (lo.min(z), hi.max(z))
                            })
                    };
                    let (lo, hi) = height_range(&all);
                    assert!(lo.abs() < 0.001 && (hi - 1.2).abs() < 0.001);
                    let (lo, hi) = height_range(&ink);
                    let expected = if down { (0., 0.4) } else { (0.8, 1.2) };
                    assert!((lo - expected.0).abs() < 0.001 && (hi - expected.1).abs() < 0.001);
                    assert!((rotation.determinant() - 1.).abs() < 0.001);
                }
            }
        }
    }
    #[test]
    fn flat_cullenect_color_sample() {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/flat-text");
        std::fs::create_dir_all(&out).unwrap();
        let bin = crate::gridfinity::Bin {
            detachable_label: true,
            label: crate::label::Config {
                enabled: true,
                text: "收纳 B8".into(),
                style: crate::label::Style::Flat,
                size: 5.,
                depth: 0.4,
                ..Default::default()
            },
            ..Default::default()
        };
        let layout = crate::gridfinity::Layout {
            bins: vec![bin.clone()],
            ..Default::default()
        };
        let item = Bom {
            path: bin.label_path(),
            qty: 1,
            enabled: true,
            group: "Test".into(),
        };
        let generated = [(
            item.path.clone(),
            crate::cullenect::label(&bin.label).unwrap(),
        )]
        .into_iter()
        .collect();
        let mut c = crate::print_plate::Config {
            center: true,
            flat_text_face_down: false,
            ..Default::default()
        };
        c.set_part_slot(&item.path, 3);
        c.set_text_slot(&item.path, 1);
        let path = out.join("Cullenect-flat.3mf");
        write_3mf(
            &mut File::create(&path).unwrap(),
            &[(item, PathBuf::new())],
            &Default::default(),
            &generated,
            &c,
            Some((&layout, &Default::default())),
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut config = String::new();
        zip.by_name("Metadata/model_settings.config")
            .unwrap()
            .read_to_string(&mut config)
            .unwrap();
        assert_eq!(config.matches("<part ").count(), 2);
        assert_eq!(config.matches("<plate>").count(), 1);
        assert!(config.contains(r#"key="extruder" value="3""#));
        assert!(config.contains(r#"key="extruder" value="1""#));
    }
    #[test]
    fn final_vertices_override_stale_size_and_local_origin() {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/export-smoke");
        std::fs::create_dir_all(&out).unwrap();
        let temp = tempfile::tempdir_in(out).unwrap();
        let inputs = vec![(
            Bom {
                path: "shifted.stl".into(),
                qty: 1,
                enabled: true,
                group: "Test".into(),
            },
            PathBuf::new(),
        )];
        for width in [20., 300.] {
            let mut mesh = Mesh::parse(TRI.as_bytes()).unwrap();
            for (v, point) in mesh.vertices.iter_mut().zip([
                [100., 40., 200.],
                [100. + width, 40., 200.],
                [100., 45., 210.],
            ]) {
                v.position = point;
            }
            // Intentionally leave all cached bounds stale.
            let generated = [("shifted.stl".into(), std::sync::Arc::new(mesh))]
                .into_iter()
                .collect();
            let destination = temp.path().join(format!("{width}.3mf"));
            let result = write_3mf(
                &mut File::create(&destination).unwrap(),
                &inputs,
                &Default::default(),
                &generated,
                &Default::default(),
                None,
            );
            if width == 300. {
                assert!(
                    result.is_err(),
                    "Oversized actual geometry must not be exported"
                );
            } else {
                result.unwrap();
                let mut zip = zip::ZipArchive::new(File::open(destination).unwrap()).unwrap();
                let mut xml = String::new();
                zip.by_name("3D/3dmodel.model")
                    .unwrap()
                    .read_to_string(&mut xml)
                    .unwrap();
                assert!(xml.contains(r#"<vertex x="-10" y="5" z="0"/>"#));
                assert!(xml.contains(r#"<vertex x="10" y="5" z="0"/>"#));
                assert!(xml.contains(r#"<vertex x="-10" y="-5" z="5"/>"#));
            }
        }
    }
    #[test]
    fn local_support_painting_survives_copies_and_respects_toggle() {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/custom-supports");
        std::fs::create_dir_all(&out).unwrap();
        let temp = tempfile::tempdir_in(out).unwrap();
        let module = "01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F.stl";
        let key = "Key (Turn On Supports).stl";
        let inputs: Vec<_> = [module, key]
            .into_iter()
            .map(|path| {
                (
                    Bom {
                        path: path.into(),
                        qty: 2,
                        enabled: true,
                        group: "Test".into(),
                    },
                    PathBuf::new(),
                )
            })
            .collect();
        let mesh = std::sync::Arc::new(crate::print_support::tests::fixture());
        let generated = [module, key]
            .into_iter()
            .map(|p| (p.into(), mesh.clone()))
            .collect();
        for enabled in [true, false] {
            let config = crate::print_plate::Config {
                module_clip_supports: enabled,
                ..Default::default()
            };
            let destination = temp.path().join(format!("supports-{enabled}.3mf"));
            write_3mf(
                &mut File::create(&destination).unwrap(),
                &inputs,
                &Default::default(),
                &generated,
                &config,
                None,
            )
            .unwrap();
            let mut zip = zip::ZipArchive::new(File::open(destination).unwrap()).unwrap();
            let mut model = String::new();
            zip.by_name("3D/3dmodel.model")
                .unwrap()
                .read_to_string(&mut model)
                .unwrap();
            assert_eq!(
                model.matches("paint_supports=\"4\"").count(),
                if enabled { 4 } else { 0 }
            );
            assert_eq!(model.matches("<triangle ").count(), 12);
            assert_eq!(model.matches("<component objectid=\"1\"").count(), 2);
            let mut settings = String::new();
            zip.by_name("Metadata/model_settings.config")
                .unwrap()
                .read_to_string(&mut settings)
                .unwrap();
            assert_eq!(
                settings
                    .matches("key=\"enable_support\" value=\"1\"")
                    .count(),
                if enabled { 4 } else { 2 }
            );
            assert_eq!(
                settings.matches("tree(manual)").count(),
                if enabled { 2 } else { 0 }
            );
        }
        let old: crate::print_plate::Config = toml::from_str("support_by_name = false").unwrap();
        assert!(old.module_clip_supports);
        let config = crate::print_plate::Config {
            module_clip_supports: false,
            ..old
        };
        let saved = toml::to_string(&config).unwrap();
        assert!(
            !toml::from_str::<crate::print_plate::Config>(&saved)
                .unwrap()
                .module_clip_supports
        );
    }
    #[test]
    fn local_module_support_sample() {
        let Ok(root) = std::env::var("VAULT_EXPORT_TEST_ROOT") else {
            return;
        };
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/custom-supports");
        std::fs::create_dir_all(&out).unwrap();
        let item = Bom {
            path: "01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F.stl".into(),
            qty: 1,
            enabled: true,
            group: "Module".into(),
        };
        for enabled in [true, false] {
            let mut options = Options::default();
            options.plate.module_clip_supports = enabled;
            options.plate.center = true;
            let destination = out.join(if enabled {
                "S3-module-local-supports.3mf"
            } else {
                "S3-module-baseline.3mf"
            });
            run_configured(
                Path::new(&root),
                std::slice::from_ref(&item),
                &destination,
                &options,
                &Default::default(),
                None,
            )
            .unwrap();
            let mut zip = zip::ZipArchive::new(File::open(destination).unwrap()).unwrap();
            let mut model = String::new();
            zip.by_name("3D/3dmodel.model")
                .unwrap()
                .read_to_string(&mut model)
                .unwrap();
            assert_eq!(model.contains("paint_supports=\"4\""), enabled);
        }
    }
    #[test]
    fn removable_labels_export_on_dedicated_plates() {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("output/cullenect-validation");
        std::fs::create_dir_all(&out).unwrap();
        let temp = tempfile::tempdir_in(&out).unwrap();
        let root = temp.path().join("library");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("tall.stl"), TRI).unwrap();
        for (multicolor, style) in [
            (false, crate::label::Style::Raised),
            (true, crate::label::Style::Raised),
            (true, crate::label::Style::Flat),
        ] {
            let bin = crate::gridfinity::Bin {
                detachable_label: true,
                label: crate::label::Config {
                    enabled: multicolor,
                    style,
                    text: "M3".into(),
                    font: crate::label::Font::ArialBold,
                    size: 4.,
                    depth: 0.2,
                    ..Default::default()
                },
                ..Default::default()
            };
            let layout = crate::gridfinity::Layout {
                bins: vec![
                    bin.clone(),
                    crate::gridfinity::Bin {
                        id: 2,
                        cell: [1, 0],
                        ..bin.clone()
                    },
                ],
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
            let mut items = items;
            items.push(Bom {
                path: layout.bins[1].label_path(),
                qty: 1,
                enabled: true,
                group: crate::gridfinity::GROUP.into(),
            });
            let mut options = Options::default();
            options.plate.set_part_slot(&bin.label_path(), 3);
            options.plate.set_part_slot("tall.stl", 3);
            if multicolor {
                options.plate.set_text_slot(&bin.label_path(), 2);
            }
            let destination = temp
                .path()
                .join(format!("separate-{multicolor}-{style:?}.3mf"));
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
            assert_eq!(label_plate.matches("<model_instance>").count(), 2);
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
            let second = settings
                .split("<object id=\"")
                .skip(1)
                .find(|o| {
                    o.split("</object>")
                        .next()
                        .unwrap()
                        .contains("Cullenect-2.stl")
                })
                .unwrap();
            let second = second.split("</object>").next().unwrap();
            assert!(second.contains("key=\"extruder\" value=\"3\""));
            assert_eq!(
                second.matches("key=\"extruder\" value=\"2\"").count(),
                if multicolor { 1 } else { 0 }
            );
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
        // Validate the actual packaged settings, including all six material slots.
        for (key, value) in [
            ("filament_type", "PETG"),
            ("filament_vendor", "Bambu Lab"),
            ("filament_ids", "GFG00"),
            ("filament_settings_id", "Bambu PETG Basic @BBL X1C"),
            ("nozzle_temperature_initial_layer", "245"),
            ("nozzle_temperature", "255"),
            ("textured_plate_temp", "70"),
            ("textured_plate_temp_initial_layer", "70"),
            ("filament_flow_ratio", "0.95"),
            ("filament_density", "1.25"),
            ("filament_max_volumetric_speed", "15"),
        ] {
            let slots = json[key].as_array().unwrap();
            assert_eq!(slots.len(), 6, "{key}");
            assert!(
                slots.iter().all(|slot| slot == value),
                "{key} must use PETG Basic in every slot"
            );
        }
        assert_eq!(
            json["default_filament_profile"][0],
            "Bambu PETG Basic @BBL X1C"
        );
        assert_eq!(json["printer_settings_id"], "Bambu Lab P1S 0.4 nozzle");
        assert_eq!(json["extruder_offset"], serde_json::json!(["0x2"]));
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
        for style in [
            crate::label::Style::Raised,
            crate::label::Style::Engraved,
            crate::label::Style::Flat,
        ] {
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
            for item in model.split("<item ").skip(1) {
                let t: Vec<f32> = item
                    .split("transform=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .map(|v| v.parse().unwrap())
                    .collect();
                assert_eq!(
                    t[8],
                    if style == crate::label::Style::Flat {
                        -1.
                    } else {
                        1.
                    }
                );
                assert!(
                    (t[11]
                        - if style == crate::label::Style::Flat {
                            2.00004
                        } else {
                            0.
                        })
                    .abs()
                        < 0.001
                );
            }
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
