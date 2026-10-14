//! A layout that shows a whole theme at once, for a screenshot that doubles as a bug check.
//!
//! TERM: **Swatch scene** — the one layout every theme is rendered through: each token as a
//! labelled swatch, each text/surface pair as it is read, the ANSI strip, and a few of the
//! real components. Generated from the theme's own token list, so a token added to a theme
//! appears without anyone editing the scene.
//! Avoid: demo, preview, mockup (CONTEXT.md reserves those for other things).
//!
//! DECISION: the scene is *generated JSX source*, not a checked-in `.jsx`. A static file
//! names tokens; the theme decides which tokens exist, and the two would drift. Rejected:
//! a fixed `swatch.jsx` — it cannot show an extra token a theme adds.
//!
//! SPEC: rendering this through the pipeline resolves every `bg-<token>` it writes. A class
//! that survives resolution as `bg-<token>` is a token the theme failed to define.

use super::{Theme, ThemeMode, ANSI_TOKENS};
use crate::theme::contrast::PAIRS;

/// JSX source for the swatch scene of `theme`.
pub fn jsx(theme: &Theme) -> String {
    let colors = &theme.colors;
    let mut tokens: Vec<&str> = colors
        .keys()
        .map(String::as_str)
        .filter(|t| !t.starts_with("ansi-"))
        .collect();
    tokens.sort_unstable();

    let mut s = String::new();
    s.push_str("import { Badge } from \"@ui/badge\";\n");
    s.push_str("import { Progress } from \"@ui/progress\";\n");
    s.push_str("export default () => (\n<div class=\"flex flex-col gap-[10px]\">\n");
    s.push_str(&format!(
        "<span class=\"text-foreground text-[13px]\">{} · {}</span>\n",
        theme.name,
        if theme.mode == ThemeMode::Light {
            "light"
        } else {
            "dark"
        }
    ));

    // Pairs as they are read: text on the surface it belongs to.
    s.push_str("<div class=\"flex flex-col gap-[4px]\">\n");
    for &(fg, bg) in PAIRS {
        s.push_str(&format!(
            "<div class=\"bg-{bg} text-{fg} px-[8px] py-[3px] text-[11px] rounded-lg border border-border\"><span>{fg} on {bg}</span></div>\n"
        ));
    }
    s.push_str("</div>\n");

    // Every token, as a swatch.
    s.push_str("<div class=\"flex flex-row flex-wrap gap-[6px]\">\n");
    for t in &tokens {
        s.push_str(&format!(
            "<div class=\"flex flex-row items-center gap-[4px] w-[120px]\"><div class=\"bg-{t} w-[14px] h-[14px] border border-border\"></div><span class=\"text-foreground text-[9px]\">{t}</span></div>\n"
        ));
    }
    s.push_str("</div>\n");

    // The 16 terminal colours, one strip.
    s.push_str("<div class=\"flex flex-row\">\n");
    for t in ANSI_TOKENS {
        s.push_str(&format!("<div class=\"bg-{t} w-[22px] h-[22px]\"></div>\n"));
    }
    s.push_str("</div>\n");

    // A few of the real components.
    s.push_str("<div class=\"flex flex-row gap-[8px] items-center\">\n<Badge><span>Default</span></Badge>\n<Badge variant=\"secondary\"><span>Secondary</span></Badge>\n<Badge variant=\"destructive\"><span>Destructive</span></Badge>\n<Badge variant=\"outline\"><span>Outline</span></Badge>\n</div>\n");
    s.push_str("<div class=\"flex flex-col gap-[4px]\"><Progress value={72} /><Progress value={30} /></div>\n");
    s.push_str("</div>\n);\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_names_every_token_the_theme_defines() {
        let theme = Theme::default_theme();
        let src = jsx(&theme);
        for token in theme.colors.keys().filter(|t| !t.starts_with("ansi-")) {
            assert!(
                src.contains(&format!("bg-{token} ")),
                "no swatch for {token}"
            );
        }
    }
}
