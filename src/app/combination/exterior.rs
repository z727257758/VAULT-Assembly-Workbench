use super::*;
use crate::catalog::exterior::{Base, Board, Exterior, Roof};
pub(in crate::app) struct Edit {
    index: usize,
    draft: Exterior,
    error: String,
}
impl App {
    pub(in crate::app) fn open_exterior(&mut self) {
        self.commit_member();
        if let Some(m) = self.members.get(self.active_member) {
            self.exterior_edit = Some(Edit {
                index: self.active_member,
                draft: m.exterior.clone(),
                error: String::new(),
            });
        }
    }
    pub(in crate::app) fn exterior_window(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.exterior_edit.take() else {
            return;
        };
        let Some(m) = self.members.get(edit.index) else {
            return;
        };
        let z = self.zh;
        let descriptors = modules(&self.members);
        let top = crate::constraints::stacking::plan(&descriptors, 0.)
            .is_ok_and(|p| !p.supports.iter().any(|s| s.contains(&edit.index)));
        let stroke = crate::constraints::stacking::plan(&descriptors, 0.).map_or(0., |p| {
            crate::constraints::exterior::pull_limit(&descriptors, &p, edit.index)
        });
        let ground = m.placement.level == 0;
        let roof_available = top && m.preset.assembly_kind == AssemblyKind::Drawer;
        let backbone_available = m.preset.selection.height != "4F";
        let mut apply = false;
        let mut cancel = false;
        let response=egui::Modal::new(egui::Id::new("overall-exterior")).show(ctx,|ui|{
            ui.set_width(520.);
            ui.heading(t(z,"整体装配 · 外部配件","Overall assembly · accessories"));
            ui.label(format!("{} · {}{} / {}",m.name,m.preset.selection.width,m.preset.selection.depth,m.preset.selection.height));
            egui::ScrollArea::vertical().max_height(600.).show(ui,|ui|{
                ui.label(t(z,"底部安装","Bottom"));
                ui.horizontal_wrapped(|ui|{
                    for (v,cn,en) in [(Base::None,"无底座","None"),(Base::Plain,"承托底座","Base"),(Base::Trolley,"底座＋双轮＋脚撑","Base + wheels + foot")] {
                        ui.add_enabled_ui(ground||v==Base::None,|ui|{ui.selectable_value(&mut edit.draft.base,v,t(z,cn,en));});
                    }
                });
                ui.small(t(z,"底座采用对应宽深的整体源件；双轮方案自动包含轮盘、衬套、轮胎和轮盖。","Whole source base matches width/depth; trolley includes disks, bushings, tyres and covers."));
                ui.separator();
                ui.add_enabled(backbone_available,egui::Checkbox::new(&mut edit.draft.backbone,t(z,"背部连接板","Backbone"))).on_hover_text(t(z,"1F / 2F 安装；4F 跨层孔位尚未校准","1F / 2F attachments; 4F cross-level holes not calibrated"));
                if ui.add_enabled(backbone_available,egui::Checkbox::new(&mut edit.draft.stationary_handle,t(z,"固定把手","Stationary handle"))).changed() && edit.draft.stationary_handle {edit.draft.backbone=true;}
                if ui.add_enabled(top&&backbone_available,egui::Checkbox::new(&mut edit.draft.pull_handle,t(z,"伸缩拉杆提手及锁紧件","Pull-out handle and locks"))).changed() && edit.draft.pull_handle {edit.draft.backbone=true;}
                if edit.draft.pull_handle {
                    ui.add(egui::Slider::new(&mut edit.draft.pull_extension,0. ..=stroke).text(t(z,"伸出量 mm","Extension mm")));
                }
                ui.horizontal_wrapped(|ui|{
                    ui.label(t(z,"挂板接口","Pegboard"));
                    for (v,label) in [(Board::None,t(z,"无","None")),(Board::Ikea,"IKEA SKÅDIS"),(Board::Ussa,"USSA")] {if ui.selectable_value(&mut edit.draft.board,v,label).changed() && v!=Board::None {edit.draft.backbone=true;}}
                });
                ui.checkbox(&mut edit.draft.wall_mount,t(z,"壁挂组件","Wall mounting"));
                ui.separator();
                ui.label(t(z,"顶部安装（抽屉独立顶盖）","Top (separate drawer roof)"));
                ui.horizontal_wrapped(|ui|{
                    for (v,cn,en) in [(Roof::None,"无","None"),(Roof::Grid,"Grid 顶盖","Grid"),(Roof::Table,"平顶","Table"),(Roof::Bowl,"托盘顶盖","Bowl"),(Roof::UnderTable,"桌下安装顶盖","Under table")] {
                        ui.add_enabled_ui(roof_available||v==Roof::None,|ui|{ui.selectable_value(&mut edit.draft.roof,v,t(z,cn,en));});
                    }
                });
                if ui.add_enabled(roof_available,egui::Checkbox::new(&mut edit.draft.top_handle,t(z,"顶部提手（Grid 顶盖）","Top handle (Grid roof)"))).changed() && edit.draft.top_handle {edit.draft.roof=Roof::Grid;}
                ui.separator();
                if edit.draft.base!=Base::None || edit.draft.roof!=Roof::None {
                    ui.small(t(z,"手册建议：底座上方、顶盖下方使用 2F 模块。这是推荐条件。","Manual recommends a 2F module above the base and below the roof."));
                }
                ui.small(t(z,"安装面按源模型估算，配合未实物验证。金属五金列入采购清单；尺寸读数只包含预览中的零件。","Source-mesh seating estimates; physical fit unverified. Purchased hardware is listed separately; dimensions include previewed parts."));
                if let Ok(parts)=self.catalog.exterior_components(&m.preset.selection,&edit.draft,m.preset.assembly_kind==AssemblyKind::Sustainer) {
                    ui.collapsing(t(z,"打印与五金清单","Print and hardware list"),|ui|{
                        for c in parts {ui.label(format!("{} × {}",c.bom.name(),c.bom.qty));}
                        for h in edit.draft.hardware(&m.preset.selection) {ui.label(h.line());}
                    });
                }
                if !edit.error.is_empty(){ui.colored_label(Color32::LIGHT_RED,&edit.error);}
            });
            ui.horizontal(|ui|{
                apply=ui.button(t(z,"应用配件","Apply")).clicked();
                if ui.button(t(z,"清除全部配件","Clear accessories")).clicked(){edit.draft=Exterior::default();}
                cancel=ui.button(t(z,"取消","Cancel")).clicked();
            });
        });
        if cancel || response.should_close() {
            return;
        }
        if apply {
            let mut next = self.members.clone();
            next[edit.index].exterior = edit.draft.clone();
            let saved = CombinationPreset {
                colors: self.global_colors.clone(),
                preset: self.member_snapshot(),
                members: next.clone(),
                active_member: self.active_member,
                combination_gap: self.combination_gap,
            };
            match saved.validate(&self.catalog) {
                Ok(()) => {
                    self.record_workflow();
                    self.members[edit.index].exterior = edit.draft;
                    self.member_selection.clear();
                    self.combination_move_source = None;
                    self.combination_view = true;
                    self.assembly(ctx);
                    return;
                }
                Err(e) => edit.error = e,
            }
        }
        self.exterior_edit = Some(edit);
    }
}
