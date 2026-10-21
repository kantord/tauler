//! `themeColor(token, NN / 100)` paints what `bg-token/NN` paints, for every form a theme may
//! write a colour in — compared as rendered pixels, because before the two shared
//! `color_with_alpha` the class side left a non-`#rrggbb` `/NN` to takumi, which painted nothing.

use rstest::rstest;
use tauler::config::FontConfig;
use tauler::theme::resolver::{resolve_theme_tokens, theme_color};
use tauler::theme::Theme;
use tauler::{init_global_ctx, render_frame_rgba};

/// The RGBA of a 4×4 render of a div with `class`, after theme resolution.
fn paint(theme: &Theme, class: &str) -> [u8; 4] {
    init_global_ctx(FontConfig::default());
    let mut layout = serde_json::json!({ "type": "div", "class": class, "children": [] });
    resolve_theme_tokens(&mut layout, theme);
    let rgba = render_frame_rgba(&layout, 4, 4, 1.0, None);
    let i = (2 * 4 + 2) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

#[rstest]
#[case::six_digit_hex("#7aa2f7")]
#[case::three_digit_hex("#7af")]
#[case::eight_digit_hex("#7aa2f780")]
#[case::oklch("oklch(0.7 0.12 260)")]
#[case::oklch_with_alpha("oklch(1 0 0 / 50%)")]
fn theme_color_paints_what_the_class_suffix_paints(#[case] value: &str) {
    let theme =
        Theme::from_yaml(&format!("name: t\nmode: dark\ncolors:\n  c: \"{value}\"\n")).unwrap();
    for percent in [13, 40, 100] {
        let class = paint(&theme, &format!("w-full h-full bg-c/{percent}"));
        let value = theme_color(&theme.colors, "c", Some(f64::from(percent) / 100.0));
        let style = paint(&theme, &format!("w-full h-full bg-[{value}]"));
        assert_eq!(class, style, "{value} at /{percent}");
    }
}
