use super::*;
use crate::scene::{MEMBER_STRIDE, Placement};

#[test]
fn exterior_parts_do_not_change_the_authoritative_assembly_kind() {
    let mut scene = drawer();
    scene.parts.truncate(1);
    scene.parts[0].pose.path = "02. SUSTAINERS/S3/Modules/S3 Module.stl".into();
    let mut extra = scene.parts[0].clone();
    extra.pose.path = "01. DRAWERS/Universal/Backplate.stl".into();
    scene.parts.push(extra);
    assert_eq!(AssemblyRules::family(&scene), AssemblyKind::Sustainer);
    assert!(crate::sustainer::is_scene(&scene));
    scene.parts.remove(0);
    assert_eq!(AssemblyRules::family(&scene), AssemblyKind::Drawer);
    assert!(!crate::sustainer::is_scene(&scene));
}

#[test]
fn support_and_pull_limits_use_the_same_nominal_heights() {
    let selection = crate::catalog::Selection::default();
    let modules = [
        stacking::Module {
            name: "box",
            selection: &selection,
            placement: Placement::default(),
            is_box: true,
        },
        stacking::Module {
            name: "drawer",
            selection: &selection,
            placement: Placement {
                level: 1,
                ..Default::default()
            },
            is_box: false,
        },
    ];
    let layout =
        AssemblyRules::layout(&modules, &[Default::default(), Default::default()], 0.).unwrap();
    assert_eq!(layout.nominal_tops, vec![61., 108.]);
    assert_eq!(AssemblyRules::pull_limit(&modules, &layout, 1), 70.);
    for (height, expected) in [("2F", 95.), ("4F", 191.)] {
        assert_eq!(
            AssemblyRules::installation_height(height, false).unwrap(),
            expected
        );
        assert_eq!(
            AssemblyRules::installation_height(height, true).unwrap(),
            expected
        );
    }
    assert!(AssemblyRules::installation_height("3F", false).is_err());
}

fn drawer() -> Scene {
    let mesh = std::sync::Arc::new(
        crate::mesh::Mesh::parse(
            b"solid a\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 120 30\nendsolid",
        )
        .unwrap(),
    );
    let parts = [
        "Modules/S3 1F Module.stl",
        "Boxes/S3 1F Box Drawer.stl",
        "Facades/S3 1F Facade.stl",
    ]
    .into_iter()
    .enumerate()
    .map(|(i, path)| Part {
        mesh: mesh.clone(),
        pose: Pose {
            path: format!("01. DRAWERS/{path}"),
            instance: 0,
            body: None,
            position: [0., 0., i as f32 * 10.],
            rotation: [0.; 3],
            travel: [4., 5., 6.],
            travel_frame: None,
        },
        color: [1.; 4],
        visible: true,
        source_size: mesh.size,
        source_bodies: 1,
    })
    .collect();
    let mut scene = Scene {
        parts,
        assembly: true,
        ..Default::default()
    };
    let mut slider = Relation::fixed(&scene, 0, 1, true).unwrap();
    if let Joint::Slider { min, max, .. } = &mut slider.joint {
        *min = 0.;
        *max = 100.;
    }
    slider.label = AssemblyRules::DRAWER_SLIDE.into();
    add(&mut scene, slider).unwrap();
    let facade = Relation::fixed(&scene, 1, 2, false).unwrap();
    add(&mut scene, facade).unwrap();
    scene
}
fn snapshot(scene: &Scene) -> String {
    toml::to_string(&scene.definition()).unwrap()
}

fn hinged_box(facade_path: &str) -> Scene {
    let mut scene = drawer();
    scene.relations.clear();
    for (part, path) in
        scene
            .parts
            .iter_mut()
            .zip(["Modules/Box Module.stl", "Roofs/Box Roof.stl", facade_path])
    {
        part.pose.path = format!("02. SUSTAINERS/{path}");
    }
    for path in ["inner.stl", "outer.stl"] {
        let mut part = scene.parts[0].clone();
        part.pose.path = format!("02. SUSTAINERS/{path}");
        scene.parts.push(part);
    }
    let mut lid = Relation::hinge(&scene, 0, 1, Vec3::ZERO, Vec3::X).unwrap();
    lid.label = crate::sustainer::LID.into();
    add(&mut scene, lid).unwrap();
    let facade = Relation::fixed(&scene, 0, 2, false).unwrap();
    add(&mut scene, facade).unwrap();
    for (parent, child, label) in [
        (2, 3, crate::sustainer::LOCK_INNER),
        (3, 4, crate::sustainer::LOCK_OUTER),
    ] {
        let mut r = Relation::hinge(&scene, parent, child, Vec3::ZERO, Vec3::X).unwrap();
        r.label = label.into();
        if let Joint::Hinge { min, max, .. } = &mut r.joint {
            *min = -180.;
            *max = 180.;
        }
        add(&mut scene, r).unwrap();
    }
    scene
}

