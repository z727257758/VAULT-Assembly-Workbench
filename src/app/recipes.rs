//! Built-in project recipes. No source models or calibrated transforms live here.
use super::*;

#[derive(Clone, Copy)]
struct Cell {
    width: &'static str,
    height: &'static str,
    kind: AssemblyKind,
    drawers: &'static [&'static str],
    split_lid: bool,
}
const fn drawer(
    width: &'static str,
    height: &'static str,
    drawers: &'static [&'static str],
) -> Cell {
    Cell {
        width,
        height,
        kind: AssemblyKind::Drawer,
        drawers,
        split_lid: false,
    }
}
const fn box_cell(width: &'static str, height: &'static str, split_lid: bool) -> Cell {
    Cell {
        width,
        height,
        kind: AssemblyKind::Sustainer,
        drawers: &[],
        split_lid,
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Group {
    Compact,
    Workbench,
    Wide,
    Mixed,
}

impl Group {
    pub const ALL: [Self; 4] = [Self::Compact, Self::Workbench, Self::Wide, Self::Mixed];

    pub fn title(self, zh: bool) -> &'static str {
        match self {
            Self::Compact => t(zh, "紧凑与便携", "Compact & portable"),
            Self::Workbench => t(zh, "桌面与工作台", "Desktop & workbench"),
            Self::Wide => t(zh, "宽幅组合", "Wide combinations"),
            Self::Mixed => t(zh, "箱抽组合", "Boxes & drawers"),
        }
    }
}

pub(super) struct Recipe {
    pub id: &'static str,
    pub group: Group,
    cn: &'static str,
    en: &'static str,
    detail_cn: &'static str,
    detail_en: &'static str,
    depth: u8,
    rows: &'static [&'static [Cell]],
}
pub(super) const RECIPES: &[Recipe] = &[
    Recipe {
        id: "compact-s3",
        group: Group::Compact,
        cn: "S3 紧凑四抽",
        en: "S3 compact four drawers",
        detail_cn: "三层：2F 双抽＋1F＋1F。",
        detail_en: "Three levels: two 1F drawers in a 2F shell, then 1F + 1F.",
        depth: 3,
        rows: &[
            &[drawer("S", "2F", &["1F", "1F"])],
            &[drawer("S", "1F", &[])],
            &[drawer("S", "1F", &[])],
        ],
    },
    Recipe {
        id: "daily-m4",
        group: Group::Workbench,
        cn: "M4 日常抽屉塔",
        en: "M4 everyday tower",
        detail_cn: "三层：2F＋2F 双抽＋1F。",
        detail_en: "Three levels: 2F, a shared 2F shell, then 1F.",
        depth: 4,
        rows: &[
            &[drawer("M", "2F", &[])],
            &[drawer("M", "2F", &["1F", "1F"])],
            &[drawer("M", "1F", &[])],
        ],
    },
    Recipe {
        id: "organizer-m5",
        group: Group::Workbench,
        cn: "M5 高低抽组合",
        en: "M5 mixed-height drawers",
        detail_cn: "4F 共用壳体放 2F＋1F＋1F，上方加 1F。",
        detail_en: "A 4F shell with 2F + 1F + 1F drawers, topped by 1F.",
        depth: 5,
        rows: &[
            &[drawer("M", "4F", &["2F", "1F", "1F"])],
            &[drawer("M", "1F", &[])],
        ],
    },
    Recipe {
        id: "wide-l4",
        group: Group::Wide,
        cn: "L4 整块与双列",
        en: "L4 whole and paired",
        detail_cn: "L 整块 2F 底层，S＋S 双抽层，L 整块 1F 顶层。",
        detail_en: "Whole L 2F, paired S+S shared shells, and whole L 1F.",
        depth: 4,
        rows: &[
            &[drawer("L", "2F", &[])],
            &[
                drawer("S", "2F", &["1F", "1F"]),
                drawer("S", "2F", &["1F", "1F"]),
            ],
            &[drawer("L", "1F", &[])],
        ],
    },
    Recipe {
        id: "bridge-l3",
        group: Group::Mixed,
        cn: "L3 双列配翻盖箱",
        en: "L3 paired drawers and box",
        detail_cn: "S＋S 的 2F 抽屉支承 L 规格 1F 翻盖箱。",
        detail_en: "Paired S+S 2F drawers support a whole L 1F box.",
        depth: 3,
        rows: &[
            &[drawer("S", "2F", &[]), drawer("S", "2F", &[])],
            &[box_cell("L", "1F", false)],
        ],
    },
    Recipe {
        id: "mixed-m3",
        group: Group::Mixed,
        cn: "M3 抽屉与翻盖箱",
        en: "M3 drawer and box",
        detail_cn: "2F 抽屉底层，1F 翻盖箱顶层。",
        detail_en: "A 2F drawer under a 1F box.",
        depth: 3,
        rows: &[&[drawer("M", "2F", &[])], &[box_cell("M", "1F", false)]],
    },
    Recipe {
        id: "tower-xl5",
        group: Group::Wide,
        cn: "XL5 S＋M 三层塔",
        en: "XL5 three-level S+M tower",
        detail_cn: "三层 S＋M：2F、2F 双抽、1F；不使用整块 XL 抽屉。",
        detail_en: "S+M on every level: 2F, shared 2F shells, then 1F.",
        depth: 5,
        rows: &[
            &[drawer("S", "2F", &[]), drawer("M", "2F", &[])],
            &[
                drawer("S", "2F", &["1F", "1F"]),
                drawer("M", "2F", &["1F", "1F"]),
            ],
            &[drawer("S", "1F", &[]), drawer("M", "1F", &[])],
        ],
    },
    Recipe {
        id: "mixed-xl4",
        group: Group::Mixed,
        cn: "XL4 S＋M 混合塔",
        en: "XL4 S+M mixed tower",
        detail_cn: "S＋M 的 2F 抽屉底层，顶层为 S 抽屉＋M 翻盖箱。",
        detail_en: "S+M 2F drawers under an S drawer and M box.",
        depth: 4,
        rows: &[
            &[drawer("S", "2F", &[]), drawer("M", "2F", &[])],
            &[drawer("S", "1F", &[]), box_cell("M", "1F", false)],
        ],
    },
    Recipe {
        id: "portable-s6",
        group: Group::Compact,
        cn: "S6 深款分盖组合",
        en: "S6 split-lid storage",
        detail_cn: "1F 抽屉底层，2F 分体顶盖翻盖箱。",
        detail_en: "A 1F drawer under a 2F split-lid box.",
        depth: 6,
        rows: &[&[drawer("S", "1F", &[])], &[box_cell("S", "2F", true)]],
    },
    Recipe {
        id: "deep-m7",
        group: Group::Workbench,
        cn: "M7 深款工作塔",
        en: "M7 deep work tower",
        detail_cn: "2F 双抽壳体，顶部为 1F 分体顶盖翻盖箱。",
        detail_en: "A shared 2F drawer shell topped by a 1F split-lid box.",
        depth: 7,
        rows: &[
            &[drawer("M", "2F", &["1F", "1F"])],
            &[box_cell("M", "1F", true)],
        ],
    },
];
impl Recipe {
    pub fn name(&self, zh: bool) -> &'static str {
        if zh { self.cn } else { self.en }
    }
    pub fn description(&self, zh: bool) -> &'static str {
        if zh { self.detail_cn } else { self.detail_en }
    }
    pub fn build(
        &self,
        catalog: &Catalog,
        colors: &crate::print_plate::ColorTheme,
        zh: bool,
    ) -> Result<CombinationPreset, String> {
        colors.validate()?;
        let mut members = Vec::new();
        for (level, cells) in self.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                let mut config = crate::print_plate::Config::default();
                colors.apply(&mut config);
                config.follow_global_colors = true;
                let mut preset = Preset {
                    selection: Selection {
                        width: cell.width.into(),
                        depth: self.depth,
                        height: cell.height.into(),
                        hidden_key: false,
                    },
                    assembly_kind: cell.kind,
                    sustainer_options: SustainerOptions {
                        split_roof: cell.split_lid,
                        ..Default::default()
                    },
                    sustainer_angle: 0.,
                    drawer_extension: 0.,

                    drawers: vec![],
                    bom: vec![],
                    print_config: Some(config),
                    gridfinity: Default::default(),
                    divider_layout: Default::default(),
                    sustainer_dividers: Default::default(),
                    label: crate::label::Config {
                        enabled: false,
                        text: String::new(),
                        ..Default::default()
                    },
                };
                super::tower::configure(&mut preset, catalog, false)?;
                if !cell.drawers.is_empty() {
                    if cell.kind != AssemblyKind::Drawer {
                        return Err("Only drawer cells can contain drawers".into());
                    }
                    preset = super::tower::drawer_pattern(&preset, catalog, cell.drawers, false)?;
                }
                members.push(Member {
                    name: format!(
                        "{} {:02} · {}{} {}",
                        t(zh, "第", "Level"),
                        level + 1,
                        cell.width,
                        self.depth,
                        cell.kind.label(zh)
                    ),
                    preset,
                    placement: crate::scene::Placement {
                        level: level as u32,
                        column: column as u32,
                        offset: [0.; 3],
                    },
                    exterior: Default::default(),
                });
            }
        }
        let preset = members
            .first()
            .ok_or("Empty combination recipe")?
            .preset
            .clone();
        let project = CombinationPreset {
            project_name: self.name(zh).into(),
            colors: colors.clone(),
            preset,
            members,
            active_member: 0,
            combination_gap: 0.,
        };
        project.validate(catalog)?;
        super::tower::rows(&project.members)?;
        Ok(project)
    }
}

