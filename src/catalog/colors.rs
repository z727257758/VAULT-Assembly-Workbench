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

/// Color component identity ignores catalog size folders, but preserves part
/// numbers and variants. Unknown files retain their names rather than merging
/// unrelated accessories into a single bucket.
pub fn component(path: &str) -> String {
    let generated = crate::print_plate::color_key(path);
    if generated != path || path.starts_with("generated/") {
        return generated.to_owned();
    }
    fn size_token(s: &str) -> bool {
        if matches!(
            s,
            "1f" | "2f" | "4f" | "2-4f" | "s" | "m" | "l" | "xl" | "s-m" | "l-xl"
        ) {
            return true;
        }
        ["xl", "s", "m", "l", "x"].into_iter().any(|prefix| {
            s.strip_prefix(prefix).is_some_and(|tail| {
                matches!(tail, "3" | "4" | "5" | "6" | "7" | "1f" | "2f" | "4f")
            })
        })
    }
    let normalized = path
        .trim_end_matches(".stl")
        .to_ascii_lowercase()
        .replace(" (turn on supports)", "")
        .split('/')
        .filter_map(|segment| {
            let segment = segment.split_once(". ").map_or(segment, |(_, s)| s);
            if segment.ends_with(" families") || size_token(segment) {
                return None;
            }
            let words: Vec<_> = segment
                .split_whitespace()
                .filter(|w| !size_token(w))
                .collect();
            let name = words.join(" ");
            let name = if path.starts_with(crate::sustainer::PREFIX) && path.contains("/Modules/") {
                name.replace("flat module", "module")
            } else {
                name
            };
            (!name.is_empty()).then_some(name)
        })
        .collect::<Vec<_>>()
        .join("/");
    let namespace = normalized.split('/').next().unwrap_or("");
    let file = normalized.rsplit('/').next().unwrap_or("");
    if matches!(namespace, "drawers" | "sustainers") {
        if path.contains("/Facades/") && file == "facades" {
            return format!("{namespace}/facade");
        }
        if file.split_whitespace().any(|word| {
            matches!(
                word,
                "module"
                    | "box"
                    | "roof"
                    | "facade"
                    | "hinge"
                    | "rods"
                    | "rails"
                    | "runner"
                    | "latch"
                    | "label"
                    | "key"
                    | "springs"
                    | "stopper"
            )
        }) {
            return format!("{namespace}/{file}");
        }
    }
    normalized
}

#[cfg(test)]
mod component_tests {
    use super::*;
    #[test]
    fn component_identity_crosses_sizes_but_preserves_function_and_variants() {
        for (a, b) in [
            (
                "01. DRAWERS/01. S Families/S3/Modules/S3 Module 1F.stl",
                "01. DRAWERS/03. L Families/L7/Modules/L7 Module 4F.stl",
            ),
            (
                "01. DRAWERS/07. Universal/Runners/x3 Runner Guide.stl",
                "01. DRAWERS/07. Universal/Runners/x7 Runner Guide.stl",
            ),
            (
                "01. DRAWERS/01. S Families/Facades/S1F (Turn On Supports).stl",
                "01. DRAWERS/03. L Families/Facades/L4F (Turn On Supports).stl",
            ),
            (
                "02. SUSTAINERS/01. S Families/S3/Modules/S3 1F Module.stl",
                "02. SUSTAINERS/03. L Families/L7/Modules/L7 4F Flat Module.stl",
            ),
            (
                "02. SUSTAINERS/01. S Families/Facades/S1F Facade.stl",
                "02. SUSTAINERS/04. XL Families/XL1F Facade.stl",
            ),
            (
                "02. SUSTAINERS/05. Universal/Hinges/S-M 1F Hinge.stl",
                "02. SUSTAINERS/05. Universal/Hinges/L 2-4F Hinge.stl",
            ),
            (
                "generated/gridfinity/Cullenect-1.stl",
                "generated/gridfinity/Cullenect-9.stl",
            ),
        ] {
            assert_eq!(component(a), component(b), "{a} / {b}");
        }
        for (a, b) in [
            (
                "01. DRAWERS/07. Universal/Runners/x3 Runner Guide.stl",
                "01. DRAWERS/07. Universal/Runners/x3 Runner Slide.stl",
            ),
            (
                "02. SUSTAINERS/05. Universal/Locks/Lock Part 1.stl",
                "02. SUSTAINERS/05. Universal/Locks/Lock Part 2.stl",
            ),
            (
                "02. SUSTAINERS/03. L Families/L7/Modules/L7 4F Flat Module.stl",
                "02. SUSTAINERS/03. L Families/L7/Modules/L7 4F Sealed Module.stl",
            ),
            (
                "01. DRAWERS/07. Universal/Label.stl",
                "02. SUSTAINERS/05. Universal/Label for Sustainers.stl",
            ),
            ("custom/bracket.stl", "custom/hinge.stl"),
        ] {
            assert_ne!(component(a), component(b), "{a} / {b}");
        }
    }
}
