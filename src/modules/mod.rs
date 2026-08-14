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
//!   builds the layout tree the same way a render does. It is the same cost the
//!   measure pass used to be, and it happens on click, not on tick.
//! - **Block-level only.** A `<span>` gets no layout node of its own, so a handler on
//!   one can never be found. That is not silent: a handler whose path never appears in
//!   the paint list is logged.

use std::collections::HashSet;
use std::rc::Rc;

use takumi::prelude::Viewport;
use takumi_core::context::RenderContext as TakumiRenderContext;
use takumi_core::geometry::transformed_rect_extents;
use takumi_core::geometry::{NodeId, Point, Size};
use takumi_core::layout::tree::{LayoutResults, LayoutTree, RenderNode};
use takumi_core::scene::{build_stacking_contexts, NodePaint, PaintItemKind, StackingContextNode};
use takumi_core::style::{Affine, ComputedStyle, SizingContext};

use crate::layout::html::{build_tree, Handlers};

/// Find the handler for a click at `(click_x, click_y)` in physical pixels.
///
/// Returns the winning node's render path alongside its `on_click` value. The
/// topmost hit wins: the paint list is in paint order, so the last thing painted over
/// a point is the first thing a click should reach.
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

    let hit = crate::render::with_global_ctx(|global| {
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

    warn_unreachable(&handlers, &hit.1);
    hit.0
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

/// A resolved click: which node won, and what it wants dispatched.
type Hit = (Vec<usize>, serde_json::Value);

/// The last painted node under the point that carries a handler, plus every path that
/// was painted at all (so unreachable handlers can be reported).
fn topmost_hit(
    painted: &[&NodePaint],
    results: &LayoutResults,
    handlers: &Handlers,
    click_x: f32,
    click_y: f32,
) -> (Option<Hit>, HashSet<Vec<usize>>) {
    let mut reachable = HashSet::new();
    let mut hit = None;

    for paint in painted {
        reachable.insert(paint.path.clone());
        let Some(handler) = handlers.iter().find(|(path, _)| *path == paint.path) else {
            continue;
        };
        let Ok(layout) = results.layout(paint.node_id) else {
            continue;
        };
        if contains(paint.transform, layout.size, click_x, click_y) {
            // Overwrite rather than stop: the list runs bottom-to-top, so the last
            // match is the one painted over all the others — which is the one a
            // click lands on.
            hit = Some((paint.path.clone(), handler.1.clone()));
        }
    }

    (hit, reachable)
}

/// Whether the point falls inside a node's border box once its transform is applied.
///
/// Axis-aligned against the *transformed* extents, so a rotated box is tested against
/// its bounding rectangle rather than its true outline — generous at the corners, and
/// the same approximation the paint bounds themselves use.
fn contains(transform: Affine, size: Size<f32>, click_x: f32, click_y: f32) -> bool {
    let Some((left, top, right, bottom)) =
        transformed_rect_extents(Point { x: 0.0, y: 0.0 }, size, transform)
    else {
        return false;
    };
    click_x >= left && click_x <= right && click_y >= top && click_y <= bottom
}

/// Report handlers that no painted node can ever deliver.
///
/// A handler on a `<span>` is the common case: inline elements get no layout node, so
/// nothing in the paint list carries their path and the click silently goes nowhere.
/// Saying so is the whole reason this is not a silent limit (`docs/adr/0018`).
fn warn_unreachable(handlers: &Handlers, reachable: &HashSet<Vec<usize>>) {
    for (path, _) in handlers {
        if !reachable.contains(path) {
            tracing::warn!(
                path = ?path,
                "on_click on a node that is never painted on its own — inline elements \
                 cannot receive clicks; put the handler on a block-level element"
            );
        }
    }
}
