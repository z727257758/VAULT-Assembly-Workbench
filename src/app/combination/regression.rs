//! Behavioral assembly regressions. Library tests are explicit opt-in and never
//! write into the external model library.
use super::*;
use crate::constraints::{PartId, stacking};
use glam::Vec3;
use std::{
    collections::HashSet,
    sync::{Arc, atomic::AtomicBool},
};
fn member(
    name: &str,
    width: &str,
    depth: u8,
    height: &str,
    is_box: bool,
    column: u32,
    level: u32,
) -> Member {
    Member {
        exterior: Default::default(),
        name: name.into(),
        placement: Placement {
            column,
            level,
            offset: [0.; 3],
        },
        preset: Preset {
            selection: Selection {
                width: width.into(),
                depth,
                height: height.into(),
                hidden_key: false,
            },
            assembly_kind: if is_box {
                AssemblyKind::Sustainer
            } else {
                AssemblyKind::Drawer
            },
            bom: vec![],
            sustainer_options: Default::default(),
            sustainer_angle: 0.,
            gridfinity: Default::default(),
            divider_layout: Default::default(),
            sustainer_dividers: Default::default(),
            label: Default::default(),
            print_config: Some(Default::default()),
        },
    }
}
fn snapshot(members: &[Member]) -> Vec<String> {
    members
        .iter()
        .map(|m| toml::to_string(m).unwrap())
        .collect()
}
fn project(members: Vec<Member>) -> CombinationPreset {
    CombinationPreset {
        preset: members[0].preset.clone(),
        members,
        active_member: 0,
        combination_gap: 0.,
    }
}
fn layout(members: &[Member]) -> stacking::Layout {
    stacking::plan(&modules(members), 0.).unwrap()
}
fn pair() -> Vec<Member> {
    vec![
        member("base", "L", 3, "1F", true, 0, 0),
        member("left", "S", 3, "2F", false, 0, 1),
        member("right", "S", 3, "2F", true, 1, 1),
        member("neighbor", "M", 4, "4F", false, 2, 0),
    ]
}
#[test]
fn same_footprint_matrix_all_standard_sizes_heights_and_types() {
    let mut cases = 0;
    for w in ["S", "M", "L", "XL"] {
        let heights = if w == "XL" {
            &["1F"][..]
        } else {
            &["1F", "2F", "4F"][..]
        };
        for depth in 3..=7 {
            for lower in heights {
                for upper in heights {
                    for mask in 0..4 {
                        let mut m = vec![
                            member("lower", w, depth, lower, mask & 1 != 0, 0, 0),
                            member("upper", w, depth, upper, mask & 2 != 0, 0, 1),
                        ];
                        project(m.clone()).validate(&Catalog::default()).unwrap();
                        let plan = layout(&m);
                        assert_eq!(plan.supports[1], vec![0]);
                        assert_eq!(plan.left[0], plan.left[1]);
                        m.reverse();
                        let reversed = layout(&m);
                        assert_eq!(reversed.supports[0], vec![1]);
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 560);
}
#[test]
fn documented_pair_matrix_both_directions_and_both_xl_orders() {
    let mut cases = 0;
    for (wide, a, b) in [("L", "S", "S"), ("XL", "S", "M"), ("XL", "M", "S")] {
        for depth in 3..=7 {
            for mask in 0..8 {
                for wide_below in [true, false] {
                    let (wl, pl) = if wide_below { (0, 1) } else { (1, 0) };
                    let m = vec![
                        member("wide", wide, depth, "1F", mask & 1 != 0, 0, wl),
                        member("a", a, depth, "2F", mask & 2 != 0, 0, pl),
                        member("b", b, depth, "2F", mask & 4 != 0, 1, pl),
                    ];
                    let p = layout(&m);
                    if wide_below {
                        assert_eq!(p.supports[1], vec![0]);
                        assert_eq!(p.supports[2], vec![0]);
                    } else {
                        assert_eq!(p.supports[0], vec![1, 2]);
                    }
                    assert!((p.left[2] - stacking::width(a).unwrap() as f32 / 10.).abs() < 0.001);
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 240);
}
#[test]
fn three_tier_bridge_requires_equal_physical_support_heights() {
    let mut m = pair();
    m.push(member("bridge", "L", 3, "1F", false, 0, 2));
    layout(&m);
    m[1].preset.selection.height = "1F".into();
    m[2].preset.selection.height = "1F".into();
    assert!(
        validate_layout(&m, 0.)
            .unwrap_err()
            .contains("Support heights differ")
    );
}
#[test]
fn mismatch_matrix_rejects_every_different_depth_without_mutation() {
    for lower in 3..=7 {
        for upper in 3..=7 {
            if lower == upper {
                continue;
            }
            let m = vec![
                member("lower", "S", lower, "2F", true, 0, 0),
                member("upper", "S", upper, "1F", false, 0, 1),
            ];
            let before = snapshot(&m);
            assert!(validate_layout(&m, 0.).unwrap_err().contains("depth"));
            assert_eq!(snapshot(&m), before);
        }
    }
}
#[test]
fn member_limit_boundary_and_invalid_saved_metadata() {
    let m: Vec<_> = (0..64)
        .map(|i| member(&format!("m{i}"), "S", 3, "1F", false, i, 0))
        .collect();
    let valid = project(m);
    valid.validate(&Catalog::default()).unwrap();
    let mut too_many = project(valid.members.clone());
    too_many
        .members
        .push(member("extra", "S", 3, "1F", true, 0, 1));
    assert!(too_many.validate(&Catalog::default()).is_err());
    for case in 0..7 {
        let mut p = project(valid.members.clone());
        match case {
            0 => p.active_member = 64,
            1 => p.combination_gap = 1.,
            2 => p.members[0].placement.offset[0] = f32::NAN,
            3 => p.members[0].preset.selection.width = "unknown".into(),
            4 => p.members[0].preset.selection.depth = 8,
            5 => {
                p.members[0].preset.selection.width = "XL".into();
                p.members[0].preset.selection.height = "2F".into();
            }
            _ => p.members[0].name = String::new(),
        };
        assert!(p.validate(&Catalog::default()).is_err(), "case {case}");
    }
}
#[test]
fn pair_detach_reattach_does_not_drift_unrelated_neighbors() {
    let original = pair();
    let old = layout(&original);
    let mut current = original.clone();
    for _ in 0..8 {
        current = position_relative(&current, &[1, 2], 0, Side::Ground).unwrap();
        current = position_relative(&current, &[1, 2], 0, Side::Above).unwrap();
        let p = layout(&current);
        assert_eq!(p.left, old.left);
        assert_eq!(p.supports, old.supports);
    }
}
#[test]
fn pair_identity_order_follows_space_not_serialization_order() {
    let m = vec![
        member("wide", "XL", 3, "1F", true, 0, 0),
        member("right", "M", 3, "2F", false, 1, 1),
        member("left", "S", 3, "2F", true, 0, 1),
    ];
    assert_eq!(paired_members(&m, 1), vec![2, 1]);
    let placed = position_relative(&m, &paired_members(&m, 1), 0, Side::Ground).unwrap();
    let p = layout(&placed);
    assert!(p.left[2] < p.left[1]);
}
#[test]
fn invalid_spatial_requests_are_atomic_including_duplicate_selection() {
    let m = vec![
        member("a", "S", 3, "1F", false, 0, 0),
        member("b", "S", 3, "1F", true, 1, 0),
    ];
    let old = snapshot(&m);
    for (ids, target) in [
        (vec![], 0),
        (vec![1, 1], 0),
        (vec![0], 0),
        (vec![7], 0),
        (vec![1], 8),
    ] {
        assert!(
            position_relative(&m, &ids, target, Side::Right).is_err(),
            "{ids:?} / {target}"
        );
        assert_eq!(snapshot(&m), old);
    }
}
#[test]
fn removing_support_and_half_pairs_is_rejected_but_whole_pair_removal_is_valid() {
    let m = pair();
    assert!(validate_layout(&m[1..], 0.).is_err());
    let half = vec![m[0].clone(), m[1].clone(), m[3].clone()];
    assert!(validate_layout(&half, 0.).is_err());
    let whole = vec![m[0].clone(), m[3].clone()];
    validate_layout(&whole, 0.).unwrap();
    assert!(position_relative(&m, &[0], 3, Side::Right).is_err());
}
#[test]
fn side_additions_preserve_labels_colors_and_neighbor_stack() {
    for side in [Side::Left, Side::Right] {
        let mut m = vec![
            member("base", "S", 3, "2F", false, 0, 0),
            member("top", "S", 3, "1F", true, 0, 1),
            member("new", "M", 5, "4F", true, 1, 0),
        ];
        m[0].preset.label.text = "BASE".into();
        m[1].preset.label.text = "TOP".into();
        m[1].preset
            .print_config
            .as_mut()
            .unwrap()
            .set_part_slot(crate::label::SUSTAINER_PATH, 3);
        let next = position_relative(&m, &[2], 0, side).unwrap();
        let p = layout(&next);
        assert_eq!(p.supports[1], vec![0]);
        for i in 0..m.len() {
            assert_eq!(
                toml::to_string(&next[i].preset).unwrap(),
                toml::to_string(&m[i].preset).unwrap()
            );
        }
        if side == Side::Left {
            assert!(p.left[2] < p.left[0]);
        } else {
            assert!(p.left[2] > p.left[0]);
        }
    }
}
#[test]
fn full_project_roundtrip_keeps_pair_order_and_independent_configuration() {
    let mut m = pair();
    m[1].preset.label.text = "A".into();
    m[2].preset.sustainer_angle = 65.;
    m[2].preset
        .print_config
        .as_mut()
        .unwrap()
        .set_part_slot(crate::label::SUSTAINER_PATH, 4);
    let expected = snapshot(&m);
    let mut p = project(m);
    p.active_member = 2;
    let restored: CombinationPreset = toml::from_str(&toml::to_string_pretty(&p).unwrap()).unwrap();
    restored.validate(&Catalog::default()).unwrap();
    assert_eq!(restored.active_member, 2);
    assert_eq!(snapshot(&restored.members), expected);
    assert_eq!(layout(&restored.members).supports[1], vec![0]);
}
#[test]
fn cancelled_load_and_invalid_layout_fail_before_any_source_file_read() {
    let m = vec![member("cancelled", "S", 3, "1F", false, 0, 0)];
    let cancel = Arc::new(AtomicBool::new(true));
    let home = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        load(Path::new("missing-test-library"), home, &m, 0., &cancel)
            .err()
            .unwrap()
            .contains("取消")
    );
    let mut floating = m;
    floating[0].placement.level = 1;
    assert!(
        load(
            Path::new("missing-test-library"),
            home,
            &floating,
            0.,
            &cancel
        )
        .err()
        .unwrap()
        .contains("support")
    );
}
fn library() -> (PathBuf, Catalog, PathBuf) {
    let root = PathBuf::from(
        std::env::var("VAULT_MODEL_ROOT").expect("Set VAULT_MODEL_ROOT to the external library"),
    );
    let catalog = Catalog::scan(root.clone()).unwrap();
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    (root, catalog, home)
}
fn configure(m: &mut [Member], catalog: &Catalog) {
    for member in m {
        let p = &mut member.preset;
        p.bom = if p.assembly_kind == AssemblyKind::Sustainer {
            catalog
                .sustainer_bom(&p.selection, p.sustainer_options)
                .unwrap()
        } else {
            catalog.bom(&p.selection)
        };
    }
}
fn assert_scene(scene: &Scene, count: usize) {
    assert!(!scene.calibrated);
    assert_eq!(scene.member_bounds.len(), count);
    let ids: HashSet<_> = scene.parts.iter().map(PartId::of).collect();
    assert_eq!(ids.len(), scene.parts.len());
    for r in &scene.relations {
        assert!(ids.contains(&r.parent) && ids.contains(&r.child));
        assert_eq!(
            r.parent.instance / MEMBER_STRIDE,
            r.child.instance / MEMBER_STRIDE
        );
    }
    for (lo, hi) in &scene.member_bounds {
        assert!(lo.is_finite() && hi.is_finite());
        assert!((*hi - *lo).min_element() > 0.);
    }
    let mut solved = scene.clone();
    crate::constraints::solve(&mut solved).unwrap();
    for (a, b) in scene.parts.iter().zip(&solved.parts) {
        assert!((Vec3::from(a.pose.position) - Vec3::from(b.pose.position)).length() < 0.02);
    }
}
#[test]
#[ignore = "requires read-only VAULT_MODEL_ROOT library"]
fn library_combination_matrix_seats_graphs_and_closed_dimensions() {
    let (root, catalog, home) = library();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut report = String::new();
    let mut cases = [
        vec![
            member("drawer", "S", 3, "1F", false, 0, 0),
            member("box", "S", 3, "2F", true, 0, 1),
        ],
        vec![
            member("box", "M", 4, "2F", true, 0, 0),
            member("drawer", "M", 4, "4F", false, 0, 1),
        ],
        vec![
            member("wide", "L", 3, "1F", true, 0, 0),
            member("a", "S", 3, "2F", true, 0, 1),
            member("b", "S", 3, "2F", false, 1, 1),
        ],
        vec![
            member("a", "S", 3, "2F", true, 0, 0),
            member("b", "S", 3, "2F", false, 1, 0),
            member("wide", "L", 3, "1F", true, 0, 1),
        ],
        vec![
            member("wide", "XL", 3, "1F", true, 0, 0),
            member("a", "S", 3, "2F", false, 0, 1),
            member("b", "M", 3, "2F", true, 1, 1),
        ],
        vec![
            member("a", "M", 3, "2F", true, 0, 0),
            member("b", "S", 3, "2F", false, 1, 0),
            member("wide", "XL", 3, "1F", true, 0, 1),
        ],
    ];
    for (case, m) in cases.iter_mut().enumerate() {
        configure(m, &catalog);
        let before = snapshot(m);
        let scene =
            load(&root, &home, m, 0., &cancel).unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert_scene(&scene, m.len());
        assert_eq!(snapshot(m), before);
        let plan = layout(m);
        let parts: Vec<_> = (0..m.len())
            .map(|i| Scene {
                parts: scene
                    .parts
                    .iter()
                    .filter(|p| p.pose.instance / MEMBER_STRIDE == i as u32)
                    .cloned()
                    .collect(),
                ..Default::default()
            })
            .collect();
        let datums: Vec<_> = parts.iter().map(|s| stacking::datum(s).unwrap()).collect();
        for (i, below) in plan.supports.iter().enumerate() {
            for &j in below {
                assert!(
                    (datums[i].bottom - datums[j].top).abs() < 0.26,
                    "case {case}: {i} on {j}"
                );
                assert!((datums[i].rear - datums[j].rear).abs() < 0.02);
            }
        }
        report.push_str(&format!("PASS case {case}: {} members, {} parts; source seating and internal graph consistent\n",m.len(),scene.parts.len()));
    }
    std::fs::create_dir_all(home.join("output/combination-regression")).unwrap();
    std::fs::write(
        home.join("output/combination-regression/library-matrix.txt"),
        report,
    )
    .unwrap();
}
#[test]
#[ignore = "requires read-only VAULT_MODEL_ROOT library"]
fn library_supporting_lid_closes_without_changing_saved_angles_or_dimensions() {
    let (root, catalog, home) = library();
    let mut m = vec![
        member("base", "S", 3, "1F", true, 0, 0),
        member("top", "S", 3, "1F", true, 0, 1),
    ];
    configure(&mut m, &catalog);
    let cancel = Arc::new(AtomicBool::new(false));
    let closed = load(&root, &home, &m, 0., &cancel).unwrap();
    m[0].preset.sustainer_angle = 65.;
    m[1].preset.sustainer_angle = 35.;
    let before = snapshot(&m);
    let opened = load(&root, &home, &m, 0., &cancel).unwrap();
    assert_scene(&opened, 2);
    assert_eq!(snapshot(&m), before);
    for (a, b) in closed.member_bounds.iter().zip(&opened.member_bounds) {
        assert!((a.0 - b.0).length() < 0.01 && (a.1 - b.1).length() < 0.01);
    }
    let angles: Vec<_> = opened
        .relations
        .iter()
        .filter(|r| r.label.contains(crate::sustainer::LID))
        .map(|r| match r.joint {
            crate::constraints::Joint::Hinge { angle, .. } => {
                (r.child.instance / MEMBER_STRIDE, angle)
            }
            _ => panic!("lid must be hinged"),
        })
        .collect();
    assert!(angles.contains(&(0, 0.)));
    assert!(
        angles
            .iter()
            .any(|&(i, a)| i == 1 && (a - 35.).abs() < 0.01)
    );
}
#[test]
#[ignore = "requires read-only VAULT_MODEL_ROOT library and large-mesh memory"]
fn library_large_xl_combination_reports_calibration_not_obsolete_size_limit() {
    let (root, catalog, home) = library();
    let mut m = vec![
        member("small", "S", 3, "1F", false, 0, 0),
        member("XL drawer", "XL", 3, "1F", false, 1, 0),
    ];
    configure(&mut m, &catalog);
    let before = snapshot(&m);
    let cancel = Arc::new(AtomicBool::new(false));
    let error = load(&root, &home, &m, 0., &cancel)
        .err()
        .expect("XL drawer seating must remain explicitly unsupported until calibrated");
    assert!(error.contains("orientation is unavailable"), "{error}");
    assert!(!error.contains("250 MB"));
    assert_eq!(snapshot(&m), before);
}

#[test]
fn exterior_dependencies_and_roundtrip() {
    use crate::catalog::exterior::*;
    let s = Selection::default();
    let mut e = Exterior {
        stationary_handle: true,
        ..Default::default()
    };
    assert!(e.validate(&s, false).is_err());
    e.backbone = true;
    assert!(e.validate(&s, false).is_ok());
    e.board = Board::Ikea;
    assert!(e.validate(&s, false).is_err());
    e.board = Board::None;
    e.stationary_handle = false;
    e.pull_handle = true;
    e.pull_extension = f32::NAN;
    assert!(e.validate(&s, false).is_err());
    e.pull_extension = 250.;
    let mut m = member("configured", "S", 3, "1F", false, 0, 0);
    m.exterior = e;
    let restored: Member = toml::from_str(&toml::to_string(&m).unwrap()).unwrap();
    assert_eq!(restored.exterior, m.exterior);
}
#[test]
fn exterior_moves_and_roof_obstructions_are_atomic() {
    use crate::catalog::exterior::*;
    let mut m = vec![
        member("base", "S", 3, "1F", false, 0, 0),
        member("top", "S", 3, "1F", false, 1, 0),
    ];
    m[1].exterior.base = Base::Plain;
    let old = snapshot(&m);
    assert!(position_relative(&m, &[1], 0, Side::Above).is_err());
    assert_eq!(snapshot(&m), old);
    m[1].exterior.base = Base::None;
    m[0].exterior.roof = Roof::Table;
    assert!(position_relative(&m, &[1], 0, Side::Above).is_err());
    m[0].exterior.roof = Roof::None;
    assert!(position_relative(&m, &[1], 0, Side::Above).is_ok());
}
#[test]
fn exterior_missing_sources_and_case_ambiguity_are_rejected() {
    use crate::catalog::exterior::*;
    let e = Exterior {
        base: Base::Plain,
        ..Default::default()
    };
    let s = Selection::default();
    assert!(
        Catalog::default()
            .exterior_components(&s, &e, false)
            .is_err()
    );
    let path = "03. MODIFICATIONS/Base/S/S3 Base.stl";
    let mut c = Catalog {
        entries: vec![crate::catalog::Entry {
            path: path.into(),
            name: "S3 Base.stl".into(),
            category: 2,
        }],
        ..Default::default()
    };
    assert_eq!(c.exterior_components(&s, &e, false).unwrap().len(), 1);
    c.entries.push(crate::catalog::Entry {
        path: path.to_lowercase(),
        name: "duplicate".into(),
        category: 2,
    });
    assert!(c.exterior_components(&s, &e, false).is_err());
}
#[test]
#[ignore = "requires read-only VAULT_MODEL_ROOT"]
fn library_exterior_sources_orientations_and_assemblies() {
    use crate::catalog::exterior::*;
    let (root, catalog, home) = library();
    let mut report = String::new();
    for width in ["S", "M", "L", "XL"] {
        for depth in 3..=7 {
            let s = Selection {
                width: width.into(),
                depth,
                ..Default::default()
            };
            for roof in [
                Roof::None,
                Roof::Grid,
                Roof::Table,
                Roof::Bowl,
                Roof::UnderTable,
            ] {
                let e = Exterior {
                    base: if roof == Roof::None {
                        Base::Plain
                    } else {
                        Base::None
                    },
                    roof,
                    ..Default::default()
                };
                let parts = catalog.exterior_components(&s, &e, false).unwrap();
                let raw = Scene::load(
                    &root,
                    &parts.iter().map(|c| c.bom.clone()).collect::<Vec<_>>(),
                    "orientation".into(),
                    false,
                )
                .unwrap();
                for p in &raw.parts {
                    let q = crate::constraints::exterior::horizontal_orientation(
                        p,
                        stacking::width(width).unwrap() as f32 / 10.,
                    )
                    .unwrap();
                    assert!((q.length() - 1.).abs() < 0.0001);
                    report.push_str(&format!("PASS {}: {:?}\n", p.pose.path, q));
                }
            }
        }
    }
    let cancel = Arc::new(AtomicBool::new(false));
    for case in 0..5 {
        let mut m = vec![
            member("bottom", "S", 3, "2F", false, 0, 0),
            member("top", "S", 3, "2F", false, 0, 1),
        ];
        m[0].exterior = Exterior {
            base: if case == 0 {
                Base::Trolley
            } else {
                Base::Plain
            },
            backbone: case == 0,
            ..Default::default()
        };
        m[1].exterior = match case {
            0 => Exterior {
                backbone: true,
                stationary_handle: false,
                pull_handle: true,
                pull_extension: 150.,
                roof: Roof::Grid,
                top_handle: true,
                ..Default::default()
            },
            1 => Exterior {
                backbone: true,
                board: Board::Ikea,
                roof: Roof::Table,
                ..Default::default()
            },
            2 => Exterior {
                backbone: true,
                board: Board::Ussa,
                roof: Roof::Bowl,
                ..Default::default()
            },
            3 => Exterior {
                wall_mount: true,
                ..Default::default()
            },
            _ => Exterior {
                roof: Roof::UnderTable,
                ..Default::default()
            },
        };
        configure(&mut m, &catalog);
        let scene =
            load(&root, &home, &m, 0., &cancel).unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert_scene(&scene, 2);
        assert!(
            scene
                .relations
                .iter()
                .any(|r| r.label.contains("Exterior estimate:"))
        );
        assert!(scene.member_bounds[0].0.y.abs() < 0.02);
        report.push_str(&format!(
            "PASS assembly {case}: {} parts\n",
            scene.parts.len()
        ));
    }
    std::fs::create_dir_all(home.join("output/assembly-accessories-review")).unwrap();
    std::fs::write(
        home.join("output/assembly-accessories-review/regression.txt"),
        report,
    )
    .unwrap();
}

#[test]
fn exterior_invalid_combinations_and_legacy_member_defaults() {
    use crate::catalog::exterior::*;
    let s = Selection::default();
    for e in [
        Exterior {
            top_handle: true,
            ..Default::default()
        },
        Exterior {
            wall_mount: true,
            base: Base::Plain,
            ..Default::default()
        },
        Exterior {
            backbone: true,
            pull_handle: true,
            stationary_handle: true,
            ..Default::default()
        },
        Exterior {
            backbone: true,
            pull_handle: true,
            board: Board::Ikea,
            ..Default::default()
        },
        Exterior {
            pull_extension: 801.,
            ..Default::default()
        },
    ] {
        assert!(e.validate(&s, false).is_err());
    }
    assert!(
        Exterior {
            roof: Roof::Grid,
            ..Default::default()
        }
        .validate(&s, true)
        .is_err()
    );
    let m = member("legacy", "S", 3, "1F", false, 0, 0);
    let mut value = toml::Value::try_from(&m).unwrap();
    value.as_table_mut().unwrap().remove("exterior");
    let restored: Member = value.try_into().unwrap();
    assert!(restored.exterior.is_empty());
}
#[test]
fn exterior_follows_moves_and_preserves_member_interior() {
    use crate::catalog::exterior::*;
    let mut m = vec![
        member("a", "S", 3, "2F", false, 0, 0),
        member("b", "M", 3, "2F", false, 1, 0),
    ];
    m[0].exterior = Exterior {
        backbone: true,
        stationary_handle: true,
        base: Base::Plain,
        ..Default::default()
    };
    m[0].preset.label.text = "KEEP".into();
    let moved = position_relative(&m, &[0], 1, Side::Right).unwrap();
    assert_eq!(moved[0].exterior, m[0].exterior);
    assert_eq!(
        toml::to_string(&moved[0].preset).unwrap(),
        toml::to_string(&m[0].preset).unwrap()
    );
    let restored = position_relative(&moved, &[0], 1, Side::Left).unwrap();
    assert_eq!(restored[0].exterior, m[0].exterior);
}
#[test]
#[ignore = "requires read-only VAULT_MODEL_ROOT"]
fn library_exterior_spacing_and_export_regression() {
    use crate::catalog::exterior::*;
    let (root, catalog, home) = library();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut m = vec![
        member("trolley", "S", 3, "2F", false, 0, 0),
        member("neighbor", "S", 3, "2F", false, 1, 0),
    ];
    m[0].exterior.base = Base::Trolley;
    configure(&mut m, &catalog);
    let scene = load(&root, &home, &m, 0., &cancel).unwrap();
    assert!(scene.member_bounds[0].1.x <= scene.member_bounds[1].0.x + 0.02);
    let old = snapshot(&m);
    for width in ["S", "M", "L", "XL"] {
        let mut one = vec![member("handle", width, 3, "1F", true, 0, 0)];
        one[0].exterior = Exterior {
            base: Base::Plain,
            backbone: true,
            pull_handle: true,
            pull_extension: 0.,
            ..Default::default()
        };
        configure(&mut one, &catalog);
        let scene = load(&root, &home, &one, 0., &cancel).unwrap();
        assert_scene(&scene, 1);
        let rods: Vec<_> = scene
            .parts
            .iter()
            .filter(|p| p.pose.path.starts_with("@hardware/"))
            .collect();
        assert_eq!(rods.len(), 2);
        assert!((rods[0].mesh.size.x - 16.).abs() < 0.001);
        let lock = scene
            .parts
            .iter()
            .find(|p| p.pose.path.ends_with(" Pull-out Handle Lock.stl"))
            .unwrap();
        let (_, hi) = crate::constraints::world_bounds(lock);
        for rod in rods {
            assert!((rod.pose.position[2] - (hi.z - 19.)).abs() < 0.05);
            assert!(
                (rod.pose.position[0]
                    - (scene.member_bounds[0].0.x + stacking::width(width).unwrap() as f32 / 20.))
                    .abs()
                    > 5.
            );
        }
    }
    assert_eq!(snapshot(&m), old);
    // A small source-only export exercises the real packaging path without duplicating the library.
    m.truncate(1);
    m[0].preset
        .bom
        .retain(|b| b.path.ends_with("/Facade Cap.stl"));
    m[0].exterior = Exterior {
        backbone: true,
        stationary_handle: true,
        ..Default::default()
    };
    let out = home.join("output/assembly-accessories-review");
    std::fs::create_dir_all(&out).unwrap();
    let temp = tempfile::tempdir_in(&out).unwrap();
    let saved = project(m);
    let target = temp.path().join("export");
    export_members(&root, &home, &target, crate::export::Format::Stl, &saved).unwrap();
    assert!(target.join("hardware.txt").is_file());
    let restored: CombinationPreset =
        toml::from_str(&std::fs::read_to_string(target.join("combination.toml")).unwrap()).unwrap();
    assert_eq!(restored.members[0].exterior, saved.members[0].exterior);
    let mut files = Vec::new();
    fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                collect(&p, files)
            } else {
                files.push(p)
            }
        }
    }
    collect(&target, &mut files);
    assert!(files.iter().any(|p| {
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .contains("Stationary Handle")
    }));
    assert!(
        !files
            .iter()
            .any(|p| p.to_string_lossy().contains("@hardware"))
    );
}

#[test]
fn pull_stroke_respects_tower_height_and_lid_clearance() {
    use crate::catalog::exterior::Exterior;
    let mut m = vec![member("short", "S", 3, "1F", false, 0, 0)];
    m[0].exterior = Exterior {
        backbone: true,
        pull_handle: true,
        pull_extension: 30.,
        ..Default::default()
    };
    assert!(validate_layout(&m, 0.).unwrap_err().contains("stroke"));
    m[0].exterior.pull_extension = 0.;
    validate_layout(&m, 0.).unwrap();
    m[0].exterior = Default::default();
    m[0].preset.selection.height = "2F".into();
    m.push(member("middle", "S", 3, "2F", false, 0, 1));
    m.push(member("top", "S", 3, "2F", false, 0, 2));
    m[2].exterior = Exterior {
        backbone: true,
        pull_handle: true,
        pull_extension: 180.,
        ..Default::default()
    };
    validate_layout(&m, 0.).unwrap();
    m[2].exterior.pull_extension = 260.;
    assert!(validate_layout(&m, 0.).is_err());
}
