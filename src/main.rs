#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod accessories;
mod app;
mod baseplate;
mod catalog;
mod collision;
mod constraints;
mod cullenect;
mod display;
mod divider_editor;
mod dividers;
mod explosion;
mod export;
mod front_panel;
mod geared_rails;
mod gridfinity;
mod gridfinity_editor;
mod label;
mod label_preview;
mod mesh;
mod module_assembly;
mod paths;
mod preview;
mod print_plate;
mod print_support;
mod render;
mod runners;
mod scene;
pub const APP_NAME: &str = "VAULT 装配工作台";
pub const APP_NAME_EN: &str = "VAULT Assembly Workbench";
pub fn app_name(zh: bool) -> &'static str {
    if zh { APP_NAME } else { APP_NAME_EN }
}

fn main() -> eframe::Result {
    let executable = std::env::current_exe().expect("Executable path unavailable");
    let home = executable
        .ancestors()
        .find(|dir| dir.join("Cargo.toml").is_file())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| executable.parent().unwrap().to_path_buf());
    let smoke = std::env::args().any(|a| a == "--smoke-test");
    let debug_ui = std::env::args().any(|a| a == "--debug-ui");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1440., 960.])
            .with_min_inner_size([1050., 700.]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, home, smoke, debug_ui)))),
    )
}

#[cfg(test)]
mod library_tests {
    use super::*;
    #[test]
    fn supplied_library_variants_and_assembly_roundtrip() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let catalog = catalog::Catalog::scan(root.into()).unwrap();
        assert!(catalog.stl_count() > 100);
        let mut combinations = 0;
        for w in ["S", "M", "L", "XL"] {
            for d in 3..=7 {
                for h in if w == "XL" {
                    vec!["1F"]
                } else {
                    vec!["1F", "2F", "4F"]
                } {
                    let s = catalog::Selection {
                        width: w.into(),
                        depth: d,
                        height: h.into(),
                        ..Default::default()
                    };
                    for b in catalog.bom(&s) {
                        assert!(catalog.resolve(&b.path).is_some(), "{}", b.path);
                    }
                    combinations += 1;
                }
            }
        }
        assert_eq!(combinations, 50);
        let xl = catalog::Selection {
            width: "XL".into(),
            depth: 6,
            ..Default::default()
        };
        let xl_scene =
            scene::Scene::load(&catalog.root, &catalog.bom(&xl), "XL6".into(), true).unwrap();
        assert!(
            xl_scene
                .parts
                .iter()
                .any(|p| p.pose.path.contains("Part 2"))
        );
        assert!(xl_scene.bounds(1.).1.is_finite());

        let s = catalog::Selection::default();
        let bom = catalog.bom(&s);
        let mut scene = scene::Scene::load(&catalog.root, &bom, "S3".into(), true).unwrap();
        assert!(scene.parts.len() > 10);
        let snapshot = toml::to_string_pretty(&scene.definition()).unwrap();
        let decoded: scene::Definition = toml::from_str(&snapshot).unwrap();
        scene.apply(&decoded);
        assert!(!scene.calibrated);
        let (center, radius) = scene.bounds(1.);
        assert!(center.is_finite() && radius.is_finite() && radius > 100.);
        // The table roof is printed diagonally: its thin axis must become viewer Y.
        let roof = catalog
            .entries
            .iter()
            .find(|e| {
                e.path.starts_with(&catalog::Catalog::prefix(&s)) && e.path.contains("Table Roof")
            })
            .map(|e| e.path.clone())
            .unwrap();
        let mut b = bom.clone();
        b.push(catalog::Bom {
            path: roof,
            qty: 1,
            group: "Roof".into(),
            enabled: true,
        });
        let roof_scene = scene::Scene::load(&catalog.root, &b, "roof".into(), true).unwrap();
        let part = roof_scene.parts.last().unwrap();
        let m = part.pose.matrix(0.);
        let mut lo = glam::Vec3::splat(f32::INFINITY);
        let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
        for v in &part.mesh.vertices {
            let p = m.transform_point3(v.position.into());
            lo = lo.min(p);
            hi = hi.max(p);
        }
        let size = hi - lo;
        assert!(size.y < size.x / 3. && size.y < size.z / 3., "roof: {size}");
    }
}

