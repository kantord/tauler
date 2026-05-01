# Tile-based Partial Rendering — Architecture Plan

## Goal

Re-render only the screen tiles that have actually changed, rather than re-running the full layout + rasterization pass on every frame.

## Per-frame pipeline

Each frame runs a fixed sequence of stages. Every stage is a pure transformation of its inputs; the only mutable cross-frame state is `frame_buf`, `prev_stub_bboxes`, and `node_dims`.

```
Scene description (node tree)
        │
        ▼
1. Reconcile        changed_ids, structure_changed
        │
        ▼
2. Measure          bboxes  (absolute position of every node)
        │
        ▼
3. Dirty detection  dirty: Set<(tx, ty)>
        │
        ▼
4. Band grouping    bands: Vec<RenderBand>
        │
        ▼  (loop over bands)
5. Flat collect     per-band node list  (absolutely-positioned JSON)
        │
        ▼
6. Render           per-band pixel buffer
        │
        ▼
7. Stitch           frame_buf  ← updated in-place
```

### Stage 1 — Reconcile

Diff the new scene tree against the previous frame's state using `ManagedSet`. Outputs:

- `changed_ids`: nodes whose content or style changed
- `structure_changed`: any node was added or removed

### Stage 2 — Measure

Produces absolute bounding boxes for every node. Two paths:

**Structure change** → full `measure_layout` on the real scene (text shaping included). Seeds `node_dims` with the measured bbox dimensions.

**Content-only change** →
1. **Isolated node measurement** — for each changed text/image leaf, call `measure_layout` on that single node to get its new natural (W, H). Update `node_dims[id]`.
2. **Stub layout pass** — replace every leaf with a fixed-size container stub using cached `node_dims`. Run `measure_layout` on the stub tree. Taffy does pure flex geometry with zero text shaping, giving accurate absolute positions for all nodes.

Image stubs reuse the node's own JSON (preserving `display:inline-block`) so their position in the parent layout matches the full-layout result exactly.

### Stage 3 — Dirty tile detection