mod audit;
pub(crate) use audit::run as audit;
mod smoke;

#[cfg(test)]
mod tests {
    use super::*;
    fn catalog() -> Catalog {
        let mut catalog = Catalog {
            root: "synthetic-library".into(),
            entries: vec![],
        };
        let mut paths = std::collections::BTreeSet::new();
        for recipe in RECIPES {
            for row in recipe.rows {
                for cell in *row {
                    for height in std::iter::once(cell.height).chain(cell.drawers.iter().copied()) {
                        let selection = Selection {
                            width: cell.width.into(),
                            depth: recipe.depth,
                            height: height.into(),
                            hidden_key: false,
                        };
                        if cell.kind == AssemblyKind::Drawer {
                            paths.extend(
                                Catalog::default()
                                    .bom(&selection)
                                    .into_iter()
                                    .map(|b| b.path),
                            );
                        } else {
                            let options = SustainerOptions {
                                split_roof: cell.split_lid,
                                ..Default::default()
                            };
                            for _ in 0..100 {
                                match catalog.sustainer_bom(&selection, options) {
                                    Ok(_) => break,
                                    Err(e) => {
                                        let path = e
                                            .strip_prefix("Missing Sustainer component: ")
                                            .expect("Unexpected synthetic catalog error");
                                        if catalog.resolve(path).is_some() {
                                            panic!("Repeated missing component");
                                        }
                                        catalog.entries.push(crate::catalog::Entry {
                                            path: path.into(),
                                            name: path.rsplit('/').next().unwrap().into(),
                                            category: 2,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for path in paths {
            if catalog.resolve(&path).is_none() {
                catalog.entries.push(crate::catalog::Entry {
                    name: path.rsplit('/').next().unwrap().into(),
                    path,
                    category: 2,
                });
            }
        }
        catalog
    }
    #[test]
    fn all_recipes_validate_and_roundtrip_with_unique_ids_and_common_footprints() {
        let catalog = catalog();
        let mut ids = std::collections::HashSet::new();
        assert!(RECIPES.len() >= 10);
        for recipe in RECIPES {
            assert!(ids.insert(recipe.id));
            for zh in [true, false] {
                let project = recipe
                    .build(&catalog, &Default::default(), zh)
                    .unwrap_or_else(|e| panic!("{}: {e}", recipe.id));
                let rows = super::super::tower::rows(&project.members).unwrap();
                assert_eq!(rows.len(), recipe.rows.len());
                assert_eq!(
                    project.members.len(),
                    recipe.rows.iter().map(|r| r.len()).sum::<usize>()
                );
                let restored: CombinationPreset =
                    toml::from_str(&toml::to_string(&project).unwrap()).unwrap();
                restored.validate(&catalog).unwrap();
                assert!(
                    restored
                        .members
                        .iter()
                        .all(|m| m.preset.selection.width != "XL")
                );
            }
        }
    }
    #[test]
    fn missing_source_rejects_entire_recipe_and_does_not_modify_existing_project() {
        let mut catalog = catalog();
        let existing = RECIPES[0]
            .build(&catalog, &Default::default(), true)
            .unwrap();
        let before = toml::to_string(&existing).unwrap();
        let missing = existing.members[0].preset.drawers[0].bom[0].path.clone();
        catalog.entries.retain(|e| e.path != missing);
        assert!(
            RECIPES[0]
                .build(&catalog, &Default::default(), true)
                .is_err()
        );
        assert_eq!(before, toml::to_string(&existing).unwrap());
        assert!(RECIPES.iter().all(|r| {
            r.build(&Catalog::default(), &Default::default(), true)
                .is_err()
        }));
    }
    #[test]
    fn new_projects_keep_independent_drawer_contents_and_follow_the_supplied_palette() {
        let catalog = catalog();
        let mut colors = crate::print_plate::ColorTheme::default();
        colors.palette[0] = [12, 34, 56];
        let mut first = RECIPES[0].build(&catalog, &colors, true).unwrap();
        let other = RECIPES[0].build(&catalog, &colors, true).unwrap();
        first.members[0].preset.drawers[0].label.text = "ONLY THIS DRAWER".into();
        assert!(first.members[0].preset.drawers[1].label.text.is_empty());
        assert!(other.members[0].preset.drawers[0].label.text.is_empty());
        for member in &first.members {
            for p in std::iter::once(&member.preset).chain(member.preset.drawers.iter()) {
                let config = p.print_config.as_ref().unwrap();
                assert_eq!(config.palette[0], [12, 34, 56]);
                assert!(config.follow_global_colors);
                assert!(p.gridfinity.bins.is_empty() && p.divider_layout.items.is_empty());
            }
        }
    }
    #[test]
    fn invalid_drawer_capacity_and_unequal_paired_heights_are_rejected() {
        let catalog = catalog();
        const BAD_CAPACITY: &[&[Cell]] = &[&[drawer("M", "2F", &["1F"])]];
        const UNEVEN: &[&[Cell]] = &[&[drawer("S", "1F", &[]), drawer("S", "2F", &[])]];
        let bad_capacity = Recipe {
            rows: BAD_CAPACITY,
            ..RECIPES[1]
        };
        assert!(
            bad_capacity
                .build(&catalog, &Default::default(), true)
                .is_err()
        );
        let uneven = Recipe {
            rows: UNEVEN,
            ..RECIPES[4]
        };
        assert!(uneven.build(&catalog, &Default::default(), true).is_err());
    }
}
