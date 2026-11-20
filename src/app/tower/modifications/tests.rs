use super::*;

#[test]
fn empty_card_adds_a_backbone_and_delete_restores_the_empty_card() {
    let mut member = Member {
        name: "Test drawer".into(),
        placement: Default::default(),
        preset: toml::from_str(
            "bom=[]\n[selection]\nwidth='S'\ndepth=3\nheight='1F'\nhidden_key=false",
        )
        .unwrap(),
        exterior: Exterior::default(),
    };
    let ctx = egui::Context::default();
    let frame = |member: &mut Member, events| {
        let mut right = 0.;
        let output = ctx.run(
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
                    right = ui.max_rect().right();
                    let mut action = None;
                    if let Some((index, draft)) =
                        cards(ui, std::slice::from_ref(member), 0., false, &mut action)
                    {
                        assert_eq!(index, 0);
                        member.exterior = draft;
                    }
                    assert!(action.is_none());
                });
            },
        );
        let labels: std::collections::BTreeMap<_, _> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.job.text.clone(),
                    text.pos + text.galley.rect.center().to_vec2(),
                )),
                _ => None,
            })
            .collect();
        (right, labels)
    };
    let click = |member: &mut Member, pos| {
        for pressed in [true, false] {
            frame(
                member,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        frame(member, vec![])
    };

    let (_, empty) = frame(&mut member, vec![]);
    let (_, menu) = click(&mut member, empty["No rear"]);
    assert!(!member.exterior.backbone);
    assert!(menu.contains_key("Wall mount"));
    let (right, configured) = click(&mut member, menu["Backbone"]);
    assert!(member.exterior.backbone);
    assert!(configured.contains_key("Pegboard"));
    let delete = egui::pos2(right - 22., configured["Rear"].y);
    let (_, restored) = click(&mut member, delete);
    assert_eq!(member.exterior, Exterior::default());
    assert!(restored.contains_key("No rear"));
}
