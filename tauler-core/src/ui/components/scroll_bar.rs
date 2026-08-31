//! The thumb and track of a scrollbar — the draggable indicator, not the clipped
//! content it scrolls (that's `@ui/scroll-area`, `scroll_area.rs`, which composes
//! this component internally and owns the viewport that clips and translates the
//! actual content).
//!
//! Unlike `<Slider>`, where the whole track captures the drag, only the thumb does
//! here — the before/after spacers are inert. And unlike `<Slider>`, which reads an
//! absolute position under the pointer, dragging the thumb is displacement-based
//! (ADR 0022): the shim adds how far the pointer has moved to `scroll_top`, the same
//! "measured from where you pressed" idea `<Knob>` uses, rather than mapping the
//! pointer's raw position onto the track.
//!
//! ## What it does not do
//!
//! It holds nothing (ADR 0012). `scroll_top` comes from outside on every tick, and
//! the thumb only sees a drag's effect once whatever owns the scroll position has
//! re-emitted it. A `<ScrollBar>` with no `on_change` is inert — a static indicator
//! of `scroll_top`.
//!
//! The thumb has a pixel floor (`MIN_THUMB_HEIGHT`) so a large `content_height` /
//! small `viewport_height` ratio can't shrink it to an unusable sliver; the spacers
//! either side have no such floor, since they're free to shrink to nothing so the
//! thumb can claim the space. And `content_height <= viewport_height` — nothing to
//! scroll — draws a full-height thumb rather than dividing by zero.

use serde_json::{Map, Value};

use crate::ui::{component, cva::Cva, rsx};

/// Pixel floor for the thumb's height, so a large content_height / small
/// viewport_height ratio can't shrink it to an unusable sliver.
const MIN_THUMB_HEIGHT: f64 = 20.0;

const SCROLL_BAR_VARIANTS: Cva = Cva {
    base: "flex flex-col h-full w-[6px]",
    variants: &[],
    defaults: &[],
};

/// The thumb's own visible styling — the spacers stay bare, since only the thumb
/// should be seen. Plain `bg-border`, not a slash-opacity variant like
/// `bg-muted-foreground/40`: takumi's Tailwind color parser has no support for the
/// `/<alpha>` modifier (verified against `takumi-core`'s `style/tw` parser — the only
/// `/` handling there is `text-sm/6`'s font-size/line-height pairing), so a
/// slash-opacity class silently drops, leaving the thumb with no background at all.
const THUMB_CLASS: &str = "bg-border rounded-full";

fn flex(amount: f64, min_height: f64) -> Option<Map<String, Value>> {
    let mut style = Map::new();
    style.insert("flexGrow".into(), Value::from(amount));
    style.insert("flexShrink".into(), Value::from(1.0));
    style.insert("flexBasis".into(), Value::from(0.0));
    style.insert("minHeight".into(), Value::from(min_height));
    Some(style)
}

/// Clamps `scroll_top` to the valid range `[0, content_height - viewport_height]`,
/// collapsing to `0.0` whenever the content already fits the viewport (avoids the
/// negative/degenerate range that would otherwise divide by zero or go negative).
/// Shared by `scroll_bar` (thumb/track position) and `@ui/scroll-area`'s content
/// translation, so the two can't drift apart.
pub(crate) fn clamp_scroll_top(scroll_top: f64, content_height: f64, viewport_height: f64) -> f64 {
    if content_height <= viewport_height {
        0.0
    } else {
        scroll_top.clamp(0.0, (content_height - viewport_height).max(0.0))
    }
}

