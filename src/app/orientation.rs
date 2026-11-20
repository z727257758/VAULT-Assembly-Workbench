use super::*;
use egui::{Pos2, Sense, Stroke, Vec2, pos2, vec2};
use glam::Vec3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Face {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl Face {
    pub(super) const ALL: [Self; 6] = [
        Self::Front,
        Self::Back,
        Self::Right,
        Self::Left,
        Self::Top,
        Self::Bottom,
    ];

    pub(super) fn label(self, zh: bool) -> &'static str {
        match self {
            Self::Front => t(zh, "前", "Front"),
            Self::Back => t(zh, "后", "Back"),
            Self::Right => t(zh, "右", "Right"),
            Self::Left => t(zh, "左", "Left"),
            Self::Top => t(zh, "上", "Top"),
            Self::Bottom => t(zh, "下", "Bottom"),
        }
    }

    pub(super) fn view(self) -> (f32, f32) {
        use std::f32::consts::{FRAC_PI_2, PI};
        let pole = FRAC_PI_2 - 0.0001;
        match self {
            Self::Front => (0., 0.),
            Self::Back => (PI, 0.),
            Self::Right => (FRAC_PI_2, 0.),
            Self::Left => (-FRAC_PI_2, 0.),
            Self::Top => (0., pole),
            Self::Bottom => (0., -pole),
        }
    }

    fn basis(self) -> (Vec3, Vec3, Vec3) {
        match self {
            Self::Front => (Vec3::Z, Vec3::X, Vec3::Y),
            Self::Back => (Vec3::NEG_Z, Vec3::X, Vec3::Y),
            Self::Right => (Vec3::X, Vec3::Z, Vec3::Y),
            Self::Left => (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            Self::Top => (Vec3::Y, Vec3::X, Vec3::Z),
            Self::Bottom => (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        }
    }
}

fn inside_face(corners: &[Pos2; 4], point: Pos2) -> bool {
    let mut positive = false;
    let mut negative = false;
    for i in 0..4 {
        let a = corners[i];
        let edge = corners[(i + 1) % 4] - a;
        let offset = point - a;
        let cross = edge.x * offset.y - edge.y * offset.x;
        positive |= cross > 0.;
        negative |= cross < 0.;
    }
    !(positive && negative)
}

pub(super) fn cube(ui: &mut egui::Ui, camera: &Camera, zh: bool) -> Option<Face> {
    let (rect, response) = ui.allocate_exact_size(vec2(113., 104.), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let toward = camera.direction();
    let right = Vec3::Y.cross(toward).normalize();
    let up = toward.cross(right);
    let project = |v: Vec3| center + vec2(v.dot(right), -v.dot(up)) * 18.;
    let dark = ui.visuals().dark_mode;

    for (axis, name, light, dark_color) in [
        (Vec3::X, "X", [179, 51, 49], [255, 125, 116]),
        (Vec3::Y, "Y", [37, 120, 68], [129, 211, 143]),
        (Vec3::Z, "Z", [43, 102, 176], [118, 172, 249]),
    ] {
        let end = project(axis * 2.25);
        let delta = end - center;
        if delta.length() < 9. {
            continue;
        }
        let color = if dark { dark_color } else { light };
        let color = Color32::from_rgb(color[0], color[1], color[2]);
        painter.line_segment([center, end], Stroke::new(1.6, color));
        painter.text(
            end + delta.normalized() * 6.,
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(10.),
            color,
        );
    }

    let pointer = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos());
    let mut hovered = None;
    for face in Face::ALL {
        let (normal, u, v) = face.basis();
        let facing = normal.dot(toward);
        if facing < 0.01 {
            continue;
        }
        let corners = [
            normal - u - v,
            normal + u - v,
            normal + u + v,
            normal - u + v,
        ]
        .map(project);
        let hot = pointer.is_some_and(|point| inside_face(&corners, point));
        if hot {
            hovered = Some(face);
        }
        let illumination = normal.dot(Vec3::new(0.3, 0.8, 0.4)).max(0.);
        let base = if dark { 48. } else { 203. };
        let fill = if hot {
            ui.visuals().selection.bg_fill
        } else {
            Color32::from_gray((base + illumination * 43.) as u8)
        };
        painter.add(egui::Shape::convex_polygon(
            corners.to_vec(),
            fill,
            Stroke::new(1., ui.visuals().widgets.noninteractive.bg_stroke.color),
        ));
        if facing > 0.32 {
            painter.text(
                project(normal),
                egui::Align2::CENTER_CENTER,
                face.label(zh),
                egui::FontId::proportional(if zh { 14. } else { 9. }),
                ui.visuals().text_color(),
            );
        }
    }
    if let Some(face) = hovered {
        response.clone().on_hover_text(face.label(zh));
    }
    if response.clicked() { hovered } else { None }
}

pub(super) fn home(ui: &mut egui::Ui, zh: bool) -> bool {
    let response =
        ui.add(egui::Button::new("").min_size(Vec2::splat(ui.spacing().interact_size.y)));
    let label = t(zh, "恢复默认视角", "Reset to default view");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let center = response.rect.center();
    let outline = [
        (-7., -1.),
        (0., -7.),
        (7., -1.),
        (7., 6.),
        (2., 6.),
        (2., 1.),
        (-2., 1.),
        (-2., 6.),
        (-7., 6.),
        (-7., -1.),
    ]
    .map(|(x, y)| pos2(center.x + x, center.y + y));
    ui.painter().add(egui::Shape::line(
        outline.to_vec(),
        Stroke::new(1.3, ui.style().interact(&response).fg_stroke.color),
    ));
    response.on_hover_text(label).clicked()
}

#[cfg(test)]
#[test]
fn face_views_follow_the_render_camera_axes() {
    for face in Face::ALL {
        let (yaw, pitch) = face.view();
        let camera = Camera {
            yaw,
            pitch,
            ..Default::default()
        };
        assert!(camera.direction().dot(face.basis().0) > 0.9999);
    }
    let square = [pos2(0., 0.), pos2(1., 0.), pos2(1., 1.), pos2(0., 1.)];
    assert!(inside_face(&square, pos2(0.5, 0.5)));
    assert!(!inside_face(&square, pos2(1.5, 0.5)));
    assert!(inside_face(
        &[square[3], square[2], square[1], square[0]],
        pos2(0.5, 0.5)
    ));
}
