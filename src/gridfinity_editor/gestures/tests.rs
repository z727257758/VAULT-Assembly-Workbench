use super::*;

#[test]
fn both_views_share_a_preview_and_only_its_owner_can_commit() {
    let mut editor = Editor::default();
    let mut layout = Layout::default();
    let s = Selection::default();
    let mut selected = None;
    let start = [1.5, 1.5];
    editor.begin_grid_drag(GridView::Scene, &layout, &s, &mut selected, start, 0.);
    editor.update_grid_drag(
        GridView::Scene,
        &mut layout,
        &s,
        &mut selected,
        Some([0.5, 1.5]),
        false,
    );
    let preview = *editor.drag_preview().unwrap();
    assert_eq!(preview.cell, [0, 1]);
    assert_eq!(preview.span, [2, 1]);
    assert!(preview.valid);
    editor.update_grid_drag(GridView::Plan, &mut layout, &s, &mut selected, None, true);
    assert!(layout.bins.is_empty());
    assert_eq!(editor.drag_preview(), Some(&preview));
    editor.update_grid_drag(
        GridView::Scene,
        &mut layout,
        &s,
        &mut selected,
        Some([0.5, 1.5]),
        true,
    );
    assert_eq!(selected, Some(preview.id));
    assert_eq!(layout.bins.len(), 1);
    assert_eq!(layout.bins[0].cell, [0, 1]);
    assert!(editor.drag_preview().is_none());
    editor.update_grid_drag(
        GridView::Scene,
        &mut layout,
        &s,
        &mut selected,
        Some(start),
        true,
    );
    assert_eq!(layout.bins.len(), 1);

    editor.begin_grid_drag(GridView::Plan, &layout, &s, &mut selected, [1.96, 1.5], 0.1);
    editor.update_grid_drag(
        GridView::Plan,
        &mut layout,
        &s,
        &mut selected,
        Some([2.96, 1.5]),
        false,
    );
    assert_eq!(editor.drag_preview().unwrap().span, [3, 1]);
    assert_eq!(layout.bins[0].span, [2, 1]);
    editor.cancel();
    assert!(editor.drag_preview().is_none());
    assert_eq!(layout.bins[0].span, [2, 1]);
}

#[test]
fn collision_outside_and_concurrent_changes_never_overwrite_layout() {
    let mut editor = Editor::default();
    let mut layout = Layout::default();
    let s = Selection::default();
    layout.add(&s, None).unwrap();
    layout.add(&s, None).unwrap();
    let saved = layout.clone();
    let mut selected = None;
    editor.begin_grid_drag(GridView::Scene, &layout, &s, &mut selected, [0.5, 0.5], 0.);
    assert_eq!(selected, Some(1));
    editor.update_grid_drag(
        GridView::Scene,
        &mut layout,
        &s,
        &mut selected,
        Some([1.5, 0.5]),
        false,
    );
    assert!(!editor.drag_preview().unwrap().valid);
    editor.update_grid_drag(GridView::Scene, &mut layout, &s, &mut selected, None, true);
    assert!(editor.error.is_some());
    assert_eq!(layout, saved);
    editor.begin_grid_drag(GridView::Scene, &layout, &s, &mut selected, [0.5, 0.5], 0.);
    layout.bins[0].label.text = "Changed elsewhere".into();
    editor.update_grid_drag(
        GridView::Scene,
        &mut layout,
        &s,
        &mut selected,
        Some([0.5, 1.5]),
        true,
    );
    assert!(!editor.active());
    assert_eq!(layout.bins[0].cell, [0, 0]);
    assert_eq!(layout.bins[0].label.text, "Changed elsewhere");
}