#[test]
fn shared_shell_retains_independent_slider_axes_and_descendant_motion() {
    let mut scene =
        AssemblyRules::shared_shell(drawer(), vec![(drawer(), 1), (drawer(), 1)]).unwrap();
    rotate_subassembly(
        &mut scene,
        0,
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    )
    .unwrap();
    let mechanism = AssemblyRules::mechanism(
        &scene,
        crate::scene::Target {
            member: 0,
            drawer: Some(1),
        },
    )
    .unwrap();
    assert_eq!(mechanism.kind, MotionKind::Drawer);
    let before: Vec<_> = scene.parts.iter().map(|p| p.pose.matrix(0.)).collect();
    let plan = AssemblyRules::plan_motion(&scene, &mechanism.child, 50.).unwrap();
    AssemblyRules::apply_motion_sample(&mut scene, &mechanism.child, plan.sample(plan.duration))
        .unwrap();
    for (part, before) in scene.parts.iter().zip(before) {
        let delta =
            part.pose.matrix(0.).transform_point3(Vec3::ZERO) - before.transform_point3(Vec3::ZERO);
        let moving = part.pose.instance / super::super::tower::DRAWER_STRIDE == 2;
        assert!(delta.abs_diff_eq(if moving { Vec3::X * 50. } else { Vec3::ZERO }, 0.001));
    }
}

#[test]
fn telescoping_guides_follow_only_their_drawer_in_a_rotated_shared_shell() {
    let mut unit = drawer();
    let mut guide = unit.parts[1].clone();
    guide.pose.path = "01. DRAWERS/Universal/Runners/x3 Runner Guide.stl".into();
    let guide_id = PartId::of(&guide);
    unit.parts.push(guide);
    let drawer_id = PartId::of(&unit.parts[1]);
    // A nonidentity mounting rotation exercises conversion of the slider's
    // housing-local axis into the drawer's own frame.
    if let Joint::Slider { rotation, max, .. } = &mut unit.relations[0].joint {
        *rotation = Quat::from_rotation_y(1.1).to_array();
        *max = 180.;
    }
    add(
        &mut unit,
        Relation {
            parent: drawer_id,
            child: guide_id,
            joint: Joint::RunnerGuide {
                position: [24., 0., 3.],
                rotation: Quat::IDENTITY.to_array(),
                inner_travel: 55.,
                outer_travel: 80.,
            },
            label: "Middle runner".into(),
        },
    )
    .unwrap();
    let mut scene =
        AssemblyRules::shared_shell(unit.clone(), vec![(unit.clone(), 1), (unit, 1)]).unwrap();
    namespace_member(&mut scene, 2).unwrap();
    rotate_subassembly(
        &mut scene,
        0,
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    )
    .unwrap();
    let target = crate::scene::Target {
        member: 2,
        drawer: Some(1),
    };
    let mechanism = AssemblyRules::mechanism(&scene, target).unwrap();
    assert_eq!(mechanism.max, 135.);
    let closed: Vec<_> = scene
        .parts
        .iter()
        .map(|p| Vec3::from(p.pose.position))
        .collect();
    for total in [25., 55., 55.25, 135., 70., 45., 0.] {
        AssemblyRules::set_slider(&mut scene, &mechanism.child, total).unwrap();
        for (part, closed) in scene.parts.iter().zip(&closed) {
            let expected = if !target.contains(part) {
                0.
            } else if part.pose.path.contains("Runner Guide") {
                (total - 55.).max(0.)
            } else {
                total
            };
            assert!(
                (Vec3::from(part.pose.position) - closed).abs_diff_eq(Vec3::X * expected, 0.002)
            );
        }
    }
    let before = snapshot(&scene);
    assert!(AssemblyRules::set_slider(&mut scene, &mechanism.child, 135.1).is_err());
    assert_eq!(snapshot(&scene), before);
    // Direct graph edits must obey the same stops, even if the driver's own
    // configured range is wider. A failed solve leaves all poses untouched.
    let r = scene
        .relations
        .iter_mut()
        .find(|r| r.child == mechanism.child)
        .unwrap();
    if let Joint::Slider { offset, .. } = &mut r.joint {
        *offset = 140.;
    }
    assert!(solve(&mut scene).is_err());
    for (part, closed) in scene.parts.iter().zip(closed) {
        assert!(Vec3::from(part.pose.position).abs_diff_eq(closed, 0.002));
    }
}

