# Partial Rendering Plan

## Goal

Skip redundant takumi work and reduce upload bandwidth by only re-rendering and
re-uploading the screen regions that actually changed between frames.

---

## Key findings from takumi source

- `render_node()` skips any node where `is_invisible()` is true — which covers
  `opacity: 0`, `display: none`, and `visibility: hidden`. Crucially,
  `visibility: hidden` preserves layout (the node still takes up space) but
  skips all rasterization (`draw_shell`, `draw_content`, `draw_inline` are
  never called). This is the **only** mechanism that actually avoids
  rasterization work.
- **`overflow: hidden` does NOT skip rasterization.** When a node falls
  outside the clipped region, takumi still runs font shaping, glyph
  rasterization, gradient computation, etc. — it only skips the final pixel
  writes via `compute_overlay_bounds` returning `None`. For text-heavy panels
  this means `overflow: hidden` alone delivers no speedup for off-screen nodes.
- CSS `transform: translateX() / translateY()` is fully supported and is a
  render-time operation (applied via `Affine` accumulation during
  `render_node`, after layout). It does not affect taffy layout. This is what
  makes the viewport-shift trick work.
- `Viewport` width/height control the canvas size that takumi allocates and
  draws into.
- `render()` calls layout first (`tree.compute_layout`) then drawing
  (`render_node`). The two phases are sequential; layout always runs on the
  full node tree regardless of visibility flags.

---

## Design

### Phase 1 — Dirty region accumulation

Extend the reconciler's `enter()` / `update()` / `exit()` lifecycle hooks to
emit bounding-box events as a side effect.

- The renderer maintains a **persistent `BoundingBoxTree`**: after every full
  render, store each node's screen-space rect (already computed by takumi for
  click handling).
- `enter(node)` → mark node's **new** bounding box dirty.
- `update(node)` → mark both **old** (from stored tree) and **new** bounding
  boxes dirty.
- `exit(node)` → mark node's **old** bounding box dirty.
- Each dirty rect is expanded by a small constant buffer (e.g. 2 logical px ×
  DPR) to catch anti-aliasing fringe.
- Dirty rects live in a `DirtyRegions` value in shared render context; they are
  flushed (consumed and cleared) at the start of each render.

### Phase 2 — Dirty region resolution

Two approaches, ordered by implementation complexity:

#### Option A — Fixed pixel grid (simpler, recommended first)

Divide the panel into a fixed N×N grid of equal-sized cells at startup. When a
dirty bounding box (with buffer) is produced in Phase 1, map it onto the grid:
any cell whose pixel rect intersects the dirty box is marked dirty. At render
time, each dirty cell is an independent render job.

Benefits:
- No interval merging or geometric collapsing needed — the grid is the output.
- Cell size is fixed, so per-cell render cost is bounded and predictable.
- Visibility culling (Phase 3 step 2) falls out for free: to find which nodes
  to mark `visibility: hidden` for a given cell, intersect each node's bounding
  box with the cell rect — this is exactly the same intersection test used to
  mark the cell dirty in the first place, so no extra bookkeeping is needed.
- Overlapping dirty boxes from multiple nodes naturally union onto the grid
  without any explicit merge step.

Recommended cell size: wide enough that the fixed layout cost per render is
amortised (e.g. 1/8 of panel width × full height for a status bar). Too small
and the layout overhead dominates; too large and savings shrink.

#### Option B — Interval merge (general, higher complexity)

Turn the raw dirty rects into a minimal non-overlapping set via interval merge.

For a horizontal panel (1D in practice):
1. Project each rect onto the X axis → intervals `[x0, x1)`.
2. Sort by `x0`, merge overlapping/adjacent intervals in one pass.
3. Reconstruct full-height rects from merged intervals.

For a 2D panel, use a scanline sweep on the Y axis, then apply the 1D merge
per horizontal band.

Output: `Vec<Rect<u32>>` with no overlaps, covering exactly the union of all
dirty input rects.

Implement Option B only if Option A's fixed cell granularity wastes too much
work in practice.

### Phase 3 — Partial takumi render

For each dirty rect `(dx, dy, dw, dh)`:

