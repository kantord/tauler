//! One derivation for every scheme: a [`Seed`] in, every theme token out.
//!
//! TERM: **Seed** — the background, the foreground and the 16 ANSI colours of one mode of one
//! scheme (see `src/themes/generate/dump.lua`). Everything else is a blend of those.
//!
//! TERM: **Ladder** — the few surface and line colours a UI needs between background and
//! foreground (`card`, `muted`, `border`, `input`, `ring`). base16 ships one; a terminal theme
//! does not, so it is derived.
//!
//! DECISION: blend in Oklab, not sRGB. Equal steps in Oklab are equal steps to the eye, so
//! the same percentages give the same visible elevation on a dark navy and on a cream
//! background; zenbones.nvim derives its ladder in HSLuv for the same reason. Rejected: sRGB
//! blends (what the first prototype used) — they work, but 10% toward the foreground is a
//! visibly different step on different backgrounds.
//!
//! DECISION: panels (`card`, `popover`, `sidebar`) fade *away* from the foreground — toward
//! black in a dark mode, toward white in a light one — and the rest of the ladder (`muted`,
//! `border`...) steps *toward* it. The first is what the maintainer's own palette script does
//! and what the reviewed prototype showed; base16 and shadcn's dark theme raise surfaces the
//! other way. Rejected: stepping panels toward the foreground — not seen, so not chosen.
//!
//! DECISION: `primary` is an ANSI colour chosen by name per workload, default blue. No scheme
//! marks a brand accent, so it cannot be derived; stylix and base16-nvim also make blue the
//! accent. ASSUMPTION: blue as the default for every workload — not confirmed.
//!
//! DECISION: a UI accent (`primary`, `destructive`, the `chart-*` hues) is, of the scheme's
//! normal and bright variant of its hue, the one that contrasts more with the background (bright
//! on a tie) — and only if that still falls under the floor is it lowered in Oklab lightness,
//! keeping its hue and giving up chroma only to stay inside sRGB. Always taking the bright variant
//! was right on dark backgrounds and wrong on light ones (tokyonight-day's are 2.5:1); a blend
//! toward black fixes the contrast but pulls neighbouring hues together. Rejected: blending
//! toward black or white (what text colours get), which on rose-pine-dawn cut the distance
//! between its two closest accents from 0.093 to 0.050.
//!
//! SPEC: the six accents stay as far apart as the scheme made them, down to [`MIN_SEPARATION`]:
//! [`accent_separation`] reports the distance before and after, and the generator refuses a
//! scheme whose accents end up closer than the smaller of the two. The `ansi-*` tokens are the
//! scheme's own and are never touched.
//!
//! DECISION: every text/surface pair in `theme::contrast::PAIRS` is nudged until it clears
//! the hard floor, so a derived theme cannot fail the contrast test by construction. A pair
//! that needed a nudge is a scheme whose own colours were below it, and the change is the
//! smallest one that gets over (12% steps toward black or white, whichever contrasts more).
//!
//! SPEC: for any seed, the output defines every core and ANSI token and has no pair below
//! `contrast::HARD_FLOOR`.

use color::{parse_color, ColorSpaceTag, DynamicColor, HueDirection, Oklab, Srgb};
use std::collections::BTreeMap;
use tauler_core::theme::{contrast, ANSI_TOKENS, CORE_TOKENS};

/// The eight names an ANSI colour has, in the order a terminal numbers them.
pub const ANSI_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

pub struct Seed {
    pub bg: String,
    pub fg: String,
    /// `ansi[0..8]` normal, `ansi[8..16]` bright, both in [`ANSI_NAMES`] order.
    pub ansi: [String; 16],
}

/// Every token this derivation defines, in the order the generated file lists them.
pub fn token_order() -> Vec<String> {
    let mut order: Vec<String> = CORE_TOKENS.iter().map(|t| t.to_string()).collect();
    order.extend((1..=5).map(|i| format!("chart-{i}")));
    order.extend(
        [
            "sidebar",
            "sidebar-foreground",
            "sidebar-primary",
            "sidebar-primary-foreground",
            "sidebar-accent",
            "sidebar-accent-foreground",
            "sidebar-border",
            "sidebar-ring",
        ]
        .map(String::from),
    );
    order.extend(ANSI_TOKENS.iter().map(|t| t.to_string()));
    order
}

