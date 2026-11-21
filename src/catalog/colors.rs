//! Default material groups for the VAULT assembly, across member sizes.
pub const ROLES: [(&str, &str, &str); 5] = [
    ("body", "主体", "Body"),
    ("functional", "功能件", "Functional parts"),
    ("accessory", "配件", "Accessories"),
    ("divider", "分隔板", "Dividers"),
    ("gridfinity", "Gridfinity 盒子", "Gridfinity bins"),
];

/// Catalog paths use '/' and preserve the original archive's family directories.
/// The VAULT manual distinguishes drawer and Sustainer assemblies (pp. 11–12)
/// from optional roofs, inserts and modifications (pp. 9–10, 13).
pub fn role(path: &str) -> &'static str {
    if crate::print_plate::color_key(path) == crate::print_plate::BIN_COLOR_KEY {
        return "gridfinity";
    }
    let mut parts = path.split('/');
    let (Some(system), Some(family), Some(entry)) = (parts.next(), parts.next(), parts.next())
    else {
        return "accessory";
    };
    let subfamily = parts.next();
    match (system, family) {
        ("01. DRAWERS", "07. Universal") => {
            let name = entry
                .split(" (")
                .next()
                .unwrap_or(entry)
                .trim_end_matches(".stl");
            match name {
                "Grid Dividers" => "divider",
                "Key Cap" => "body",
                "Runners"
                    if path.rsplit('/').next().is_some_and(|file| {
                        file.contains("Runner Guide") || file.contains("Runner Slide")
                    }) =>
                {
                    "body"
                }
                "Geared Rails" | "Runners" | "Latch" | "Springs" => "functional",
                _ => "accessory",
            }
        }
        ("02. SUSTAINERS", "05. Universal") => match entry {
            "Dividers" => "divider",
            "Hinges" | "Locks" | "Rods" => "functional",
            "Facades" => "body",
            _ => "accessory",
        },
        ("01. DRAWERS", "04. L² Families" | "06. XL² Families") => {
            // Twin-width module files sit directly in their size folder.
            if subfamily.is_some_and(|file| file.contains(" Module ")) {
                "body"
            } else {
                "accessory"
            }
        }
        ("01. DRAWERS", family) if family.ends_with(" Families") => match (entry, subfamily) {
            ("Facades" | "XL Facade 1F.stl", _) | (_, Some("Boxes" | "Modules")) => "body",
            _ => "accessory",
        },
        ("02. SUSTAINERS", family) if family.ends_with(" Families") => match (entry, subfamily) {
            ("Facades" | "XL1F Facade.stl", _) | (_, Some("Modules")) => "body",
            (_, Some("Roof" | "Roofs")) => {
                if parts
                    .next()
                    .is_some_and(|file| file.eq_ignore_ascii_case("Connector for Split Roof.stl"))
                {
                    "functional"
                } else {
                    "body"
                }
            }
            _ => "accessory",
        },
        _ => "accessory",
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
mod tests {
    use super::*;

    #[test]
    fn roles_follow_the_assembly_families() {
        for (group, paths) in [
            (
                "body",
                &[
                    "01. DRAWERS/05. XL Families/XL7/Boxes/XL7 Back Panel.stl",
                    "01. DRAWERS/06. XL² Families/XL7²/XL7² Module 1F.stl",
                    "01. DRAWERS/01. S Families/Facades/S1F (Turn On Supports).stl",
                    "01. DRAWERS/07. Universal/Key Cap.stl",
                    "01. DRAWERS/07. Universal/Runners/x3 Runner Guide.stl",
                    "01. DRAWERS/07. Universal/Runners/x3 Runner Slide.stl",
                    "01. DRAWERS/07. Universal/Runners/XL Runners/x7 Runner Guide XL.stl",
                    "01. DRAWERS/07. Universal/Runners/XL Runners/x7 Runner Slide XL.stl",
                    "02. SUSTAINERS/03. L Families/L7/Modules/L7 4F Dividers Module.stl",
                    "02. SUSTAINERS/01. S Families/S7/Roof/S7 Roof Part 1.stl",
                    "02. SUSTAINERS/02. M Families/M7/Roofs/M7 Roof Whole.stl",
                    "02. SUSTAINERS/04. XL Families/XL1F Facade.stl",
                ][..],
            ),
            (
                "functional",
                &[
                    "01. DRAWERS/07. Universal/Runners/Runner Stopper.stl",
                    "01. DRAWERS/07. Universal/Geared Rails/S Rails.stl",
                    crate::sustainer::STACKING_LATCH,
                    "02. SUSTAINERS/05. Universal/Rods/S-M Rods.stl",
                    "02. SUSTAINERS/05. Universal/Locks/1 SET OF PINS FOR 1 LOCK.stl",
                    "02. SUSTAINERS/03. L Families/L7/Roofs/Connector For Split Roof.stl",
                ][..],
            ),
            (
                "accessory",
                &[
                    "01. DRAWERS/07. Universal/Key (Turn On Supports).stl",
                    "01. DRAWERS/07. Universal/Hidden Key (Turn On Supports).stl",
                    "01. DRAWERS/07. Universal/Hidden Key Part 2 (Turn on Supports).stl",
                    "01. DRAWERS/01. S Families/S3/Roofs/S3 Grid Roof.stl",
                    "01. DRAWERS/01. S Families/S3/Gridfinity Baseplate/S3 Baseplate.stl",
                    "02. SUSTAINERS/05. Universal/Baseplates/01. S/S3 Baseplate.stl",
                    "03. MODIFICATIONS/Backbone/Handle Lock Knob Part 1.stl",
                    "03. MODIFICATIONS/Wheels/Cover.stl",
                    crate::label::PATH,
                    crate::label::SUSTAINER_PATH,
                    "generated/gridfinity/Cullenect-4.stl",
                    "custom/Dividers/Folding Lid.stl",
                ][..],
            ),
            (
                "divider",
                &[
                    "01. DRAWERS/07. Universal/Grid Dividers/34.5MM TALL (full height of 1F drawers)/7.stl",
                    crate::dividers::PIN,
                    "02. SUSTAINERS/05. Universal/Dividers/L Width Dividers/L-4F Width Divider.stl",
                ][..],
            ),
            ("gridfinity", &["generated/gridfinity/Gridfinity-4.stl"][..]),
        ] {
            for path in paths {
                assert_eq!(role(path), group, "{path}");
            }
        }
    }

    #[test]
    #[ignore = "requires VAULT_MODEL_ROOT"]
    fn vault_archive_components_have_consistent_roles_across_sizes() {
        use std::collections::BTreeMap;
        let root = crate::test_support::required_env("VAULT_MODEL_ROOT");
        let catalog = crate::catalog::Catalog::scan(root.into()).unwrap();
        let mut identities = BTreeMap::new();
        let mut counts = BTreeMap::new();
        for entry in catalog
            .entries
            .iter()
            .filter(|e| e.name.to_ascii_lowercase().ends_with(".stl"))
        {
            let group = role(&entry.path);
            if let Some(previous) = identities.insert(component(&entry.path), group) {
                assert_eq!(previous, group, "{}", entry.path);
            }
            *counts.entry(group).or_insert(0) += 1;
        }
        // Audited 261021_VAULT_USSA_S archive. Gridfinity bins are generated locally.
        assert_eq!(
            counts,
            BTreeMap::from([
                ("body", 301),
                ("functional", 29),
                ("accessory", 276),
                ("divider", 127),
            ])
        );
    }

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
