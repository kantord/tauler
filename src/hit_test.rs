//! Resolving a click to the handler that should receive it.
//!
//! The obvious implementation — walk the measured tree and the layout tree together,
//! pairing children by index — is wrong, and `docs/adr/0018` explains why: takumi
//! *replaces* a node's measured children with flat inline boxes whenever that node
//! holds inline content, so the two trees stop describing the same shape as soon as
//! any element contains text.
//!
//! So the binding is made where both trees are known at once. The walk in
//! [`crate::layout::html`] records each `on_click` against the child-index path of the
//! node it landed on, and takumi's scene walk hands back the same kind of path for
//! every painted node. Matching them is a lookup, not a reconstruction.
//!
//! Two consequences worth knowing:
//!
//! - **Layout runs here.** Hit-testing needs the geometry takumi computed, so this
//!   builds the layout tree the same way a render does. It happens on click, not on
//!   tick, so it costs nothing until someone actually clicks.
//! - **Block-level only.** A `<span>` that is not a flex or grid item gets no layout
//!   node of its own, so a handler on one can never be found. That is not silent: the
//!   first click on the surface logs the handler, naming it as its author wrote it.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Mutex;

use takumi::prelude::Viewport;
use takumi_core::context::RenderContext as TakumiRenderContext;
use takumi_core::geometry::transformed_rect_extents;
use takumi_core::geometry::{NodeId, Point};
use takumi_core::layout::tree::{LayoutResults, LayoutTree, RenderNode};
use takumi_core::scene::{build_stacking_contexts, NodePaint, PaintItemKind, StackingContextNode};
use takumi_core::style::{Affine, ComputedStyle, SizingContext};

use crate::layout::html::{build_tree, Binding, Bindings};

/// Handlers already reported as unreachable, so the warning fires once rather than on
/// every click for the lifetime of the process. Keyed by the node's own description,
/// so an unreachable handler that moves in the tree is reported again.
static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Where an element is on screen, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// The pointer's position relative to this box, as a handler sees it: CSS pixels
    /// from the top-left, unclamped, so it goes negative above or left and past
    /// `width`/`height` beyond (`docs/adr/0020`).
    ///
    /// `at` and `press` are both in physical pixels — where the pointer is now, and
    /// where the button went down. They come back as `x`/`y` and `press_x`/`press_y`
    /// in the same frame, so a handler can subtract one from the other and get a
    /// displacement without keeping anything between calls (`docs/adr/0022`). On the
    /// press itself the two are the same point.
    pub fn pointer(
        &self,
        at: (f32, f32),
        press: (f32, f32),
        dpr: f32,
        buttons: u16,
    ) -> serde_json::Value {
        let dpr = if dpr > 0.0 { dpr } else { 1.0 };
        serde_json::json!({
            "x": (at.0 - self.x) / dpr,
            "y": (at.1 - self.y) / dpr,
            "press_x": (press.0 - self.x) / dpr,
            "press_y": (press.1 - self.y) / dpr,
            "width": self.width / dpr,
            "height": self.height / dpr,
            "buttons": buttons,
        })
    }
}

/// What a pointer landed on.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// Fires on a press. An array of intents, or `{"$handler": n}`.
    pub on_click: Option<serde_json::Value>,
    /// Fires on a press and on every motion until release, and captures the pointer.
    pub on_drag: Option<serde_json::Value>,
    /// The element's box, kept so a drag can be measured against it after the tree
    /// that produced it has been rebuilt (`docs/adr/0020`).
    pub rect: Rect,
}

