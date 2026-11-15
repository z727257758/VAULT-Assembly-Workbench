//! Compose independent drawer mechanisms inside a shared full-height shell.
use super::Scene;
use crate::constraints::PartId;
use glam::Vec3;
fn retain(scene: &mut Scene, housing: bool) {
    scene
        .parts
        .retain(|p| crate::catalog::tower::housing(&p.pose.path) == housing);
    crate::constraints::tower::retain_present(scene);
}
pub fn shared_shell(shell: Scene, drawers: Vec<(Scene, u8)>) -> Result<Scene, String> {
    crate::constraints::AssemblyRules::shared_shell(shell, drawers)
}
pub(crate) fn compose(mut shell: Scene, drawers: Vec<(Scene, u8)>) -> Result<Scene, String> {
    retain(&mut shell, true);
    let parent = shell
        .parts
        .iter()
        .find(|p| p.pose.path.contains("/Modules/"))
        .ok_or("缺少共享模块 / Shared housing missing")?;
    let parent_id = PartId::of(parent);
    let frame = parent.pose.matrix(0.);
    let inverse = frame.inverse();
    let (lo, hi) = crate::constraints::world_bounds(parent);
    let mut remaining: u8 = drawers.iter().map(|(_, height)| *height).sum();
    for (index, (mut scene, height)) in drawers.into_iter().enumerate() {
        remaining -= height;
        let module = scene
            .parts
            .iter()
            .find(|p| p.pose.path.contains("/Modules/"))
            .ok_or("缺少抽屉定位基准 / Drawer datum missing")?;
        let (a, b) = crate::constraints::world_bounds(module);
        let shift = Vec3::new(
            (lo.x + hi.x - a.x - b.x) * 0.5,
            lo.y - a.y + remaining as f32 * crate::constraints::tower::SLOT_PITCH,
            hi.z - b.z,
        );
        for part in &mut scene.parts {
            part.pose.position = (Vec3::from(part.pose.position) + shift).to_array();
        }
        crate::constraints::tower::namespace_drawer(&mut scene, index)?;
        let mounts = crate::constraints::AssemblyRules::drawer_mounts(&scene)?;
        retain(&mut scene, false);
        crate::constraints::tower::bind_drawer_roots(
            &mut scene, &parent_id, inverse, index, &mounts,
        )?;
        shell.parts.extend(scene.parts);
        shell.relations.extend(scene.relations);
        shell.mating_checks.extend(scene.mating_checks);
    }
    crate::constraints::solve(&mut shell)?;
    shell.calibrated = false;
    shell.constraint_warning=Some("多抽屉按源模块槽距估算；接口与承载尚未实物验证 / Drawer positions use estimated module slots; fit and load unverified".into());
    Ok(shell)
}