fn hex_of(c: DynamicColor) -> String {
    let rgba = c.to_alpha_color::<Srgb>().to_rgba8();
    format!("#{:02x}{:02x}{:02x}", rgba.r, rgba.g, rgba.b)
}

fn parse(hex: &str) -> DynamicColor {
    parse_color(hex).unwrap_or_else(|e| panic!("`{hex}` is not a colour: {e:?}"))
}

/// `pct_a`% of `a` and the rest `b`, mixed in Oklab.
fn mix(a: &str, b: &str, pct_a: f32) -> String {
    let m = parse(a)
        .interpolate(parse(b), ColorSpaceTag::Oklab, HueDirection::default())
        .eval(1.0 - pct_a / 100.0);
    hex_of(m)
}

fn ratio(a: &str, b: &str) -> f64 {
    contrast::ratio(a, b).expect("derived colours always parse")
}

/// Whichever of black and white contrasts more with `on`.
fn extreme_for(on: &str) -> &'static str {
    if ratio("#ffffff", on) >= ratio("#000000", on) {
        "#ffffff"
    } else {
        "#000000"
    }
}

/// `fg` nudged toward black or white until it clears `floor` on `on`.
fn nudge(fg: &str, on: &str, floor: f64) -> String {
    let target = extreme_for(on);
    let mut out = fg.to_string();
    for _ in 0..40 {
        if ratio(&out, on) >= floor {
            break;
        }
        out = mix(target, &out, 12.0);
    }
    out
}

/// Of `a` and `b`, the one that reads better on `on`.
fn best_on<'a>(on: &str, a: &'a str, b: &'a str) -> &'a str {
    if ratio(a, on) >= ratio(b, on) {
        a
    } else {
        b
    }
}

/// Is this background dark enough that panels should fade toward black?
pub fn is_dark(bg: &str) -> bool {
    ratio(bg, "#000000") < ratio(bg, "#ffffff")
}

/// The smallest Oklab distance two accents may end up at before they read as one colour.
pub const MIN_SEPARATION: f64 = 0.05;

