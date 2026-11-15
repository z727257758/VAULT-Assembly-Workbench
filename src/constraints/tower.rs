//! Tower and drawer-capacity rules, separate from UI and file matching.
pub const DRAWER_STRIDE: u32 = 1000;
pub const SLOT_PITCH: f32 = super::AssemblyRules::DRAWER_SLOT_PITCH;
pub fn units(height: &str) -> Result<u8, String> {
    match height {
        "1F" => Ok(1),
        "2F" => Ok(2),
        "4F" => Ok(4),
        _ => Err("未知层高 / Unknown height".into()),
    }
}
pub fn drawer_heights(module: &str, drawers: &[&str]) -> Result<(), String> {
    let capacity = units(module)?;
    if drawers.is_empty()
        || drawers.len() > 4
        || drawers
            .iter()
            .map(|h| units(h))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .sum::<u8>()
            != capacity
    {
        return Err("抽屉高度之和必须等于模块层高 / Drawer heights must fill the module".into());
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Row {
    pub level: u32,
    pub members: Vec<usize>,
    pub height: String,
    pub width: String,
    pub depth: u8,
}
pub fn rows(modules: &[super::stacking::Module<'_>]) -> Result<Vec<Row>, String> {
    let plan = super::stacking::plan(modules, 0.)?;
    let mut by_level = std::collections::BTreeMap::<u32, Vec<usize>>::new();
    for (i, m) in modules.iter().enumerate() {
        by_level.entry(m.placement.level).or_default().push(i);
    }
    let mut rows = Vec::new();
    let mut overall = None;
    for (level, mut ids) in by_level {
        ids.sort_by(|a, b| plan.left[*a].total_cmp(&plan.left[*b]));
        let first = &modules[ids[0]];
        let widths: Vec<_> = ids
            .iter()
            .map(|i| modules[*i].selection.width.as_str())
            .collect();
        let width=match widths.as_slice() {[single]=>*single,["S","S"]=>"L",["S","M"]|["M","S"]=>"XL",_=>return Err("当前组合含独立并列装配，请新建统一宽深的层式项目；原组合仍可查看和保存。 / Independent towers: start a new layer project; existing design is preserved.".into())};
        if plan.left[ids[0]].abs() > 0.01
            || ids.iter().any(|i| {
                modules[*i].selection.depth != first.selection.depth
                    || modules[*i].selection.height != first.selection.height
            })
        {
            return Err("各层须完整覆盖同一宽度、深度，层内模块须等高 / Layers must share a complete footprint and height".into());
        }
        let key = (width.to_owned(), first.selection.depth);
        if overall.as_ref().is_some_and(|old| old != &key) {
            return Err("当前组合不是统一宽深的完整塔 / Incomplete common-footprint tower".into());
        }
        overall = Some(key);
        rows.push(Row {
            level,
            members: ids,
            height: first.selection.height.clone(),
            width: width.into(),
            depth: first.selection.depth,
        });
    }
    Ok(rows)
}
pub fn columns(width: &str, split: bool) -> Result<Vec<&str>, String> {
    match (width, split) {
        ("L", true) => Ok(vec!["S", "S"]),
        ("XL", true) => Ok(vec!["S", "M"]),
        ("S" | "M" | "L" | "XL", false) => Ok(vec![width]),
        _ => Err("仅 L 支持 S＋S，XL 支持 S＋M / Unsupported split".into()),
    }
}

pub fn retain_present(scene: &mut crate::scene::Scene) {
    let ids: std::collections::HashSet<_> = scene.parts.iter().map(super::PartId::of).collect();
    scene
        .relations
        .retain(|r| ids.contains(&r.parent) && ids.contains(&r.child));
    scene
        .mating_checks
        .retain(|r| ids.contains(&r.parent) && ids.contains(&r.child));
}
pub fn namespace_drawer(scene: &mut crate::scene::Scene, drawer: usize) -> Result<(), String> {
    if drawer >= 4 || scene.parts.iter().any(|p| p.pose.instance >= DRAWER_STRIDE) {
        return Err("子抽屉实例标识超出范围 / Drawer instance namespace exhausted".into());
    }
    let offset = (drawer as u32 + 1) * DRAWER_STRIDE;
    for part in &mut scene.parts {
        part.pose.instance += offset;
    }
    for relation in &mut scene.relations {
        relation.parent.instance += offset;
        relation.child.instance += offset;
    }
    for check in &mut scene.mating_checks {
        check.parent.instance += offset;
        check.child.instance += offset;
    }
    Ok(())
}
pub fn bind_drawer_roots(
    scene: &mut crate::scene::Scene,
    parent: &super::PartId,
    parent_inverse: glam::Mat4,
    drawer: usize,
    mounts: &[super::SliderMount],
) -> Result<(), String> {
    super::AssemblyRules::bind_drawer_roots(scene, parent, parent_inverse, drawer, mounts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drawer_capacity_is_exact() {
        assert!(drawer_heights("2F", &["1F", "1F"]).is_ok());
        assert!(drawer_heights("4F", &["2F", "1F", "1F"]).is_ok());
        assert!(drawer_heights("2F", &["2F", "1F"]).is_err());
        assert!(drawer_heights("4F", &["1F"]).is_err());
    }
}
