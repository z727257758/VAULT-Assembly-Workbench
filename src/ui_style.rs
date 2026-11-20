//! Shared visual language for the workbench and its standalone editors.
use eframe::egui::{
    self, Button, Color32, CornerRadius, Frame, IntoAtoms, Margin, Response, Stroke, Style, Theme,
    Ui, Widget,
};

pub(crate) const SWATCH_SIZE: f32 = 28.;
const CONTROL_RADIUS: CornerRadius = CornerRadius::same(6);
const CARD_RADIUS: CornerRadius = CornerRadius::same(10);
const WINDOW_RADIUS: CornerRadius = CornerRadius::same(12);

pub(crate) fn apply(ctx: &egui::Context, theme: Theme) {
    let dark = theme == Theme::Dark;
    let color = |day: [u8; 3], night: [u8; 3]| {
        let [r, g, b] = if dark { night } else { day };
        Color32::from_rgb(r, g, b)
    };
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let surface = color([255, 255, 255], [35, 40, 37]);
    let inset = color([240, 244, 241], [28, 33, 30]);
    let border = color([203, 214, 207], [66, 77, 70]);
    let ink = color([34, 47, 39], [231, 238, 233]);
    let accent = color([36, 103, 65], [152, 216, 174]);
    let hover = color([230, 240, 233], [48, 62, 52]);
    let selection = color([211, 234, 219], [48, 78, 59]);

    visuals.panel_fill = color([248, 250, 248], [24, 28, 26]);
    visuals.window_fill = surface;
    visuals.faint_bg_color = inset;
    visuals.extreme_bg_color = color([225, 232, 227], [17, 22, 19]);
    visuals.text_edit_bg_color = Some(surface);
    visuals.weak_text_color = Some(color([86, 102, 92], [167, 183, 172]));
    visuals.hyperlink_color = accent;
    visuals.warn_fg_color = color([137, 81, 16], [245, 194, 112]);
    visuals.error_fg_color = color([166, 52, 52], [255, 157, 151]);
    visuals.selection.bg_fill = selection;
    visuals.selection.stroke = Stroke::new(1., color([26, 77, 46], [212, 244, 221]));
    visuals.slider_trailing_fill = true;
    visuals.window_stroke = Stroke::new(1., border);
    visuals.window_corner_radius = WINDOW_RADIUS;
    visuals.menu_corner_radius = CARD_RADIUS;
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 66 } else { 24 }),
    };
    visuals.popup_shadow = visuals.window_shadow;

    let widget = |fill, edge, text| egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1., edge),
        fg_stroke: Stroke::new(1., text),
        corner_radius: CONTROL_RADIUS,
        expansion: 0.,
    };
    visuals.widgets.noninteractive = widget(inset, border, ink);
    visuals.widgets.inactive = widget(surface, border, ink);
    visuals.widgets.inactive.bg_fill = border;
    visuals.widgets.hovered = widget(hover, accent, accent);
    visuals.widgets.active = widget(selection, accent, ink);
    visuals.widgets.open = widget(hover, accent, ink);

    ctx.style_mut_of(theme, |style| {
        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(8., 8.);
        style.spacing.button_padding = egui::vec2(9., 5.);
        style.spacing.interact_size.y = 32.;
        style.spacing.window_margin = Margin::same(16);
        // Keep the scrollbar clear of the content even before it is hovered.
        style.spacing.scroll = egui::style::ScrollStyle {
            floating_width: 5.,
            floating_allocated_width: 5.,
            dormant_background_opacity: 1.,
            active_background_opacity: 1.,
            interact_background_opacity: 1.,
            dormant_handle_opacity: 0.6,
            active_handle_opacity: 0.8,
            ..egui::style::ScrollStyle::floating()
        };
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
    });
    ctx.set_theme(theme);
}

pub(crate) fn quiet_button<'a>(text: impl IntoAtoms<'a>) -> Button<'a> {
    Button::new(text).frame_when_inactive(false)
}

pub(crate) fn choice_button<'a>(selected: bool, text: impl IntoAtoms<'a>) -> Button<'a> {
    Button::selectable(selected, text)
}

