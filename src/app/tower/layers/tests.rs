use super::super::{LayerCard, State};
use super::*;

#[test]
fn roof_box_drawer_and_base_headers_expand_exclusively() {
    let ctx = egui::Context::default();
    let mut state = State::default();
    let cards = [
        (LayerCard::Roof, "Top cover"),
        (LayerCard::Layer(1), "Drawer"),
        (LayerCard::Layer(0), "Box"),
        (LayerCard::Base, "Base"),
    ];
    let mut time = 0.;
    let mut frame = |events| {
        time += 0.1;
        let mut rects = [egui::Rect::NOTHING; 4];
        let mut bodies = [false; 4];
        let open_card = state.open_layer_card;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(460., 700.),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_max_width(360.);
                    for (index, (card, title)) in cards.into_iter().enumerate() {
                        let expanded = open_card == Some(card);
                        let body = |ui: &mut egui::Ui| {
                            bodies[index] = true;
                            ui.label("Card options");
                        };
                        let (rect, open) = match card {
                            LayerCard::Layer(level) => {
                                let response = layer(
                                    ui,
                                    Header {
                                        level,
                                        kind: title,
                                        size: "S3",
                                        height: "1F",
                                        height_mm: 47.,
                                        can_delete: true,
                                        zh: false,
                                    },
                                    expanded,
                                    body,
                                );
                                (response.response.rect, response.open)
                            }
                            LayerCard::Roof | LayerCard::Base => {
                                let id = if card == LayerCard::Roof {
                                    "tower-roofs"
                                } else {
                                    "tower-bases"
                                };
                                let response = section(ui, id, title, "S3", false, expanded, body);
                                (response.response.rect, response.inner)
                            }
                        };
                        rects[index] = rect;
                        if let Some(open) = open {
                            state.toggle_layer_card(card, open);
                        }
                    }
                });
            },
        );
        (rects, bodies)
    };

    frame(vec![]);
    let (mut rects, bodies) = frame(vec![]);
    assert_eq!(bodies, [false, false, true, false]);
    for (clicked, expected) in [
        (0, Some(0)),
        (2, Some(2)),
        (1, Some(1)),
        (3, Some(3)),
        (3, None),
    ] {
        let pos = rects[clicked].left_top() + egui::vec2(90., 24.);
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
        let (next_rects, bodies) = frame(vec![]);
        assert_eq!(bodies, std::array::from_fn(|index| expected == Some(index)));
        rects = next_rects;
    }
}

#[test]
fn section_header_click_preserves_other_sections() {
    let ctx = egui::Context::default();
    let frame = |events| {
        let mut sections = [false; 2];
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400., 500.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    sections = [
                        group(ui, "layers", "Layers", true, |_| {}),
                        group(ui, "colors", "Colors", false, |_| {}),
                    ];
                });
            },
        );
        sections
    };
    assert_eq!(frame(vec![]), [true, false]);
    let pos = egui::pos2(345., 25.);
    for expected in [false, true] {
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert_eq!(frame(vec![]), [expected, false]);
    }
}

