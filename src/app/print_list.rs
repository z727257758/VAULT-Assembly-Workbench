use super::*;

#[derive(Default)]
pub(super) struct RowChanges {
    pub changed: bool,
    pub toggle: Option<(String, bool, u32)>,
    pub preview: Option<String>,
}

pub(super) fn rows<'a>(
    ui: &mut egui::Ui,
    items: impl Iterator<Item = &'a mut Bom>,
    selection: &Selection,
    zh: bool,
    details: Option<&Catalog>,
) -> RowChanges {
    let mut edits = RowChanges::default();
    for item in items {
        ui.push_id(item.path.clone(), |ui| {
            let rule = item.print_rule();
            let required = rule == crate::catalog::PrintRule::RequiredRails;
            let locked = required || rule == crate::catalog::PrintRule::LayoutManaged;
            item.normalize_print(selection);
            let name = crate::accessories::info(&item.path, zh)
                .map_or_else(|| item.name().to_owned(), |info| info.name);
            let checkbox = |ui: &mut egui::Ui| {
                let mut enabled = item.enabled;
                let response = ui.add_enabled(!locked, egui::Checkbox::new(&mut enabled, name));
                if response.changed() {
                    edits.toggle = Some((item.path.clone(), enabled, item.qty));
                }
                if required {
                    response.on_hover_text(t(
                        zh,
                        "必备：一份包含上下两根齿条",
                        "Required: one file contains both racks",
                    ));
                } else {
                    response.on_hover_text(&item.path);
                }
            };
            let mut qty = item.qty;
            let mut quantity = |ui: &mut egui::Ui| {
                edits.changed |= ui
                    .add_enabled(
                        !locked && rule != crate::catalog::PrintRule::FacadeCaps,
                        egui::DragValue::new(&mut qty).range(1..=100),
                    )
                    .on_hover_text(t(zh, "源 STL 文件份数", "Source STL copies"))
                    .changed();
            };
            if let Some(catalog) = details {
                checkbox(ui);
                ui.horizontal(|ui| {
                    ui.small(group_label(zh, &item.group));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(t(zh, "预览", "Preview")).clicked() {
                            edits.preview = Some(item.path.clone());
                        }
                        quantity(ui);
                        ui.small(t(zh, "份数", "Copies"));
                    });
                });
                if catalog.resolve(&item.path).is_none()
                    && !item.path.starts_with(crate::gridfinity::PREFIX)
                {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        t(zh, "源文件缺失", "Source file missing"),
                    );
                }
                ui.separator();
            } else {
                ui.horizontal_wrapped(|ui| {
                    checkbox(ui);
                    quantity(ui);
                });
            }
            item.qty = qty;
        });
    }
    edits
}

pub(super) fn checked_jobs<'a>(
    members: impl IntoIterator<Item = (usize, &'a Member)>,
    catalog: &Catalog,
) -> Result<Vec<(usize, tower::PrintJob)>, String> {
    let mut output = Vec::new();
    let mut copies = 0u64;
    for (index, member) in members {
        let p = &member.preset;
        let extras = catalog.exterior_components(
            &p.selection,
            &member.exterior,
            p.assembly_kind == AssemblyKind::Sustainer,
        )?;
        let mut jobs = p.print_jobs(&member.name);
        jobs[0].items.extend(extras.into_iter().map(|c| c.bom));
        for mut job in jobs {
            job.items.retain(|b| b.enabled);
            copies += job.items.iter().map(|b| u64::from(b.qty)).sum::<u64>();
            if copies > 1000 {
                return Err(
                    "单次排盘最多 1000 份 / Print layout supports up to 1000 copies".into(),
                );
            }
            if !job.items.is_empty() {
                output.push((index, job));
            }
        }
    }
    if output.is_empty() {
        return Err("打印清单没有勾选零件 / No checked parts in the print list".into());
    }
    Ok(output)
}

pub(super) fn set_accessory(
    selection: &Selection,
    bom: &mut Vec<Bom>,
    drawer_inserts: Option<(&mut crate::dividers::Layout, &mut crate::gridfinity::Layout)>,
    path: String,
    enabled: bool,
    qty: u32,
) -> Result<(), String> {
    let Some((dividers, bins)) = drawer_inserts else {
        Catalog::set_accessory(bom, path, enabled, qty);
        return Ok(());
    };
    let plate = path.contains("/Gridfinity Baseplate/");
    let divider = path.contains("/Grid Dividers/");
    if enabled && plate {
        let mut next = bins.clone();
        next.baseplate = path.clone();
        next.validate(selection)
            .map_err(|e| format!("{e}；请先调整盒子再切换底板"))?;
        *bins = next;
    }
    dividers.select_accessory(bom, selection, path, enabled, qty);
    if plate || divider {
        bins.baseplate = bom
            .iter()
            .find(|b| b.enabled && b.path.contains("/Gridfinity Baseplate/"))
            .map(|b| b.path.clone())
            .unwrap_or_default();
        if bins.baseplate.is_empty() {
            bins.bins.clear();
        }
        bins.sync_bom(bom);
    }
    Ok(())
}

pub(super) fn apply_project_edit(
    members: &mut [Member],
    target: crate::scene::Target,
    edited: Preset,
) {
    let owner = &mut members[target.member].preset;
    let destination = match target.drawer {
        Some(index) => &mut owner.drawers[index],
        None => owner,
    };
    destination.bom = edited.bom;
    destination.divider_layout = edited.divider_layout;
    destination.gridfinity = edited.gridfinity;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incompatible_plate_change_keeps_the_previous_selection_and_bins() {
        let selection = Selection::default();
        let full = "parts/Gridfinity Baseplate/Standard.stl".to_string();
        let narrower = "parts/Gridfinity Baseplate/Handle.stl".to_string();
        let mut bom = Vec::new();
        let mut dividers = crate::dividers::Layout::default();
        let mut bins = crate::gridfinity::Layout::default();
        set_accessory(
            &selection,
            &mut bom,
            Some((&mut dividers, &mut bins)),
            full.clone(),
            true,
            1,
        )
        .unwrap();
        let width = crate::gridfinity::grid_for(&selection, &full).unwrap()[0];
        bins.bins.push(crate::gridfinity::Bin {
            span: [width, 1],
            ..Default::default()
        });
        bins.sync_bom(&mut bom);
        let previous = serde_json::to_value((&bom, &bins, &dividers)).unwrap();
        assert!(
            set_accessory(
                &selection,
                &mut bom,
                Some((&mut dividers, &mut bins)),
                narrower,
                true,
                1
            )
            .is_err()
        );
        assert_eq!(
            serde_json::to_value((&bom, &bins, &dividers)).unwrap(),
            previous
        );
        set_accessory(
            &selection,
            &mut bom,
            Some((&mut dividers, &mut bins)),
            full,
            false,
            1,
        )
        .unwrap();
        assert!(bins.bins.is_empty() && bins.baseplate.is_empty());
        assert!(
            bom.iter()
                .all(|b| !b.path.starts_with(crate::gridfinity::PREFIX))
        );
    }
}
