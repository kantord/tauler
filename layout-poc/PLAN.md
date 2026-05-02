# Tile-based Partial Rendering — Architecture Plan

## Goal

Re-render only the screen tiles that have actually changed, rather than
re-running the full layout + rasterisation pass on every frame.

---

## Per-frame pipeline (current implementation)

**Persistent cross-frame state:**
`frame_buf`, `prev_stub_bboxes`, `tile_node_map`, `node_dims`, `tile_cache`

- **Reconcile** — diff new scene tree against previous state via `ManagedSet`
  → `changed_ids`. If empty and `frame_buf` populated: return immediately (zero cost).

- **Build node map** — single O(N) tree walk into `HashMap<id, &FakeNode>`,
  shared by the measure step and fingerprinting.

- **Measure changed leaves** — for each id in `changed_ids`: shape text node
  via `measure_natural` or read image dims directly; update `node_dims` cache;
  track whether any dimension or collection `tw` actually changed
  (`dims_changed`, `collection_changed`).

- **Stub layout** — *skipped* if `!dims_changed && !collection_changed`
  (positions provably identical to last frame; reuse `prev_stub_bboxes`).
  Otherwise: replace every leaf with a fixed-size container using cached
  `node_dims`, run taffy flex geometry with zero text shaping → `bboxes`.

- **Dirty detection + tile_node_map update**
  - *First frame:* full `build_tile_node_map`, all tiles dirty, `frame_buf` zeroed.
  - *Changed nodes* O(changed × tiles_per_node): remove old tile entries / mark
    old tiles dirty, insert new entries / mark new tiles dirty.
  - *Moved nodes* O(total_nodes): for any node not in `changed_ids` whose bbox
    shifted > 0.5 px (layout reflow), same remove/insert/mark-dirty — folded
    into the same loop at no extra asymptotic cost.