/// A vertical scrollbar: a track with a thumb whose position and size are derived
/// from `scroll_top`, `content_height`, and `viewport_height` — the thumb sits at
/// `scroll_top / content_height` down the track and spans `viewport_height /
/// content_height` of it, floored at `MIN_THUMB_HEIGHT` so it never shrinks below a
/// draggable size.
///
/// `<ScrollArea>` renders one of these internally, so most layouts never reach for
/// `<ScrollBar>` directly. Import it standalone when a scrollable region needs its
/// indicator drawn somewhere other than where `<ScrollArea>` puts it — alongside the
/// viewport rather than inside it, for instance.
///
/// It never remembers anything (ADR 0012): `scroll_top` is read every tick from
/// whatever owns it, and `on_change` receives where a drag has moved the thumb to and
/// returns the intents to send. Omit `on_change` and the scrollbar still renders — a
/// static indicator of `scroll_top`, not interactive.
///
/// # JSX
/// ```jsx
/// <ScrollBar
///   scroll_top={40}
///   content_height={480}
///   viewport_height={120}
///   class="h-[120px]"
/// />
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/scroll-area
#[component("@ui/scroll-bar")]
pub fn scroll_bar(
    scroll_top: f64,
    content_height: f64,
    viewport_height: f64,
    on_drag: Option<Value>,
    show_scrollbar: Option<bool>,
    class: Option<String>,
) -> Node {
    let root_class = SCROLL_BAR_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    let scroll_top = clamp_scroll_top(scroll_top, content_height, viewport_height);
    let needed = content_height > viewport_height;
    let show_thumb = show_scrollbar.unwrap_or(needed);
    let (before, thumb_frac, after) = if content_height <= viewport_height {
        (0.0, 1.0, 0.0)
    } else {
        let before = scroll_top / content_height;
        let thumb_frac = viewport_height / content_height;
        let after = (content_height - scroll_top - viewport_height) / content_height;
        (before, thumb_frac, after)
    };
    let thumb_style = flex(thumb_frac, MIN_THUMB_HEIGHT);
    let thumb = if show_thumb {
        rsx! { <div class={THUMB_CLASS} style={thumb_style} on_drag={on_drag} /> }
    } else {
        rsx! { <div style={thumb_style} /> }
    };
    rsx! {
        <div class={root_class}>
            <div style={flex(before, 0.0)} />
            {thumb}
            <div style={flex(after, 0.0)} />
        </div>
    }
}

/// The `<ScrollBar>` shim's JavaScript half.
///
/// Owns the arithmetic, because only JavaScript can run when the pointer moves: it
/// turns the thumb's own pixel displacement into a change in `scroll_top` and calls
/// `on_change` with it (ADR 0021). Rust never learns what a range is.
pub const SCROLL_BAR_SHIM_JS: &str = r#"
    globalThis.__tauler_scroll_bar = (props) => {
        const {
            scroll_top = 0, content_height, viewport_height, on_change, show_scrollbar,
            class: cls,
        } = props ?? {};
        const rendered = { scroll_top, content_height, viewport_height };
        if (show_scrollbar != null) rendered.show_scrollbar = show_scrollbar;
        if (cls != null) rendered.class = cls;
        if (typeof on_change === "function") {
            // Registered here rather than by `h`: this calls the Rust component
            // directly, so these props never pass through the node flattener.
            rendered.on_drag = __tauler_handler_ref((p) => {
                // The thumb itself captures the drag (only the thumb, per
                // scroll_bar.rs's only_the_thumb_captures_drags), so p.height/p.width
                // are the thumb's own rendered pixel size, not the track's. Absent the
                // thumb's minimum-height floor, its height is
                // (viewport_height / content_height) * trackHeightPx, so back-solving:
                // trackHeightPx = p.height * content_height / viewport_height, and
                // pixels-per-content-unit = trackHeightPx / content_height
                //                         = p.height / viewport_height.
                // This degrades once MIN_THUMB_HEIGHT (scroll_bar.rs) pins p.height to
                // its floor rather than the true ratio, so drag speed goes slightly
                // non-linear at extreme content/viewport ratios. Accepted, the same
                // tolerance this codebase already gives Knob's hub dead-zone and
                // Slider's step-snapping — a thumb-only capture has no way to learn
                // the real track height, so solving it exactly is not on the table.
                const dy = p.y - p.press_y;
                const contentDelta = p.height > 0 ? dy * (viewport_height / p.height) : 0;
                const maxScrollTop = Math.max(content_height - viewport_height, 0);
                const newScrollTop = Math.min(
                    Math.max(scroll_top + contentDelta, 0),
                    maxScrollTop,
                );
                return __tauler_intents(on_change(newScrollTop));
            });
        }
        return __ui_scroll_bar(rendered);
    };
"#;