Mark a tile dirty if:
- A changed node's new bbox touches it (content changed).
- Any node's bbox moved by more than 0.5 px vs the previous frame's stub bboxes (layout reflow — e.g. `justify-between` redistributing space when a sibling's width changes).

We always compare **stub-vs-stub** across frames to avoid false positives from the small systematic gap between stub and full-measure coordinate systems.

### Stage 4 — Band grouping

Rather than one render covering the union of all dirty tiles, dirty tiles are clustered into spatially contiguous bands, giving one `takumi_render` call per band.

Two candidate groupings are computed cheaply:
- **Y-bands**: sort dirty tiles by row, merge tiles whose shadow-expanded Y intervals overlap.
- **X-bands**: same, along the column axis.

The grouping with the lower estimated total render area is chosen:

```
estimated_area = Σ  (band_width + 2×SHADOW_BUF) × (band_height + 2×SHADOW_BUF)
```

This is axis-agnostic: tall narrow canvases (vertical sidebars) naturally prefer Y-bands; wide flat canvases (horizontal bars) prefer X-bands; square canvases pick whichever is cheaper. No hard-coded orientation.

Two tiles are merged into the same band only if their shadow-expanded intervals overlap (merge threshold = `2 × SHADOW_BUF / TILE_SIZE` tiles apart), ensuring that merging two distant clusters never increases the total render area.

### Stage 5 — Flat collect

For each band, `collect_flat` walks the scene tree and emits every node whose bbox overlaps the band's query region (tile area + SHADOW_BUF padding). Each node is emitted as an absolutely-positioned JSON element at `(scene_x − qx, scene_y − qy)`.

Collection containers forward only visual Tailwind classes (bg-*, shadow-*, rounded-*, etc.); layout classes are stripped since they have no effect on absolute-positioned childless containers.

### Stage 6 — Render

Each band's flat node list is wrapped in a `display:block position:relative` root container and passed to `takumi_render`. This is a full render — no shortcuts. The canvas is `(band_w + 2×SHADOW_BUF) × (band_h + 2×SHADOW_BUF)`.

### Stage 7 — Stitch

Dirty tiles are cropped from their band's pixel buffer (offset by SHADOW_BUF to skip the shadow-bleed border) and copied into `frame_buf` at their correct screen coordinates.

## Key takumi internals

- `render_node()` skips any node where `is_invisible()` is true — covering `opacity: 0`, `display: none`, `visibility: hidden`. `visibility: hidden` preserves layout but skips all rasterization (`draw_shell`, `draw_content`, `draw_inline`).
- **`overflow: hidden` does NOT skip rasterization.** Takumi still runs font shaping and glyph rasterization for clipped nodes; it only skips the final pixel writes. `overflow: hidden` alone delivers no speedup for off-screen nodes.
- CSS `transform: translateX/Y` is a render-time operation (applied via `Affine` accumulation in `render_node`, after layout). It does not affect taffy layout.
- `render()` runs layout first (`tree.compute_layout`) then drawing (`render_node`). The two phases are sequential; layout always runs on the full node tree.

## Constants requiring tuning

These are hardcoded values that affect correctness or performance. Each has a rationale for its current value and a note on how to tune it.

| Constant | Current value | Role | Tuning guidance |
|---|---|---|---|
| `TILE_SIZE` | 32 px | Render granularity. Smaller = more precise dirty regions; larger = fewer render calls and less stitch overhead. | Profile render call overhead vs area savings at 16, 32, 64. Match to typical "smallest changed region" size. |
| `SHADOW_BUF` | 32 px | Extra border around each tile to capture shadow bleed into neighbouring tiles. Must be ≥ the maximum shadow spread used in any scene. | Set to `ceil(max_shadow_spread)`. `shadow-2xl` spreads ~20 px; current 32 px has headroom. Reducing saves render area. |
| `MERGE_THRESHOLD` | `2 × SHADOW_BUF / TILE_SIZE` = 2 tiles | Maximum gap (in tile units) between two dirty tiles on the same axis before they are split into separate bands. Derived from when shadow-expanded intervals stop overlapping. | Recalculates automatically if TILE_SIZE or SHADOW_BUF change. No independent tuning needed unless the derivation changes. |
| Move detection threshold | 0.5 px | A node is considered "moved" if its stub bbox x or y shifts by more than this between frames. Prevents false-positive dirty tiles from floating-point noise. | Lower → more aggressive dirty marking (safer). Higher → risk of missing subpixel reflows. 0.5 px is at the anti-aliasing boundary; unlikely to need changing. |
| `PERFECT_THRESHOLD` | 0.05 (5% ratio) | A frame is pixel-perfect when `error_weighted / changed_area_weighted < 0.05`. Both use the cubic perceptual metric; the ratio approximates "what fraction of visually-significant changed pixels are rendered incorrectly". | Tighten toward 0.02 to catch subtler compositing errors; loosen toward 0.10 if AA noise at tile boundaries generates false flags. |
| Per-render fixed overhead `O_fixed` | **0.050 ms** (calibrated) | Amortised setup cost of one `takumi_render` call regardless of canvas size. Much lower than the initial 1.0 ms guess — splitting is cheap. | Re-run self-calibration after any takumi upgrade; expect value to change. |
| Area cost coefficient `k_area` | **2.97×10⁻⁵ ms/px** (calibrated) | Cost per pixel of canvas area in a `takumi_render` call, excluding per-node work. | Dominated by rasterisation throughput. Changes with resolution or GPU. |
| Node cost coefficient `k_nodes` | **0.106 ms/node** (calibrated) | Cost per node in the flat scene (text shaping dominates; image/container nodes cheaper but treated uniformly here). | Could split into k_text / k_image / k_container for more precision. |

**Self-calibration** (R² = 0.996 on 56 samples): the linear model fits actual render times extremely well. O_FIXED was 20× over-estimated in the initial guess, which caused the merge algorithm to over-merge; the calibrated values fix this.

---

## Categorical + spatial band grouping with cost-model merging (implemented)

### tile→node map

Maintain a persistent `tile_node_map: HashMap<(tx,ty), BTreeSet<NodeId>>` across frames.

Each frame, only update entries for changed nodes — O(changed_nodes × tiles_per_node), typically ~24 operations for a sidebar frame:

```
for id in changed_ids:
    for t in tiles_covered_by(prev_stub_bboxes[id]):   tile_node_map[t].remove(id)
    for t in tiles_covered_by(bboxes[id]):              tile_node_map[t].insert(id)
```

Static nodes never touch this map. The full O(total_nodes × total_tiles) scan never happens.

#### Stage 4 replacement: categorical + spatial + cost-model merge

```
Step 1 — Categorical split
  For each dirty tile t: look up tile_node_map[t] → its exact render-node set
  Group dirty tiles with identical node sets → categorical groups
  (tiles sharing a node set will always be spatially close in practice,
   because they share exactly the same scene objects)

Step 2 — Spatial banding within each group
  Within each categorical group, apply the current spatial banding algorithm
  Result: a collection of (render_band, node_set) candidates
  Each candidate has a small, precise node set and a tight spatial bbox

Step 3 — Greedy cost-model merge
  For each pair of candidates A and B, compute:

    merge_benefit(A, B) = O_fixed
                        − k_area  × (area(A∪B) − area(A) − area(B))
                        − k_nodes × (|node_set(A)∪node_set(B)| − |node_set(A)| − |node_set(B)|)

  Repeatedly merge the highest-benefit pair while benefit > 0.
  Stop when no merge reduces total estimated cost.

Step 4 — Render each final (band, node_set)
  collect_flat uses the node_set as a whitelist instead of a bbox query:
    for node_id in node_set:
        emit node at absolute position bboxes[node_id], offset by (qx, qy)
  No tree walk needed — the node set IS the render list.
```

#### Properties

- **No dimensional bias**: merge decisions are based on cost, not axis. Diagonal tile patterns, L-shapes, and scattered clusters all handled uniformly.
- **Minimal node sets**: each render only includes nodes that actually intersect at least one dirty tile in the band — no "gap" nodes included.
- **Minimal render area**: spatial banding within groups keeps the union bbox tight; cost-model merge prevents wasteful cross-group merges.
- **Controlled render call count**: the greedy merge ensures `O_fixed` overhead is amortised — candidates are only kept separate when the area/node savings justify the extra call.
- **Incremental cost**: tile_node_map update is O(changed_nodes × tiles_per_node) per frame, not O(total_nodes × total_tiles).

#### Implementation notes

- `BTreeSet<NodeId>` as the map key enables cheap set equality (hash or ordered comparison). For ~50 nodes per tile, each comparison is O(50) — negligible.
- Greedy merge is O(n² × |node_set|) per pass over n candidates. For n ≤ 10 candidates per frame, this is ~5 000 operations — trivial.
- Constants `O_fixed`, `k_area`, `k_nodes` are calibrated automatically at startup by OLS regression (see below).
- The existing spatial banding (current stage 4) is a degenerate special case of this algorithm with a single implicit categorical group and no node-set filtering — a valid fallback until the tile_node_map is implemented.

### OLS self-calibration

The cost model `time = O_fixed + k_area×area + k_nodes×n_nodes` is linear, so the coefficients can be fitted exactly by ordinary least squares — no iterative optimisation needed.

**Process (two-pass, runs automatically):**

1. **Pass 1 — sample collection**: run all benchmark suites with default constants. For each `takumi_render` call, record `(canvas_area px², n_nodes, elapsed_ms)`.

2. **OLS fit**: accumulate X^T X and X^T y in one pass over the ~50–100 samples, then solve the 3×3 normal equations with Gaussian elimination (partial pivoting). Area is scaled by 10⁴ internally to avoid numerical ill-conditioning. Coefficients are clamped to physically meaningful positive values.

3. **Pass 2 — real benchmark**: re-run all suites with the calibrated `CostModel`. Report results and embed calibration stats in the HTML summary.

**Observed fit quality**: R² = 0.996 on 56 samples. The linear model explains 99.6% of render time variance — the form is correct. Key finding: `O_fixed` was 20× over-estimated in the initial hardcoded guess (1.0 ms vs true 0.05 ms), causing the greedy merge to over-merge. Calibration fixed this automatically.

**When to re-calibrate**: after any takumi version upgrade, or when moving to different hardware. The benchmark self-calibrates on every run so constants are always current.

### Tile render cache

The most impactful remaining optimisation. A status bar's state space is small and highly repetitive — clock digits cycle every 60 minutes, workspace focus switches between a fixed set of known states, metric values recur. Rendered tile pixels can be cached and reused whenever the same visual state reappears.

#### Cache structure

A single LRU cache (target budget: ~20 MB) keyed by **content fingerprint**:

```
lru_cache: LruCache<u64, Vec<u8>>   // fingerprint → TILE_SIZE×TILE_SIZE×4 bytes
```

20 MB / 4 KB per tile = ~5 000 cached tile states — enough to cover the entire state space of a typical status bar several times over (790 tiles × ~2 visual states each = 1 580 entries).

The cache is global across all tiles and all scenes; any tile from any part of the canvas that hashes to the same fingerprint reuses the same entry.

#### Cache key

Hash together, for every node in `tile_node_map[(tx, ty)]`:

```
(node_id, bboxes[node_id], text_content_or_color_or_dims, tw_classes)
```

Bboxes are `f32` values and must be included because fractional node positions affect subpixel text rendering — two tiles showing the same text at different fractional offsets produce different pixels. Including the full bbox captures this. The tile's canvas dimensions are constant (TILE_SIZE, SHADOW_BUF) and do not need hashing.

This fingerprint is essentially a hash of the flat scene JSON that `collect_flat_whitelist` would produce — information we already hold at this point in the pipeline.

#### Pipeline integration — pre-removal is mandatory

Cache hits must be resolved and **removed from the dirty set before** the candidate grouping step. If a cache-hit tile is left in the dirty set, it gets absorbed into a candidate band, forcing the band's union bbox to grow and pulling in extra nodes. The render covers pixels that were already correctly stitched from cache — pure waste.

The correct pipeline:

```
dirty tiles
  → fingerprint each tile, check LRU
  → partition into {hits, misses}
  → stitch hits immediately (no render call)
  → compute_candidates(misses only)      ← operates on miss tiles only
  → greedy merge
  → render + update cache + stitch misses
```

Pre-removal also benefits the miss candidates: with hit tiles gone, bands are tighter, node sets are smaller, and the merge decisions are better.

#### Expected behaviour in steady state

| Scenario | Cache behaviour |
|---|---|
| Time tick | Clock tiles hit after first full minute cycle; ~100% hit rate at warmup |
| Focus A→B→A | First A→B: both workspace tiles miss, both states cached. First B→A: both tiles hit — zero render calls for workspace area |
| New notification text | Always miss (never-seen content) |
| Metric value stabilises (CPU=15% for N frames) | Hit from frame 2 onward |
| Cold start | All misses; cache warms over the first few frames |

In the focus-cycling case after warmup: the workspace tiles are resolved from cache, pre-removed from dirty, and `compute_candidates` sees only the datetime tile. One tiny band, one cheap render — regardless of how many workspace cards exist on screen.

#### Memory accounting

4 KB per entry × 5 000 entries = 20 MB. Entries are evicted LRU when the budget is exceeded. The effective working set for a typical sidebar session is well under 2 000 entries, so eviction should be rare in practice.

---

## Known issues / caveats

### Text stub loses layout-affecting `tw` classes

Text nodes are stubbed as plain fixed-size containers — their `tw` is dropped entirely. This means layout-affecting classes on text nodes (`ml-auto`, `flex-1`, `w-[Npx]`, `whitespace-nowrap` affecting sizing) are lost in the stub layout. This causes two bugs:

1. **Wrong bbox**: nodes with `ml-auto` or similar get placed immediately after their sibling in the stub flex row instead of at their correct position. The dirty tiles computed from this wrong bbox cover the wrong area.
2. **Stale pixels**: because the old-position dirty marking also uses the (wrong) stub bbox, the actual rendered pixels at the correct position are never cleared, leaving stale text visible (e.g. old phase label persisting after a phase transition in the keyframe animation suite).

**Fix**: preserve layout-relevant Tailwind classes when generating text stubs. Specifically, the stub container should carry any `tw` class that affects flex sizing or positioning (`ml-auto`, `flex-1`, `flex-shrink-0`, `w-*`, `h-*`, `min-w-*`, `max-w-*`).

### Correctness metric: dirty-tile-only measurement (planned)

The current metric compares the full incremental frame against the full fresh render. This unfairly penalises the renderer when:

- A dirty tile's re-render uses flat absolute positioning (from stub bboxes) that differs sub-pixel from the full flex render — producing imperceptible but measurable differences on MEM/DISK columns in Dense Metrics, for example.
- The full render shifts static text sub-pixel-wise due to layout reflow from changed siblings, while those tiles were not re-rendered in the incremental path.

The correct measurement: compare only within dirty tiles. Skipped tiles are not the renderer's responsibility for this frame — they should be separately verified as unchanged.

**Planned fix**: mask both `error_weighted` and `chg_w` to the dirty-tile pixel region only.

### CSS property inheritance (potential future issue)

Isolated node measurement has no parent context. Takumi does not implement CSS inheritance (all properties are declared directly via Tailwind classes), so this is a non-issue today.

## Test suites

13 suites currently implemented: Simple Status Bar, Shadow Cards, Blurred Overlay, Dense Metrics Grid, Realistic Sidebar, Shrink Bug, Moving Ball, Tile Crossing, Panel Focus Cycle, Diagonal Scatter, Notification Badge, Progress Fill, Keyframe Animation.

Diff pattern guide for new suites:
- **Whole-glyph displacement** → bbox tracking bug (stub position wrong)
- **Single-pixel border fringe** → SHADOW_BUF too small for the shadow in use
- **Wrong alpha / compositing** → flat-render ordering or node-set inclusion bug
- **Text at wrong position with stale old text visible** → text node `tw` lost in stub (ml-auto / flex-1 bug)

## Performance results (release build, realistic sidebar suite)

| Event | Speedup | Notes |
|---|---|---|
| Time tick only | **10–12×** | 1 band, ~3 tiles dirty |
| Claude usage update | **~7–8×** | 1 band, bottom cards |
| Focus change + time tick | **4–5×** | 2 bands (workspace area + datetime) |
| Overall suite (10 frames) | **7.4×** | mixed events |

Smaller suites: 1–2× (scenes are small; cold-frame overhead proportionally larger, incremental savings smaller).