#[cfg(test)]
mod display_integration_tests {
    use super::*;
    #[test]
    fn springs_file_yields_two_bodies_not_two_copies_of_a_pair() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let path = "01. DRAWERS/07. Universal/Springs.stl";
        let bytes_before = std::fs::read(root.join(path)).unwrap();
        let source = mesh::Mesh::load(&root.join(path)).unwrap();
        let catalog = catalog::Catalog::scan(root.clone()).unwrap();
        let bom = catalog.bom(&catalog::Selection::default());
        let item = bom.iter().find(|b| b.path == path).unwrap().clone();
        assert_eq!(
            item.qty, 1,
            "Print one unchanged source file containing a pair"
        );
        let mut scene =
            scene::Scene::load(&root, std::slice::from_ref(&item), "springs".into(), true).unwrap();
        assert_eq!(scene.parts.len(), 2);
        assert_ne!(scene.parts[0].mesh_key(), scene.parts[1].mesh_key());
        assert_eq!(
            scene
                .parts
                .iter()
                .map(|p| p.mesh.vertices.len())
                .sum::<usize>(),
            source.vertices.len()
        );
        assert_eq!(scene.parts[0].mesh.vertices.len() / 3, 20816);
        assert_eq!(scene.parts[1].mesh.vertices.len() / 3, 20484);
        for p in &scene.parts {
            assert!((p.mesh.size.y - 37.).abs() < 0.01);
            assert!(p.source_size.y > 75.);
            assert_eq!(p.source_bodies, 2);
        }
        assert!(scene.parts[0].pose.position[0] < 0. && scene.parts[1].pose.position[0] > 0.);
        let old_position = scene.parts[0].pose.position;
        let legacy = scene::Definition {
            calibrated: true,
            relations: vec![],
            poses: vec![scene::Pose {
                path: path.into(),
                instance: 0,
                body: None,
                position: [999.; 3],
                rotation: [0.; 3],
                travel: [0.; 3],
                travel_frame: None,
            }],
        };
        scene.apply(&legacy);
        assert_eq!(scene.parts[0].pose.position, old_position);
        assert!(
            !scene.calibrated,
            "Legacy whole-file poses must not silently apply to bodies"
        );
        let mut saved = scene.definition();
        saved.poses[0].position = [1., 2., 3.];
        saved.poses[1].position = [4., 5., 6.];
        let restored: scene::Definition =
            toml::from_str(&toml::to_string(&saved).unwrap()).unwrap();
        scene.apply(&restored);
        assert_eq!(scene.parts[0].pose.position, [1., 2., 3.]);
        assert_eq!(scene.parts[1].pose.position, [4., 5., 6.]);
        let standalone = scene::Scene::load(
            &root,
            std::slice::from_ref(&item),
            "source layout".into(),
            false,
        )
        .unwrap();
        assert_eq!(standalone.parts.len(), 2);
        let mut lo = glam::Vec3::splat(f32::INFINITY);
        let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
        for p in &standalone.parts {
            for v in &p.mesh.vertices {
                let v = p.pose.matrix(0.).transform_point3(v.position.into());
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        assert!(
            (lo - source.min).length() < 0.001 && (hi - source.max).length() < 0.001,
            "Standalone preserves the original file layout"
        );
        let double = catalog::Bom { qty: 2, ..item };
        let two_sets = scene::Scene::load(&root, &[double], "2 copies".into(), true).unwrap();
        assert_eq!(two_sets.parts.len(), 4);
        assert_eq!(
            bytes_before,
            std::fs::read(root.join(path)).unwrap(),
            "Source file remains byte-for-byte unchanged"
        );
    }
}

#[cfg(test)]
mod constraint_integration_tests {
    use super::*;
    #[test]
    fn standard_drawer_constraints_follow_and_reduce_facade_crossings() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let catalog = catalog::Catalog::scan(root.into()).unwrap();
        let bom = catalog.bom(&catalog::Selection::default());
        let mut s = scene::Scene::load(&catalog.root, &bom, "S3".into(), true).unwrap();
        assert_eq!(s.relations.len(), s.parts.len() - 1);
        let facade = s
            .parts
            .iter()
            .position(|p| p.pose.path.contains("/Facades/"))
            .unwrap();
        let springs: Vec<_> = s
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.pose.path.ends_with("/Springs.stl"))
            .map(|(i, _)| i)
            .collect();
        let positions: Vec<_> = s.parts.iter().map(|p| p.pose.position).collect();
        let drawer = s
            .parts
            .iter()
            .position(|p| p.pose.path.contains("/Boxes/"))
            .unwrap();
        for relation in &mut s.relations {
            if relation.child == constraints::PartId::of(&s.parts[drawer])
                && let constraints::Joint::Slider { offset, .. } = &mut relation.joint
            {
                *offset = 25.;
            }
        }
        constraints::solve(&mut s).unwrap();
        for i in [drawer, facade, springs[0], springs[1]] {
            assert!((s.parts[i].pose.position[2] - positions[i][2] - 25.).abs() < 0.002);
        }
        let guide = s
            .parts
            .iter()
            .position(|p| p.pose.path.contains("Runner Guide"))
            .unwrap();
        assert_eq!(s.parts[guide].pose.position, positions[guide]);
        let slide = s
            .parts
            .iter()
            .position(|p| p.pose.path.contains("Runner Slide"))
            .unwrap();
        assert!((s.parts[slide].pose.position[2] - positions[slide][2] - 25.).abs() < 0.002);
        let encoded = toml::to_string_pretty(&s.definition()).unwrap();
        let decoded: scene::Definition = toml::from_str(&encoded).unwrap();
        let mut restored = scene::Scene::load(&catalog.root, &bom, "S3".into(), true).unwrap();
        restored.apply(&decoded);
        assert_eq!(restored.relations.len(), s.relations.len());
        assert!(
            (restored.parts[facade].pose.position[2] - s.parts[facade].pose.position[2]).abs()
                < 0.002
        );
        let base = scene::Scene::load(&catalog.root, &bom, "S3".into(), true).unwrap();
        let mut previous = base.clone();
        previous.relations.clear();
        let module = previous
            .parts
            .iter()
            .position(|p| p.pose.path.contains("/Modules/"))
            .unwrap();
        let depth = previous.parts[module].source_size.z;
        let delta = depth * 0.52 - previous.parts[facade].pose.position[2];
        previous.parts[facade].pose.position[2] += delta;
        for part in &mut previous.parts {
            let name = part.pose.path.rsplit('/').next().unwrap();
            if name == "Springs.stl" {
                part.pose.position[2] = depth * 0.53;
            } else if name.contains("Key") || name == "Label.stl" || name == "Facade Cap.stl" {
                part.pose.position[2] += delta;
            }
        }
        let before = collision::check(&previous, 0.01).unwrap();
        println!(
            "Before basic relations: {} crossing pairs",
            before.pairs.len()
        );
        let report = collision::check(&base, 0.01).unwrap();
        assert!(report.is_current(&base));
        for p in &report.pairs {
            if p.a == facade || p.b == facade {
                println!(
                    "{} / {} {:?}",
                    base.parts[p.a].label(false),
                    base.parts[p.b].label(false),
                    base.parts[facade]
                        .pose
                        .matrix(0.)
                        .inverse()
                        .transform_point3(p.point)
                );
            }
        }
        let mut modified = base.clone();
        modified.parts[0].pose.position[0] += 1.;
        assert!(!report.is_current(&modified));
        println!(
            "S3 relations: {}; remaining crossing pairs: {}",
            base.relations.len(),
            report.pairs.len()
        );
        for pair in &report.pairs {
            println!(
                "{} / {}",
                base.parts[pair.a].label(false),
                base.parts[pair.b].label(false)
            );
            if pair.a == facade || pair.b == facade {
                println!(
                    "facade-local {:?}",
                    base.parts[facade]
                        .pose
                        .matrix(0.)
                        .inverse()
                        .transform_point3(pair.point)
                );
            }
        }
    }
}

