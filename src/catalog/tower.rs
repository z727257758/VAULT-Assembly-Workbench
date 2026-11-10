//! Manufacturing subsets for one shared shell with independently sized drawers.
use super::Bom;
pub fn housing(path: &str) -> bool {
    path.contains("/Modules/") || path.ends_with("/Latch.stl")
}
pub fn housing_items(items: &[Bom]) -> Vec<Bom> {
    items.iter().filter(|b| housing(&b.path)).cloned().collect()
}
pub fn drawer_items(items: &[Bom]) -> Vec<Bom> {
    items
        .iter()
        .filter(|b| !housing(&b.path))
        .cloned()
        .collect()
}
