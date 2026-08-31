//! The viewport half of the scroll pair — clips overflowing content to its box and
//! translates it by `scroll_top`. Composes `<ScrollBar>` (`scroll_bar.rs`)
//! internally for the draggable indicator, so most layouts reach for `<ScrollArea>`
//! alone rather than assembling the two by hand.
//!
//! Stateless like its sibling (ADR 0012): `scroll_top` comes from outside every
//! tick, and dragging the composed scrollbar only changes what's on screen once
//! whatever owns the value has re-emitted it through `on_change`.
//!
//! Where `<ScrollBar>` only draws the indicator, `<ScrollArea>` is the one that takes
//! real child content via `children` — the actual scrollable material, clipped by
//! the viewport and translated by `-scroll_top` px.

use serde_json::{Map, Value};

use super::scroll_bar::{clamp_scroll_top, ScrollBar, ScrollBarProps};
use crate::ui::{component, cva::Cva, rsx, Node};

const VIEWPORT_VARIANTS: Cva = Cva {
    base: "flex-1 h-full overflow-hidden",
    variants: &[],
    defaults: &[],
};

const SCROLL_AREA_VARIANTS: Cva = Cva {
    base: "flex flex-row",
    variants: &[],
    defaults: &[],
};

/// The content's transform style: translated up (negative y) by the clamped scroll
/// offset. `translate` (not `translateY`) is takumi-core's recognized longhand, and
/// it takes an x/y pair, so the horizontal component must stay `0px`.
fn content_translate(clamped_scroll_top: f64) -> Option<Map<String, Value>> {
    let mut style = Map::new();
    style.insert(
        "translate".into(),
        Value::from(format!("0px -{clamped_scroll_top}px")),
    );
    Some(style)
}

/// A scrollable viewport: clips `children` to its box and translates them up by
/// `scroll_top`, with a `<ScrollBar>` rendered alongside as the draggable indicator.
/// `content_height` and `viewport_height` describe the content's full height and the
/// visible window into it — the same terms `<ScrollBar>` uses, since the two share
/// `clamp_scroll_top` so their notions of the valid range can't drift apart.
///
/// It never remembers anything (ADR 0012): `scroll_top` is read every tick from
/// whatever owns it, and `on_change` (wired through to the composed scrollbar's
/// thumb) receives where a drag has moved it and returns the intents to send.
///
/// ```jsx
/// <Module bin="~/.cargo/bin/tauler-logs">
///   {(data, events) => (
///     <ScrollArea
///       scroll_top={data?.scroll_top ?? 0}
///       content_height={data?.lines?.length * 18 ?? 0}
///       viewport_height={120}
///       on_change={top => events.setScrollTop({ top })}
///       class="h-[120px] w-[240px]"
///     >
///       {(data?.lines ?? []).map(line => <span class="text-[11px]">{line}</span>)}
///     </ScrollArea>
///   )}
/// </Module>
/// ```
///
/// The drag sets nothing locally: it sends intents, the module changes `scroll_top`,
/// and the next tick brings the new value back — the same round trip `<Slider>` and
/// `<Knob>` make. Omit `on_change` and `<ScrollArea>` still renders, clipped and
/// translated to `scroll_top`, just not draggable.
///
/// `show_scrollbar` controls the composed scrollbar's visibility: omitted, it shows
/// only when `content_height > viewport_height` (nothing to scroll draws no visible
/// thumb, matching shadcn's own default); `true` forces it to always show, `false`
/// forces it to always hide, regardless of whether scrolling is actually needed.
///
/// # JSX
/// ```jsx
/// <ScrollArea
///   scroll_top={40}
///   content_height={300}
///   viewport_height={120}
///   class="h-[120px] w-[240px]"
/// >
///   <span class="text-[11px]">a lot of content…</span>
/// </ScrollArea>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/scroll-area
#[component("@ui/scroll-area")]
pub fn scroll_area(
    children: Vec<Node>,
    scroll_top: f64,
    content_height: f64,
    viewport_height: f64,
    on_drag: Option<Value>,
    show_scrollbar: Option<bool>,
    class: Option<String>,
) -> Node {
    let root_class = SCROLL_AREA_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    let viewport_class = VIEWPORT_VARIANTS.resolve(&[], "");
    let clamped_scroll_top = clamp_scroll_top(scroll_top, content_height, viewport_height);
    let content_style = content_translate(clamped_scroll_top);
    let bar = ScrollBar::render(ScrollBarProps {
        scroll_top,
        content_height,
        viewport_height,
        on_drag,
        show_scrollbar,
        class: None,
    });
    rsx! {
        <div class={root_class}>
            <div class={viewport_class}>
                <div style={content_style}>{children}</div>
            </div>
            {bar}
        </div>
    }
}