1. **Viewport-shift via CSS overflow trick:**

   Wrap the existing node tree in two extra layers injected at render time
   (not part of the user's JSX):

   ```
   [outer: width=dw, height=dh, overflow=hidden]
     [inner: position=absolute, left=-(dx), top=-(dy), width=panel_w, height=panel_h]
       [original node tree]
   ```

   The outer container clips to the dirty region size; the inner container
   shifts all content left/up so the dirty region appears at origin. Takumi's
   layout sees `(panel_w, panel_h)` for the inner container, so child positions
   are correct. The canvas allocated by takumi is `dw × dh`.

2. **Visibility culling — the load-bearing step:**

   Walk the node tree before rendering. Mark any node whose bounding box (from
   the stored `BoundingBoxTree`) does not intersect `(dx, dy, dw, dh)` as
   `visibility: hidden`. Takumi skips all rasterization for those nodes while
   keeping their layout positions intact.

   This is what delivers the actual speedup. The viewport-shift trick produces
   the correctly-sized output; visibility culling is what makes the render fast.
   Without it, every node rasterizes regardless of the smaller canvas, because
   `overflow: hidden` only skips pixel writes, not rasterization.

   The bounding boxes needed here come directly from the click-handler bounding
   box tree — no additional tracking infrastructure is required.

3. **Blit into master framebuffer:**

   The dirty-rect render returns a small `dw × dh` RGBA image. Copy it into
   the master BGRX framebuffer at offset `(dx, dy)`.

4. **Partial upload:**

   - **X11:** call `put_image_chunked` only for the row range `[dy, dy+dh)`,
     passing `y = dy` as the image origin.
   - **Wayland:** update only the relevant region of the SHM buffer, then call
     `damage_buffer(dx, dy, dw, dh)` instead of full-surface damage.

### Phase 4 — Full-render fallback

Keep the existing full-render path. Use it when:
- No previous `BoundingBoxTree` exists (first frame).
- Dirty region area > ~60% of panel area (partial path has more overhead than
  it saves at that point).
- Viewport or DPR changed (layout is invalidated globally).

---

## Performance model

| Cost component | Full render | Partial render (D% dirty) |
|---|---|---|
| taffy layout | 1× (full tree) | 1× (full tree, unchanged) |
| Node traversal | 1× | 1× (but most nodes hit `is_invisible()` early) |
| Rasterization | 1× | ~D% (only visible nodes rasterize) |
| Canvas allocation | `w×h×4` bytes | `dw×dh×4` bytes |
| Upload (X11 PutImage) | full buffer | dirty rect only |
| Wayland damage | full surface | dirty rect only |

Rasterization (font shaping, glyph drawing, gradient fill) dominates render
time for text-heavy status bars. Layout is cheap relative to glyph work.
Therefore rendering a 10% dirty region with 90% of nodes marked invisible costs
roughly 10–15% of a full render. The fixed costs (layout + traversal overhead)
form a floor that is negligible at panel sizes.

The partial path adds overhead only when the dirty region is large. The
full-render fallback threshold (~60% dirty area) ensures the partial path is
never slower than a full render by more than a small constant factor.

---

## Data structures

```rust
struct Rect { x: u32, y: u32, w: u32, h: u32 }

struct DirtyRegions {
    rects: Vec<Rect>,
}

impl DirtyRegions {
    fn mark(&mut self, r: Rect, buffer_px: u32);
    /// Returns collapsed non-overlapping set and clears self.
    fn flush_collapsed(&mut self) -> Vec<Rect>;
}

struct BoundingBoxTree {
    // keyed by stable node identity (structural path or explicit key)
    nodes: HashMap<NodeKey, Rect>,
}
```

---

## Open questions

1. **Stable node identity:** The reconciler needs a stable key per node across
   frames to match old bounding boxes to updated nodes. JSX `key` props are the
   obvious source; structural path (parent-index chain) is the fallback.

2. **Nodes that span a dirty boundary:** Any node whose bounding box
   *intersects* the dirty rect must stay visible — only fully-outside nodes are
   hidden. This is conservative but correct; it avoids holes at region edges.

3. **Reflow from sibling changes:** If node A changes size and causes adjacent
   nodes to shift (flexbox reflow), those siblings' previous-frame bounding
   boxes are stale. We'd incorrectly mark them invisible. For a status bar this
   is rare (items are typically fixed-width or in stable-total-width flex
   containers). When it does happen the artifact lasts one frame — the next
   full-dirty render corrects it. Conservatively, expanding the dirty region to
   cover any node whose flex sibling changed would catch most reflow cases.

3. **Background of unchanged region in master framebuffer:** The master
   framebuffer is only updated at dirty rects. Unchanged regions must retain
   their previous pixel values, so the framebuffer must persist across frames
   (no allocation per frame).

4. **Wayland SHM pool management:** Currently the SHM buffer is fully
   overwritten each frame. For partial upload the pool buffer must be kept alive
   and mutated in-place for dirty regions only, then committed. This requires
   holding a mutable reference to the mapped SHM memory across frames.

---

## Testing approach

### Step 1 — Validate the upload path

Without changing takumi at all:
- Render the full frame as normal.
- Artificially split the output RGBA buffer into two halves at `y = height/2`.
- Upload each half as a separate `PutImage` (X11) or SHM-partial + damage_buffer (Wayland).
- Verify the composited result is pixel-perfect.

This de-risks the upload side with zero risk to correctness.

### Step 2 — Validate visibility culling + viewport-shift together

Hardcode a dirty rect covering the right 30% of the panel. Mark all nodes
whose previous-frame bounding boxes don't intersect it as `visibility: hidden`.
Inject the wrapper-node trick to produce a 30%-wide canvas. Blit into a
pre-initialized framebuffer containing the previous full render, upload. Verify
only the right 30% changed and the result is pixel-perfect.

Note: step 1 (upload path) can be tested without touching takumi at all by
splitting the existing full RGBA buffer in half and uploading each piece
separately. Do not attempt two separate takumi calls without the visibility
culling + wrapper-node trick — the layout re-flows with a half-size viewport
and produces wrong output.

### Step 3 — Wire up dirty tracking

Enable `enter/update/exit` side effects; remove hardcoded dirty rect. Run a
panel where one value changes on a timer. Instrument to log how many bytes are
uploaded per frame vs the full-frame baseline.
