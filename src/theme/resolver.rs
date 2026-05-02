use super::{Theme, ThemeMode};
use std::collections::HashMap;

pub fn resolve_tw_in_json(value: &mut serde_json::Value, theme: &Theme, mode: ThemeMode) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(tw) = map.get_mut("tw") {
                if let Some(s) = tw.as_str() {
                    let resolved = resolve_tw(s, theme, mode);
                    *tw = serde_json::Value::String(resolved);
                }
            }
            for v in map.values_mut() {
                resolve_tw_in_json(v, theme, mode);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                resolve_tw_in_json(v, theme, mode);
            }
        }
        _ => {}
    }
}

pub fn resolve_tw(classes: &str, theme: &Theme, mode: ThemeMode) -> String {
    let colors = theme.colors_for_mode(mode);
    classes
        .split_whitespace()
        .filter_map(|token| {
            let (modifier, inner) = split_modifier(token);
            let (important, inner) = strip_important(inner);
            let (inner, opacity) = strip_opacity(inner);
            let resolved = resolve_inner(inner, colors, &theme.radius);
            let resolved = if let Some(op) = opacity {
                format!("{}{}", resolved, op)
            } else {
                resolved
            };
            let resolved = if important {
                format!("!{}", resolved)
            } else {
                resolved
            };
            apply_modifier(modifier, resolved, mode)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_important(inner: &str) -> (bool, &str) {
    if let Some(rest) = inner.strip_prefix('!') {
        (true, rest)
    } else {
        (false, inner)
    }
}

fn strip_opacity(inner: &str) -> (&str, Option<&str>) {
    if let Some(pos) = inner.rfind('/') {
        let suffix = &inner[pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return (&inner[..pos], Some(&inner[pos..]));
        }
    }
    (inner, None)
}

fn split_modifier(token: &str) -> (Option<&str>, &str) {
    if let Some(pos) = token.find(':') {
        let (m, rest) = token.split_at(pos);
        (Some(m), &rest[1..])
    } else {
        (None, token)
    }
}

fn try_color(prefix: &str, input: &str, colors: &HashMap<String, String>) -> Option<String> {
    let key = input.strip_prefix(prefix)?;
    let value = colors.get(key)?;
    Some(format!("{}[{}]", prefix, value.replace(' ', "_")))
}

fn try_radius(input: &str, radius: &HashMap<String, String>) -> Option<String> {
    let suffix = input.strip_prefix("rounded-")?;
    if let Some(value) = radius.get(suffix) {
        return Some(format!("rounded-[{}]", value));
    }
    // directional: rounded-{dir}-{key}
    const DIRS: &[&str] = &[
        "t", "r", "b", "l", "tl", "tr", "br", "bl", "ss", "se", "es", "ee",
    ];
    for dir in DIRS {
        if let Some(key) = suffix.strip_prefix(&format!("{}-", dir)) {
            if let Some(value) = radius.get(key) {
                return Some(format!("rounded-{}-[{}]", dir, value));
            }
        }
    }
    None
}

fn resolve_inner(
    inner: &str,
    colors: &HashMap<String, String>,
    radius: &HashMap<String, String>,
) -> String {
    try_color("bg-", inner, colors)
        .or_else(|| try_color("text-", inner, colors))
        .or_else(|| try_color("border-", inner, colors))
        .or_else(|| try_radius(inner, radius))
        .unwrap_or_else(|| inner.to_string())
}

fn apply_modifier(modifier: Option<&str>, resolved: String, mode: ThemeMode) -> Option<String> {
    match modifier {
        Some("dark") => (mode == ThemeMode::Dark).then_some(resolved),
        Some("light") => (mode == ThemeMode::Light).then_some(resolved),
        Some(m) => Some(format!("{}:{}", m, resolved)),
        None => Some(resolved),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn test_theme() -> Theme {
        Theme::from_yaml(
            r#"
colors:
  light:
    primary: "oklch(0.205 0 0)"
    muted-foreground: "oklch(0.556 0 0)"
    border: "oklch(0.922 0 0)"
  dark:
    primary: "oklch(0.922 0 0)"
    foreground: "oklch(0.985 0 0)"
    muted-foreground: "oklch(0.708 0 0)"
    border: "oklch(1 0 0 / 10%)"
radius:
  sm: "0.375rem"
  lg: "0.625rem"
"#,
        )
        .unwrap()
    }

    #[rstest]
    #[case::bg_primary_dark_uses_dark_color_value(
        "bg-primary",
        ThemeMode::Dark,
        "bg-[oklch(0.922_0_0)]"
    )]
    #[case::bg_unknown_key_passes_through("bg-unknown", ThemeMode::Dark, "bg-unknown")]
    #[case::no_prefix_match_passes_through("flex", ThemeMode::Dark, "flex")]
    #[case::text_foreground_dark_uses_dark_color_value(
        "text-foreground",
        ThemeMode::Dark,
        "text-[oklch(0.985_0_0)]"
    )]
    #[case::text_muted_foreground_matches_longest_key(
        "text-muted-foreground",
        ThemeMode::Dark,
        "text-[oklch(0.708_0_0)]"
    )]
    #[case::border_border_dark_uses_dark_color_value(
        "border-border",
        ThemeMode::Dark,
        "border-[oklch(1_0_0_/_10%)]"
    )]
    #[case::rounded_lg_substitutes_radius_value(
        "rounded-lg",
        ThemeMode::Dark,
        "rounded-[0.625rem]"
    )]
    #[case::rounded_t_lg("rounded-t-lg", ThemeMode::Dark, "rounded-t-[0.625rem]")]
    #[case::breakpoint_prefix_stripped_resolved_and_reattached(
        "md:bg-primary",
        ThemeMode::Dark,
        "md:bg-[oklch(0.922_0_0)]"
    )]
    #[case::dark_modifier_dark_mode_emits_resolved_inner_token(
        "dark:bg-primary",
        ThemeMode::Dark,
        "bg-[oklch(0.922_0_0)]"
    )]
    #[case::dark_modifier_light_mode_drops_token("dark:bg-primary", ThemeMode::Light, "")]
    #[case::light_modifier_light_mode_emits_resolved_inner_token(
        "light:bg-primary",
        ThemeMode::Light,
        "bg-[oklch(0.205_0_0)]"
    )]
    #[case::light_modifier_dark_mode_drops_token("light:bg-primary", ThemeMode::Dark, "")]
    #[case::important_prefix_resolves_inner_and_reattaches(
        "!bg-primary",
        ThemeMode::Dark,
        "!bg-[oklch(0.922_0_0)]"
    )]
    #[case::breakpoint_and_important_combined_resolves_inner(
        "md:!bg-primary",
        ThemeMode::Dark,
        "md:!bg-[oklch(0.922_0_0)]"
    )]
    #[case::unknown_modifier_preserved_and_inner_resolved(
        "foobar:bg-primary",
        ThemeMode::Dark,
        "foobar:bg-[oklch(0.922_0_0)]"
    )]
    #[case::bg_primary_opacity_50("bg-primary/50", ThemeMode::Dark, "bg-[oklch(0.922_0_0)]/50")]
    fn resolve_tw_token_cases(
        #[case] input: &str,
        #[case] mode: ThemeMode,
        #[case] expected: &str,
    ) {
        let theme = test_theme();
        assert_eq!(resolve_tw(input, &theme, mode), expected);
    }

    #[test]
    fn resolve_tw_multiple_tokens_processed_independently() {
        let theme = test_theme();
        assert_eq!(
            resolve_tw("flex bg-primary", &theme, ThemeMode::Dark),
            "flex bg-[oklch(0.922_0_0)]"
        );
    }

    #[test]
    fn resolve_tw_dark_modifier_light_mode_drops_token_from_multi_class_string() {
        let theme = test_theme();
        assert_eq!(
            resolve_tw("flex dark:bg-primary", &theme, ThemeMode::Light),
            "flex"
        );
    }

    #[test]
    fn resolve_tw_light_modifier_dark_mode_drops_token_from_multi_class_string() {
        let theme = test_theme();
        assert_eq!(
            resolve_tw("flex light:bg-primary", &theme, ThemeMode::Dark),
            "flex"
        );
    }
}
