use super::*;
use crate::scene::Target;
use std::collections::BTreeSet;

type BomKey = (String, Option<(usize, Option<usize>)>);

#[derive(Default)]
pub(super) struct LayerBom {
    lines: BTreeMap<BomKey, Vec<Bom>>,
}

pub(super) fn layer_title(level: u32, zh: bool) -> String {
    if zh {
        format!("第 {} 层", level + 1)
    } else {
        format!("Layer {}", level + 1)
    }
}

fn layer_members(members: &[Member], level: u32) -> impl Iterator<Item = usize> {
    members
        .iter()
        .enumerate()
        .filter(move |(_, m)| m.placement.level == level)
        .map(|(i, _)| i)
}

pub(super) fn set_layer_checked(
    members: &[Member],
    selection: &mut BTreeSet<usize>,
    level: u32,
    checked: bool,
) {
    for i in layer_members(members, level) {
        if checked {
            selection.insert(i);
        } else {
            selection.remove(&i);
        }
    }
}

impl App {
    pub(super) fn layer_export_checkbox(&mut self, ui: &mut egui::Ui, level: u32) {
        let (count, selected) = layer_members(&self.members, level).fold((0, 0), |(n, s), i| {
            (n + 1, s + usize::from(self.member_selection.contains(&i)))
        });
        let mut checked = count > 0 && selected == count;
        let mixed = selected > 0 && !checked;
        if ui
            .add(
                egui::Checkbox::new(&mut checked, t(self.zh, "勾选导出", "Include in export"))
                    .indeterminate(mixed),
            )
            .changed()
        {
            set_layer_checked(&self.members, &mut self.member_selection, level, checked);
        }
    }
}

pub(super) fn aligned_row(
    ui: &mut egui::Ui,
    right: impl FnOnce(&mut egui::Ui),
    left: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            right(ui);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                left(ui);
            });
        });
    });
}

impl LayerBom {
    pub(super) fn add(&mut self, item: Bom, target: Target) {
        // Generated paths carry local bin IDs. The same ID can represent
        // different geometry or lettering in different drawers.
        let owner = item
            .path
            .starts_with(crate::gridfinity::PREFIX)
            .then_some((target.member, target.drawer));
        self.lines
            .entry((item.path.clone(), owner))
            .or_default()
            .push(item);
    }

    pub(super) fn lines(&self) -> impl Iterator<Item = &[Bom]> {
        self.lines.values().map(Vec::as_slice)
    }

    pub(super) fn enabled_copies(&self) -> u32 {
        self.lines()
            .flatten()
            .filter(|item| item.enabled)
            .map(|item| item.qty)
            .sum()
    }

    pub(super) fn show(&self, ui: &mut egui::Ui, zh: bool) {
        for items in self.lines() {
            let copies: u32 = items
                .iter()
                .filter(|item| item.enabled)
                .map(|item| item.qty)
                .sum();
            if copies == 0 {
                continue;
            }
            let item = &items[0];
            let name = crate::accessories::info(&item.path, zh)
                .map_or_else(|| item.name().to_owned(), |info| info.name);
            aligned_row(
                ui,
                |ui| {
                    ui.label(format!("× {copies}"));
                },
                |ui| {
                    ui.add(egui::Label::new(name).truncate())
                        .on_hover_text(&item.path);
                },
            );
        }
    }
}

pub(super) fn project_layers(members: &[Member], catalog: &Catalog) -> BTreeMap<u32, LayerBom> {
    let mut layers = BTreeMap::<u32, LayerBom>::new();
    for (index, member) in members.iter().enumerate() {
        let layer = layers.entry(member.placement.level).or_default();
        for job in member.preset.print_jobs(&member.name) {
            let target = Target {
                member: index,
                drawer: job.drawer,
            };
            for item in job.items {
                layer.add(item, target);
            }
        }
        if let Ok(extras) = catalog.exterior_components(
            &member.preset.selection,
            &member.exterior,
            member.preset.assembly_kind == AssemblyKind::Sustainer,
        ) {
            let target = Target {
                member: index,
                drawer: None,
            };
            for extra in extras {
                layer.add(extra.bom, target);
            }
        }
    }
    layers
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

pub(super) struct PrintBatch {
    name: String,
    config: crate::print_plate::Config,
    jobs: Vec<(usize, tower::PrintJob)>,
}

pub(super) struct PreparedBatch {
    pub name: String,
    pub config: crate::print_plate::Config,
    pub items: Vec<Bom>,
    pub owners: Vec<crate::scene::Target>,
    pub plan: crate::export::PreparedPrint,
}

pub(super) fn batch_jobs(
    jobs: Vec<(usize, tower::PrintJob)>,
    planning: crate::export::Planning,
    project: &str,
) -> Vec<PrintBatch> {
    let mut batches: Vec<PrintBatch> = Vec::new();
    for (index, job) in jobs {
        let config = job.preset.print_config.clone().unwrap_or_default();
        if planning == crate::export::Planning::Combined
            && let Some(batch) = batches
                .iter_mut()
                .find(|b| b.config.shares_print_project(&config))
        {
            batch.jobs.push((index, job));
        } else {
            let name = if planning == crate::export::Planning::Combined {
                project.to_owned()
            } else {
                job.name.clone()
            };
            batches.push(PrintBatch {
                name,
                config,
                jobs: vec![(index, job)],
            });
        }
    }
    batches
}

impl PrintBatch {
    pub(super) fn prepare(
        self,
        root: &Path,
        home: &Path,
        cancel: Option<&crate::preview::Cancel>,
    ) -> Result<PreparedBatch, String> {
        let mut items = Vec::new();
        let mut owners = Vec::new();
        let mut plan = crate::export::PreparedPrint::default();
        for (member, job) in self.jobs {
            crate::preview::check(cancel)?;
            let config = job.preset.print_config.unwrap_or_default();
            let (mut inputs, geometry) = crate::export::prepare_job_geometry(
                root,
                &job.items,
                &job.preset.label,
                &config,
                Some((home, &job.preset.gridfinity, &job.preset.selection)),
            )
            .map_err(|e| format!("{}: {e}", job.name))?;
            owners.extend(std::iter::repeat_n(
                crate::scene::Target {
                    member,
                    drawer: job.drawer,
                },
                inputs.len(),
            ));
            items.append(&mut inputs);
            plan.append_geometry(geometry);
        }
        crate::preview::check(cancel)?;
        plan.pack(&items, &self.config)
            .map_err(|e| format!("{}: {e}", self.name))?;
        Ok(PreparedBatch {
            name: self.name,
            config: self.config,
            items,
            owners,
            plan,
        })
    }
}

impl App {
    pub(super) fn print_planning_ui(&mut self, ui: &mut egui::Ui) {
        use crate::export::Planning;
        let old = self.export_options.planning;
        let z = self.zh;
        ui.horizontal_wrapped(|ui| {
            ui.label(t(z, "摆盘", "Planning"));
            ui.selectable_value(&mut self.export_options.planning, Planning::PerMember, t(z, "按单体", "By member"));
            ui.selectable_value(&mut self.export_options.planning, Planning::Combined, t(z, "按整体", "Combined"))
                .on_hover_text(t(z, "相同打印设置和耗材槽的零件可以跨成员合盘。", "Parts can share plates across members with matching print settings and filament slots."));
        });
        if old != self.export_options.planning
            && let Err(e) = self.save_settings()
        {
            self.status = e;
        }
    }
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