#[cfg(test)]
mod contact_audit {
    use super::*;
    #[test]
    #[ignore]
    fn dump_contact_regions() {
        let c = catalog::Catalog::scan(std::env::var("VAULT_MODEL_ROOT").unwrap().into()).unwrap();
        let scene =
            scene::Scene::load(&c.root, &c.bom(&Default::default()), "audit".into(), true).unwrap();
        let report = collision::check(&scene, 0.01).unwrap();
        let mut out = String::new();
        for pair in &report.pairs {
            let hits = collision::audit_pair(&scene, pair.a, pair.b);
            out += &format!(
                "{} / {}: {} triangle crossings\n",
                scene.parts[pair.a].label(false),
                scene.parts[pair.b].label(false),
                hits.len()
            );
            for i in [pair.a, pair.b] {
                let inv = scene.parts[i].pose.matrix(0.).inverse();
                let mut lo = glam::Vec3::splat(f32::INFINITY);
                let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
                for &p in &hits {
                    let p = inv.transform_point3(p);
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                out += &format!(
                    "  {} local bounds {:?} .. {:?}\n",
                    scene.parts[i].label(false),
                    lo,
                    hi
                );
            }
        }
        std::fs::create_dir_all("output").unwrap();
        std::fs::write("output/contact-audit.txt", &out).unwrap();
        println!("{out}");
    }
}
