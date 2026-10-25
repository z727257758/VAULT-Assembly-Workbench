//! Human-readable accessory metadata. File paths remain the stable identifiers.
#[derive(Clone)]
pub struct Info {
    pub group: &'static str,
    pub name: String,
    pub description: &'static str,
}
fn tr(zh: bool, cn: &'static str, en: &'static str) -> &'static str {
    if zh { cn } else { en }
}
pub fn info(path: &str, zh: bool) -> Option<Info> {
    if path.starts_with(crate::gridfinity::PREFIX) {
        if let Some(id) = path
            .rsplit('/')
            .next()
            .unwrap_or("")
            .strip_prefix("Cullenect-")
            .and_then(|s| s.strip_suffix(".stl"))
        {
            return Some(Info {
                group: tr(zh, "Gridfinity 标牌", "Gridfinity labels"),
                name: format!(
                    "{} {id}",
                    tr(zh, "Cullenect 可拆标牌", "Cullenect removable label")
                ),
                description: tr(
                    zh,
                    "单独打印的 36 × 11 mm 标牌，卡入盒体标签台；配合待试打印确认。",
                    "Separately printed 36 x 11 mm label for the bin socket; physical fit unverified.",
                ),
            });
        }
        let id = path
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim_start_matches("Gridfinity-")
            .trim_end_matches(".stl");
        return Some(Info {
            group: tr(zh, "Gridfinity 盒子", "Gridfinity bins"),
            name: format!("{} {id}", tr(zh, "Gridfinity 收纳盒", "Gridfinity bin")),
            description: tr(
                zh,
                "由盒子编辑器生成，支持跨格尺寸、自动层高和标签文字。",
                "Generated from the layout, with grid sizes, automatic height and label text.",
            ),
        });
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    if !file.to_ascii_lowercase().ends_with(".stl") {
        return None;
    }
    let stem = file.strip_suffix(".stl").unwrap_or(file);
    let spec = stem.split_whitespace().next().unwrap_or("");
    let (group, title, description) = if path.starts_with("03. MODIFICATIONS/Base/") {
        (
            tr(zh, "底座", "Base"),
            tr(zh, "外部承托底座", "External support base"),
            tr(
                zh,
                "安装在 Module 下方的外部底座，可与内部收纳配置同时使用。",
                "External base beneath the module; compatible with drawer inserts.",
            ),
        )
    } else if path.contains("/Gridfinity Baseplate/") {
        (
            tr(zh, "收纳底板", "Storage baseplates"),
            if stem.contains("2 Handles") {
                tr(
                    zh,
                    "Gridfinity 底板 · 双提手",
                    "Gridfinity baseplate · two handles",
                )
            } else if stem.contains("1 Handle") {
                tr(
                    zh,
                    "Gridfinity 底板 · 单提手",
                    "Gridfinity baseplate · one handle",
                )
            } else {
                tr(
                    zh,
                    "Gridfinity 底板 · 标准版",
                    "Gridfinity baseplate · standard",
                )
            },
            tr(
                zh,
                "抽屉内的 Gridfinity 收纳底板；同规格选择一种提手版本。",
                "Gridfinity drawer insert; choose one handle variant per drawer.",
            ),
        )
    } else if path.contains("/Grid Dividers/") {
        let title = if stem.contains("pin_tight") {
            tr(zh, "分隔板连接销 · 紧配合", "Divider pin · tight")
        } else if stem.contains("pin_loose") {
            tr(zh, "分隔板连接销 · 松配合", "Divider pin · loose")
        } else if stem.contains("Straight Edge") {
            tr(zh, "分隔板直边端件", "Divider straight end")
        } else if stem.contains("Rounded Edge") {
            tr(zh, "分隔板圆角端件", "Divider rounded end")
        } else {
            tr(zh, "网格分隔板", "Grid divider")
        };
        let height = if path.contains("82.5MM") {
            "82.5 mm"
        } else if path.contains("34.5MM") {
            "34.5 mm"
        } else if path.contains("20MM") {
            "20 mm"
        } else {
            ""
        };
        return Some(Info {
            group: tr(zh, "内部分隔", "Internal dividers"),
            name: if stem.starts_with(|c: char| c.is_ascii_digit()) {
                format!("{title} · {height} · {stem}")
            } else {
                format!("{title} {height}").trim().into()
            },
            description: tr(
                zh,
                "按内部布局选择高度和长度编号；文件编号不是毫米尺寸。连接销松紧版本按实际配合选择。",
                "Choose height and length code for your layout; numeric file codes are not millimeters. Select pin fit as needed.",
            ),
        });
    } else if path.contains("/Geared Rails/") {
        (
            tr(zh, "齿条（必备）", "Geared rails (required)"),
            tr(
                zh,
                "上下锁止齿条 · 一份含两件",
                "Upper/lower locking racks · two bodies per file",
            ),
            tr(
                zh,
                "默认抽屉必备锁止件；一个源文件包含上下两根，展示时分别定位。",
                "Required locking racks in this drawer configuration; one source file contains the upper and lower racks.",
            ),
        )
    } else if path.contains("/Manual Supports/") {
        (
            tr(zh, "打印辅助", "Printing aids"),
            tr(zh, "手动打印支撑", "Manual print support"),
            tr(
                zh,
                "与所选深度配套的独立打印支撑，不是永久装配零件。",
                "Separate print support for the selected depth; not a permanent assembly part.",
            ),
        )
    } else if path.contains("/Wheels/") {
        let title = match stem {
            "Tyre (TPU)" => tr(zh, "轮胎 · TPU", "Tyre · TPU"),
            "Disk" => tr(zh, "轮盘", "Wheel disk"),
            "Bushing" => tr(zh, "轮轴衬套", "Wheel bushing"),
            "Cover" => tr(zh, "轮毂盖", "Wheel cover"),
            "Jig" => tr(zh, "车轮辅助夹具", "Wheel jig"),
            _ => stem,
        };
        return Some(Info {
            group: tr(zh, "移动轮组", "Wheels"),
            name: title.into(),
            description: tr(
                zh,
                "移动轮组的独立零件；按轮数分别配置各零件份数。轮胎文件标注使用 TPU。",
                "Individual wheel component; set copies for each component to suit wheel count. The tyre file specifies TPU.",
            ),
        });
    } else if path.contains("/Pegboard Attachment/") {
        return Some(Info {
            group: tr(zh, "洞洞板安装", "Pegboard mounts"),
            name: format!(
                "{} · {} · {stem}",
                tr(zh, "洞洞板连接件", "Pegboard adapter"),
                if path.contains("IKEA Skadis") {
                    "IKEA SKÅDIS"
                } else {
                    "USSA"
                }
            ),
            description: tr(
                zh,
                "按洞洞板制式及模块宽度、层高选择连接件。",
                "Match the pegboard system and module width/height.",
            ),
        });
    } else if path.contains("/Wall Mounts/") {
        (
            tr(zh, "壁挂安装", "Wall mounting"),
            if stem == "Wall Mount Second Part" {
                tr(zh, "壁挂座 · 配套第二件", "Wall mount · second part")
            } else {
                tr(zh, "壁挂座 · 主件", "Wall mount · main part")
            },
            tr(
                zh,
                "墙面安装配件；主件按模块宽度和层高筛选，第二件单独配置。",
                "Wall mounting accessory; main part matches width/height. Configure the second part separately.",
            ),
        )
    } else if path.contains("/Feet/") {
        (
            tr(zh, "底座与脚座", "Bases and feet"),
            tr(zh, "支撑脚座", "Support feet"),
            tr(
                zh,
                "对应宽度系列的支撑脚座；份数按源文件统计。",
                "Support feet for the matching width; copies count source files.",
            ),
        )
    } else if path.contains("/Base/") {
        (
            tr(zh, "底座与脚座", "Bases and feet"),
            if stem.contains("Connector") {
                tr(zh, "底座连接件", "Base connector")
            } else if stem.contains("Part 1") {
                tr(zh, "底座 · 分体第一件", "Base · split part 1")
            } else if stem.contains("Part 2") {
                tr(zh, "底座 · 分体第二件", "Base · split part 2")
            } else if stem.contains("Whole") {
                tr(zh, "底座 · 整体版", "Base · whole")
            } else {
                tr(zh, "底座", "Base")
            },
            tr(
                zh,
                "模块底部安装配件；整体版与分体版二选一，分体版需分别选择两件。",
                "Module base accessory; choose whole or split construction. Select both parts for a split base.",
            ),
        )
    } else if path.contains("/Backbone/") {
        (
            tr(zh, "背部连接与锁止", "Backbone and locks"),
            if stem.contains("Knob") {
                tr(zh, "提手锁紧旋钮", "Handle lock knob")
            } else if stem.contains("Handle Lock") {
                tr(zh, "抽拉提手锁止件", "Pull-out handle lock")
            } else if stem.contains("Connector") {
                tr(zh, "模块连接件", "Module connector")
            } else {
                tr(zh, "背部连接架", "Backbone")
            },
            tr(
                zh,
                "背部连接或提手锁止系统的独立配件；按对应方案选择通用或专用版本。",
                "Individual backbone or handle-lock component; choose universal or family-specific parts for your configuration.",
            ),
        )
    } else if path.contains("/Stationary Handle/")
        || path.contains("/Pull-out Handle/")
        || stem.starts_with("Top Handle")
        || stem.starts_with("Heawyweight Roof")
    {
        (
            tr(zh, "提手与顶盖附件", "Handles and roof accessories"),
            if stem.starts_with("Heawyweight Roof Insert") {
                tr(zh, "重载顶盖嵌件", "Heavyweight roof insert")
            } else if stem.starts_with("Heawyweight Roof Handle") {
                tr(zh, "重载顶盖提手", "Heavyweight roof handle")
            } else if stem.starts_with("Top Handle") {
                tr(
                    zh,
                    "顶部提手 · 打印需支撑",
                    "Top handle · print with supports",
                )
            } else if stem.contains("Stationary") {
                tr(zh, "固定提手", "Stationary handle")
            } else {
                tr(zh, "抽拉提手", "Pull-out handle")
            },
            tr(
                zh,
                "搬运或抽拉用提手配件；重载顶盖附件需与对应顶盖配套。",
                "Carrying/pull-out accessory; heavyweight roof parts require the corresponding roof.",
            ),
        )
    } else {
        return None;
    };
    let mut variant = if stem.contains("Knob") {
        String::new()
    } else if stem.starts_with("Universal") {
        format!(
            "{} {}",
            tr(zh, "通用", "Universal"),
            stem.split_whitespace()
                .find(|v| ["1F", "2F", "4F"].contains(v))
                .unwrap_or("")
        )
        .trim()
        .into()
    } else if stem.starts_with("Connector for Modules") {
        stem.split_whitespace().last().unwrap_or("").into()
    } else if stem.starts_with("Top Handle")
        || stem.starts_with("Heawyweight")
        || stem == "Base Connector"
        || stem == "Wall Mount Second Part"
    {
        String::new()
    } else {
        spec.to_string()
    };
    if path.contains("/Backbone/") {
        if stem.contains("Part 1") {
            variant = format!("{variant} {}", tr(zh, "第一件", "part 1"))
                .trim()
                .into();
        }
        if stem.contains("Part 2") {
            variant = format!("{variant} {}", tr(zh, "第二件", "part 2"))
                .trim()
                .into();
        }
    }
    Some(Info {
        group,
        name: if variant.is_empty() {
            title.into()
        } else {
            format!("{title} · {variant}")
        },
        description,
    })
}

/// Mutually exclusive construction options, scoped to the same model directory.
pub fn conflicts(a: &str, b: &str) -> bool {
    if a == b {
        return false;
    }
    let dir = |p: &str| p.rsplit_once('/').map(|(d, _)| d.to_string());
    if dir(a) != dir(b) {
        return false;
    }
    if a.contains("/Gridfinity Baseplate/") {
        return true;
    }
    if a.contains("/Base/") {
        let stem = |p: &str| {
            p.rsplit('/')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string()
        };
        return stem(a) == stem(b)
            && (a.contains(" Base Whole") && b.contains(" Base Part")
                || b.contains(" Base Whole") && a.contains(" Base Part"));
    }
    false
}

/// Length codes are documented in Grid Dividers/01. INSTRUCTIONS/widths.jpg.
pub fn divider_matches(path: &str, selection: &crate::catalog::Selection) -> bool {
    if path.contains("82.5MM") && selection.height == "1F" {
        return false;
    }
    let name = path.rsplit('/').next().unwrap_or("");
    let code: String = name.chars().take_while(char::is_ascii_digit).collect();
    let Ok(code) = code.parse::<u32>() else {
        return true;
    };
    let width = match selection.width.as_str() {
        "S" => 10,
        "M" => 18,
        "L" => 26,
        _ => 32,
    };
    code <= width.max(u32::from(selection.depth) * 4 - 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Bom, Catalog, Selection};
    #[test]
    fn drawer_options_only_and_required_racks_survive_old_presets() {
        let Ok(root) = std::env::var("VAULT_MODEL_ROOT") else {
            return;
        };
        let c = Catalog::scan(root.into()).unwrap();
        for width in ["S", "M", "L", "XL"] {
            let selection = Selection {
                width: width.into(),
                ..Default::default()
            };
            let entries = c.accessories(&selection);
            assert!(!entries.is_empty());
            for e in entries {
                assert!(
                    e.path.contains("/Gridfinity Baseplate/") || e.path.contains("/Grid Dividers/")
                );
                assert!(!e.path.contains("82.5MM"));
                for zh in [true, false] {
                    assert!(!info(&e.path, zh).unwrap().description.is_empty());
                }
            }
            let defaults = c.bom(&selection);
            let rack = defaults
                .iter()
                .find(|b| b.path.contains("/Geared Rails/"))
                .unwrap();
            assert!(rack.enabled && rack.qty == 1);
        }
        let s = Selection::default();
        let mut old = c.bom(&s);
        let divider = "01. DRAWERS/07. Universal/Grid Dividers/20MM TALL/5.stl".to_string();
        let plate =
            "01. DRAWERS/01. S Families/S3/Gridfinity Baseplate/S3 Baseplate.stl".to_string();
        Catalog::set_accessory(&mut old, divider.clone(), true, 4);
        Catalog::set_accessory(&mut old, plate.clone(), true, 1);
        Catalog::set_accessory(
            &mut old,
            "03. MODIFICATIONS/Wheels/Tyre (TPU).stl".into(),
            true,
            4,
        );
        let rack = old
            .iter_mut()
            .find(|b| b.path.contains("/Geared Rails/"))
            .unwrap();
        rack.qty = 2;
        rack.enabled = false;
        let next = Selection {
            width: "M".into(),
            ..s.clone()
        };
        let bom = c.reconfigure(&next, &old);
        assert!(
            bom.iter()
                .any(|b| b.path == divider && b.qty == 4 && b.enabled)
        );
        assert!(
            !bom.iter()
                .any(|b| b.path == plate || b.path.contains("/Wheels/"))
        );
        assert!(
            bom.iter()
                .any(|b| b.path.contains("/Geared Rails/") && b.qty == 1 && b.enabled)
        );
        let saved: Saved = toml::from_str(&toml::to_string(&Saved { bom }).unwrap()).unwrap();
        assert!(saved.bom.iter().any(|b| b.path == divider && b.qty == 4));
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Saved {
        bom: Vec<Bom>,
    }
    #[test]
    fn alternatives_replace_without_duplicates_and_keep_split_pair() {
        let mut bom = Vec::new();
        let base = "model/Gridfinity Baseplate/A.stl";
        let variant = "model/Gridfinity Baseplate/B.stl";
        Catalog::set_accessory(&mut bom, base.into(), true, 2);
        Catalog::set_accessory(&mut bom, variant.into(), true, 1);
        Catalog::set_accessory(&mut bom, variant.into(), true, 3);
        assert_eq!(bom.len(), 2);
        assert!(!bom[0].enabled && bom[1].enabled && bom[1].qty == 3);
        assert!(conflicts(
            "03. MODIFICATIONS/Base/S/S6 Base Whole.stl",
            "03. MODIFICATIONS/Base/S/S6 Base Part 1.stl"
        ));
        assert!(!conflicts(
            "03. MODIFICATIONS/Base/S/S6 Base Part 1.stl",
            "03. MODIFICATIONS/Base/S/S6 Base Part 2.stl"
        ));
        assert!(!conflicts(
            "03. MODIFICATIONS/Base/S/S7 Base Whole.stl",
            "03. MODIFICATIONS/Base/S/S6 Base Part 1.stl"
        ));
    }
}