#[test]
fn lid_path_releases_before_lifting_and_closes_before_latching_even_after_reversal() {
    let mut scene = hinged_box("Facade.stl");
    let lid = PartId::of(&scene.parts[1]);
    let root = scene.parts[0].pose.matrix(0.);
    let open = AssemblyRules::plan_motion(&scene, &lid, 90.).unwrap();
    let release = open.sample(open.release_time * 0.5);
    assert_eq!(release.value, 0.);
    assert!(release.latch > 0. && release.latch < 1.);
    AssemblyRules::apply_motion_sample(&mut scene, &lid, release).unwrap();
    let before = snapshot(&scene);
    assert!(
        AssemblyRules::apply_motion_sample(
            &mut scene,
            &lid,
            MotionSample {
                value: 1.,
                latch: 0.5
            }
        )
        .is_err()
    );
    assert_eq!(before, snapshot(&scene));
    let middle = open.sample(open.release_time + open.move_time * 0.5);
    assert!((middle.value - 45.).abs() < 0.001);
    assert_eq!(middle.latch, 1.);
    AssemblyRules::apply_motion_sample(&mut scene, &lid, middle).unwrap();
    let close = AssemblyRules::plan_motion(&scene, &lid, 0.).unwrap();
    assert_eq!(close.sample(0.).value, middle.value);
    let closing = close.sample(close.move_time * 0.5);
    assert!(closing.value > 0.);
    assert_eq!(closing.latch, 1.);
    AssemblyRules::apply_motion_sample(&mut scene, &lid, closing).unwrap();
    let shut = close.sample(close.move_time);
    assert_eq!(shut.value, 0.);
    assert_eq!(shut.latch, 1.);
    AssemblyRules::apply_motion_sample(&mut scene, &lid, shut).unwrap();
    let latching = close.sample(close.move_time + close.lock_time * 0.5);
    assert_eq!(latching.value, 0.);
    assert!(latching.latch > 0. && latching.latch < 1.);
    AssemblyRules::apply_motion_sample(&mut scene, &lid, latching).unwrap();
    let reopen = AssemblyRules::plan_motion(&scene, &lid, 65.).unwrap();
    assert!((reopen.sample(0.).latch - latching.latch).abs() < 0.001);
    AssemblyRules::apply_motion_sample(&mut scene, &lid, reopen.sample(reopen.duration)).unwrap();
    assert!(scene.parts[0].pose.matrix(0.).abs_diff_eq(root, 0.001));
    assert_eq!(
        AssemblyRules::mechanism_for_joint(&scene, &lid)
            .unwrap()
            .latch,
        1.
    );
}

#[test]
fn animation_rechecks_support_travel_and_missing_lock_constraints_before_committing() {
    let mut scene = hinged_box("Facade.stl");
    let lid = PartId::of(&scene.parts[1]);
    let plan = AssemblyRules::plan_motion(&scene, &lid, 65.).unwrap();
    scene.member_supports = vec![vec![], vec![0]];
    let before = snapshot(&scene);
    assert!(AssemblyRules::plan_motion(&scene, &lid, 65.).is_err());
    assert!(
        AssemblyRules::apply_motion_sample(&mut scene, &lid, plan.sample(plan.duration)).is_err()
    );
    assert_eq!(snapshot(&scene), before);
    scene.member_supports.clear();
    scene
        .relations
        .retain(|r| r.label != crate::sustainer::LOCK_OUTER);
    let before = snapshot(&scene);
    assert!(AssemblyRules::plan_motion(&scene, &lid, 65.).is_err());
    assert!(AssemblyRules::set_joint_value(&mut scene, &lid, 65.).is_err());
    assert_eq!(snapshot(&scene), before);
    let mut scene = drawer();
    let child = PartId::of(&scene.parts[1]);
    let before = snapshot(&scene);
    for goal in [-1., 101., f32::NAN, f32::INFINITY] {
        assert!(AssemblyRules::plan_motion(&scene, &child, goal).is_err());
    }
    assert!(
        AssemblyRules::apply_motion_sample(
            &mut scene,
            &child,
            MotionSample {
                value: 101.,
                latch: 0.
            }
        )
        .is_err()
    );
    assert_eq!(snapshot(&scene), before);
}
fn pair() -> Scene {
    Scene::combine_positioned(
        vec![
            (drawer(), Placement::default(), 0.),
            (
                drawer(),
                Placement {
                    column: 1,
                    ..Default::default()
                },
                0.,
            ),
        ],
        0.,
        Some(vec![Vec3::ZERO, Vec3::new(40., 0., 0.)]),
    )
    .unwrap()
}

