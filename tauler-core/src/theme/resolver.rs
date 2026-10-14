use super::{Theme, ThemeMode};
use std::collections::HashMap;

/// Rewrite theme tokens in every `class` attribute in the tree.
///
/// Only tokens this project defines are touched — a colour, a radius, a font. Everything else
/// is passed through verbatim, which is what lets one attribute carry both theme
/// tokens and plain Tailwind utilities takumi resolves on its own.
///
/// The theme's own `mode` decides which `dark:` / `light:` variants survive: the ones that match
/// are unwrapped, the others dropped.
pub fn resolve_theme_tokens(value: &mut serde_json::Value, theme: &Theme) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(class) = map.get_mut("class") {
                if let Some(s) = class.as_str() {
                    let resolved = resolve_tw(s, theme);
                    *class = serde_json::Value::String(resolved);
                }
            }
            for v in map.values_mut() {
                resolve_theme_tokens(v, theme);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                resolve_theme_tokens(v, theme);
            }
        }
        _ => {}
    }
}

pub fn resolve_tw(classes: &str, theme: &Theme) -> String {
    let (colors, mode) = (&theme.colors, theme.mode);
    classes
        .split_whitespace()
        .filter_map(|token| {
            let (modifier, inner) = split_modifier(token);
            let (important, inner) = strip_important(inner);
            let (inner, opacity) = strip_opacity(inner);
            let resolved = resolve_inner(inner, colors, &theme.radius, &theme.fonts);
            let resolved = match opacity {
                Some(op) => with_opacity(resolved, op),
                None => resolved,
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

/// `utility-[#rrggbb]` with `/<percent>` applied, as the 8-digit hex `utility-[#rrggbbaa]`.
///
/// SPEC: a hex theme colour with an opacity renders at that opacity. takumi does not read
/// `bg-[#rrggbb]/50` — text came out black and a background vanished — but does read the 8-digit
/// form, so that is what a token with an opacity becomes. Any other value (an `oklch()`, say)
/// keeps the suffix as written.
fn with_opacity(resolved: String, opacity: &str) -> String {
    let hex_alpha = resolved
        .strip_suffix(']')
        .and_then(|s| s.rsplit_once("-[#"))
        .filter(|(_, rgb)| rgb.len() == 6 && rgb.chars().all(|c| c.is_ascii_hexdigit()))
        .zip(
            opacity
                .strip_prefix('/')
                .and_then(|p| p.parse::<u32>().ok()),
        );
    match hex_alpha {
        Some(((prefix, rgb), percent)) => {
            let alpha = (percent.min(100) * 255 + 50) / 100;
            format!("{prefix}-[#{rgb}{alpha:02x}]")
        }
        None => format!("{resolved}{opacity}"),
    }
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

fn try_font(input: &str, fonts: &HashMap<String, String>) -> Option<String> {
    try_color("font-", input, fonts)
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
    fonts: &HashMap<String, String>,
) -> String {
    try_color("bg-", inner, colors)
        .or_else(|| try_color("text-", inner, colors))
        .or_else(|| try_color("border-", inner, colors))
        .or_else(|| try_radius(inner, radius))
        .or_else(|| try_font(inner, fonts))
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

    /// A theme for `mode`, with that mode's palette: the dark one is the light one inverted.
    fn test_theme(mode: ThemeMode) -> Theme {
        let (label, colors) = match mode {
            ThemeMode::Dark => (
                "dark",
                r#"
  primary: "oklch(0.922 0 0)"
  foreground: "oklch(0.985 0 0)"
  muted-foreground: "oklch(0.708 0 0)"
  border: "oklch(1 0 0 / 10%)""#,
            ),
            ThemeMode::Light => (
                "light",
                r#"
  primary: "oklch(0.205 0 0)"
  muted-foreground: "oklch(0.556 0 0)"
  border: "oklch(0.922 0 0)""#,
            ),
        };
        Theme::from_yaml(&format!(
            r#"
name: test
mode: {label}
colors:{colors}
radius:
  sm: "0.375rem"
  lg: "0.625rem"
fonts:
  heading: "Lora"
  display: "JetBrains Mono"
"#
        ))
        .unwrap()
    }

    /// Claim: a hex token with an opacity becomes an 8-digit hex, the form takumi renders.
    #[rstest]
    #[case("text-foreground/50", "text-[#c0caf580]")]
    #[case("bg-foreground/10", "bg-[#c0caf51a]")]
    #[case("border-foreground/100", "border-[#c0caf5ff]")]
    #[case("bg-foreground/0", "bg-[#c0caf500]")]
    fn a_hex_token_with_an_opacity_becomes_eight_digit_hex(
        #[case] class: &str,
        #[case] expected: &str,
    ) {
        let theme =
            Theme::from_yaml("name: t\nmode: dark\ncolors:\n  foreground: \"#c0caf5\"\n").unwrap();
        assert_eq!(resolve_tw(class, &theme), expected);
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
    #[case::font_heading_resolves_to_theme_value("font-heading", ThemeMode::Dark, "font-[Lora]")]
    #[case::font_unknown_key_passes_through("font-unknown", ThemeMode::Dark, "font-unknown")]
    #[case::font_sans_not_defined_in_theme_passes_through_unchanged(
        "font-sans",
        ThemeMode::Dark,
        "font-sans"
    )]
    #[case::font_display_value_with_space_replaces_space_with_underscore(
        "font-display",
        ThemeMode::Dark,
        "font-[JetBrains_Mono]"
    )]
    fn resolve_tw_token_cases(
        #[case] input: &str,
        #[case] mode: ThemeMode,
        #[case] expected: &str,
    ) {
        let theme = test_theme(mode);
        assert_eq!(resolve_tw(input, &theme), expected);
    }

    #[test]
    fn resolve_tw_multiple_tokens_processed_independently() {
        let theme = test_theme(ThemeMode::Dark);
        assert_eq!(
            resolve_tw("flex bg-primary", &theme),
            "flex bg-[oklch(0.922_0_0)]"
        );
    }

    #[test]
    fn resolve_tw_dark_modifier_light_mode_drops_token_from_multi_class_string() {
        let theme = test_theme(ThemeMode::Light);
        assert_eq!(resolve_tw("flex dark:bg-primary", &theme), "flex");
    }

    #[test]
    fn resolve_tw_light_modifier_dark_mode_drops_token_from_multi_class_string() {
        let theme = test_theme(ThemeMode::Dark);
        assert_eq!(resolve_tw("flex light:bg-primary", &theme), "flex");
    }
}
