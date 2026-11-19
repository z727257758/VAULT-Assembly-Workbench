use super::*;
use glam::{Mat4, Vec3};

pub(super) const PITCH: f32 = 42.;

/// Cell (0,0) is the back-left socket. Both layout axes run opposite to the
/// baseplate mesh axes, exactly as in gridfinity::apply_preview.
#[derive(Clone)]
pub(super) struct Projection {
    pub rect: egui::Rect,
    pub grid: [u32; 2],
    pub camera: Camera,
    pub corner: Vec3,
    pub plate: Mat4,
}

impl Projection {
    pub fn cell_at(&self, p: egui::Pos2, height: f32) -> Option<[f32; 2]> {
        let (origin, direction) = self.camera.ray(p, self.rect);
        let inverse = self.plate.inverse();
        let origin = inverse.transform_point3(origin);
        let direction = inverse.transform_vector3(direction);
        if direction.y.abs() < 0.0001 {
            return None;
        }
        let distance = (self.corner.y + height - origin.y) / direction.y;
        let point = origin + direction * distance;
        (distance >= 0. && point.is_finite()).then(|| self.local_cell(point))
    }

    pub fn local_cell(&self, p: Vec3) -> [f32; 2] {
        [(self.corner.x - p.x) / PITCH, (self.corner.z - p.z) / PITCH]
    }

    pub fn screen(&self, at: [f32; 2], height: f32) -> egui::Pos2 {
        let local = self.corner + Vec3::new(-at[0] * PITCH, height, -at[1] * PITCH);
        let world = self.plate.transform_point3(local);
        let ndc = self
            .camera
            .matrix(self.rect.aspect_ratio())
            .project_point3(world);
        self.rect.center()
            + egui::vec2(ndc.x * self.rect.width(), -ndc.y * self.rect.height()) * 0.5
    }

    pub fn inside(&self, at: [f32; 2]) -> bool {
        (0. ..self.grid[0] as f32).contains(&at[0]) && (0. ..self.grid[1] as f32).contains(&at[1])
    }

    pub fn outline(&self, cell: [f32; 2], span: [u32; 2], height: f32) -> [egui::Pos2; 4] {
        let [x, y] = cell;
        let [w, d] = span.map(|n| n as f32);
        [[x, y], [x + w, y], [x + w, y + d], [x, y + d]].map(|p| self.screen(p, height))
    }
}