#[test]
fn mechanical_motion_follows_parent_axis_and_keeps_other_member_and_explosion_separate() {
    let mut scene = pair();
    rotate_subassembly(
        &mut scene,
        3,
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    )
    .unwrap();
    let before: Vec<_> = scene.parts.iter().map(|p| p.pose.matrix(0.)).collect();
    let travel: Vec<_> = scene
        .parts
        .iter()
        .map(|p| (p.pose.travel, p.pose.travel_frame))
        .collect();
    let child = PartId::of(&scene.parts[4]);
    AssemblyRules::set_joint_value(&mut scene, &child, 25.).unwrap();
    for (i, part) in scene.parts.iter().enumerate() {
        let delta = part.pose.matrix(0.).transform_point3(Vec3::ZERO)
            - before[i].transform_point3(Vec3::ZERO);
        assert!(delta.abs_diff_eq(if i >= 4 { Vec3::X * 25. } else { Vec3::ZERO }, 0.001));
        assert_eq!((part.pose.travel, part.pose.travel_frame), travel[i]);
    }
    for value in [f32::NAN, f32::INFINITY, -1., 101.] {
        let before = snapshot(&scene);
        assert!(AssemblyRules::set_slider(&mut scene, &child, value).is_err());
        assert_eq!(snapshot(&scene), before);
    }
}

#[test]
fn failure_after_lid_solving_rolls_back_poses_joints_and_display_offsets() {
    let mut scene = hinged_box("unmounted.stl");
    let child = PartId::of(&scene.parts[1]);
    scene.member_supports = vec![vec![], vec![0]];
    let before = snapshot(&scene);
    assert!(
        AssemblyRules::set_joint_value(&mut scene, &child, 45.)
            .unwrap_err()
            .contains("Supporting lid")
    );
    assert_eq!(snapshot(&scene), before);
    scene.member_supports.clear();
    let before = snapshot(&scene);
    // The hinge can solve, but this deliberately incomplete box has no facade
    // for deriving its explosion frame. Failure must undo the solved movement.
    assert!(
        AssemblyRules::set_joint_value(&mut scene, &child, 45.)
            .unwrap_err()
            .contains("facade")
    );
    assert_eq!(snapshot(&scene), before);
}

#[test]
fn combination_extraction_is_repeatable_and_failed_later_members_do_not_partially_commit() {
    let mut scene = pair();
    let closed: Vec<_> = scene.parts.iter().map(|p| p.pose.matrix(0.)).collect();
    let joints = serde_json::to_string(&scene.definition().relations).unwrap();
    AssemblyRules::reset_explosion(&mut scene).unwrap();
    let first = snapshot(&scene);
    AssemblyRules::reset_explosion(&mut scene).unwrap();
    assert_eq!(first, snapshot(&scene));
    assert_eq!(
        joints,
        serde_json::to_string(&scene.definition().relations).unwrap()
    );
    for (part, before) in scene.parts.iter().zip(closed) {
        assert!(part.pose.matrix(0.).abs_diff_eq(before, 0.001));
    }
    scene
        .parts
        .retain(|p| p.pose.instance / MEMBER_STRIDE == 0 || !p.pose.path.contains("/Boxes/"));
    let before = snapshot(&scene);
    assert!(AssemblyRules::reset_explosion(&mut scene).is_err());
    assert_eq!(before, snapshot(&scene));
}

