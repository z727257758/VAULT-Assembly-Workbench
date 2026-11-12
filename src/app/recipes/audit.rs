use super::*;

const TOLERANCE: f32 = 0.01;
const LIMITS: &str = "Triangle-surface crossings at sampled poses only. Coplanar contact, full containment, continuous motion, clearances, fasteners and physical fit are not certified. Envelope overlap is not a crossing finding.";
#[derive(Clone, Serialize)]
struct LocalPair {
    a: String,
    b: String,
    instance_a: u32,
    instance_b: u32,
    body_a: Option<u32>,
    body_b: Option<u32>,
}
#[derive(Serialize)]
struct MemberCheck {
    name: String,
    crossings: Vec<LocalPair>,
}
#[derive(Serialize)]
struct MemberPair {
    member_a: usize,
    member_b: usize,
    a: String,
    b: String,
    instance_a: u32,
    instance_b: u32,
    point_mm: [f32; 3],
}
#[derive(Serialize)]
struct PoseCheck {
    explosion: f32,
    crossings: Vec<MemberPair>,
}
#[derive(Serialize)]
struct RecipeCheck {
    id: String,
    name: String,
    error: Option<String>,
    members: Vec<MemberCheck>,
    between_members: Vec<PoseCheck>,
}
#[derive(Serialize)]
struct Report {
    tolerance_mm: f32,
    limitations: &'static str,
    recipes: Vec<RecipeCheck>,
}

pub(crate) fn run(home: &Path) -> Result<(), String> {
    let root = std::env::var("VAULT_MODEL_ROOT")
        .ok()
        .map(PathBuf::from)
        .or_else(|| Settings::load(home).and_then(|s| s.startup_root()))
        .ok_or("Select a model library first, or set VAULT_MODEL_ROOT")?;
    let args: Vec<_> = std::env::args().collect();
    let filter = args
        .iter()
        .position(|a| a == "--preset-id")
        .map(|i| args.get(i + 1).cloned().ok_or("Missing --preset-id value"))
        .transpose()?;
    if filter
        .as_ref()
        .is_some_and(|id| !RECIPES.iter().any(|r| r.id == id))
    {
        return Err("Unknown --preset-id".into());
    }
    let catalog = Catalog::scan(root.clone())?;
    let folder = home.join("output/preset-audit");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut report = Report {
        tolerance_mm: TOLERANCE,
        limitations: LIMITS,
        recipes: vec![],
    };
    let mut cache = BTreeMap::<String, Vec<LocalPair>>::new();
    for recipe in RECIPES
        .iter()
        .filter(|r| filter.as_ref().is_none_or(|id| r.id == id))
    {
        println!("Auditing {} / {}", recipe.id, recipe.name(true));
        let mut result = RecipeCheck {
            id: recipe.id.into(),
            name: recipe.name(true).into(),
            error: None,
            members: vec![],
            between_members: vec![],
        };
        let check = (|| -> Result<(), String> {
            let project = recipe.build(&catalog, &Default::default(), true)?;
            std::fs::write(
                folder.join(format!("{}.toml", recipe.id)),
                toml::to_string_pretty(&project).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let scene =
                super::super::combination::load(&root, home, &project.members, 0., &cancel)?;
            for (index, member) in project.members.iter().enumerate() {
                let key = serde_json::to_string(&member.preset).map_err(|e| e.to_string())?;
                if !cache.contains_key(&key) {
                    println!("  internal: {}", member.name);
                    let local = Scene {
                        parts: scene
                            .parts
                            .iter()
                            .filter(|p| {
                                (p.pose.instance / crate::scene::MEMBER_STRIDE) as usize == index
                            })
                            .cloned()
                            .collect(),
                        ..Default::default()
                    };
                    let diagnostic = crate::collision::check(&local, TOLERANCE)?;
                    let pairs = diagnostic
                        .pairs
                        .iter()
                        .map(|pair| {
                            let a = &local.parts[pair.a].pose;
                            let b = &local.parts[pair.b].pose;
                            LocalPair {
                                a: a.path.clone(),
                                b: b.path.clone(),
                                instance_a: a.instance % crate::scene::MEMBER_STRIDE,
                                instance_b: b.instance % crate::scene::MEMBER_STRIDE,
                                body_a: a.body,
                                body_b: b.body,
                            }
                        })
                        .collect();
                    cache.insert(key.clone(), pairs);
                }
                result.members.push(MemberCheck {
                    name: member.name.clone(),
                    crossings: cache[&key].clone(),
                });
            }
            for explosion in [0., 0.45, 1.] {
                println!("  between members: {:.0}%", explosion * 100.);
                let pairs = crate::collision::check_between_members(&scene, TOLERANCE, explosion)?;
                result.between_members.push(PoseCheck {
                    explosion,
                    crossings: pairs
                        .into_iter()
                        .map(|pair| {
                            let a = &scene.parts[pair.a].pose;
                            let b = &scene.parts[pair.b].pose;
                            MemberPair {
                                member_a: (a.instance / crate::scene::MEMBER_STRIDE) as usize + 1,
                                member_b: (b.instance / crate::scene::MEMBER_STRIDE) as usize + 1,
                                a: a.path.clone(),
                                b: b.path.clone(),
                                instance_a: a.instance % crate::scene::MEMBER_STRIDE,
                                instance_b: b.instance % crate::scene::MEMBER_STRIDE,
                                point_mm: pair.point.to_array(),
                            }
                        })
                        .collect(),
                });
            }
            Ok(())
        })();
        if let Err(error) = check {
            println!("  INCOMPLETE: {error}");
            result.error = Some(error);
        }
        report.recipes.push(result);
        write_report(&folder, &report)?;
    }
    if report.recipes.iter().any(|r| r.error.is_some()) {
        return Err(format!(
            "Some preset checks are incomplete; see {}",
            folder.join("report.md").display()
        ));
    }
    println!(
        "Completed {} presets; report: {}",
        report.recipes.len(),
        folder.join("report.md").display()
    );
    Ok(())
}

fn write_report(folder: &Path, report: &Report) -> Result<(), String> {
    std::fs::write(
        folder.join("report.json"),
        serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut text = String::from(
        "# 组合预设表面相交核查\n\n容差 0.01 mm。数字为检测到的零件表面相交对数。成员内部按相同配置复用诊断；成员之间逐组合、逐展示状态重新检查。\n\n| 预设 | 成员内部 | 闭合时成员之间 | 45% 展开 | 完整展开 | 执行状态 |\n| --- | ---: | ---: | ---: | ---: | --- |\n",
    );
    for recipe in &report.recipes {
        let counts: Vec<_> = recipe
            .between_members
            .iter()
            .map(|p| p.crossings.len().to_string())
            .collect();
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            recipe.name,
            recipe
                .members
                .iter()
                .map(|m| m.crossings.len())
                .sum::<usize>(),
            counts.first().map(String::as_str).unwrap_or("未完成"),
            counts.get(1).map(String::as_str).unwrap_or("未完成"),
            counts.get(2).map(String::as_str).unwrap_or("未完成"),
            recipe.error.as_deref().unwrap_or("完成")
        ));
    }
    text.push_str("\n本核查只检测采样姿态下的非共面三角面表面相交。不覆盖完全包容、共面接触、连续运动、公差、五金、承载或实体试装。未检出相交不表示装配已通过 CAD 或制造验证。详细零件路径与定位坐标见 report.json；原始模型只读。\n");
    std::fs::write(folder.join("report.md"), text).map_err(|e| e.to_string())
}