pub(super) fn camera_for(part: &Part, plate: Mat4, rect: egui::Rect, show_housing: bool) -> Camera {
    let model = part.pose.matrix(0.);
    let lo = part.mesh.min;
    let hi = part.mesh.max;
    // Taller drawer walls need a steeper view to keep the front sockets accessible.
    let pitch = (hi.y - lo.y).atan2(PITCH * 0.67).clamp(0.86, 1.26);
    let side = 0.16f32;
    let eye = plate
        .transform_vector3(Vec3::new(
            side.sin() * pitch.cos(),
            pitch.sin(),
            -side.cos() * pitch.cos(),
        ))
        .normalize();
    let mut camera = Camera {
        yaw: eye.x.atan2(eye.z),
        pitch: eye.y.asin().clamp(-1.56, 1.56),
        center: model.transform_point3((lo + hi) * 0.5),
        radius: ((hi - lo).length() * 0.5).max(1.),
        ..Default::default()
    };
    let matrix = camera.matrix(rect.aspect_ratio()) * model;
    let mut extent = 0.01f32;
    for x in [lo.x, hi.x] {
        for y in [lo.y, hi.y] {
            for z in [lo.z, hi.z] {
                let p = matrix.project_point3(Vec3::new(x, y, z));
                extent = extent.max(p.x.abs()).max(p.y.abs());
            }
        }
    }
    camera.zoom = if show_housing { 0.78 } else { 0.86 } / extent;
    // Move the drawer slightly down the frame to leave the installation visible behind it.
    if show_housing {
        camera.pan[1] = 0.12;
    }
    camera
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn dragging_from_a_bin_rim_to_an_empty_socket_uses_the_destination_height() {
        let view = Projection {
            grid: [3, 3],
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(620., 530.)),
            camera: Camera {
                yaw: std::f32::consts::PI,
                pitch: 0.9,
                ..Default::default()
            },
            plate: Mat4::IDENTITY,
            corner: Vec3::new(63., 0., 63.),
        };
        let rim = 38.;
        let state = State {
            bins: vec![Footprint {
                id: 1,
                cell: [0, 0],
                span: [1, 1],
                height: rim,
                body: 0,
                label: None,
            }],
            ..Default::default()
        };
        let from = view.screen([0.5, 0.5], rim);
        let to = view.screen([1.5, 0.5], 0.);
        let start = state.hit(&view, from).unwrap().0;
        let end = state.hit(&view, to).unwrap().0;
        assert!((view.cell_at(to, rim).unwrap()[1] - end[1]).abs() > 0.5);
        let s = Selection::default();
        let mut layout = Layout::default();
        layout.add(&s, None).unwrap();
        let mut selected = None;
        let mut editor = Editor::default();
        editor.begin_grid_drag(GridView::Scene, &layout, &s, &mut selected, start, 0.1);
        editor.update_grid_drag(
            GridView::Scene,
            &mut layout,
            &s,
            &mut selected,
            Some(end),
            true,
        );
        assert_eq!(layout.bins[0].cell, [1, 0]);
    }

    #[test]
    fn opening_the_edit_drawer_preserves_its_housing_and_other_drawer() {
        use crate::constraints::{self, Joint, Relation};
        let size = Vec3::new(140., 160., 40.);
        let mesh = Arc::new(Mesh {
            min: -size * 0.5,
            max: size * 0.5,
            vertices: vec![],
            size,
        });
        let target = Target {
            member: 0,
            drawer: Some(1),
        };
        let sibling = Target {
            member: 0,
            drawer: Some(0),
        };
        let part = |path: &str, instance: u32, y: f32| Part {
            mesh: mesh.clone(),
            pose: crate::scene::Pose {
                path: path.into(),
                instance,
                body: None,
                position: [0., y, 0.],
                rotation: [0., 180., 0.],
                travel: [0.; 3],
                travel_frame: None,
            },
            color: [1.; 4],
            visible: true,
            source_bodies: 1,
            source_size: size,
        };
        let mut source = Scene {
            assembly: true,
            parts: vec![
                part("shared/Modules/body", 0, 0.),
                part("shared/Boxes/same", sibling.instance(0), 48.),
                part("shared/Boxes/same", target.instance(0), 0.),
                part("shared/plate", target.instance(0), 4.),
            ],
            ..Default::default()
        };
        for (parent, child, slider) in [(0, 1, true), (0, 2, true), (2, 3, false)] {
            let mut joint = Relation::fixed(&source, parent, child, slider).unwrap();
            if let Joint::Slider { min, max, .. } = &mut joint.joint {
                *min = 0.;
                *max = 100.;
                joint.label = AssemblyRules::DRAWER_SLIDE.into();
            }
            constraints::add(&mut source, joint).unwrap();
        }
        let other_id = PartId::of(&source.parts[1]);
        AssemblyRules::set_slider(&mut source, &other_id, 20.).unwrap();
        let mut cache = None;
        let opened = OpenDrawer::get(&mut cache, &source, target).unwrap();
        assert!(Arc::ptr_eq(
            &opened,
            &OpenDrawer::get(&mut cache, &source, target).unwrap()
        ));
        for i in [0, 1] {
            assert!(
                opened.parts[i]
                    .pose
                    .matrix(0.)
                    .abs_diff_eq(source.parts[i].pose.matrix(0.), 0.001)
            );
        }
        assert_eq!(AssemblyRules::mechanism(&source, target).unwrap().value, 0.);
        assert_eq!(
            AssemblyRules::mechanism(&opened, target).unwrap().value,
            100.
        );
        for i in [2, 3] {
            assert!(
                (Vec3::from(opened.parts[i].pose.position)
                    - Vec3::from(source.parts[i].pose.position))
                .abs_diff_eq(Vec3::Z * 100., 0.001)
            );
        }
        let pose = opened.parts[3].pose.matrix(0.);
        let rect = egui::Rect::from_min_size(egui::pos2(328., 50.), egui::vec2(580., 630.));
        let view = Projection {
            rect,
            camera: camera_for(&opened.parts[2], pose, rect, true),
            plate: pose,
            corner: Vec3::new(63., 0., 63.),
            grid: [3, 3],
        };
        let pos = view.screen([1.5, 2.5], 30.);
        assert!(rect.contains(pos));
        let cell = view.cell_at(pos, 30.).unwrap();
        assert!((cell[0] - 1.5).abs() < 0.001 && (cell[1] - 2.5).abs() < 0.001);
    }

    #[test]
    fn asymmetric_socket_grid_roundtrips_on_the_translated_rotated_drawer() {
        let plate = Mat4::from_rotation_translation(
            Quat::from_rotation_y(0.63),
            Vec3::new(135., 72., -65.),
        );
        let corner = Vec3::new(69., -7., 127.);
        let grid = [2, 5];
        for size in [egui::vec2(360., 620.), egui::vec2(820., 450.)] {
            let view = Projection {
                rect: egui::Rect::from_min_size(egui::pos2(24., 61.), size),
                grid,
                camera: Camera {
                    center: plate.transform_point3(corner),
                    pitch: 1.37,
                    radius: 240.,
                    ..Default::default()
                },
                corner,
                plate,
            };
            for (cell, height) in [([0.5, 0.5], 0.), ([1.5, 4.5], 31.55), ([0., 5.], 170.)] {
                let at = view.cell_at(view.screen(cell, height), height).unwrap();
                assert!((at[0] - cell[0]).abs() < 0.001 && (at[1] - cell[1]).abs() < 0.001);
            }
            // Last socket in the measured ascending coordinate arrays maps to the first cell.
            assert_eq!(
                view.local_cell(corner - Vec3::new(21., 0., 21.)),
                [0.5, 0.5]
            );
            assert!(!view.inside([2., 4.5]));
        }
    }

    #[test]
    fn moving_a_bin_and_its_label_shares_the_plate_transform_and_keeps_source_poses() {
        let rotation = Quat::from_rotation_y(0.65);
        let plate = Mat4::from_rotation_translation(rotation, Vec3::new(32., 70., -120.));
        let corner = Vec3::new(84., 3., 105.);
        let h = 31.55;
        let view = Projection {
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(560., 430.)),
            grid: [4, 5],
            camera: Camera::default(),
            corner,
            plate,
        };
        let size = Vec3::new(2. * PITCH - 0.5, h, PITCH - 0.5);
        let pos = plate.transform_point3(corner + Vec3::new(-PITCH, h / 2., -PITCH / 2.));
        let body = Part {
            mesh: Arc::new(Mesh {
                min: -size / 2.,
                max: size / 2.,
                size,
                vertices: vec![],
            }),
            pose: crate::scene::Pose {
                path: "bin".into(),
                instance: 2,
                body: None,
                position: pos.to_array(),
                rotation: [0., 0.65f32.to_degrees(), 0.],
                travel: [0.; 3],
                travel_frame: None,
            },
            visible: true,
            color: [1.; 4],
            source_bodies: 1,
            source_size: size,
        };
        let mut label = body.clone();
        label.pose.position = (pos + Vec3::new(6., 12., -4.)).to_array();
        let mut drawn = [body.clone(), label];
        let bin = Bin {
            cell: [1, 2],
            span: [2, 1],
            ..Default::default()
        };
        let shift = display_offset(&bin, h, &body, &view).unwrap();
        assert!(shift.abs_diff_eq(
            plate.transform_vector3(Vec3::new(-PITCH, 0., -2. * PITCH)),
            0.001
        ));
        for part in &mut drawn {
            move_display_part(part, shift);
        }
        assert_eq!(body.pose.position, pos.to_array());
        assert!(display_offset(&bin, h, &drawn[0], &view).unwrap().length() < 0.001);
        assert!(
            (Vec3::from(drawn[1].pose.position) - Vec3::from(drawn[0].pose.position))
                .abs_diff_eq(Vec3::new(6., 12., -4.), 0.001)
        );
        assert!(
            display_offset(
                &Bin {
                    span: [1, 1],
                    ..bin
                },
                h,
                &drawn[0],
                &view
            )
            .is_none()
        );
    }
}