- **Cache lookup** — fingerprint every dirty tile (FNV-1a over `(tx,ty)` + each
  node's id, bbox, content). Check LRU; hits are stitched into `frame_buf`
  immediately and removed from dirty so they don't inflate candidate band areas.

- **Candidate grouping**
  - *Categorical split:* group dirty tiles by identical `tile_node_map` node set.
  - *Spatial banding:* within each group merge adjacent tiles into rectangular
    bands; pick Y-bands vs X-bands by estimated render area.
  - *Greedy cost-model merge:* merge pairs while
    `O_fixed − k_area×ΔArea − k_nodes×ΔNodes > 0`.

- **Render** — for each candidate: `collect_flat_whitelist` emits only its node
  set as absolutely-positioned JSON onto a `(band + 2×SHADOW_BUF)` canvas;
  one `takumi_render` call per candidate.

- **Stitch + cache store** — crop each `TILE_SIZE×TILE_SIZE` block from the
  render (skipping shadow border), copy into `frame_buf`, store in LRU cache.

---

## Constants

| Constant | Value | Role |
|---|---|---|
| `TILE_SIZE` | 32 px | Render granularity |
| `SHADOW_BUF` | 32 px | Shadow bleed capture border (≥ max shadow spread) |
| `MERGE_THRESHOLD` | `2 × SHADOW_BUF / TILE_SIZE` = 2 tiles | Max gap before bands split; derived, not tuned independently |
| `TILE_CACHE_MB` | 10 MB → 2 560 entries at TILE_SIZE=32 | LRU tile cache budget; entry count auto-derived |
| Move threshold | 0.5 px | Min bbox shift to declare a node moved |
| `PERFECT_THRESHOLD` | 0.05 | Max error/changed-area ratio for ✓ |

---

## Cost model

The greedy merge uses a linear cost model:

```
estimated_time(candidate) = O_fixed + k_area × canvas_area + k_nodes × n_nodes
merge if: savings_from_one_fewer_call > extra_area_cost + extra_node_cost
```

**PoC calibration** (OLS on ~56 render samples, R² = 0.996):

| Coefficient | Calibrated value | Initial guess | Error |
|---|---|---|---|
| `O_fixed` | 0.050 ms | 1.0 ms | 20× over |
| `k_area` | 2.97×10⁻⁵ ms/px | 4×10⁻⁵ ms/px | ~25% over |
| `k_nodes` | 0.106 ms/node | 0.3 ms/node | 3× over |

The bad initial `O_fixed` caused over-merging; calibration fixed it.

### Cost model collapse plan

The OLS runtime calibration is a **PoC diagnostic tool**, not a production
feature. Before production, the model should be collapsed to hardcoded constants.

**Steps before collapsing:**

1. **Model selection** — compare 3-param (`O_fixed + k_area + k_nodes`) vs
   2-param (`O_fixed + k_area` only). If R² is similar, drop `k_nodes` —
   simpler merge formula, no node-counting needed.

2. **Sensitivity analysis** — vary `O_fixed` ±50%, measure how often merge
   decisions actually change. If the answer is "rarely", the constant is robust
   and any value in the right order of magnitude works.

3. **Node type split** — `k_nodes` currently treats text, image, and container
   nodes identically. Text shaping dominates; containers are nearly free.
   Splitting into `k_text` / `k_nontext` would improve accuracy at the cost
   of tracking node type counts per candidate. Worth it only if the uniform
   model makes materially wrong merge decisions.

4. **Stability check** — run calibration N times, measure variance of each
   coefficient. High variance → the constant is noise-sensitive and a simple
   default is safer than a calibrated value.

5. **Collapse** — hardcode the final values as compile-time constants in
   `PartialRenderContext::default()`. Make them config-file tuneable
   (`o_fixed_ms`, `k_area`, `k_nodes`) with the calibrated values as defaults.
   Remove the OLS machinery and the two-pass benchmark loop from production code.

---

## Production API design

Two-layer initialisation separating process-level (shared) from panel-level
(per-scene) state.

```rust
// ── Process-level — one per process ──────────────────────────────────────────
struct PartialRenderContext {
    global:     GlobalContext,                // fonts; expensive to load, shared
    tile_cache: LruCache<u64, Vec<u8>>,       // shared across scenes — cross-panel hits
    cost_model: CostModel,                    // hardcoded constants (see above)
}

impl PartialRenderContext {
    fn new() -> Self { ... }                  // load fonts, init cache
    fn create_scene(&self) -> PartialRenderScene { PartialRenderScene::new() }
}

// ── Panel-level — one per bar / panel ────────────────────────────────────────
struct PartialRenderScene {
    frame_buf:        Vec<u8>,
    prev_stub_bboxes: HashMap<String, Rect>,
    tile_node_map:    HashMap<(u32,u32), BTreeSet<String>>,
    node_dims:        HashMap<String, (f32,f32)>,
    incr_set:         ManagedSet<FakeNode>,
    changed_ids:      Vec<String>,
}

impl PartialRenderScene {
    fn render_frame(&mut self, ctx: &mut PartialRenderContext, root: FakeNode) -> &[u8] {
        self.reconcile(root);
        if self.is_noop() { return &self.frame_buf; }
        let node_map = build_node_map(&self.root);
        let bboxes   = self.measure(ctx, &node_map);
        let dirty    = self.update_dirty_and_tile_map(&bboxes);
        let misses   = ctx.cache_lookup(&mut self.frame_buf, dirty, &bboxes, &node_map);
        let cands    = group_candidates(misses, &self.tile_node_map, &ctx.cost_model);
        self.render_and_stitch(ctx, &bboxes, &cands, &node_map);
        &self.frame_buf
    }
}
```

`render_frame` takes `&mut ctx` so the tile cache (in ctx) is mutated during
the call without needing `Arc<Mutex<>>` — costae is single-threaded.

`create_scene` needs no parameters; canvas dimensions are determined on the
first call to `render_frame` from the scene's first full render.

---

## Key takumi internals

- `render_node()` skips any node where `is_invisible()` is true — covering
  `opacity: 0`, `display: none`, `visibility: hidden`. `visibility: hidden`
  preserves layout but skips all rasterisation.
- **`overflow: hidden` does NOT skip rasterisation.** Takumi still runs font
  shaping and glyph rasterisation for clipped nodes; it only skips the final
  pixel writes. `overflow: hidden` alone delivers no speedup for off-screen nodes.
- CSS `transform: translateX/Y` is a render-time operation; it does not affect
  taffy layout.
- `render()` runs layout first (`tree.compute_layout`) then drawing
  (`render_node`). The two phases are sequential; layout always runs on the
  full node tree.

---

## Test suites (15 implemented)

Simple Status Bar, Shadow Cards, Blurred Overlay, Dense Metrics Grid,
Realistic Sidebar, Shrink Bug, Moving Ball, Tile Crossing, Panel Focus Cycle,
Diagonal Scatter, Notification Badge, Progress Fill, Keyframe Animation,
Notification Panel, Scroll List.

Diff pattern guide:
- **Whole-glyph displacement** → bbox tracking bug (stub position wrong)
- **Single-pixel border fringe** → SHADOW_BUF too small for the shadow in use
- **Wrong alpha / compositing** → flat-render ordering or node-set inclusion bug
- **Text at wrong position with stale old text visible** → layout-affecting `tw`
  dropped from stub (ml-auto / flex-1 bug)

---

## Visual regression tests (14 implemented, all passing)

### Principle

Each test stores two independent golden PNG snapshots — one for the full
renderer, one for the incremental renderer — and asserts both are byte-identical
on every subsequent run. Because the renderer is deterministic, comparison is
exact byte equality: no tolerance, no perceptual metric. The `insta` crate
manages the hash snapshot lifecycle.

```
layout-poc/test-snapshots/
  <name>__full_f<N>.png    ← always written for visual inspection
  <name>__full_f<N>.snap   ← insta hash; fails CI if pixel data changed
  <name>__incr_f<N>.png
  <name>__incr_f<N>.snap
```

### Snapshot update workflow

```sh
cargo insta test --accept -p layout-poc   # review + accept changed snapshots
cargo test -p layout-poc                  # CI: fail if any hash changed
```

### Test list (all implemented)

**Bug regression guards**
- `reg_overflow_clip_rounding` — overflow-hidden clip fix (progress fill, frame 1)
- `reg_ml_auto_positioning` — ml-auto stub positioning fix (keyframe frame 5)
- `reg_node_shrink_stale_pixels` — wide→narrow text erasure (shrink bug frame 1)
- `reg_image_resize_dirty_region` — image resize dirty region (progress fill frames 3, 8)
- `reg_moved_node_clears_old_position` — tile boundary clearing (tile crossing frame 1)
- `reg_structure_change_no_ghost` — removed nodes leave no ghost pixels (inline 3-frame suite)

**Compositing correctness**
- `test_shadow_tile_boundary` — shadow-2xl at tile boundary (shadow cards frame 1)
- `test_rounded_clip_all_widths` — overflow clip at 14%, 57%, 100%, reset, zero
- `test_ml_auto_all_phases` — ml-auto across IDLE/RISING/PEAK/FALLING transitions

**Golden representatives**
- `golden_cold_frame_exact_match` — cold frame must be pixel-identical in both paths
- `golden_clock_tick` — clock-only update, frames 1–3
- `golden_workspace_focus_change` — focus switch, panel focus frames 4 and 8
- `golden_notification_rotation` — active notification cycles, frames 0, 2, 4
- `golden_scroll_frame` — scroll at depth 1, 5, 10

---

## Performance results (debug build, realistic sidebar suite)

| Event | Speedup |
|---|---|
| Time tick (same-width text, stub skipped) | **~20×** |
| Focus change + time tick | **~7×** |
| Cache-warmed focus A→B→A | **>>20×** (zero renders on return) |
| Overall 10-frame suite | **~16×** |

Suites with all-dirty frames (Scroll List): ~1.5× — stub layout savings only,
no tile skipping possible when every tile changes every frame.