/// The `<ScrollArea>` shim's JavaScript half.
///
/// The arithmetic is duplicated from `scroll_bar.rs`'s shim rather than shared — this
/// codebase's convention is one shim per component (see `slider.rs`, `knob.rs`) — but
/// it has to stay identical: `ScrollArea` composes `ScrollBar` in Rust and forwards
/// `on_drag` straight through unchanged (see `scroll_area`, above), so the thumb that
/// ends up capturing the drag is the very same element either shim's arithmetic has
/// to explain.
pub const SCROLL_AREA_SHIM_JS: &str = r#"
    globalThis.__tauler_scroll_area = (props) => {
        const {
            children, scroll_top = 0, content_height, viewport_height, on_change,
            show_scrollbar, class: cls,
        } = props ?? {};
        const rendered = { children, scroll_top, content_height, viewport_height };
        if (show_scrollbar != null) rendered.show_scrollbar = show_scrollbar;
        if (cls != null) rendered.class = cls;
        if (typeof on_change === "function") {
            // Registered here rather than by `h`: this calls the Rust component
            // directly, so these props never pass through the node flattener.
            rendered.on_drag = __tauler_handler_ref((p) => {
                // See scroll_bar.rs's SCROLL_BAR_SHIM_JS for the derivation and the
                // MIN_THUMB_HEIGHT caveat this shares — p.height/p.width here are
                // still the thumb's own rendered pixel size, since the thumb is what
                // captures the drag no matter which component's shim built the
                // handler.
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
        return __ui_scroll_area(rendered);
    };
"#;

/// Puts both halves in place: the Rust renderer under `__ui_scroll_area`, and the
/// shim that `@ui/scroll-area` actually exports.
#[cfg(feature = "quickjs")]
fn register_scroll_area(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    (__UI_ENTRY_SCROLL_AREA.register)(ctx)?;
    ctx.eval::<(), _>(SCROLL_AREA_SHIM_JS)
}

/// What `import { ScrollArea } from "@ui/scroll-area"` resolves to.
///
/// Registered in place of `__UI_ENTRY_SCROLL_AREA`: the Rust half stays reachable from
/// JavaScript but is not importable, so there is only one `ScrollArea` and it is the
/// one that accepts `on_change`.
#[cfg(feature = "quickjs")]
pub const __UI_ENTRY_SCROLL_AREA_SHIM: crate::ui::registry::EsEntry =
    crate::ui::registry::EsEntry {
        module_path: "@ui/scroll-area",
        export_name: "ScrollArea",
        global_name: "__tauler_scroll_area",
        register: register_scroll_area,
    };

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiComponent;

    fn render() -> Value {
        serde_json::to_value(ScrollArea::render(ScrollAreaProps {
            children: vec![],
            scroll_top: 0.0,
            content_height: 100.0,
            viewport_height: 50.0,
            on_drag: None,
            show_scrollbar: None,
            class: None,
        }))
        .expect("scroll area serialises")
    }

    /// The root composes two children: the clipped/translated viewport, and the
    /// scrollbar built via a direct call to `scroll_bar(...)`.
    #[test]
    fn the_root_has_a_viewport_and_a_scrollbar() {
        let rendered = render();

        assert_eq!(
            rendered["children"].as_array().expect("children").len(),
            2,
            "the viewport and the scrollbar"
        );
    }

    /// The first child is the viewport; it must clip content that overflows it
    /// (the actual translate-based scrolling is a later cycle).
    #[test]
    fn the_viewport_clips_overflow() {
        let rendered = render();

        assert!(
            rendered["children"][0]["class"]
                .as_str()
                .unwrap()
                .contains("overflow-hidden"),
            "the viewport's class must include overflow-hidden"
        );
    }

    /// The viewport's single child is the real scrollable content; it carries a
    /// `translate` style whose vertical component is `-scroll_top` (clamped to
    /// `[0, content_height - viewport_height]`, the same range `scroll_bar.rs`
    /// clamps to). `translate` is takumi-core's individual CSS transform
    /// property — it takes an x/y `SpacePair<Length>`, so a single value moves
    /// both axes; the horizontal component must stay `0px` to avoid a spurious
    /// horizontal shift (takumi-core-0.23.0/src/style/stylesheets.rs:1072,
    /// `translate: SpacePair<Length>`; takumi-core-0.23.0/src/style/properties/
    /// space_pair.rs `SpacePair::from_single` sets x == y, which is why a lone
    /// `-{px}px` would be wrong here).
    #[test]
    fn the_content_translates_by_the_clamped_scroll_position() {
        let in_range = serde_json::to_value(ScrollArea::render(ScrollAreaProps {
            children: vec![],
            scroll_top: 20.0,
            content_height: 100.0,
            viewport_height: 50.0,
            on_drag: None,
            show_scrollbar: None,
            class: None,
        }))
        .expect("scroll area serialises");
        assert_eq!(
            in_range["children"][0]["children"][0]["style"]["translate"], "0px -20px",
            "in-range scroll_top translates the content up by that many pixels"
        );

        let out_of_range = serde_json::to_value(ScrollArea::render(ScrollAreaProps {
            children: vec![],
            scroll_top: 400.0,
            content_height: 100.0,
            viewport_height: 50.0,
            on_drag: None,
            show_scrollbar: None,
            class: None,
        }))
        .expect("scroll area serialises");
        assert_eq!(
            out_of_range["children"][0]["children"][0]["style"]["translate"], "0px -50px",
            "scroll_top beyond the max clamps to content_height - viewport_height"
        );
    }

    /// The content div is the real scrollable content, so whatever the caller
    /// passes as `children` must reach it verbatim — not be discarded.
    #[test]
    fn the_content_renders_the_given_children() {
        let child = rsx! { <span /> };
        let rendered = serde_json::to_value(ScrollArea::render(ScrollAreaProps {
            children: vec![child.clone()],
            scroll_top: 0.0,
            content_height: 100.0,
            viewport_height: 50.0,
            on_drag: None,
            show_scrollbar: None,
            class: None,
        }))
        .expect("scroll area serialises");

        let content_children = rendered["children"][0]["children"][0]["children"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        assert_eq!(
            content_children.len(),
            1,
            "the content div must render exactly the one child that was passed in"
        );
        assert_eq!(
            content_children.first(),
            Some(&serde_json::to_value(&child).expect("child serialises")),
            "the content div's child must match the node that was passed in"
        );
    }

    /// `on_drag` is a pass-through to the scrollbar: the caller hands `ScrollArea` a
    /// handler, and it must reach the scrollbar's thumb (its own middle child, per
    /// `scroll_bar.rs`'s `only_the_thumb_captures_drags`) unchanged.
    #[test]
    fn the_on_drag_handler_reaches_the_scrollbar() {
        let handler = serde_json::json!({"$handler": 1});
        let rendered = serde_json::to_value(ScrollArea::render(ScrollAreaProps {
            children: vec![],
            scroll_top: 20.0,
            content_height: 100.0,
            viewport_height: 50.0,
            on_drag: Some(handler.clone()),
            show_scrollbar: None,
            class: None,
        }))
        .expect("scroll area serialises");

        assert_eq!(
            rendered["children"][1]["children"][1]["on_drag"], handler,
            "the scrollbar's thumb carries the on_drag handler passed to ScrollArea"
        );
    }

    /// Mirrors `scroll_bar.rs`'s `class_is_appended_to_the_track`: the caller's
    /// `class` prop must reach the root element's resolved class string.
    #[test]
    fn class_is_appended_to_the_root() {
        let rendered = serde_json::to_value(ScrollArea::render(ScrollAreaProps {
            children: vec![],
            scroll_top: 0.0,
            content_height: 100.0,
            viewport_height: 50.0,
            on_drag: None,
            show_scrollbar: None,
            class: Some("h-[200px]".into()),
        }))
        .expect("scroll area serialises");

        assert!(rendered["class"].as_str().unwrap().ends_with("h-[200px]"));
    }
}