/// Find the handler for a pointer at `(click_x, click_y)`, in physical pixels.
///
/// The topmost hit wins: the paint list runs bottom-to-top, so the last thing painted
/// over a point is the first thing a click should reach.
pub fn hit_test(
    layout: &serde_json::Value,
    width: u32,
    height: u32,
    dpr: f32,
    click_x: f32,
    click_y: f32,
) -> Option<Hit> {
    let (node, handlers) = build_tree(layout)
        .map_err(|e| tracing::error!(error = %e, "layout parse error"))
        .ok()?;
    if handlers.is_empty() {
        return None;
    }

    let (hit, reachable) = crate::render::with_global_ctx(|global| {
        let context = TakumiRenderContext::builder()
            .fonts(global.fonts.snapshot_with_fallbacks(None))
            .images(Rc::new(global.images.clone()))
            .sizing(
                SizingContext::builder()
                    .viewport(
                        Viewport::new((Some(width), Some(height))).with_device_pixel_ratio(dpr),
                    )
                    .build(),
            )
            .style(Box::new(ComputedStyle::default()))
            .build();

        let root = RenderNode::from_node(&context, node);
        let mut tree = LayoutTree::from_render_node(&root);
        tree.compute_layout(context.sizing.viewport.into());
        let results = tree.into_results();

        let contexts = build_stacking_contexts(
            &root,
            &results,
            NodeId::ROOT,
            Affine::IDENTITY,
            (Some(width as f32), Some(height as f32)),
        )
        .map_err(|e| tracing::error!(error = %e, "stacking context build failed"))
        .ok()?;

        let mut painted = Vec::new();
        collect_paints(&contexts, 0, &mut painted);
        Some(topmost_hit(&painted, &results, &handlers, click_x, click_y))
    })?;

    warn_unreachable(&handlers, &reachable);
    hit
}

/// Flatten the stacking contexts into one list, in paint order.
///
/// Nested contexts are spliced in where their entry sits, so "later in this list"
/// means "painted on top of" for the whole tree, not just within one context.
fn collect_paints<'a>(
    contexts: &'a [StackingContextNode],
    index: usize,
    out: &mut Vec<&'a NodePaint>,
) {
    let Some(context) = contexts.get(index) else {
        return;
    };
    if let Some(root) = context.root() {
        out.push(root);
    }
    for bucket in context.in_paint_order() {
        for item in bucket {
            match &item.kind {
                PaintItemKind::Node(paint) => out.push(paint),
                PaintItemKind::Context(child) => collect_paints(contexts, *child, out),
            }
        }
    }
}

/// The last painted node under the point that carries a handler, plus every path that
/// was painted at all — the second half is what tells an unreachable handler from a
/// reachable one that simply wasn't clicked.
fn topmost_hit(
    painted: &[&NodePaint],
    results: &LayoutResults,
    handlers: &Bindings,
    click_x: f32,
    click_y: f32,
) -> (Option<Hit>, HashSet<Vec<usize>>) {
    let mut reachable = HashSet::new();
    let mut hit = None;

    for paint in painted {
        let Ok(layout) = results.layout(paint.node_id) else {
            continue;
        };
        // Appearing in the paint list is not the same as being clickable: an inline
        // element gets an entry with a zero-area box, which no point can ever fall
        // inside. Counting that as reachable is what would make the warning silent
        // in exactly the case it exists for.
        if layout.size.width > 0.0 && layout.size.height > 0.0 {
            reachable.insert(paint.path.clone());
        }
        let Some(handler) = handlers.iter().find(|h| h.path == paint.path) else {
            continue;
        };
        // Axis-aligned against the *transformed* extents, so a rotated box is tested
        // against its bounding rectangle rather than its true outline — generous at the
        // corners, and the same approximation the paint bounds themselves use.
        if let Some((left, top, right, bottom)) =
            transformed_rect_extents(Point { x: 0.0, y: 0.0 }, layout.size, paint.transform)
        {
            if click_x >= left && click_x <= right && click_y >= top && click_y <= bottom {
                // Overwrite rather than stop: the list runs bottom-to-top, so the last
                // match is the one painted over all the others — which is the one a
                // click lands on.
                hit = Some(Hit {
                    on_click: handler.on_click.clone(),
                    on_drag: handler.on_drag.clone(),
                    rect: Rect {
                        x: left,
                        y: top,
                        width: right - left,
                        height: bottom - top,
                    },
                });
            }
        }
    }

    (hit, reachable)
}