/// Distance between two colours in Oklab; about 0.02 is a just-noticeable difference.
pub fn distance(a: &str, b: &str) -> f64 {
    let lab = |c: &str| parse(c).to_alpha_color::<Oklab>().components;
    let (a, b) = (lab(a), lab(b));
    (0..3)
        .map(|i| f64::from(a[i] - b[i]).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn min_pairwise(colours: &[String]) -> f64 {
    let mut min = f64::INFINITY;
    for (i, a) in colours.iter().enumerate() {
        for b in &colours[i + 1..] {
            min = min.min(distance(a, b));
        }
    }
    min
}

/// `c` with its chroma reduced just enough to be an sRGB colour (hue and lightness kept).
fn into_gamut(c: DynamicColor) -> DynamicColor {
    let mut c = c;
    for _ in 0..60 {
        let rgb = c.to_alpha_color::<Srgb>().components;
        if rgb[..3].iter().all(|v| (-0.0005..=1.0005).contains(v)) {
            break;
        }
        c = c.map_in(ColorSpaceTag::Oklch, |l, ch, h, a| [l, ch * 0.97, h, a]);
    }
    c
}

/// `c` lowered (on a light background) or raised (on a dark one) in Oklab lightness until it
/// clears `floor` against `bg`. Hue is kept; chroma only gives way to stay inside sRGB.
fn lightness_nudge(c: &str, bg: &str, floor: f64) -> String {
    if ratio(c, bg) >= floor {
        return c.to_string();
    }
    let step = if extreme_for(bg) == "#000000" {
        -0.005
    } else {
        0.005
    };
    let base = parse(c);
    let mut best = c.to_string();
    for i in 1..=200 {
        let delta = step * i as f32;
        let candidate =
            into_gamut(base.map_in(ColorSpaceTag::Oklch, |l, ch, h, a| [l + delta, ch, h, a]));
        best = hex_of(candidate);
        if ratio(&best, bg) >= floor {
            break;
        }
    }
    best
}

/// The six accent hues (red, green, yellow, blue, magenta, cyan) as the scheme draws them and as
/// they are used for UI: `(donor, ui)`. The donor is the better-contrasting of the normal and
/// bright variant; the UI colour is that, lowered if it still sits under the floor.
pub fn ui_accents(seed: &Seed) -> (Vec<String>, Vec<String>) {
    let donor: Vec<String> = (1..=6)
        .map(|i| {
            let (normal, bright) = (&seed.ansi[i], &seed.ansi[8 + i]);
            if ratio(normal, &seed.bg) > ratio(bright, &seed.bg) {
                normal.clone()
            } else {
                bright.clone()
            }
        })
        .collect();
    let ui = donor
        .iter()
        .map(|c| lightness_nudge(c, &seed.bg, contrast::HARD_FLOOR))
        .collect();
    (donor, ui)
}

/// The smallest distance between two of the six accents, before and after they are made
/// UI-safe: `(donor, ui)`.
pub fn accent_separation(seed: &Seed) -> (f64, f64) {
    let (donor, ui) = ui_accents(seed);
    (min_pairwise(&donor), min_pairwise(&ui))
}

/// Every token for `seed`, with `primary` naming the ANSI colour (one of [`ANSI_NAMES`]) that
/// is the accent.
pub fn derive(seed: &Seed, primary: &str) -> BTreeMap<String, String> {
    let (bg, fg) = (seed.bg.as_str(), seed.fg.as_str());
    let accents = ui_accents(seed).1;
    let accent = |name: &str| {
        let i = ANSI_NAMES
            .iter()
            .position(|n| *n == name)
            .unwrap_or_else(|| panic!("`{name}` is not an ANSI colour name"));
        assert!((1..=6).contains(&i), "`{name}` is not an accent hue");
        accents[i - 1].clone()
    };

    let panel = mix(bg, if is_dark(bg) { "#000000" } else { "#ffffff" }, 87.0);
    let elevated = mix(bg, fg, 90.0);
    let border = mix(bg, fg, 80.0);
    let input = mix(bg, fg, 85.0);
    let ring = mix(bg, fg, 70.0);
    let muted_fg = mix(bg, fg, 40.0);
    let p = accent(primary);
    let soft_accent = mix(&p, bg, 25.0);
    let red = accent("red");

    let mut t: BTreeMap<String, String> = BTreeMap::new();
    let mut set = |k: &str, v: String| {
        t.insert(k.to_string(), v);
    };
    set("background", bg.to_string());
    set("foreground", fg.to_string());
    set("card", panel.clone());
    set("card-foreground", fg.to_string());
    set("popover", panel.clone());
    set("popover-foreground", fg.to_string());
    set("primary", p.clone());
    set("primary-foreground", best_on(&p, bg, fg).to_string());
    set("secondary", elevated.clone());
    set("secondary-foreground", fg.to_string());
    set("muted", elevated.clone());
    set("muted-foreground", muted_fg);
    set("accent", soft_accent.clone());
    set("accent-foreground", fg.to_string());
    set("destructive", red.clone());
    set("destructive-foreground", best_on(&red, bg, fg).to_string());
    set("border", border.clone());
    set("input", input);
    set("ring", ring.clone());
    for (i, name) in ["red", "green", "yellow", "blue", "magenta"]
        .iter()
        .enumerate()
    {
        set(&format!("chart-{}", i + 1), accent(name));
    }
    set("sidebar", panel);
    set("sidebar-primary", p);
    set("sidebar-accent", soft_accent);
    set("sidebar-border", border);
    set("sidebar-ring", ring);
    for (i, name) in ANSI_NAMES.iter().enumerate() {
        set(&format!("ansi-{name}"), seed.ansi[i].clone());
        set(&format!("ansi-bright-{name}"), seed.ansi[8 + i].clone());
    }

    // Nudge every text colour that sits below the floor on the surface it is read on.
    for &(foreground, surface) in contrast::PAIRS {
        let surface_colour = t[surface].clone();
        let nudged = nudge(&t[foreground], &surface_colour, contrast::HARD_FLOOR);
        t.insert(foreground.to_string(), nudged);
    }
    // The sidebar reads the same text colours as the panels it echoes.
    for (sidebar, source) in [
        ("sidebar-foreground", "card-foreground"),
        ("sidebar-primary-foreground", "primary-foreground"),
        ("sidebar-accent-foreground", "accent-foreground"),
    ] {
        t.insert(sidebar.to_string(), t[source].clone());
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauler_core::theme::Theme;

    /// A dark navy seed, a light cream one and a deliberately awkward one: a mid-grey
    /// background, where neither black nor white text is comfortable.
    fn seeds() -> Vec<(&'static str, Seed)> {
        let ansi = |h: [&str; 16]| h.map(String::from);
        vec![
            (
                "dark",
                Seed {
                    bg: "#1a1b26".into(),
                    fg: "#c0caf5".into(),
                    ansi: ansi([
                        "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7",
                        "#7dcfff", "#a9b1d6", "#414868", "#ff899d", "#9fe044", "#faba4a",
                        "#8db0ff", "#c7a9ff", "#a4daff", "#c0caf5",
                    ]),
                },
            ),
            (
                "light",
                Seed {
                    bg: "#fbf1c7".into(),
                    fg: "#654735".into(),
                    ansi: ansi([
                        "#654735", "#c14a4a", "#6c782e", "#b47109", "#45707a", "#945e80",
                        "#4c7a5d", "#a89984", "#928374", "#c14a4a", "#6c782e", "#b47109",
                        "#45707a", "#945e80", "#4c7a5d", "#654735",
                    ]),
                },
            ),
            (
                "mid-grey",
                Seed {
                    bg: "#808080".into(),
                    fg: "#8a8a8a".into(),
                    ansi: ansi([
                        "#000000", "#aa0000", "#00aa00", "#aa5500", "#0000aa", "#aa00aa",
                        "#00aaaa", "#aaaaaa", "#555555", "#ff5555", "#55ff55", "#ffff55",
                        "#5555ff", "#ff55ff", "#55ffff", "#ffffff",
                    ]),
                },
            ),
        ]
    }

    fn all_seeds() -> Vec<(&'static str, Seed)> {
        let mut v = seeds();
        v.push(("pastel", pastel()));
        v.push(("tokyonight-day", tokyonight_day()));
        v
    }

    fn theme_of(tokens: &BTreeMap<String, String>) -> Theme {
        let block = |t: &BTreeMap<String, String>| {
            t.iter()
                .map(|(k, v)| format!("  {k}: \"{v}\"\n"))
                .collect::<String>()
        };
        Theme::from_yaml(&format!("name: t\nmode: dark\ncolors:\n{}", block(tokens)))
            .expect("derived tokens form a theme")
    }

    #[test]
    fn every_seed_derives_every_core_and_ansi_token() {
        for (name, seed) in all_seeds() {
            let theme = theme_of(&derive(&seed, "blue"));
            let missing = theme.missing_tokens();
            assert!(missing.is_empty(), "{name}: {missing:?}");
        }
    }

    #[test]
    fn no_seed_derives_a_pair_below_the_hard_floor() {
        for (name, seed) in all_seeds() {
            let theme = theme_of(&derive(&seed, "blue"));
            let failures = contrast::failures(&theme, contrast::HARD_FLOOR);
            assert!(failures.is_empty(), "{name}: {failures:?}");
        }
    }

    #[test]
    fn primary_is_the_bright_variant_of_the_named_ansi_colour() {
        let (_, seed) = seeds().remove(0);
        let t = derive(&seed, "green");
        assert_eq!(t["primary"], "#9fe044");
        assert_eq!(t["sidebar-primary"], "#9fe044");
    }

    #[test]
    fn panels_fade_away_from_the_foreground_in_both_modes() {
        let mut all = seeds();
        let (_, light) = all.remove(1);
        let (_, dark) = all.remove(0);
        // Dark: the card is darker than the background. Light: it is lighter.
        assert!(ratio(&derive(&dark, "blue")["card"], "#000000") < ratio(&dark.bg, "#000000"));
        assert!(ratio(&derive(&light, "blue")["card"], "#ffffff") < ratio(&light.bg, "#ffffff"));
    }

    /// A pastel light scheme: its accents are all under 3:1 on its background.
    fn pastel() -> Seed {
        Seed {
            bg: "#fdf6e3".into(),
            fg: "#5c6a72".into(),
            ansi: [
                "#5c6a72", "#f85552", "#8da101", "#dfa000", "#3a94c5", "#df69ba", "#35a77c",
                "#dfddc8", "#829181", "#f85552", "#8da101", "#dfa000", "#3a94c5", "#df69ba",
                "#35a77c", "#f4f0d9",
            ]
            .map(String::from),
        }
    }

    /// tokyonight-day: its bright accents are 2.5:1 but its normal ones are not.
    fn tokyonight_day() -> Seed {
        Seed {
            bg: "#e1e2e7".into(),
            fg: "#3760bf".into(),
            ansi: [
                "#b4b5b9", "#f52a65", "#587539", "#8c6c3e", "#2e7de9", "#9854f1", "#007197",
                "#6172b0", "#a1a6c5", "#ff4774", "#5c8524", "#a27629", "#358aff", "#a463ff",
                "#007ea8", "#3760bf",
            ]
            .map(String::from),
        }
    }

    #[test]
    fn the_better_contrasting_variant_of_a_hue_is_the_accent() {
        let t = derive(&tokyonight_day(), "blue");
        assert_eq!(
            t["primary"], "#2e7de9",
            "normal blue, not the 2.6:1 bright one"
        );
        assert_eq!(t["destructive"], "#f52a65");
    }

    #[test]
    fn every_ui_accent_clears_the_floor_even_on_a_pastel_scheme() {
        for seed in [pastel(), tokyonight_day()] {
            let (_, ui) = ui_accents(&seed);
            for c in &ui {
                assert!(
                    ratio(c, &seed.bg) >= contrast::HARD_FLOOR,
                    "{c} on {}",
                    seed.bg
                );
            }
        }
    }

    #[test]
    fn lowering_an_accent_keeps_its_hue() {
        let seed = pastel();
        let (donor, ui) = ui_accents(&seed);
        let hue = |c: &str| parse(c).to_alpha_color::<color::Oklch>().components[2];
        for (a, b) in donor.iter().zip(&ui) {
            let d = (hue(a) - hue(b)).abs();
            assert!(d.min(360.0 - d) < 3.0, "{a} -> {b} moved the hue by {d}°");
        }
    }

    #[test]
    fn making_accents_ui_safe_does_not_pull_them_together() {
        for seed in [pastel(), tokyonight_day()] {
            let (donor, ui) = accent_separation(&seed);
            assert!(
                ui >= donor.min(MIN_SEPARATION) - 1e-9,
                "accents went from {donor:.3} apart to {ui:.3}"
            );
        }
    }

    #[test]
    fn an_accent_already_over_the_floor_is_left_alone() {
        let seed = tokyonight_day();
        let (donor, ui) = ui_accents(&seed);
        assert_eq!(donor, ui);
    }

    #[test]
    fn mixing_a_colour_with_itself_changes_nothing() {
        assert_eq!(mix("#1a1b26", "#1a1b26", 40.0), "#1a1b26");
    }

    #[test]
    fn the_token_order_lists_each_token_once() {
        let order = token_order();
        let mut sorted = order.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), order.len());
    }
}