#[test]
fn unstructured_sources_remain_illustrative_and_layout_metadata_must_match() {
    let mut scene = Scene {
        assembly: true,
        ..Default::default()
    };
    AssemblyRules::initialize(&mut scene, &[]).unwrap();
    assert!(scene.constraint_warning.is_some());
    assert!(!scene.calibrated);
    assert!(AssemblyRules::layout(&[], &[Default::default()], 0.).is_err());
    let layout = stacking::Layout {
        column_starts: vec![0, 1648],
        nominal_tops: vec![61., 122.],
        left: vec![0., 0.],
        supports: vec![vec![], vec![0]],
        order: vec![0, 1],
    };
    assert_eq!(AssemblyRules::preview_lid_angle(&layout, 0, 75.), 0.);
    assert_eq!(AssemblyRules::preview_lid_angle(&layout, 1, 75.), 75.);
}

#[test]
#[ignore = "requires read-only VAULT_MODEL_ROOT"]
fn library_mixed_members_use_independent_lids_locks_sliders_and_repeatable_extraction() {
    let root = std::path::PathBuf::from(crate::test_support::required_env("VAULT_MODEL_ROOT"));
    let catalog = crate::catalog::Catalog::scan(root.clone()).unwrap();
    let mut members = Vec::new();
    for (i, (width, is_box)) in [("S", false), ("S", true), ("M", true)]
        .into_iter()
        .enumerate()
    {
        let selection = crate::catalog::Selection {
            width: width.into(),
            ..Default::default()
        };
        let bom = if is_box {
            catalog
                .sustainer_bom(&selection, Default::default())
                .unwrap()
        } else {
            catalog.bom(&selection)
        };
        let scene = Scene::load(&root, &bom, width.into(), true).unwrap();
        members.push((
            scene,
            Placement {
                column: i as u32,
                ..Default::default()
            },
            0.,
        ));
    }
    let mut scene = Scene::combine_positioned(
        members,
        0.,
        Some(vec![
            Vec3::ZERO,
            Vec3::new(200., 0., 0.),
            Vec3::new(400., 0., 0.),
        ]),
    )
    .unwrap();
    let target = scene
        .relations
        .iter()
        .find(|r| {
            r.child.instance / MEMBER_STRIDE == 1
                && AssemblyRules::label(&r.label) == crate::sustainer::LID
        })
        .unwrap()
        .child
        .clone();
    let before = snapshot(&scene);
    assert!(AssemblyRules::set_lid_angle(&mut scene, 55.).is_err());
    assert_eq!(before, snapshot(&scene));
    let unaffected: Vec<_> = scene
        .parts
        .iter()
        .filter(|p| p.pose.instance / MEMBER_STRIDE != 1)
        .map(|p| (PartId::of(p), p.pose.matrix(0.)))
        .collect();
    AssemblyRules::set_joint_value(&mut scene, &target, 55.).unwrap();
    for (id, matrix) in unaffected {
        assert!(
            scene
                .parts
                .iter()
                .find(|p| PartId::of(p) == id)
                .unwrap()
                .pose
                .matrix(0.)
                .abs_diff_eq(matrix, 0.001)
        );
    }
    assert!(
        scene
            .relations
            .iter()
            .filter(|r| r.child.instance / MEMBER_STRIDE == 2
                && [
                    crate::sustainer::LID,
                    crate::sustainer::LOCK_INNER,
                    crate::sustainer::LOCK_OUTER
                ]
                .contains(&AssemblyRules::label(&r.label)))
            .all(|r| matches!(r.joint,Joint::Hinge{angle,..} if angle==0.))
    );
    let before = snapshot(&scene);
    assert!(AssemblyRules::set_lid_angle_for(&mut scene, &target, 111.).is_err());
    assert_eq!(before, snapshot(&scene));
    let drawer = scene
        .relations
        .iter()
        .find(|r| AssemblyRules::label(&r.label) == AssemblyRules::DRAWER_SLIDE)
        .unwrap()
        .child
        .clone();
    AssemblyRules::set_joint_value(&mut scene, &drawer, 25.).unwrap();
    AssemblyRules::reset_explosion(&mut scene).unwrap();
    let first = snapshot(&scene);
    AssemblyRules::reset_explosion(&mut scene).unwrap();
    assert_eq!(first, snapshot(&scene));
}
