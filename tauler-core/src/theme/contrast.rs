//! WCAG contrast of the text-on-surface pairs a theme defines.
//!
//! TERM: **Pair** — a foreground token and the surface token it is drawn on
//! (`card-foreground` on `card`). The `-foreground` suffix is shadcn's convention for
//! "the text colour for this surface"; `muted-foreground` is the exception, which is drawn
//! on `background` and `card`.
//!
//! DECISION: `muted-foreground` on `muted` is *not* a pair. `bg-muted` is a track or fill in
//! every component that uses it (`Progress`, `Slider`, `Knob`) and the alternating `DataTable`
//! row, which draws `text-foreground` on it — none draws `muted-foreground` there. No
//! tokyonight-day palette role clears 3:1 on its `bg_highlight` anyway. Revisit if a
//! component starts putting muted text on `bg-muted`.
//!
//! SPEC: a built-in passes when every pair in [`PAIRS`] reaches [`HARD_FLOOR`].
//!
//! DECISION: alpha is ignored. A pair containing a translucent colour is measured as if
//! opaque, which is only right when it is drawn on a surface of about its own lightness.
//! None of the pairs below are translucent in the default or tokyonight; `bg-muted/30` in
//! `DataTable` is alpha at the *use* site, which a theme cannot see.

use super::Theme;

/// Below this a pair is a failure (WCAG's floor for large text and UI components).
///
/// DECISION: 3:1 is the floor, not 4.5:1 (body-text AA). A scheme's own colours can sit below
/// 4.5 on purpose (tokyonight-day's foreground is 4.52:1 on its background), and the derivation
/// keeps a scheme's colours and nudges only what falls under the floor. Rejected: 4.5 as the
/// gate — it would rewrite most light themes.
pub const HARD_FLOOR: f64 = 3.0;

/// `(foreground, surface)` — a colour and what it is drawn on.
pub const PAIRS: &[(&str, &str)] = &[
    ("foreground", "background"),
    ("card-foreground", "card"),
    ("popover-foreground", "popover"),
    ("primary-foreground", "primary"),
    ("secondary-foreground", "secondary"),
    ("muted-foreground", "background"),
    ("muted-foreground", "card"),
    ("accent-foreground", "accent"),
    ("destructive-foreground", "destructive"),
    // Not text: the two accents that must stand out from the page they sit on.
    ("primary", "background"),
    ("destructive", "background"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub foreground: &'static str,
    pub surface: &'static str,
    /// `None` when either value is not a colour CSS Color 4 can parse.
    pub ratio: Option<f64>,
}

/// WCAG contrast ratio between two CSS colours, or `None` if either does not parse.
pub fn ratio(a: &str, b: &str) -> Option<f64> {
    let lum = |s: &str| {
        color::parse_color(s)
            .ok()
            .map(|c| f64::from(c.relative_luminance()))
    };
    let (la, lb) = (lum(a)?, lum(b)?);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    Some((hi + 0.05) / (lo + 0.05))
}

/// Every pair below `floor`; a pair with a token the theme does not define is skipped
/// (that is `missing_tokens`' business, not contrast's).
pub fn failures(theme: &Theme, floor: f64) -> Vec<Failure> {
    let mut out = Vec::new();
    for &(foreground, surface) in PAIRS {
        let (Some(fg), Some(bg)) = (theme.colors.get(foreground), theme.colors.get(surface)) else {
            continue;
        };
        let r = ratio(fg, bg);
        if r.is_none_or(|r| r < floor) {
            out.push(Failure {
                foreground,
                surface,
                ratio: r,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_twenty_one() {
        assert!((ratio("#000000", "#ffffff").unwrap() - 21.0).abs() < 0.01);
    }

    #[test]
    fn oklch_and_hex_are_both_understood() {
        assert!(ratio("oklch(0.145 0 0)", "#ffffff").unwrap() > 15.0);
    }

    #[test]
    fn an_unparseable_colour_is_no_ratio() {
        assert_eq!(ratio("not-a-colour", "#fff"), None);
    }
}
