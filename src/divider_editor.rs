//! Native egui top-view editor; mutations go through layout validation/history.
use crate::{
    catalog::{Catalog, Selection},
    dividers::{self, Grid, Layout, PITCH, Placement},
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
#[derive(Default)]
pub struct Editor {
    pub open: bool,
    selected: Option<usize>,
    path: String,
    vertical: bool,
    height: usize,
    add_mode: bool,
    undo: Vec<Layout>,
    redo: Vec<Layout>,
    drag: Option<(Layout, Pos2)>,
    pub error: String,
}
impl Editor {
    pub fn reset(&mut self) {
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        self.error.clear();
        self.add_mode = false;
    }
    fn commit(&mut self, layout: &mut Layout, next: Layout, c: &Catalog, s: &Selection) -> bool {
        if *layout == next {
            return false;
        }
        if let Err(e) = next.validate(c, s) {
            self.error = e;
            return false;
        }
        self.undo.push(layout.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        *layout = next;
        self.error.clear();
        true
    }
    fn candidate(&self, layout: &Layout, s: &Selection) -> Layout {
        let mut l = layout.clone();
        l.enabled = true;
        l.specification = dividers::spec(s);
        l
    }
    fn placement_at(&self, g: Grid, center: [f32; 2]) -> Placement {
        let n = dividers::code(&self.path).unwrap();
        let axis = usize::from(self.vertical);
        let limits = [g.cols, g.rows];
        let mut start = center.map(|v| v.round() as i32);
        // Full-span pieces have a fixed longitudinal anchor. Clicking anywhere
        // across their span chooses only the perpendicular row/column.
        start[axis] = if n == g.full_code(self.vertical) {
            g.full_start(self.vertical)
        } else {
            (center[axis] - (n - 1) as f32 / 2.)
                .round()
                .clamp(0., (limits[axis] - n).max(0) as f32) as i32
        };
        Placement {
            path: self.path.clone(),
            start,
            vertical: self.vertical,
        }
    }
    fn add(
        &mut self,
        layout: &mut Layout,
        c: &Catalog,
        s: &Selection,
        g: Grid,
        at: Option<[f32; 2]>,
    ) -> bool {
        if self.path.is_empty() {
            return false;
        }
        let mut next = self.candidate(layout, s);
        let center = at.unwrap_or([g.cols as f32 / 2., g.rows as f32 / 2.]);
        let mut p = self.placement_at(g, center);
        let start = p.start;
        if at.is_none() {
            let mut found = None;
            // Try the center first, then remaining legal grid anchors.
            for start in std::iter::once(start)
                .chain((0..g.rows).flat_map(|y| (0..g.cols).map(move |x| [x, y])))
            {
                p.start = start;
                let mut trial = next.clone();
                trial.items.push(p.clone());
                if trial.validate(c, s).is_ok() {
                    found = Some(p.clone());
                    break;
                }
            }
            let Some(placed) = found else {
                self.error = "没有足够空间放置该隔板，请选择较短型号或移走已有隔板".into();
                return false;
            };
            p = placed;
        }
        next.items.push(p);
        let i = next.items.len() - 1;
        if self.commit(layout, next, c, s) {
            self.selected = Some(i);
            true
        } else {
            false
        }
    }
    fn history(&mut self, layout: &mut Layout, redo: bool) -> bool {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let Some(next) = from.pop() else {
            return false;
        };
        to.push(std::mem::replace(layout, next));
        self.selected = None;
        self.error.clear();
        true
    }
    pub fn selected_part(&self, layout: &Layout) -> Option<(String, u32)> {
        let i = self.selected?;
        let p = layout.items.get(i)?;
        Some((
            p.path.clone(),
            layout.items[..i]
                .iter()
                .filter(|n| n.path == p.path)
                .count() as u32,
        ))
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        c: &Catalog,
        s: &Selection,
        layout: &mut Layout,
    ) -> bool {
        if !self.open {
            return false;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("内部分隔编辑 · 底孔网格").id(egui::Id::new("divider-editor"))
            .default_pos([24.,95.]).default_size([660.,650.]).min_width(600.).open(&mut open).show(ctx,|ui| {
            let g=match Grid::new(s) {Ok(g)=>g,Err(e)=>{ui.label(e);return;}};
            ui.label(format!("{} · 10.5 mm 孔距 · {} × {} 个底孔",dividers::spec(s),g.cols,g.rows));
            ui.label("点击放置；拖动隔板吸附底孔。底座不允许重叠；贯穿整宽/整深自动配端件，每块配 2 个松配合连接销。");
            ui.label("编辑时三维仅显示抽屉与隔板，关闭后恢复视图。布局与 Gridfinity 底板互斥；T 形/十字接合暂不开放。尺寸标注为底座范围及底部净距。");
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!self.undo.is_empty(),egui::Button::new("撤销")).clicked() {
                    changed|=self.history(layout,false);
                }
                if ui.add_enabled(!self.redo.is_empty(),egui::Button::new("重做")).clicked() {
                    changed|=self.history(layout,true);
                }
                if ui.add_enabled(layout.enabled || !layout.items.is_empty(),egui::Button::new("移除分隔配置")).clicked() {
                    let next=Layout {specification:dividers::spec(s),..Default::default()};changed|=self.commit(layout,next,c,s);self.selected=None;
                }
                ui.separator();ui.label(format!("{} 块隔板",layout.items.len()));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("高度");
                egui::ComboBox::from_id_salt("divider-height").selected_text(["20 mm","34.5 mm","82.5 mm"][self.height]).show_ui(ui,|ui| {
                    ui.selectable_value(&mut self.height,0,"20 mm");ui.selectable_value(&mut self.height,1,"34.5 mm");
                    if s.height!="1F" {ui.selectable_value(&mut self.height,2,"82.5 mm");}
                });
                if s.height=="1F" && self.height==2 {self.height=0;}
                ui.selectable_value(&mut self.vertical,false,"横向");ui.selectable_value(&mut self.vertical,true,"纵向");
                let folder=["20MM","34.5MM","82.5MM"][self.height];
                let mut choices:Vec<_>=c.accessories(s).into_iter().filter(|e|e.path.contains("/Grid Dividers/") && e.path.contains(folder) && dividers::code(&e.path).is_some()).collect();
                choices.sort_by_key(|e|dividers::code(&e.path));
                if !choices.iter().any(|e|e.path==self.path) {self.path=choices.iter().find(|e|dividers::code(&e.path)==Some(g.full_code(self.vertical))).or(choices.first()).map(|e|e.path.clone()).unwrap_or_default();}
                egui::ComboBox::from_id_salt("divider-length").width(175.).selected_text(self.path.rsplit('/').next().unwrap_or("选择型号")).show_ui(ui,|ui| {
                    for e in &choices {let n=dividers::code(&e.path).unwrap();ui.selectable_value(&mut self.path,e.path.clone(),format!("{} · {:.1} mm",e.name,n as f32*PITCH));}
                });
                if ui.button("添加隔板").clicked() {changed|=self.add(layout,c,s,g,None);}
                if ui.toggle_value(&mut self.add_mode,"点击放置").changed() {self.error.clear();self.drag=None;}
                if self.add_mode && ui.button("结束放置").clicked() {self.add_mode=false;self.error.clear();}
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("平行等分（吸附网格）");
                for count in [2,3] {
                    if ui.button(format!("{count} 等分")).clicked() {
                        let folder=["20MM","34.5MM","82.5MM"][self.height];
                        if let Some(e)=c.accessories(s).into_iter().find(|e|e.path.contains(folder)&&dividers::code(&e.path)==Some(g.full_code(self.vertical))) {
                            let mut next=self.candidate(layout,s);next.items.clear();
                            for k in 1..count {
                                let cross=if self.vertical {g.cols} else {g.rows};
                                let row=((cross-1) as f32*k as f32/count as f32).round() as i32;
                                let start=if self.vertical {[row,g.full_start(true)]} else {[g.full_start(false),row]};
                                next.items.push(Placement {path:e.path.clone(),start,vertical:self.vertical});
                            }
                            changed|=self.commit(layout,next,c,s);self.selected=None;
                        }
                    }
                }
                ui.add_enabled_ui(self.selected.is_some_and(|i|i<layout.items.len()),|ui| {
                    let i=self.selected.unwrap_or(0);
                    if ui.button("删除选中").clicked() {let mut next=layout.clone();next.items.remove(i);changed|=self.commit(layout,next,c,s);self.selected=None;}
                    if ui.button("复制选中").clicked() {self.path=layout.items[i].path.clone();self.vertical=layout.items[i].vertical;changed|=self.add(layout,c,s,g,None);}
                    if ui.button("旋转 90°").clicked() {
                        let mut next=layout.clone();let p=&mut next.items[i];let n=dividers::code(&p.path).unwrap();
                        let old_axis=usize::from(p.vertical);let mut center=p.start.map(|n|n as f32);
                        center[old_axis]+=(n-1) as f32/2.;p.vertical= !p.vertical;
                        let axis=usize::from(p.vertical);p.start[1-axis]=center[1-axis].round() as i32;
                        p.start[axis]=if n==g.full_code(p.vertical) {g.full_start(p.vertical)} else {(center[axis]-(n-1) as f32/2.).round() as i32};
                        changed|=self.commit(layout,next,c,s);
                    }
                });
            });
            ui.allocate_ui_with_layout(Vec2::new(ui.available_width(),64.),egui::Layout::top_down(egui::Align::Min),|ui| {
            if let Some(i)=self.selected.filter(|i|*i<layout.items.len()) {
                let mut next=layout.clone();let p=&mut next.items[i];let mut edit=false;
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("#{} {}",i+1,p.path.rsplit('/').next().unwrap_or("")));
                    ui.label("起始列");edit|=ui.add(egui::DragValue::new(&mut p.start[0]).range(0..=g.cols-1)).changed();
                    ui.label("起始行");edit|=ui.add(egui::DragValue::new(&mut p.start[1]).range(0..=g.rows-1)).changed();
                    if ui.button("替换为当前型号").clicked() {p.path=self.path.clone();edit=true;}
                });
                if edit {changed|=self.commit(layout,next,c,s);}
            }
            else {ui.horizontal_wrapped(|ui| {ui.label("选中隔板后可修改位置或替换型号。");});}
            });
            if self.add_mode && ctx.input(|i|i.key_pressed(egui::Key::Escape) || i.pointer.secondary_clicked()) {self.add_mode=false;self.error.clear();}
            let hint=if !self.error.is_empty() {self.error.as_str()} else if self.add_mode {"放置模式：绿色可放置，红色有冲突；点击连续添加，Esc / 右键退出。"} else {"选择模式：单击选中，拖动移动。点击放置开关可进入添加模式。"};
            ui.add(egui::Label::new(egui::RichText::new(hint).color(if self.error.is_empty(){Color32::GRAY}else{Color32::LIGHT_RED})).truncate()).on_hover_text(hint);
            let available=Vec2::new(ui.available_width(),ui.available_height().clamp(260.,470.));
            let (canvas,response)=ui.allocate_exact_size(available,egui::Sense::click_and_drag());
            let margin=36.;let size=Vec2::new(g.right-g.left,g.front-g.rear);
            let scale=((canvas.width()-margin*2.)/size.x).min((canvas.height()-margin*2.)/size.y);
            let origin=canvas.center()-size*scale/2.;
            let to_screen=|x:f32,y:f32|origin+Vec2::new((x-g.left)*scale,(y-g.rear)*scale);
            let box_rect=Rect::from_min_max(to_screen(g.left,g.rear),to_screen(g.right,g.front));
            let paint=ui.painter();paint.rect_filled(box_rect,5.,Color32::from_rgb(32,43,52));
            paint.rect_stroke(box_rect,5.,Stroke::new(2.,Color32::GRAY),egui::StrokeKind::Inside);
            paint.text(box_rect.center_top()-Vec2::new(0.,16.),egui::Align2::CENTER_CENTER,format!("{:.1} mm · 后部",size.x),egui::FontId::proportional(13.),Color32::WHITE);
            paint.text(box_rect.center_bottom()+Vec2::new(0.,16.),egui::Align2::CENTER_CENTER,format!("前部 / 面板 · 深 {:.1} mm",size.y),egui::FontId::proportional(13.),Color32::WHITE);
            for y in 0..g.rows {for x in 0..g.cols {let p=g.point([x as f32,y as f32]);paint.circle_filled(to_screen(p[0],p[1]),1.8,Color32::from_gray(100));}}
            let mut hit=None;
            for (i,p) in layout.items.iter().enumerate() {
                let b=g.bounds(p);let rect=Rect::from_min_max(to_screen(b[0],b[1]),to_screen(b[2],b[3]));
                let color=if self.selected==Some(i) {Color32::from_rgb(244,173,70)} else {Color32::from_rgb(66,157,173)};
                paint.rect_filled(rect,2.,color.gamma_multiply(0.35));paint.rect_stroke(rect,2.,Stroke::new(2.,color),egui::StrokeKind::Inside);
                let center=g.center(p);let l=dividers::code(&p.path).unwrap() as f32*PITCH/2.;
                let (a,b)=if p.vertical {(to_screen(center[0],center[1]-l),to_screen(center[0],center[1]+l))}else{(to_screen(center[0]-l,center[1]),to_screen(center[0]+l,center[1]))};
                paint.line_segment([a,b],Stroke::new(4.,color));
                paint.text(rect.center(),egui::Align2::CENTER_CENTER,format!("#{} · {:.0}",i+1,l*2.),egui::FontId::proportional(12.),Color32::WHITE);
                if response.interact_pointer_pos().is_some_and(|p|rect.contains(p)) {hit=Some(i);}
            }
            // Clear widths between full-span parallel divider bases.
            if !layout.items.is_empty() && layout.items.iter().all(|p|g.full(p)&&p.vertical==layout.items[0].vertical) {
                let vertical=layout.items[0].vertical;
                let mut spans:Vec<_>=layout.items.iter().map(|p|{let b=g.bounds(p);if vertical {(b[0],b[2])}else{(b[1],b[3])}}).collect();spans.sort_by(|a,b|a.0.total_cmp(&b.0));
                let mut from=if vertical {g.left}else{g.rear};let end=if vertical {g.right}else{g.front};
                for (lo,hi) in spans.into_iter().chain(std::iter::once((end,end))) {
                    let center=(from+lo)/2.;let point=if vertical {to_screen(center,g.rear+8.)}else{to_screen(g.left+14.,center)};
                    paint.text(point,egui::Align2::CENTER_CENTER,format!("{:.1}",lo-from),egui::FontId::proportional(12.),Color32::LIGHT_GREEN);from=hi;
                }
            }
            let hover=if self.add_mode && !self.path.is_empty() {response.hover_pos().filter(|p|box_rect.contains(*p)).map(|p| {
                let raw=(p-origin)/scale+Vec2::new(g.left,g.rear);
                let center=[raw.x/PITCH+(g.cols-1) as f32/2.,(raw.y+3.5)/PITCH+(g.rows-1) as f32/2.];
                let placement=self.placement_at(g,center);
                let mut next=self.candidate(layout,s);next.items.push(placement.clone());
                (placement,next)
            })}else{None};
            if let Some((placement,next))=&hover {
                let valid=next.validate(c,s);let color=if valid.is_ok(){Color32::LIGHT_GREEN}else{Color32::LIGHT_RED};
                let b=g.bounds(placement);let rect=Rect::from_min_max(to_screen(b[0],b[1]),to_screen(b[2],b[3]));
                paint.rect_filled(rect,2.,color.gamma_multiply(0.18));paint.rect_stroke(rect,2.,Stroke::new(2.,color),egui::StrokeKind::Inside);
                paint.text(rect.center(),egui::Align2::CENTER_CENTER,if valid.is_ok(){"点击放置"}else{"不可放置"},egui::FontId::proportional(13.),color);
                if let Err(reason)=valid {response.clone().on_hover_text(reason);}
            }
            if response.clicked() {
                if self.add_mode {
                    if let Some((_,next))=hover {let i=next.items.len()-1;if self.commit(layout,next,c,s){self.selected=Some(i);changed=true;}}
                } else {self.selected=hit;}
            }
            if !self.add_mode && response.drag_started() {self.selected=hit;if hit.is_some() {self.drag=response.interact_pointer_pos().map(|p|(layout.clone(),p));}}
            if !self.add_mode && response.dragged() && let (Some(i),Some((baseline,start)),Some(pos))=(self.selected,self.drag.as_ref(),response.interact_pointer_pos()) {
                let delta=(pos-*start)/(scale*PITCH);let mut next=baseline.clone();
                next.items[i].start[0]+=delta.x.round() as i32;next.items[i].start[1]+=delta.y.round() as i32;
                if g.full(&baseline.items[i]) {let axis=usize::from(baseline.items[i].vertical);next.items[i].start[axis]=baseline.items[i].start[axis];}
                match next.validate(c,s) {Ok(())=>{if *layout!=next {*layout=next;changed=true;}self.error.clear();},Err(e)=>self.error=e}
            }
            if response.drag_stopped() && let Some((old,_))=self.drag.take() && old!=*layout {self.undo.push(old);self.redo.clear();}
        });
        self.open = open;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_pointer_clicks_match_preview_without_moving_canvas() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let s = Selection::default();
        let g = Grid::new(&s).unwrap();
        let ctx = egui::Context::default();
        let mut style = (*ctx.style()).clone();
        style.animation_time = 0.;
        ctx.set_style(style);
        let mut editor = Editor {
            open: true,
            add_mode: true,
            path: "01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/10-S, x3.stl".into(),
            ..Default::default()
        };
        let mut layout = Layout::default();
        let mut time = 0.;
        let mut frame = |events: Vec<egui::Event>, editor: &mut Editor, layout: &mut Layout| {
            time += 1. / 60.;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500., 1100.))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    editor.show(ctx, &c, &s, layout);
                },
            );
            output
                .shapes
                .into_iter()
                .find_map(|shape| match shape.shape {
                    egui::Shape::Rect(r) if r.fill == Color32::from_rgb(32, 43, 52) => Some(r.rect),
                    _ => None,
                })
        };
        let mut canvas = frame(vec![], &mut editor, &mut layout);
        for _ in 0..5 {
            canvas = frame(vec![], &mut editor, &mut layout);
        }
        let canvas = canvas.expect("editor canvas after sizing frames");
        for (x, y, count) in [(0., 2., 1), (11., 6., 2), (5., 6., 2)] {
            let raw = g.point([x, y]);
            let p = canvas.min
                + Vec2::new(
                    (raw[0] - g.left) / (g.right - g.left) * canvas.width(),
                    (raw[1] - g.rear) / (g.front - g.rear) * canvas.height(),
                );
            frame(vec![egui::Event::PointerMoved(p)], &mut editor, &mut layout);
            for pressed in [true, false] {
                frame(
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                    &mut editor,
                    &mut layout,
                );
            }
            let after = frame(vec![], &mut editor, &mut layout).expect("visible canvas");
            assert_eq!(layout.items.len(), count);
            assert!(
                (after.min - canvas.min).length() < 0.1,
                "canvas must not jump after selecting the new divider"
            );
            assert!((after.size() - canvas.size()).length() < 0.1);
        }
        assert_eq!(layout.items[0].start, [1, 2]);
        assert_eq!(layout.items[1].start, [1, 6]);
    }
    #[test]
    fn click_anchors_full_spans_and_rounds_short_centers_once() {
        let g = Grid::new(&Selection::default()).unwrap();
        let mut editor = Editor {
            path: "10-S, x3.stl".into(),
            ..Default::default()
        };
        for x in [-0.4, 0., 2., 5.5, 9., 11.4] {
            assert_eq!(editor.placement_at(g, [x, 5.2]).start, [1, 5]);
        }
        editor.vertical = true;
        for y in [0., 3., 6., 11.] {
            assert_eq!(editor.placement_at(g, [6.2, y]).start, [6, 1]);
        }
        editor.vertical = false;
        editor.path = "6.stl".into();
        assert_eq!(editor.placement_at(g, [4.1, 5.]).start, [2, 5]);
        assert_eq!(editor.placement_at(g, [4.9, 5.]).start, [2, 5]);
        assert_eq!(editor.placement_at(g, [0., 5.]).start, [0, 5]);
        assert_eq!(editor.placement_at(g, [11., 5.]).start, [6, 5]);
        editor.path = "5.stl".into();
        assert_eq!(editor.placement_at(g, [4.1, 5.]).start, [2, 5]);
    }
    #[test]
    fn repeated_clicks_keep_the_row_and_reject_overlap_transactionally() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let s = Selection::default();
        let g = Grid::new(&s).unwrap();
        let mut editor = Editor {
            path: "01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/10-S, x3.stl".into(),
            ..Default::default()
        };
        let mut layout = Layout::default();
        assert!(editor.add(&mut layout, &c, &s, g, Some([0., 2.])));
        assert!(editor.add(&mut layout, &c, &s, g, Some([11., 6.])));
        assert_eq!(layout.items[0].start, [1, 2]);
        assert_eq!(layout.items[1].start, [1, 6]);
        let before = layout.clone();
        let history = editor.undo.len();
        assert!(!editor.add(&mut layout, &c, &s, g, Some([5., 6.])));
        assert_eq!(layout, before);
        assert_eq!(editor.undo.len(), history);
    }
    #[test]
    fn edits_are_transactional_and_history_branches() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        let s = Selection::default();
        let g = Grid::new(&s).unwrap();
        let mut editor = Editor {
            path: "01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/10-S, x3.stl".into(),
            ..Default::default()
        };
        let mut layout = Layout::default();
        assert!(editor.add(&mut layout, &c, &s, g, None));
        assert_eq!(layout.items[0].start, [1, 6]);
        let first = layout.clone();
        assert!(editor.add(&mut layout, &c, &s, g, None));
        let second = layout.clone();
        assert_eq!(layout.items.len(), 2);
        let mut invalid = layout.clone();
        invalid.items[0].start = [-1, 0];
        assert!(!editor.commit(&mut layout, invalid, &c, &s));
        assert_eq!(layout, second);
        assert!(editor.history(&mut layout, false));
        assert_eq!(layout, first);
        assert!(editor.history(&mut layout, true));
        assert_eq!(layout, second);
        assert!(editor.history(&mut layout, false));
        let mut moved = layout.clone();
        moved.items[0].start[1] += 1;
        assert!(editor.commit(&mut layout, moved, &c, &s));
        assert!(editor.redo.is_empty());
        assert!(!editor.history(&mut layout, true));
    }
}
