//! Class-variance lookup for the `@ui` components: base classes, then the selected
//! variants, then the caller's `class`.
//!
//! The three are merged, not concatenated. takumi resolves two conflicting utilities in one
//! class string by Tailwind's property sort order, not by "last class wins", so a plain
//! concatenation lets a component's `h-full` silently beat a caller's `h-[120px]`. [`merge`]
//! drops each earlier class that a later one conflicts with, tailwind-merge style, so the
//! caller always wins. The grouping is hand-written and covers only the utility families the
//! components and layouts use; a class it does not know never conflicts with anything.

pub struct Cva {
    pub base: &'static str,
    pub variants: &'static [(&'static str, &'static [(&'static str, &'static str)])],
    pub defaults: &'static [(&'static str, &'static str)],
}

impl Cva {
    pub fn resolve(&self, selections: &[(&str, Option<&str>)], extra_tw: &str) -> String {
        let mut layers = vec![self.base];

        for (axis, value_opt) in selections {
            let effective = value_opt.or_else(|| {
                self.defaults
                    .iter()
                    .find(|(d_axis, _)| d_axis == axis)
                    .map(|(_, d_val)| *d_val)
            });

            if let Some(value) = effective {
                if let Some((_, axis_variants)) = self.variants.iter().find(|(a, _)| a == axis) {
                    if let Some((_, classes)) = axis_variants.iter().find(|(v, _)| *v == value) {
                        layers.push(classes);
                    }
                }
            }
        }

        layers.push(extra_tw);
        merge(layers.iter().flat_map(|layer| layer.split_whitespace()))
    }
}

/// Joins `classes` in order, dropping every class that a later class conflicts with: same
/// variant prefix (`hover:` …) and the same conflict group, or a group the later one covers
/// (`p-2` covers `px-3`).
fn merge<'a>(classes: impl Iterator<Item = &'a str>) -> String {
    let mut kept: Vec<(&str, &str, Option<String>)> = Vec::new();

    for class in classes {
        let (prefix, utility) = split_prefix(class);
        let group = conflict_group(utility);
        if let Some(group) = &group {
            let covered = covers(group);
            kept.retain(|(p, _, g)| {
                *p != prefix
                    || !g
                        .as_deref()
                        .is_some_and(|g| g == group || covered.contains(&g))
            });
        }
        kept.push((prefix, class, group));
    }

    kept.iter()
        .map(|(_, class, _)| *class)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits `hover:bg-primary` into (`hover:`, `bg-primary`), ignoring colons inside `[...]`.
fn split_prefix(class: &str) -> (&str, &str) {
    let mut depth = 0i32;
    let mut split = 0;
    for (i, c) in class.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            ':' if depth == 0 => split = i + 1,
            _ => {}
        }
    }
    class.split_at(split)
}

/// Groups that `group` also overrides, beyond itself.
fn covers(group: &str) -> &'static [&'static str] {
    match group {
        "p" => &["px", "py", "pt", "pr", "pb", "pl"],
        "px" => &["pl", "pr"],
        "py" => &["pt", "pb"],
        "m" => &["mx", "my", "mt", "mr", "mb", "ml"],
        "mx" => &["ml", "mr"],
        "my" => &["mt", "mb"],
        "inset" => &["inset-x", "inset-y", "top", "right", "bottom", "left"],
        "inset-x" => &["left", "right"],
        "inset-y" => &["top", "bottom"],
        "size" => &["w", "h"],
        "gap" => &["gap-x", "gap-y"],
        "overflow" => &["overflow-x", "overflow-y"],
        "border-w" => &[
            "border-w-x",
            "border-w-y",
            "border-w-t",
            "border-w-r",
            "border-w-b",
            "border-w-l",
        ],
        "border-w-x" => &["border-w-l", "border-w-r"],
        "border-w-y" => &["border-w-t", "border-w-b"],
        _ => &[],
    }
}

