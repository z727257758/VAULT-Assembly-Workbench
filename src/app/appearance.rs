use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Appearance {
    #[default]
    Light,
    Dark,
}

pub(super) const BRAND: Color32 = Color32::from_rgb(47, 125, 79);

impl Appearance {
    pub fn apply(self, ctx: &egui::Context) {
        let dark = self == Self::Dark;
        ctx.set_theme(if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        let mut style = (*ctx.style()).clone();
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        let surface = if dark {
            Color32::from_gray(17)
        } else {
            Color32::WHITE
        };
        let subtle = if dark {
            Color32::from_gray(25)
        } else {
            Color32::from_gray(247)
        };
        let border = if dark {
            Color32::from_gray(56)
        } else {
            Color32::from_gray(212)
        };
        let text = if dark {
            Color32::from_gray(245)
        } else {
            Color32::from_gray(20)
        };
        visuals.panel_fill = surface;
        visuals.window_fill = surface;
        visuals.extreme_bg_color = subtle;
        visuals.faint_bg_color = subtle;
        visuals.hyperlink_color = if dark {
            Color32::from_rgb(127, 209, 159)
        } else {
            BRAND
        };
        visuals.selection.bg_fill = if dark {
            Color32::from_rgb(34, 73, 47)
        } else {
            Color32::from_rgb(230, 242, 234)
        };
        visuals.selection.stroke = egui::Stroke::new(1., text);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1., border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1., text);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            widget.corner_radius = egui::CornerRadius::same(6);
            widget.fg_stroke = egui::Stroke::new(1., text);
        }
        visuals.widgets.inactive.bg_fill = surface;
        visuals.widgets.inactive.weak_bg_fill = subtle;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., border);
        visuals.widgets.hovered.bg_fill = if dark {
            Color32::from_gray(44)
        } else {
            Color32::from_gray(239)
        };
        visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1., text.gamma_multiply(0.5));
        visuals.widgets.active.bg_fill = visuals.selection.bg_fill;
        visuals.widgets.active.weak_bg_fill = visuals.selection.bg_fill;
        visuals.window_corner_radius = egui::CornerRadius::same(12);
        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(8., 8.);
        style.spacing.button_padding = egui::vec2(10., 6.);
        style.spacing.interact_size.y = 32.;
        style.spacing.window_margin = egui::Margin::same(16);
        for (kind, size) in [
            (egui::TextStyle::Body, 14.),
            (egui::TextStyle::Button, 14.),
            (egui::TextStyle::Small, 12.),
            (egui::TextStyle::Heading, 20.),
        ] {
            style
                .text_styles
                .insert(kind, egui::FontId::proportional(size));
        }
        ctx.set_style(style);
    }
}

pub(super) fn floating_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().window_fill.gamma_multiply(0.97))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(12.)
        .inner_margin(8.)
        .shadow(egui::epaint::Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: Color32::from_black_alpha(18),
        })
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

pub(super) fn size_choices<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    value: &mut T,
    choices: &[(T, &str)],
) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(7.)
        .inner_margin(3.)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.;
            ui.horizontal(|ui| {
                let width = ((ui.available_width() - 3. * (choices.len() - 1) as f32)
                    / choices.len() as f32)
                    .max(26.);
                for (choice, label) in choices {
                    let selected = value == choice;
                    let button = egui::Button::new(*label)
                        .fill(if selected {
                            ui.visuals().panel_fill
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if selected {
                            ui.visuals().widgets.noninteractive.bg_stroke
                        } else {
                            egui::Stroke::NONE
                        });
                    if ui.add_sized([width, 30.], button).clicked() {
                        *value = choice.clone();
                    }
                }
            });
        });
}
