use crate::{
    catalog::{Catalog, Selection, SustainerOptions},
    sustainer_dividers::{self as dividers, Axis, Geometry, Layout, Placement},
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

#[derive(Default)]
pub struct Editor {
    pub open: bool,
    pub selected: Option<usize>,
    axis: Axis,
    lane: usize,
    add_mode: bool,
    undo: Vec<Layout>,
    redo: Vec<Layout>,
    drag: Option<Layout>,
    error: String,
}
impl Editor {
    pub fn reset(&mut self) {
        let open = self.open;
        *self = Self {
            open,
            ..Default::default()
        };
    }
    fn commit(&mut self, layout: &mut Layout, next: Layout) -> bool {
        if *layout == next {
            return false;
        }
        let mut unique = std::collections::BTreeSet::new();
        if next.items.iter().any(|p| !unique.insert(p)) {
            self.error = "同一槽位已有隔板".into();
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
    fn nearest(g: &Geometry, axis: Axis, lane: usize, point: [f32; 2]) -> Option<Placement> {
        g.choices(axis, lane).into_iter().min_by(|a, b| {
            let distance = |p: &Placement| {
                let line = g.line(p).unwrap();
                let k = usize::from(axis == Axis::Width);
                (line[0][k] - point[k]).abs()
            };
            distance(a).total_cmp(&distance(b))
        })
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        c: &Catalog,
        s: &Selection,
        o: SustainerOptions,
        geometry: Option<&Geometry>,
        layout: &mut Layout,
    ) -> bool {
        if !self.open {
            return false;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("箱子隔板配置 · 壁槽定位").id(egui::Id::new("sustainer-divider-editor"))
            .default_pos([24.,95.]).default_size([660.,630.]).min_width(580.).open(&mut open).show(ctx,|ui| {
            let Some(g)=geometry.filter(|g|g.specification==dividers::spec(s,o)) else {
                ui.label("正在读取当前箱体的槽位，请等待装配预览完成。"); return;
            };
            ui.label(format!("{}{} / {} · {} 排横槽 · {} 个内部区域",s.width,s.depth,s.height,g.rows.len(),g.lanes.len()));
            ui.label("点击槽位添加隔板；选中后可拖动吸附到其他槽位。虚线表示可用槽位。");
            ui.small("型号和高度随箱体匹配，数量自动加入打印清单。位置来自源模型槽位测量，尚未实物确认。");
            ui.small("装配前请按说明书第 6 页移除槽内手动支撑；预览保留原始支撑，表面检查可能提示相交。");
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!self.undo.is_empty(),egui::Button::new("撤销")).clicked() {
                    self.redo.push(layout.clone());*layout=self.undo.pop().unwrap();self.selected=None;changed=true;self.error.clear();
                }
                if ui.add_enabled(!self.redo.is_empty(),egui::Button::new("重做")).clicked() {
                    self.undo.push(layout.clone());*layout=self.redo.pop().unwrap();self.selected=None;changed=true;self.error.clear();
                }
                if ui.add_enabled(!layout.items.is_empty(),egui::Button::new("清空隔板")).clicked() {
                    changed|=self.commit(layout,Layout{specification:dividers::spec(s,o),..Default::default()});self.selected=None;
                }
                ui.label(format!("共 {} 块",layout.items.len()));
            });
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.axis,Axis::Width,"横向隔板");
                if !g.columns.is_empty() {ui.selectable_value(&mut self.axis,Axis::Depth,"纵向隔板");}
                else {self.axis=Axis::Width;}
                self.lane=self.lane.min(g.lanes.len().saturating_sub(1));
                if self.axis==Axis::Width && g.lanes.len()>1 {
                    ui.label("区域");
                    for lane in 0..g.lanes.len() {ui.selectable_value(&mut self.lane,lane,format!("{}",lane+1));}
                }
                if ui.button("添加隔板").clicked() {
                    let choices=g.choices(self.axis,self.lane);
                    let center=(choices.len().saturating_sub(1)) as f32/2.;
                    if let Some(p)=choices.into_iter().filter(|p|!layout.items.contains(p)).min_by(|a,b|((a.slot as f32-center).abs()).total_cmp(&(b.slot as f32-center).abs())) {
                        let mut next=layout.clone();next.specification=dividers::spec(s,o);next.items.push(p);
                        if self.commit(layout,next) {self.selected=Some(layout.items.len()-1);changed=true;}
                    } else {self.error="此方向 / 区域的槽位已全部占用".into();}
                }
                ui.toggle_value(&mut self.add_mode,"点击放置");
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("填满当前方向 / 区域").clicked() {
                    let mut next=layout.clone();next.specification=dividers::spec(s,o);
                    for p in g.choices(self.axis,self.lane) {if !next.items.contains(&p){next.items.push(p);}}
                    changed|=self.commit(layout,next);
                }
                if ui.add_enabled(self.selected.is_some_and(|i|i<layout.items.len()),egui::Button::new("删除选中")).clicked() {
                    let mut next=layout.clone();next.items.remove(self.selected.take().unwrap());changed|=self.commit(layout,next);
                }
            });
            ui.allocate_ui_with_layout(Vec2::new(ui.available_width(),48.),egui::Layout::top_down(egui::Align::Min),|ui| {
                ui.set_min_height(48.);
                if let Some(i)=self.selected.filter(|i|*i<layout.items.len()) {
                    let p=&layout.items[i];
                    let name=c.sustainer_divider(s,o,p.axis,p.lane).unwrap_or_default();
                    ui.label(format!("#{} · 槽位 {} · 区域 {}",i+1,p.slot+1,p.lane+1));
                    ui.add(egui::Label::new(egui::RichText::new(name.rsplit('/').next().unwrap_or("")).small()).truncate());
                } else {ui.label("选择模式：点击选中隔板，拖动更换槽位。");}
            });
            let hint=if self.error.is_empty() {if self.add_mode {"放置模式：点击虚线槽位添加，Esc 退出。"}else{"关闭编辑器后恢复完整箱体视图。"}}else{&self.error};
            ui.add(egui::Label::new(egui::RichText::new(hint).color(if self.error.is_empty(){Color32::GRAY}else{Color32::LIGHT_RED})).truncate()).on_hover_text(hint);
            if ctx.input(|i|i.key_pressed(egui::Key::Escape)) {self.add_mode=false;}
            let (canvas,response)=ui.allocate_exact_size(Vec2::new(ui.available_width(),ui.available_height().clamp(280.,440.)),egui::Sense::click_and_drag());
            let size=Vec2::new(g.bounds[2]-g.bounds[0],g.bounds[3]-g.bounds[1]);
            let scale=((canvas.width()-60.)/size.x).min((canvas.height()-60.)/size.y);
            let origin=canvas.center()-size*scale/2.;
            let screen=|p:[f32;2]|origin+Vec2::new(p[0]-g.bounds[0],p[1]-g.bounds[1])*scale;
            let rect=Rect::from_min_size(origin,size*scale);
            let paint=ui.painter();
            paint.rect_filled(rect,3.,Color32::from_rgb(32,43,52));
            paint.rect_stroke(rect,3.,Stroke::new(2.,Color32::GRAY),egui::StrokeKind::Inside);
            paint.text(rect.center_top()-Vec2::new(0.,15.),egui::Align2::CENTER_CENTER,format!("后部 · {:.1} mm",size.x),egui::FontId::proportional(13.),Color32::WHITE);
            paint.text(rect.center_bottom()+Vec2::new(0.,15.),egui::Align2::CENTER_CENTER,format!("面板侧 · 深 {:.1} mm",size.y),egui::FontId::proportional(13.),Color32::WHITE);
            for lane in g.lanes.windows(2) {
                paint.rect_filled(Rect::from_min_max(screen([lane[0][1],g.bounds[1]]),screen([lane[1][0],g.bounds[3]])),0.,Color32::GRAY);
            }
            let line_screen=|p:&Placement|g.line(p).map(|l|[screen(l[0]),screen(l[1])]);
            for axis in [Axis::Width,Axis::Depth] {
                for lane in 0..if axis==Axis::Width {g.lanes.len()}else{1} {
                    for p in g.choices(axis,lane) {
                        if let Some(line)=line_screen(&p) {
                            paint.add(egui::Shape::dashed_line(&line,Stroke::new(1.,Color32::from_gray(90)),5.,4.));
                        }
                    }
                }
            }
            let distance=|p:Pos2,line:[Pos2;2]|{
                let d=line[1]-line[0];let t=((p-line[0]).dot(d)/d.length_sq()).clamp(0.,1.);
                (p-(line[0]+d*t)).length()
            };
            let mut hit=None;
            for (i,p) in layout.items.iter().enumerate() {
                if let Some(line)=line_screen(p) {
                    let color=if self.selected==Some(i){Color32::from_rgb(244,173,70)}else{Color32::from_rgb(66,180,180)};
                    paint.line_segment(line,Stroke::new(4.,color));
                    paint.text(line[0]+(line[1]-line[0])*0.22,egui::Align2::CENTER_BOTTOM,format!("#{}",i+1),egui::FontId::proportional(12.),color);
                    if response.interact_pointer_pos().is_some_and(|pos|distance(pos,line)<9.) {hit=Some(i);}
                }
            }
            let candidate=response.hover_pos().filter(|p|rect.contains(*p)).and_then(|pos|{
                let p=(pos-origin)/scale+Vec2::new(g.bounds[0],g.bounds[1]);
                let lane=g.lanes.iter().position(|l|(l[0]..=l[1]).contains(&p.x)).unwrap_or(self.lane);
                Self::nearest(g,self.axis,lane,[p.x,p.y])
            });
            if self.add_mode && let Some(p)=&candidate && let Some(line)=line_screen(p) {
                paint.line_segment(line,Stroke::new(4.,if layout.items.contains(p){Color32::LIGHT_RED}else{Color32::LIGHT_GREEN}));
            }
            if response.clicked() {
                if self.add_mode {
                    if let Some(p)=candidate {let mut next=layout.clone();next.specification=dividers::spec(s,o);next.items.push(p);if self.commit(layout,next){self.selected=Some(layout.items.len()-1);changed=true;}}
                } else {self.selected=hit;}
            }
            if !self.add_mode && response.drag_started() {
                let pressed=ctx.input(|i|i.pointer.press_origin());
                self.selected=pressed.and_then(|pos|layout.items.iter().enumerate().filter_map(|(i,p)|line_screen(p).map(|line|(i,distance(pos,line)))).filter(|(_,d)|*d<9.).min_by(|a,b|a.1.total_cmp(&b.1)).map(|(i,_)|i));
                self.drag=self.selected.map(|_|layout.clone());
            }
            if response.dragged() && let (Some(i),Some(baseline),Some(pos))=(self.selected,self.drag.as_ref(),response.interact_pointer_pos()) {
                let raw=(pos-origin)/scale+Vec2::new(g.bounds[0],g.bounds[1]);
                let old=&baseline.items[i];
                let lane=g.lanes.iter().position(|l|(l[0]..=l[1]).contains(&raw.x)).unwrap_or(old.lane);
                if let Some(p)=Self::nearest(g,old.axis,lane,[raw.x,raw.y]) {
                    if layout.items.iter().enumerate().all(|(j,item)|j==i || *item!=p) {
                        if layout.items[i]!=p {layout.items[i]=p;changed=true;}self.error.clear();
                    } else {self.error="目标槽位已有隔板".into();}
                }
            }
            if response.drag_stopped() && let Some(old)=self.drag.take() && old!=*layout {self.undo.push(old);self.redo.clear();}
        });
        self.open = open;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_add_duplicate_drag_and_history_use_wall_slots() {
        let g = Geometry {
            specification: dividers::spec(&Selection::default(), SustainerOptions::default()),
            bounds: [-50., -60., 50., 60.],
            floor: 0.,
            rows: vec![-30., 0., 30.],
            columns: vec![],
            lanes: vec![[-50., 50.]],
        };
        let s = Selection::default();
        let o = SustainerOptions::default();
        let c = Catalog {
            root: Default::default(),
            entries: vec![],
        };
        let mut layout = Layout {
            specification: g.specification.clone(),
            ..Default::default()
        };
        let mut editor = Editor {
            open: true,
            add_mode: true,
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut style = (*ctx.style()).clone();
        style.animation_time = 0.;
        ctx.set_style(style);
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
                    editor.show(ctx, &c, &s, o, Some(&g), layout);
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
        let mut canvas = None;
        for _ in 0..6 {
            canvas = frame(vec![], &mut editor, &mut layout);
        }
        let canvas = canvas.unwrap();
        let point = |row: usize| {
            Pos2::new(
                canvas.center().x,
                canvas.top() + (g.rows[row] + 60.) / 120. * canvas.height(),
            )
        };
        let click = |p: Pos2, pressed: bool| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        for (row, count) in [(0, 1), (0, 1), (1, 2)] {
            let p = point(row);
            frame(vec![egui::Event::PointerMoved(p)], &mut editor, &mut layout);
            for pressed in [true, false] {
                frame(vec![click(p, pressed)], &mut editor, &mut layout);
            }
            let after = frame(vec![], &mut editor, &mut layout).unwrap();
            assert_eq!(layout.items.len(), count);
            assert!(
                (canvas.min - after.min).length() < 0.1,
                "row={row} before={canvas:?} after={after:?}"
            );
        }
        editor.add_mode = false;
        let start = point(0);
        let end = point(2);
        frame(
            vec![egui::Event::PointerMoved(start), click(start, true)],
            &mut editor,
            &mut layout,
        );
        frame(
            vec![egui::Event::PointerMoved(end)],
            &mut editor,
            &mut layout,
        );
        frame(vec![click(end, false)], &mut editor, &mut layout);
        assert_eq!(layout.items[0].slot, 2);
        assert_eq!(editor.undo.last().unwrap().items[0].slot, 0);
        let before = layout.clone();
        let mut next = layout.clone();
        next.items[0] = next.items[1].clone();
        assert!(!editor.commit(&mut layout, next));
        assert_eq!(layout, before);
    }
}