/// The conflict group of one utility (variant prefix already stripped), or `None` for a
/// class that conflicts with nothing.
fn conflict_group(utility: &str) -> Option<String> {
    let utility = utility.trim_start_matches('!').trim_start_matches('-');
    let group = match utility {
        "flex" | "inline-flex" | "block" | "inline-block" | "inline" | "grid" | "hidden" => {
            "display"
        }
        "absolute" | "relative" | "fixed" | "static" | "sticky" => "position",
        "flex-row" | "flex-col" | "flex-row-reverse" | "flex-col-reverse" => "flex-direction",
        "flex-wrap" | "flex-nowrap" | "flex-wrap-reverse" => "flex-wrap",
        "grow" => "grow",
        "shrink" => "shrink",
        "border" => "border-w",
        "rounded" => "rounded",
        _ => {
            let (head, value) = utility.split_once('-')?;
            return match head {
                "text" => text_group(value).map(str::to_string),
                "border" => Some(border_group(value)),
                "bg" => bg_is_color(value).then(|| "bg".to_string()),
                "font" => Some(font_group(value).to_string()),
                "rounded" => Some(rounded_group(value)),
                // Two-part names: `min-w-…`, `inset-x-…`, `gap-y-…`, `overflow-x-…`.
                "min" | "max" => match value.split_once('-') {
                    Some((axis @ ("w" | "h"), _)) => Some(format!("{head}-{axis}")),
                    _ => None,
                },
                "inset" | "gap" | "overflow" => match value.split_once('-') {
                    Some((axis @ ("x" | "y"), _)) => Some(format!("{head}-{axis}")),
                    _ => Some(head.to_string()),
                },
                "w" | "h" | "size" | "p" | "px" | "py" | "pt" | "pr" | "pb" | "pl" | "m" | "mx"
                | "my" | "mt" | "mr" | "mb" | "ml" | "top" | "right" | "bottom" | "left"
                | "items" | "justify" | "self" | "grow" | "shrink" | "flex" | "leading"
                | "tracking" | "opacity" => Some(head.to_string()),
                _ => None,
            };
        }
    };
    Some(group.to_string())
}

/// `text-` is size, alignment or color depending on its value.
fn text_group(value: &str) -> Option<&'static str> {
    match value {
        "left" | "center" | "right" | "justify" | "start" | "end" => Some("text-align"),
        "ellipsis" | "clip" | "wrap" | "nowrap" | "balance" | "pretty" => None,
        _ if is_named_size(value) || is_arbitrary_length(value) => Some("text-size"),
        _ => Some("text-color"),
    }
}

/// `border-` is width (all sides or one side), style or color depending on its value.
fn border_group(value: &str) -> String {
    let (side, rest) = match value.split_once('-') {
        Some((side @ ("x" | "y" | "t" | "r" | "b" | "l"), rest)) => (Some(side), rest),
        _ if matches!(value, "x" | "y" | "t" | "r" | "b" | "l") => (Some(value), ""),
        _ => (None, value),
    };
    let is_width = rest.is_empty() || rest.parse::<u32>().is_ok() || is_arbitrary_length(rest);
    match (side, is_width) {
        (Some(side), true) => format!("border-w-{side}"),
        (None, true) => "border-w".to_string(),
        _ if matches!(
            rest,
            "solid" | "dashed" | "dotted" | "double" | "none" | "hidden"
        ) =>
        {
            "border-style".to_string()
        }
        (Some(side), false) => format!("border-color-{side}"),
        (None, false) => "border-color".to_string(),
    }
}

fn bg_is_color(value: &str) -> bool {
    !matches!(
        value,
        "cover" | "contain" | "auto" | "center" | "repeat" | "no-repeat" | "none"
    ) && !["gradient", "linear", "radial", "conic", "[url"]
        .iter()
        .any(|p| value.starts_with(p))
}

fn font_group(value: &str) -> &'static str {
    let is_weight = matches!(
        value,
        "thin"
            | "extralight"
            | "light"
            | "normal"
            | "medium"
            | "semibold"
            | "bold"
            | "extrabold"
            | "black"
    ) || value.trim_matches(['[', ']']).parse::<u32>().is_ok();
    if is_weight {
        "font-weight"
    } else {
        "font-family"
    }
}

/// `rounded-lg` is all corners; `rounded-t-lg` / `rounded-tl` is its own corner group.
fn rounded_group(value: &str) -> String {
    let side = value.split('-').next().unwrap_or(value);
    if matches!(side, "t" | "r" | "b" | "l" | "tl" | "tr" | "br" | "bl") {
        format!("rounded-{side}")
    } else {
        "rounded".to_string()
    }
}

fn is_named_size(value: &str) -> bool {
    matches!(value, "xs" | "sm" | "base" | "lg")
        || value
            .strip_suffix("xl")
            .is_some_and(|n| n.is_empty() || n.parse::<u8>().is_ok())
}

/// `[12px]`, `[1.5rem]`, `[length:var(--x)]`: an arbitrary value that is a length.
fn is_arbitrary_length(value: &str) -> bool {
    value.strip_prefix('[').is_some_and(|v| {
        v.starts_with(|c: char| c.is_ascii_digit() || c == '.') || v.starts_with("length:")
    })
}

