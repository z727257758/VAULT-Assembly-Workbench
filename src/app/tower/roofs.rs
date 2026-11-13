//! Top configuration follows the tower footprint, independently of drawer splits.
use super::*;
use crate::catalog::exterior::Roof;
mod smoke;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum Layout {
    Whole,
    Apart,
}

#[derive(Clone, Copy)]
struct Cover {
    kind: Roof,
    handle: bool,
}
struct Choice {
    layout: Layout,
    covers: Vec<Cover>,
}
impl Choice {
    fn capture(members: &[Member]) -> Result<Self, String> {
        let all = rows(members)?;
        let top = all.last().ok_or("缺少顶层 / Missing top layer")?;
        let first = &members[top.members[0]].exterior;
        let layout = if top.members.len() == 1
            || can_cover(members, top)
                && (first.roof_width.as_deref() == Some(top.width.as_str())
                    || first.roof_width.is_none()
                        && top
                            .members
                            .iter()
                            .all(|&i| members[i].exterior.roof == Roof::None))
        {
            Layout::Whole
        } else {
            // Legacy projects with separate covers retain their original arrangement.
            Layout::Apart
        };
        Ok(Self {
            layout,
            covers: top
                .members
                .iter()
                .map(|&i| Cover {
                    kind: members[i].exterior.roof,
                    handle: members[i].exterior.top_handle,
                })
                .collect(),
        })
    }
    fn primary(&self) -> Cover {
        self.covers
            .iter()
            .copied()
            .find(|c| c.kind != Roof::None)
            .unwrap_or(self.covers[0])
    }
    fn apply(&self, members: &mut [Member]) -> Result<(), String> {
        let all = rows(members)?;
        let top = all.last().ok_or("缺少顶层 / Missing top layer")?;
        let whole =
            top.members.len() == 1 || self.layout == Layout::Whole && can_cover(members, top);
        for &i in &top.members {
            let exterior = &mut members[i].exterior;
            exterior.roof = Roof::None;
            exterior.roof_width = None;
            exterior.top_handle = false;
        }
        if whole {
            let first = &mut members[top.members[0]];
            if first.preset.assembly_kind == AssemblyKind::Drawer {
                let cover = self.primary();
                first.exterior.roof = cover.kind;
                first.exterior.top_handle = cover.handle && cover.kind == Roof::Grid;
                if top.members.len() > 1 {
                    first.exterior.roof_width = Some(top.width.clone());
                }
            }
        } else {
            for (column, &i) in top.members.iter().enumerate() {
                let member = &mut members[i];
                if member.preset.assembly_kind != AssemblyKind::Drawer {
                    continue;
                }
                let cover = if self.layout == Layout::Whole {
                    self.primary()
                } else {
                    self.covers
                        .get(column)
                        .copied()
                        .unwrap_or_else(|| self.primary())
                };
                member.exterior.roof = cover.kind;
                member.exterior.top_handle = cover.handle && cover.kind == Roof::Grid;
            }
            // Remember an explicitly empty split without inventing a printed part.
            let first = &mut members[top.members[0]];
            first.exterior.roof_width = Some(first.preset.selection.width.clone());
        }
        Ok(())
    }
}
pub(super) fn layout(members: &[Member]) -> Result<Layout, String> {
    Ok(Choice::capture(members)?.layout)
}
pub(super) fn can_cover(members: &[Member], top: &crate::constraints::tower::Row) -> bool {
    top.members
        .iter()
        .all(|&i| members[i].preset.assembly_kind == AssemblyKind::Drawer)
}
pub(super) fn set_layout(members: &mut [Member], layout: Layout) -> Result<(), String> {
    let mut choice = Choice::capture(members)?;
    let all = rows(members)?;
    let top = all.last().ok_or("缺少顶层 / Missing top layer")?;
    if layout == Layout::Whole && !can_cover(members, top) {
        return Err(
            "翻盖箱自带顶盖；混合顶层请使用分体顶部 / Boxes keep their own lids; use a split top"
                .into(),
        );
    }
    if choice.layout == Layout::Whole && layout == Layout::Apart {
        choice.covers = vec![choice.primary(); top.members.len()];
    }
    choice.layout = layout;
    choice.apply(members)
}
pub(super) fn reconcile(original: &[Member], next: &mut [Member]) -> Result<(), String> {
    Choice::capture(original)?.apply(next)
}