// Keep the native button's layout, keyboard interaction, disabled opacity and accessibility.
// Installing the role on this widget alone also works inside add_enabled and add_sized.
pub(crate) fn primary_button<'a>(text: impl IntoAtoms<'a>) -> impl Widget + 'a {
    let button = Button::new(text);
    move |ui: &mut Ui| {
        let previous = ui.style().clone();
        let visuals = ui.visuals_mut();
        let accent = visuals.hyperlink_color;
        let on_accent = if visuals.dark_mode {
            Color32::from_rgb(20, 46, 29)
        } else {
            Color32::WHITE
        };
        let widgets = &mut visuals.widgets;
        for (state, fill) in [
            (&mut widgets.inactive, accent),
            (&mut widgets.hovered, accent.lerp_to_gamma(on_accent, 0.10)),
            (&mut widgets.active, accent.lerp_to_gamma(on_accent, 0.18)),
        ] {
            state.weak_bg_fill = fill;
            state.fg_stroke.color = on_accent;
            state.bg_stroke = Stroke::new(1., fill);
        }
        widgets.active.bg_stroke = Stroke::new(1.5, on_accent);
        let response = ui.add(button);
        ui.set_style(previous);
        response
    }
}

pub(crate) fn danger_button<'a>(text: impl IntoAtoms<'a>) -> impl Widget + 'a {
    let button = Button::new(text);
    move |ui: &mut Ui| {
        let previous = ui.style().clone();
        let visuals = ui.visuals_mut();
        let error = visuals.error_fg_color;
        let widgets = &mut visuals.widgets;
        widgets.inactive.fg_stroke.color = error;
        for state in [&mut widgets.hovered, &mut widgets.active] {
            state.fg_stroke.color = error;
            state.bg_stroke.color = error;
            state.weak_bg_fill = visuals.window_fill.lerp_to_gamma(error, 0.10);
        }
        let response = ui.add(button);
        ui.set_style(previous);
        response
    }
}

pub(crate) fn card_frame(ui: &Ui, selected: bool) -> Frame {
    let visuals = ui.visuals();
    Frame::new()
        .fill(visuals.window_fill)
        .stroke(if selected {
            Stroke::new(1., visuals.hyperlink_color)
        } else {
            visuals.window_stroke
        })
        .corner_radius(CARD_RADIUS)
        .inner_margin(8)
}

pub(crate) fn inset_frame(ui: &Ui) -> Frame {
    Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(CONTROL_RADIUS)
        .inner_margin(8)
}

pub(crate) fn floating_frame(ui: &Ui) -> Frame {
    card_frame(ui, false).shadow(ui.visuals().window_shadow)
}

pub(crate) fn dialog_frame(style: &Style) -> Frame {
    Frame::window(style)
}

pub(crate) fn panel_frame(style: &Style, margin: Margin) -> Frame {
    Frame::new()
        .fill(style.visuals.panel_fill)
        .inner_margin(margin)
}

pub(crate) fn segmented_choices<T: PartialEq + Clone>(
    ui: &mut Ui,
    value: &mut T,
    choices: &[(T, &str)],
) {
    if choices.is_empty() {
        return;
    }
    inset_frame(ui).inner_margin(3).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.;
        ui.spacing_mut().button_padding.x = 4.;
        ui.horizontal(|ui| {
            let gaps = ui.spacing().item_spacing.x * (choices.len() - 1) as f32;
            let width = ((ui.available_width() - gaps) / choices.len() as f32).max(1.);
            for (choice, label) in choices {
                let button = choice_button(value == choice, *label);
                if ui
                    .add_sized([width, ui.spacing().interact_size.y], button)
                    .clicked()
                {
                    *value = choice.clone();
                }
            }
        });
    });
}

pub(crate) fn swatch(ui: &mut Ui, color: Color32, selected: bool, label: &str) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(SWATCH_SIZE), egui::Sense::click());
    response.widget_info(|| {
        let hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
        let name = if label.is_empty() {
            hex
        } else {
            format!("{label} · {hex}")
        };
        egui::WidgetInfo::selected(
            egui::WidgetType::RadioButton,
            ui.is_enabled(),
            selected,
            name,
        )
    });
    let center = rect.center();
    let visuals = ui.visuals();
    ui.painter()
        .circle(center, 10., color, visuals.window_stroke);
    if selected || response.hovered() || response.has_focus() {
        let width = if selected { 2. } else { 1. };
        let ink = if response.has_focus() {
            visuals.text_color()
        } else {
            visuals.hyperlink_color
        };
        ui.painter()
            .circle_stroke(center, 13., Stroke::new(width, ink));
    }
    if !label.is_empty() {
        let [r, g, b, _] = egui::Rgba::from(color).to_array();
        let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let ink = if luminance > 0.179 {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            label,
            egui::TextStyle::Small.resolve(ui.style()),
            ink,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