#[cfg(test)]
mod tests {
    use super::Cva;
    use std::collections::BTreeSet;

    fn base_only(base: &'static str) -> Cva {
        Cva {
            base,
            variants: &[],
            defaults: &[],
        }
    }

    fn classes(resolved: &str) -> BTreeSet<&str> {
        resolved.split_whitespace().collect()
    }

    fn resolve_extra(base: &'static str, extra_tw: &str) -> String {
        base_only(base).resolve(&[], extra_tw)
    }

    #[test]
    fn caller_height_replaces_base_height_and_keeps_unrelated_classes() {
        let resolved = resolve_extra("flex flex-col h-full w-[6px]", "h-[120px]");
        assert_eq!(
            classes(&resolved),
            classes("flex flex-col w-[6px] h-[120px]")
        );
    }

    #[test]
    fn caller_width_replaces_base_width() {
        let resolved = resolve_extra("flex flex-row w-full h-[4px]", "w-[40px]");
        assert_eq!(
            classes(&resolved),
            classes("flex flex-row h-[4px] w-[40px]")
        );
    }

    #[test]
    fn caller_axis_padding_replaces_same_axis_and_keeps_other_axis() {
        let resolved = resolve_extra("rounded-lg px-3 py-[10px]", "px-2");
        assert_eq!(classes(&resolved), classes("rounded-lg py-[10px] px-2"));
    }

    #[test]
    fn caller_all_sides_padding_replaces_both_axis_paddings() {
        let resolved = resolve_extra("rounded-lg px-3 py-[10px]", "p-2");
        assert_eq!(classes(&resolved), classes("rounded-lg p-2"));
    }

    #[test]
    fn caller_text_size_replaces_base_text_size_and_keeps_text_color() {
        let resolved = resolve_extra("text-[12px] text-muted-foreground", "text-[16px]");
        assert_eq!(
            classes(&resolved),
            classes("text-muted-foreground text-[16px]")
        );
    }

    #[test]
    fn caller_text_color_replaces_base_text_color_and_keeps_text_size() {
        let resolved = resolve_extra(
            "font-semibold text-[14px] text-muted-foreground",
            "text-foreground",
        );
        assert_eq!(
            classes(&resolved),
            classes("font-semibold text-[14px] text-foreground")
        );
    }

    #[test]
    fn caller_border_color_replaces_base_border_color_and_keeps_border_width() {
        let resolved = resolve_extra("rounded-lg border border-border", "border-red-500");
        assert_eq!(
            classes(&resolved),
            classes("rounded-lg border border-red-500")
        );
    }

    #[test]
    fn caller_theme_border_color_replaces_base_border_color() {
        let resolved = resolve_extra("rounded-lg border border-border", "border-primary");
        assert_eq!(
            classes(&resolved),
            classes("rounded-lg border border-primary")
        );
    }

    #[test]
    fn caller_border_width_replaces_base_border_width_and_keeps_border_color() {
        let resolved = resolve_extra("rounded-lg border border-border", "border-2");
        assert_eq!(
            classes(&resolved),
            classes("rounded-lg border-border border-2")
        );
    }

    #[test]
    fn caller_background_color_replaces_base_background_color() {
        let resolved = resolve_extra("flex w-[40px] rounded-full bg-muted", "bg-primary");
        assert_eq!(
            classes(&resolved),
            classes("flex w-[40px] rounded-full bg-primary")
        );
    }

    #[test]
    fn variant_class_replaces_conflicting_base_class() {
        const CVA: Cva = Cva {
            base: "inline-flex rounded-full bg-muted text-[12px]",
            variants: &[("variant", &[("primary", "bg-primary")])],
            defaults: &[],
        };
        let resolved = CVA.resolve(&[("variant", Some("primary"))], "");
        assert_eq!(
            classes(&resolved),
            classes("inline-flex rounded-full text-[12px] bg-primary")
        );
    }

    #[test]
    fn prefixed_class_does_not_replace_unprefixed_class() {
        let resolved = resolve_extra("flex bg-muted", "hover:bg-primary");
        assert_eq!(
            classes(&resolved),
            classes("flex bg-muted hover:bg-primary")
        );
    }

    #[test]
    fn empty_extra_returns_base_unchanged() {
        let base = "rounded-lg border border-border bg-card px-3 py-[10px]";
        assert_eq!(resolve_extra(base, ""), base);
    }
}