pub(super) fn show(
    app: &App,
    ui: &mut egui::Ui,
    top: &crate::constraints::tower::Row,
    action: &mut Option<Action>,
) {
    let z = app.zh;
    let current = layout(&app.members).unwrap_or(Layout::Apart);
    let empty = top.members.iter().all(|&i| {
        app.members[i].preset.assembly_kind == AssemblyKind::Drawer
            && app.members[i].exterior.roof == Roof::None
    });
    let summary = format!(
        "{}{} · {}",
        top.width,
        top.depth,
        if current == Layout::Whole {
            t(z, "整块", "Whole")
        } else {
            t(z, "分体", "Split")
        }
    );
    super::layers::section(
        ui,
        "tower-roofs",
        if empty {
            t(z, "无顶盖", "No top cover")
        } else {
            t(z, "顶盖", "Top cover")
        },
        &summary,
        empty,
        app.smoke && std::env::args().any(|a| a == "--smoke-top"),
        |ui| {
            ui.small(t(
                z,
                "顶部宽深跟随整体尺寸；组合方式独立于抽屉层。",
                "Top width and depth follow the overall size, independently of drawer splits.",
            ));
            if top.members.len() > 1 {
                let mut selected = current;
                ui.label(t(z, "组合方式", "Arrangement"));
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(can_cover(&app.members, top), |ui| {
                        ui.selectable_value(
                            &mut selected,
                            Layout::Whole,
                            format!("{} {}{}", t(z, "整块", "Whole"), top.width, top.depth),
                        );
                    })
                    .response
                    .on_hover_text(t(
                        z,
                        "整块顶盖需要完整的抽屉顶层；翻盖箱保留自带顶盖。",
                        "A whole roof requires drawer modules; boxes retain their hinged lids.",
                    ));
                    ui.selectable_value(
                        &mut selected,
                        Layout::Apart,
                        top.members
                            .iter()
                            .map(|&i| {
                                format!("{}{}", app.members[i].preset.selection.width, top.depth)
                            })
                            .collect::<Vec<_>>()
                            .join(" + "),
                    );
                });
                if selected != current {
                    *action = Some(Action::RoofLayout(selected));
                }
            }
            let owners = if current == Layout::Whole {
                vec![top.members[0]]
            } else {
                top.members.clone()
            };
            for (column, i) in owners.into_iter().enumerate() {
                let member = &app.members[i];
                ui.push_id(("tower-roof", i), |ui| {
                    let width = if current == Layout::Whole {
                        &top.width
                    } else {
                        &member.preset.selection.width
                    };
                    if current == Layout::Apart {
                        ui.small(format!(
                            "{} · {width}{}",
                            if column == 0 {
                                t(z, "左侧", "Left")
                            } else {
                                t(z, "右侧", "Right")
                            },
                            top.depth
                        ));
                    } else {
                        ui.small(format!(
                            "{} · {width}{}",
                            t(z, "顶部规格", "Top size"),
                            top.depth
                        ));
                    }
                    if member.preset.assembly_kind == AssemblyKind::Sustainer {
                        ui.label(t(z, "箱子自带顶盖", "Box hinged lid"));
                        return;
                    }
                    let mut roof = member.exterior.roof;
                    egui::ComboBox::from_id_salt("roof")
                        .width(ui.available_width())
                        .selected_text(roof_name(roof, z))
                        .show_ui(ui, |ui| {
                            for value in [
                                Roof::None,
                                Roof::Table,
                                Roof::Grid,
                                Roof::Bowl,
                                Roof::UnderTable,
                            ] {
                                ui.selectable_value(&mut roof, value, roof_name(value, z));
                            }
                        });
                    if roof != member.exterior.roof {
                        *action = Some(Action::Roof(i, roof));
                    }
                    let mut handle = member.exterior.top_handle;
                    if ui
                        .add_enabled(
                            roof == Roof::Grid,
                            egui::Checkbox::new(
                                &mut handle,
                                t(z, "顶部提手（Grid 顶盖）", "Top handle (Grid roof)"),
                            ),
                        )
                        .changed()
                    {
                        *action = Some(Action::TopHandle(i, handle));
                    }
                });
            }
        },
    );
}