/// Report handlers that no painted node can ever deliver, once each.
///
/// A handler on an inline `<span>` is the common case: it gets no layout node, so
/// nothing in the paint list carries its path and the click silently goes nowhere.
/// Saying so is the whole reason this is not a silent limit (`docs/adr/0018`).
fn warn_unreachable(handlers: &Bindings, reachable: &HashSet<Vec<usize>>) {
    let unreachable: Vec<&Binding> = handlers
        .iter()
        .filter(|h| !reachable.contains(&h.path))
        .collect();
    if unreachable.is_empty() {
        return;
    }

    let mut warned = WARNED.lock().unwrap();
    let warned = warned.get_or_insert_with(HashSet::new);
    for handler in unreachable {
        if warned.insert(handler.label.clone()) {
            tracing::warn!(
                node = %handler.label,
                "on_click on a node that is never painted on its own — inline elements \
                 cannot take clicks; move the handler to a block-level element"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{hit_test, Rect};
    use crate::config::FontConfig;
    use crate::init_global_ctx;

    /// A drag is measured from where it started, and a handler that remembers nothing
    /// between calls cannot keep that itself (`docs/adr/0022`).
    #[test]
    fn the_pointer_carries_where_the_press_landed() {
        let rect = Rect {
            x: 100.0,
            y: 10.0,
            width: 40.0,
            height: 40.0,
        };
        let p = rect.pointer((130.0, 30.0), (110.0, 20.0), 1.0, 1);
        assert_eq!(p["x"], 30.0);
        assert_eq!(p["y"], 20.0);
        assert_eq!(p["press_x"], 10.0, "in the same frame as x");
        assert_eq!(p["press_y"], 10.0);
    }

    /// Both points are CSS pixels, so a mapper's arithmetic is free of the scale.
    #[test]
    fn the_press_point_is_scaled_like_the_pointer() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 80.0,
        };
        let p = rect.pointer((40.0, 40.0), (20.0, 20.0), 2.0, 1);
        assert_eq!(p["x"], 20.0);
        assert_eq!(p["press_x"], 10.0);
    }

    fn inline_handler(id: &str, class: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "div",
            "style": {"width": 200, "height": 100},
            "children": [{
                "type": "span",
                "id": id,
                "class": class,
                "on_click": [{"channel": "t", "event": {"type": "x"}}],
                "children": ["x"],
            }],
        })
    }

    /// ADR 0018 promises the inline limit is "not silent": the handler is named the
    /// way its author wrote it, so a path of child indices never reaches a log line.
    #[test]
    #[tracing_test::traced_test]
    fn an_unreachable_handler_is_named_in_a_warning() {
        init_global_ctx(FontConfig::default());
        hit_test(
            &inline_handler("dismiss", "text-[11px]"),
            200,
            100,
            1.0,
            20.0,
            10.0,
        );

        assert!(logs_contain("WARN"), "an unreachable handler warns");
        assert!(
            logs_contain(r#"<span id="dismiss" class="text-[11px]">"#),
            "the warning names the node as it was written"
        );
    }

    /// The warning fires per handler, not per click.
    #[test]
    #[tracing_test::traced_test]
    fn an_unreachable_handler_warns_only_once() {
        init_global_ctx(FontConfig::default());
        let layout = inline_handler("warn-once-probe", "");
        for _ in 0..5 {
            hit_test(&layout, 200, 100, 1.0, 20.0, 10.0);
        }

        logs_assert(|lines: &[&str]| {
            let n = lines
                .iter()
                .filter(|l| l.contains("warn-once-probe"))
                .count();
            match n {
                1 => Ok(()),
                _ => Err(format!("expected 1 warning across 5 clicks, got {n}")),
            }
        });
    }
}
