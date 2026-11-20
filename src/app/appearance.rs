use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Appearance {
    #[default]
    Light,
    Dark,
}

impl Appearance {
    pub fn apply(self, ctx: &egui::Context) {
        let theme = match self {
            Self::Light => egui::Theme::Light,
            Self::Dark => egui::Theme::Dark,
        };
        crate::ui_style::apply(ctx, theme);
    }
}

/// A restrained radial stage, drawn behind the transparent native GPU target.
pub(super) fn paint_stage(ui: &egui::Ui, rect: egui::Rect) {
    let dark = ui.visuals().dark_mode;
    let edge = if dark {
        Color32::from_gray(18)
    } else {
        Color32::from_gray(222)
    };
    let glow = if dark {
        Color32::from_gray(38)
    } else {
        Color32::from_gray(247)
    };
    let center = egui::pos2(
        rect.left() + rect.width() * 0.55,
        rect.top() + rect.height() * 0.35,
    );
    let mut mesh = egui::Mesh::default();
    let steps = 32;
    for y in 0..=steps {
        for x in 0..=steps {
            let pos = egui::pos2(
                egui::lerp(rect.x_range(), x as f32 / steps as f32),
                egui::lerp(rect.y_range(), y as f32 / steps as f32),
            );
            let distance = egui::vec2(
                (pos.x - center.x) / rect.width(),
                (pos.y - center.y) / rect.height(),
            )
            .length();
            let amount = (distance / 0.85).clamp(0., 1.);
            let value = egui::lerp(glow.r() as f32..=edge.r() as f32, amount) as u8;
            mesh.colored_vertex(pos, Color32::from_gray(value));
        }
    }
    for y in 0..steps {
        for x in 0..steps {
            let i = y * (steps + 1) + x;
            mesh.add_triangle(i, i + 1, i + steps + 1);
            mesh.add_triangle(i + 1, i + steps + 2, i + steps + 1);
        }
    }
    ui.painter().add(mesh);
}