/// Puts both halves in place: the Rust renderer under `__ui_scroll_bar`, and the shim
/// that `@ui/scroll-bar` actually exports.
#[cfg(feature = "quickjs")]
fn register_scroll_bar(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    (__UI_ENTRY_SCROLL_BAR.register)(ctx)?;
    ctx.eval::<(), _>(SCROLL_BAR_SHIM_JS)
}

/// What `import { ScrollBar } from "@ui/scroll-bar"` resolves to.
///
/// Registered in place of `__UI_ENTRY_SCROLL_BAR`: the Rust half stays reachable from
/// JavaScript but is not importable, so there is only one `ScrollBar` and it is the
/// one that accepts `on_change`.
#[cfg(feature = "quickjs")]
pub const __UI_ENTRY_SCROLL_BAR_SHIM: crate::ui::registry::EsEntry = crate::ui::registry::EsEntry {
    module_path: "@ui/scroll-bar",
    export_name: "ScrollBar",
    global_name: "__tauler_scroll_bar",
    register: register_scroll_bar,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiComponent;

    fn render(scroll_top: f64, on_drag: Option<Value>) -> Value {
        serde_json::to_value(ScrollBar::render(ScrollBarProps {
            scroll_top,
            content_height: 100.0,
            viewport_height: 50.0,
            on_drag,
            show_scrollbar: None,
            class: None,
        }))
        .expect("scroll bar serialises")
    }

    fn parts(rendered: &Value) -> &Vec<Value> {
        rendered["children"].as_array().expect("children")
    }

    fn grow(part: &Value) -> f64 {
        part["style"]["flexGrow"].as_f64().expect("flexGrow")
    }

    /// Only block-level elements are hittable (ADR 0018).
    #[test]
    fn the_track_is_a_block_element() {
        assert_eq!(render(0.0, None)["type"], "div");
    }

    /// The three segments (before spacer, thumb, after spacer) tile the track as flex
    /// fractions of `content_height`, mirroring `slider.rs`'s `flex`/`grow` precedent
    /// generalized from two segments to three.
    #[test]
    fn the_segments_reflect_scroll_position_as_flex_fractions() {
        let rendered = render(20.0, None);
        let parts = parts(&rendered);

        assert!(
            (grow(&parts[0]) - 0.2).abs() < 1e-9,
            "before spacer is scroll_top / content_height"
        );
        assert!(
            (grow(&parts[1]) - 0.5).abs() < 1e-9,
            "thumb is viewport_height / content_height"
        );
        assert!(
            (grow(&parts[2]) - 0.3).abs() < 1e-9,
            "after spacer is the remaining content below the viewport"
        );
    }

    /// The root is inert; only the middle child (the thumb) captures drags. This is
    /// the opposite placement from `slider.rs`, where the root itself is the single
    /// capturing element.
    #[test]
    fn only_the_thumb_captures_drags() {
        let handler = serde_json::json!({"$handler": 1});
        let rendered = render(0.0, Some(handler.clone()));

        let children = rendered["children"].as_array().expect("children");
        assert_eq!(children.len(), 3, "before spacer, thumb, after spacer");

        assert_eq!(children[1]["on_drag"], handler, "the thumb captures drags");
        assert!(
            children[0].get("on_drag").is_none(),
            "the before spacer never captures anything"
        );
        assert!(
            children[2].get("on_drag").is_none(),
            "the after spacer never captures anything"
        );
        assert!(
            rendered.get("on_drag").is_none(),
            "the root is no longer the capturing element"
        );
    }

    /// The thumb needs a pixel floor so a large content_height / small viewport_height
    /// ratio doesn't shrink it to an unusable sliver; the spacers have no such floor,
    /// since they're allowed to shrink to nothing so the thumb can claim it.
    #[test]
    fn the_thumb_has_a_minimum_height_floor() {
        let rendered = render(20.0, None);
        let parts = parts(&rendered);

        assert_eq!(
            parts[1]["style"]["minHeight"], 20.0,
            "the thumb has a minimum pixel height floor"
        );
        assert_eq!(
            parts[0]["style"]["minHeight"], 0.0,
            "the before spacer keeps no floor"
        );
        assert_eq!(
            parts[2]["style"]["minHeight"], 0.0,
            "the after spacer keeps no floor"
        );
    }

    /// Mirrors `slider.rs`'s `a_value_outside_the_range_pins_to_the_nearer_end`,
    /// generalized to `scroll_top`'s valid range of `[0, content_height -
    /// viewport_height]` (here `[0, 50.0]`).
    #[test]
    fn an_out_of_range_scroll_top_pins_to_the_nearer_end() {
        let below = render(-400.0, None);
        let below_parts = parts(&below);
        assert_eq!(grow(&below_parts[0]), 0.0, "pinned to the top");
        assert_eq!(grow(&below_parts[2]), 0.5, "all remaining content is below");

        let above = render(400.0, None);
        let above_parts = parts(&above);
        assert_eq!(grow(&above_parts[0]), 0.5, "pinned to the bottom");
        assert_eq!(grow(&above_parts[2]), 0.0, "no remaining content is below");
    }

    /// Mirrors `slider.rs`'s `a_degenerate_range_draws_an_empty_track`: a viewport at
    /// least as large as the content (nothing to scroll) must draw a full-height
    /// thumb rather than an overflowing one, and literal-zero content must not divide
    /// by zero and produce NaN.
    #[test]
    fn a_degenerate_content_height_draws_a_full_thumb() {
        let rendered = serde_json::to_value(ScrollBar::render(ScrollBarProps {
            scroll_top: 0.0,
            content_height: 10.0,
            viewport_height: 50.0,
            on_drag: None,
            show_scrollbar: Some(true),
            class: None,
        }))
        .unwrap();
        let full_parts = parts(&rendered);
        assert_eq!(grow(&full_parts[0]), 0.0, "no before spacer");
        assert_eq!(grow(&full_parts[1]), 1.0, "thumb fills the whole track");
        assert_eq!(grow(&full_parts[2]), 0.0, "no after spacer");

        let rendered = serde_json::to_value(ScrollBar::render(ScrollBarProps {
            scroll_top: 0.0,
            content_height: 0.0,
            viewport_height: 0.0,
            on_drag: None,
            show_scrollbar: Some(true),
            class: None,
        }))
        .unwrap();
        let zero_parts = parts(&rendered);
        assert!(
            !grow(&zero_parts[0]).is_nan(),
            "before spacer must not be NaN"
        );
        assert!(!grow(&zero_parts[1]).is_nan(), "thumb must not be NaN");
        assert!(
            !grow(&zero_parts[2]).is_nan(),
            "after spacer must not be NaN"
        );
        assert_eq!(grow(&zero_parts[0]), 0.0, "no before spacer");
        assert_eq!(grow(&zero_parts[1]), 1.0, "thumb fills the whole track");
        assert_eq!(grow(&zero_parts[2]), 0.0, "no after spacer");
    }

    /// Mirrors `slider.rs`'s `class_is_appended_to_the_track`: the author's `class`
    /// prop must reach the root element's resolved class string.
    #[test]
    fn class_is_appended_to_the_track() {
        let rendered = serde_json::to_value(ScrollBar::render(ScrollBarProps {
            scroll_top: 0.0,
            content_height: 100.0,
            viewport_height: 50.0,
            on_drag: None,
            show_scrollbar: None,
            class: Some("h-[160px]".into()),
        }))
        .unwrap();
        assert!(rendered["class"].as_str().unwrap().ends_with("h-[160px]"));
    }

    /// Real shadcn/Radix ScrollArea behavior: by default ("auto", `show_scrollbar:
    /// None`), a scrollbar whose content already fits the viewport draws no visible
    /// thumb at all — no `THUMB_CLASS`, no `on_drag` — rather than the full-height
    /// draggable thumb `a_degenerate_content_height_draws_a_full_thumb` covers.
    #[test]
    fn auto_hides_the_thumb_when_nothing_needs_scrolling() {
        let rendered = serde_json::to_value(ScrollBar::render(ScrollBarProps {
            scroll_top: 0.0,
            content_height: 10.0,
            viewport_height: 50.0,
            on_drag: Some(serde_json::json!({"$handler": 1})),
            class: None,
            show_scrollbar: None,
        }))
        .unwrap();
        let parts = parts(&rendered);

        assert!(
            parts[1].get("class").is_none(),
            "no visible thumb class when nothing needs scrolling"
        );
        assert!(
            parts[1].get("on_drag").is_none(),
            "an invisible scrollbar shouldn't be draggable"
        );
    }
}