struct Harness {
    ctx: egui::Context,
    time: f64,
    expanded: bool,
    checked: bool,
    can_delete: bool,
    card: egui::Rect,
    checkbox: egui::Rect,
    combo: egui::Rect,
    action: Option<Action>,
    drop_target: egui::Rect,
    dropped: bool,
}
impl Harness {
    fn new() -> Self {
        let mut h = Self {
            ctx: Default::default(),
            time: 0.,
            expanded: false,
            checked: false,
            can_delete: true,
            card: egui::Rect::NOTHING,
            checkbox: egui::Rect::NOTHING,
            combo: egui::Rect::NOTHING,
            action: None,
            drop_target: egui::Rect::NOTHING,
            dropped: false,
        };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }
    fn frame(&mut self, events: Vec<egui::Event>) {
        self.time += 0.1;
        let ctx = self.ctx.clone();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(460., 700.),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_max_width(360.);
                    let card = layer(
                        ui,
                        Header {
                            level: 0,
                            kind: "Drawer",
                            size: "M5",
                            height: "2F",
                            height_mm: 100.,
                            can_delete: self.can_delete,
                            zh: false,
                        },
                        self.expanded,
                        |ui| {
                            self.checkbox =
                                ui.checkbox(&mut self.checked, "Export this layer").rect;
                            self.combo = egui::ComboBox::from_id_salt("nested-combo")
                                .selected_text("Options")
                                .show_ui(ui, |ui| {
                                    let _ = ui.button("Nested action");
                                })
                                .response
                                .rect;
                            ui.label("Blank area and inert text open the layer");
                            ui.add_space(24.);
                        },
                    );
                    self.card = card.response.rect;
                    self.action = card.action;
                    if let Some(open) = card.open {
                        self.expanded = open;
                    }
                    let target = surface(ui, "drop-target", false, false, |ui| {
                        ui.set_min_height(60.);
                        ui.label("Drop layer here");
                    });
                    self.drop_target = target.response.rect;
                    self.dropped |= target
                        .response
                        .dnd_release_payload::<u32>()
                        .is_some_and(|p| *p == 0);
                });
            },
        );
    }
    fn pointer(&mut self, pos: egui::Pos2, pressed: bool) {
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    fn click(&mut self, pos: egui::Pos2) {
        self.pointer(pos, true);
        self.pointer(pos, false);
    }
    fn settle(&mut self) {
        self.frame(vec![]);
        self.frame(vec![]);
    }
}
#[test]
fn card_padding_title_and_blank_header_all_expand_the_layer() {
    for target in 0..3 {
        let mut h = Harness::new();
        let pos = match target {
            0 => h.card.left_top() + egui::vec2(2., 2.),
            1 => h.card.left_top() + egui::vec2(90., 24.),
            _ => h.card.right_top() + egui::vec2(-90., 24.),
        };
        h.click(pos);
        assert!(h.expanded, "card target {target} did not expand");
        assert!(h.action.is_none());
    }
}
#[test]
fn child_controls_keep_priority_and_blank_body_keeps_layer_open() {
    let mut h = Harness::new();
    h.click(h.card.center());
    h.settle();
    h.click(h.checkbox.center());
    assert!(h.checked && h.expanded);
    h.click(h.combo.center());
    assert!(h.expanded, "combo popup toggled its parent");
    h.frame(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    h.settle();
    h.click(h.card.center_bottom() - egui::vec2(0., 12.));
    assert!(h.expanded, "blank body unexpectedly closed the layer");
    h.click(h.card.left_top() + egui::vec2(90., 24.));
    assert!(!h.expanded, "header should still allow collapsing");
}
#[test]
fn visible_copy_delete_and_disabled_delete_do_not_toggle_card() {
    for (copy, enabled) in [(true, true), (false, true), (false, false)] {
        let mut h = Harness::new();
        h.can_delete = enabled;
        h.click(h.card.center());
        h.settle();
        h.click(h.card.right_top() + egui::vec2(if copy { -58. } else { -22. }, 24.));
        assert!(h.expanded);
        assert!(matches!(
            (copy, enabled, &h.action),
            (true, _, Some(Action::Duplicate(0)))
                | (false, true, Some(Action::Delete(0)))
                | (false, false, None)
        ));
    }
}
#[test]
fn dragging_handle_reorders_without_expanding_or_collapsing_layer() {
    let mut h = Harness::new();
    let handle = h.card.left_top() + egui::vec2(16., 24.);
    h.pointer(handle, true);
    h.frame(vec![egui::Event::PointerMoved(
        handle + egui::vec2(0., 12.),
    )]);
    h.frame(vec![egui::Event::PointerMoved(h.drop_target.center())]);
    h.pointer(h.drop_target.center(), false);
    assert!(h.dropped, "drag did not reach the other card");
    assert!(!h.expanded, "drag accidentally expanded the source");
}
