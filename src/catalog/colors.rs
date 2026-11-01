//! Semantic print-color categories shared across member sizes.
pub const ROLES: [(&str, &str, &str); 6] = [
    ("shell", "外壳 / 顶盖", "Shell / lid"),
    ("facade", "面板", "Facade"),
    ("insert", "内部收纳 / 隔板", "Inserts / dividers"),
    ("label", "标签底片", "Label body"),
    ("accessory", "其他配件", "Other accessories"),
    ("text", "标签文字", "Label text"),
];
/// Matching stays with the catalog. Unknown source families use accessory color.
pub fn role(path: &str) -> &'static str {
    let file = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if crate::label::is_label(path) || path.starts_with("generated/gridfinity/Cullenect-") {
        "label"
    } else if path.contains("/Facades/") || file.contains("facade") {
        "facade"
    } else if path.contains("/Modules/")
        || path.contains("/Covers/")
        || file.contains("cover")
        || file
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == "lid")
        || file.contains("roof")
        || path.contains("/Roofs/")
    {
        "shell"
    } else if path.starts_with("generated/gridfinity/Gridfinity-")
        || path.contains("/Gridfinity Baseplate/")
        || path.contains("/Boxes/")
        || file.contains("divider")
        || path.contains("/Drawers/")
    {
        "insert"
    } else {
        "accessory"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runner_slide_is_an_accessory_not_a_lid() {
        assert_eq!(
            role("01. DRAWERS/07. Universal/Runner Slide.stl"),
            "accessory"
        );
        assert_eq!(
            role("01. DRAWERS/01. S Families/Facades/S1F (Turn On Supports).stl"),
            "facade"
        );
        assert_eq!(
            role("02. SUSTAINERS/02. M Families/M4/Roof/M4 Roof.stl"),
            "shell"
        );
    }
}
