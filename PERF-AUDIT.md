# Performance audit — 2026-09-19

Scope: every optimization that improves speed, responsiveness, memory or CPU **without
changing the architecture** fixed by the ADRs (whole-tree re-eval per Tick, one Render
worker, canonical-JSON frame cache, single Notifier loop, superseding slots, software
rasterization, subprocess Streams). Nothing here proposes a reconciler, a GPU path, a
second drawing thread, or a different data model.

Method: six parallel read-only code audits (main loop, JS eval, render, windowing, helper
binaries, cross-cutting) plus measurements on the live desktop process and the criterion
bench. Every finding below was verified against the code at `7d338ce`; the ones marked
**measured** were also observed live or in a profile.

---

## 0. Baseline measurements

### Live process (this desktop, 3 outputs, ~12 subprocesses, 71 min uptime)

| metric | value |
|---|---|
| RSS | 237 MB (220 MB anonymous) |
| threads | 67 (62 unnamed) |
| average CPU over uptime | 2.1 % of one core |
| idle CPU (10 s sample, nothing changing) | 1.1 % |
| **idle wakeups, whole process** | **226 / s** |

Idle wakeups by thread (10 s sample):

| rate | thread | cause |
|---|---|---|
| 126 / s | presenter | `recv_timeout(8 ms)` poll — `src/presenter/mod.rs:20` |
| 20 / s | reconciler | 50 ms sleep slices — `src/units.rs:428-435` |
| 20 / s | reconciler watchdog | `sleep(50 ms)` loop — `src/units.rs:386-401` |
| 20 / s | `tauler:outputs` builtin | `poll_for_event` + `sleep(50 ms)` — `src/x11/outputs.rs:391` |
| 14 / s | main loop | real stream lines (user scripts) + 400 ms supervision |
| 8 / s | one pool stdout reader | a chatty user script |

Largest anonymous mappings: 63 MB and 40 MB (thread malloc arenas: worker frames, second
QuickJS runtime), 40 MB main heap, 39 MB and 32 MB single mmaps. The 32 MB one is exactly
the 3840×2160×4 wallpaper frame for DP-4.

Side observation, out of repo scope: the layout's own helper scripts cost more CPU than
tauler does (`enwiro-claude-activity-stream.sh` 104 s vs tauler 92 s over the same
uptime), and two `tauler-github-data` / `tauler-devcontainers` processes from a previous
tauler instance are still alive under PID 1 (the known restart leak).

### Bench (`cargo bench --bench pipeline`, synthetic fixture, ten states, four panels)

| stage | time |
|---|---|
| `eval_only` | 1.62 ms |
| `eval_and_parse` | 1.74 ms |
| `draw_only` (all cache misses) | 7.95 ms |
| `eval_to_pixels` | 10.14 ms |

### Trace baseline (2026-09-19, ADR 0042, volume slider drag in the sidebar)

`RUST_LOG=info,tauler::trace=debug`, four mid-drag frames, then the settle:

| Hop | mid-drag | settle (cache hit) |
|---|---|---|
| pass (pointer → intent sent) | 0.1–0.5 ms | 0.1 ms |
| answered (`tauler-volume` script round trip) | 35–50 ms | 10 ms |
| eval | 3.5–5.2 ms | 3.5 ms |
| queued (worker slot; spike = previous draw still running) | 0.5 ms, once 24.9 | 0.5 ms |
| render (sidebar raster incl. BGRX) | 55–57 ms | 0.3 ms |
| presented (copy + 5 MB PutImage) | 0.7 ms | 0.7 ms |
| **total** | **96–119 ms** | **15 ms** |

tauler's own non-raster overhead is ~5–6 ms per frame, already inside ADR 0042's 10 ms
budget. The two big Hops are the raster (accepted, ADR 0011) and the user's Module
script (outside the repo). The Trace does not see the presenter's 0–8 ms poll: the
origin is stamped at receipt. Xorg event timestamps are `CLOCK_MONOTONIC` ms, so a
`socket=` Hop (event.time → receipt) is the one addition that makes step 1 measurable.

Profile of `eval_to_pixels` (perf, symbolized): `render_frame_keyed`'s own body — the
RGBA→BGRX loop — is **8 % of the whole pipeline** by itself. Font matching redone per
render inside takumi (`FontRef::table_data`, `match_font`, `load_font`, `CachedFamily`
drops, `select_font`) is another ~10 %. `JSON.stringify` is ~15 % of `eval_only`.

---

## 1. Idle CPU: stop the polling threads

All four are **measured** live and account for ~185 of the 226 wakeups/s. Each fix is
local to one file. Expected result: an idle bar wakes ≤ 5 times a second.

These are four instances of one rule, recorded in ADR 0042: a thread blocks on all of
its sources at once, a stop is a source, and sending is waking. Verified design per
backend (2026-09-19): the presenters keep one shape on X11, Wayland and macOS — block on
commands and display events together, `Shutdown` ends the loop — with each platform's
own event loop doing the blocking (calloop 0.14.4, already compiled via
smithay-client-toolkit, confined to `src/presenter/`; winit's `EventLoopProxy` on macOS).
The one shared piece is a `CommandSender` (std channel + optional wake hook, the
`PresenterEvents` pattern in reverse); it must **not** be calloop's sender, which would
drag calloop onto macOS. Forced changes: Wayland's `EventQueue` moves out of
`WaylandDisplayServer` (the calloop source owns it, `WaylandState` becomes `pub`); X11
drains `poll_for_event` after *every* dispatch, since a `.reply()` inside RandR or
`PaintWallpaper` empties the socket and buffers events the fd will never signal; macOS
needs an explicit `Shutdown` message (winit never reports a dropped sender). The
reconciler and watchdog use a stop channel and `recv_timeout`; `tauler:outputs` needs a
wakeable stop token (`rustix::event::poll` over the X fd and a pipe end), which changes
the builtin source's `AtomicBool` signature for its one caller. No `Driver` type: the
two source families (channels+deadlines vs fds) share no code.

**1.1 Presenter thread polls at 125 Hz and adds a 0–8 ms Hop to every pointer event.**
`src/presenter/mod.rs:15-34` blocks at most 8 ms on the command channel, then calls the
non-blocking `poll_for_event` (`src/presenter/x11.rs:108`; Wayland identical in
`src/presenter/wayland.rs:26-48`). Input is therefore noticed by polling — the same cost
ADR 0024 removed from the main loop, one thread earlier.
Fix (X11): a reader thread blocking in `conn.wait_for_event()` that forwards events into
the same channel as commands (`enum PresenterInput { Command, X }`), presenter blocks on
`recv()` with no timeout. x11rb supports a concurrent waiter and sender on one
`RustConnection` (it drops its lock while polling the fd). Fix (Wayland): `poll(2)` on the
connection fd from `prepare_read()` plus a pipe/eventfd the command sender writes to.
Impact: −125 wakeups/s, −0–8 ms on every click/drag event. Confidence high.

**1.2 Reconciler thread and its watchdog each wake 20×/s forever.**
`sleep_until_due` sleeps in `STOP_POLL` (50 ms) slices so a stop is noticed quickly;
`watch_sweeps` sleeps 50 ms unconditionally even when no Sweep is running. Fix: a
`Condvar`/`park_timeout` woken from `Reconciler::drop`, sleep the full interval; watchdog
parks until a Sweep starts. Also `src/units.rs:345` clones the entire stream map every
250 ms only to compare it — compare under the read guard, clone only when different.
Impact: −40 wakeups/s and a HashMap clone every 250 ms. Confidence high.

**1.3 `tauler:outputs` thread polls X11 every 50 ms.**
`src/x11/outputs.rs:374-398`. Fix: `poll(2)` on the connection fd with the 400 ms
supervision timeout for the stop check, or plain `wait_for_event`. Impact: −20 wakeups/s
when a layout uses `tauler:outputs` (this one does). Confidence high.

**1.4 The coalescing floor sleep runs even for passes that did nothing.**
`src/data/data_loop/mod.rs:354-356` sleeps up to 2 ms after every pass, including the
2.5/s supervision wakeups. Fix: sleep the floor only when the pass consumed a ping or
drained ≥1 item. Impact: tiny; listed because it is one `if`.

---

## 2. Per-Pass and per-Tick work on the main thread

The tick thread is the interaction path (ADR 0024). Everything here runs on every stream
line or pointer event; several also run on the idle 2.5/s supervision pass.

**2.1 `update_a11y` deep-clones every Panel's content twice per Pass, idle included, with
no AT attached.** `src/app.rs:1409-1413` calls it unconditionally at the end of every
tick; `SurfaceSets::panel_specs()` (`src/surface/mod.rs:347-349`) clones each
`SurfaceSpec` including `content`, `PanelInfo { content: s.content.clone() }`
(`src/app.rs:1215`) clones again, then `a11y::reconcile` deep-compares
(`src/a11y/mod.rs:489-515`). accesskit's `update_if_active` gate comes *after* all of
this. Fix: `panel_specs()` → iterator of `&SurfaceSpec`, `PanelInfo.content: &Value`,
and only call `update_a11y` on passes where surfaces were reconciled. Impact: two
whole-tree clones + one deep compare per panel per pass (0.1–1 ms per pass for a dense
rice). The single largest idle-pass cost. Confidence high. Found independently by four
audits.
Caveat: accesskit activates when `org.a11y.Status.IsEnabled` is true, not when an AT
attaches. On a GNOME/KDE session every Repaint then runs a full takumi layout per panel
on the tick thread (`a11y::build_tree` → `painted_boxes`). CONTEXT.md's "nothing is built
when none is attached" only holds on at-spi-less sessions.

**2.2 Every Tick triggers one extra Pass and two redundant pool reconciles.**
`src/app.rs:272` calls `handle.set_desired(combined)` on every tick, which pings the
notifier; the next pass runs `set_desired` (reconcile both pools) and then the
unconditional reconcile at `src/data/data_loop/mod.rs:323-329` (both pools again + two
`event_txs_snapshot` rebuilds). Each reconcile clones `desired_processes` including every
inline script body and deep-compares specs (`optative-process-pool` `supervisor.rs:64-94`),
then `try_wait`s every child. Fix: in `App` keep the last desired set and call
`set_desired` only when it differs; in the loop, skip the unconditional reconcile when
`set_desired` ran this pass and rate-limit supervision to once per `SUPERVISION_INTERVAL`
(what ADR 0024 says that timer is for). Impact: one wasted pass + two reconciles + one
a11y sweep per data change (1 Hz clock → 1/s; drag at 25 intents/s → 25/s). Confidence
high.

**2.3 Three to four deep clones of the layout tree per Tick.**
`src/app.rs:811` `out.layout.clone()` (every caller owns `out`), `:824-825` clone
`stream_calls`/`module_calls` (already cloned once at `src/jsx.rs:949-950` instead of
`mem::take`), `src/layout/mod.rs:205-211` deep-clones each panel's subtree out of the
root, `src/app.rs:274` clones `output_map` per tick, `src/units.rs:52-70` `strip_items`
does `remove`+`insert("children".to_string())` per object node. Fix: pass `EvalOutput` by
value, resolve theme in place, by-value `parse_root_node` that `take()`s children,
`get_mut` + `retain` in `strip_items`, pass `&mut self.output_map`. Impact: 0.1–0.5 ms
per tick against the 1.6 ms eval. Confidence high.

**2.4 `SurfaceSpec.content` should be `Arc<Value>`.**
Content is cloned into every `RenderRequest` (`src/surface/mod.rs:79`), into
`Create`/`Resize`/`Move`/`PaintWallpaper` commands (`:181-211`; `Move` uses geometry
only), per press for hit-testing (`src/app.rs:1137`, `:1247`), and by 2.1. One type
change makes all hand-offs refcount bumps and dissolves the borrow-checker clones.
Equality stays deep (needed by the surface diff). Confidence high.

**2.5 `preload_layout_images` takes the global render-context write lock every Tick and
may deep-clone the whole `RenderContext`.** `src/app.rs:821` → `src/render/mod.rs:463`
→ `with_global_ctx_mut` (`:113-119`) runs `Arc::make_mut` before the closure checks
whether anything needs loading. The worker holds a snapshot Arc for the whole 40–90 ms
draw, so any tick landing in that window (common during drags and fast streams) clones
parley's `FontContext` (fontique `Collection` + `SourceCache`) and the images map. Fix:
collect `src`s under a read snapshot, take the write lock only when at least one is
missing (steady state: never). Impact: tens to hundreds of µs per overlapping tick plus a
writer contending with the worker. Confidence high.

**2.6 `<I3Layout>` round-trips every Panel's entire subtree through serde every Tick.**
`tauler-core/src/globals.rs:66-79` passes the `<Panel>` declarations *with their
children* to `__ui_i3_layout`; `PanelDecl.children: Vec<serde_json::Value>`
(`tauler-core/src/ui/components/i3_layout.rs:44`) deserializes the whole bar content
JS→Rust, `:113` puts it back, `ui/mod.rs:128` converts Rust→JS. Rust never looks at the
children. Fix: send `decls.map(({children, ...d}) => d)`, reattach `children` in JS
afterwards, drop the field from `PanelDecl`. Impact: removes a JS↔Rust conversion of
essentially the entire bar per tick — the same class of per-node boundary crossing that
`globals.rs:14-17` measured at 60 % of an evaluation when the flatten step was Rust.
Estimated 1–2 ms of the ~4.7 ms real-desktop eval the fixture header cites. Confidence
high on mechanism, medium on magnitude. Probably the largest single per-tick win.

**2.7 `h` crosses into Rust per element and allocates three objects per node.**
`src/jsx.rs:737` wraps `__esto_h` (Rust, `optative-script` `runtime.rs:80-141`): FFI
call, two `Object::new`, `Object.assign` fetched through `globals().get("Object")` per
call, then `__tauler_flatten_node` (`globals.rs:21-28`) allocates a third object. The web
runtime already has a pure-JS `estoH` with identical semantics. Fix: define `h` fully in
`JSX_GLOBALS_JS` producing the flat node in one step; `flatten_passthrough` at
`src/jsx.rs:946` becomes dead. Impact: 0.5–1.5 ms per tick on 600 nodes (estimate).
Confidence medium-high. Keep `__tauler_register_handlers` behaviour for string tags.

**2.8 Theme token resolution re-splits every class string and allocates per token every
Tick.** `tauler-core/src/theme/resolver.rs:33-54,123`: `split_whitespace`, `to_string()`
per token even when nothing matched, `collect` + `join`, unconditional replace. Theme and
mode only change on reload. Fix: memoize `raw class → resolved` in a `HashMap` owned by
`App`, cleared where `self.theme`/`self.theme_mode` are reassigned
(`src/app.rs:1030-1032`); or at least a no-alloc fast path when no token can resolve.
Impact: ~1–2k small allocations per tick → hash lookups; 100–300 µs. Confidence medium-high.

**2.9 `<Workspaces>` runs a full takumi layout pass every Tick.**
`globals.rs:91` → `src/jsx.rs:130-138` → `src/workspaces.rs:126-138` `measure_content_rect`
→ `hit_test.rs` `build_tree` + `compute_layout` + stacking contexts, then `wrapper.clone()`.
The wrapper is static per layout in practice. Fix: one-slot memo keyed by (hash of the
wrapper JSON string already produced in `json_of`, width, height) → `Rect`, invalidated on
`FontsChanged`/reload. Impact: 0.3–3 ms per tick for layouts using `<Workspaces>` (this
one does). Confidence high on mechanism.

**2.10 Stream keys carry the full inline script text and are hashed and cloned per value
and per Tick.** Key type `(String, Option<String>)` where the option is the whole script
body; `ProcessIdentity.key = "bin:script"` (`src/app.rs:171`); per line:
`identity_to_stream_key` allocates `format!` + 2 clones (`src/data/data_loop/mod.rs:137-146`),
`tick` hashes the key twice (`src/app.rs:1281-1287`); per tick: `useStringStream` pushes
`(bin.clone(), script.clone())` and SipHashes the script (`src/jsx.rs:751-759`), `eval`
`clone_from`s the whole map (`:915-918`) although `App` already owns an identical
`SharedStreamValues` Arc that the reconciler evaluator receives. Fix: hand the render
evaluator the App's Arc; intern the script as `Arc<str>` or key by `(bin, hash)` with a
side table. ADR 0009's identity semantics are preserved by a hash. Confidence medium.

**2.11 Rust-backed UI components round-trip their `children` subtree twice per nesting
level.** `tauler-core/src/ui/mod.rs:123-129`: `from_value(props)` deserializes the whole
subtree, `to_value(render())` rebuilds it. `card`/`card_header`/`badge`/`table*` only wrap
children under one `div`; `<Card><CardHeader><CardTitle>` converts the innermost subtree six
times, and drops unknown props (`id`, `data-*`) on the way. Fix: opt-in passthrough — remove
`children` from the JS props before `from_value`, reattach the original JS value on the
output root. Not for components that inspect children (`data_table`, `scroll_area`).
Impact: O(subtree) per component instance per tick; zero on the synthetic bench, real on
Card-heavy bars. Confidence high on mechanism.

**2.12 Smaller per-tick items (do while touching the same lines).**
- `globals` serialized JS↔Rust every tick even when empty (`src/jsx.rs:924-937`); create
  the JS object once, `JSON.stringify` + string-compare, parse only on change.
- `reset_handlers` compiles `"__tauler_handlers.length = 0;"` in a separate
  `context.with` per tick (`src/jsx.rs:851-855`); `eval_units` compiles a script per
  Sweep (`:982-986`). Use property sets / saved `Persistent<Function>`.
- `event_txs_snapshot` rebuilt under a mutex every pass (`src/data/data_loop/mod.rs:290-297`);
  rate-limit with 2.2.
- `on_item` re-sends every item over a second same-thread channel (`src/main.rs:404-406`);
  a single handler trait with `on_item`/`on_tick` removes the hop.
- `outbox.answered` runs per item even when the outbox is idle (`src/app.rs:1281-1283`).
- `compress_motion` allocates two `Vec`s per pass (`src/app.rs:1333`, `src/pointer.rs:300`);
  `retain` in place.

---

## 3. Per-Repaint work (Render worker and presenter)

**3.1 RGBA→BGRX conversion allocates a second framebuffer and pushes 4 bytes at a
time.** `src/render/mod.rs:157-162`. **Measured: 8 % of `eval_to_pixels`** — ~0.8 ms per
ten small panels, ~1–2 ms for a full-height panel, ~10–20 ms for a 4K wallpaper, plus a
fresh 3 MB / 32 MB allocation each time. Fix: swizzle in place on the `Vec` takumi
returned (`chunks_exact_mut(4)`: swap 0↔2, set 3 to 0) and `Arc::new(rgba)`. Same
row-wise `chunks_exact` treatment for `crop_bgrx_to_rgba` (`src/backdrop/mod.rs:76-102`,
~0.5 ms per crop, per-pixel bounds checks). Confidence high.

**3.2 X11 `update_image` copies every frame a third time and keeps it.**
`src/x11/panel.rs:283-284` `panel.bgrx = Arc::new(bgrx.to_vec())` because the trait
(`src/display_manager.rs:22`) takes `&[u8]`, so the `Arc` that `SurfaceFrame` exists to
share is lost at the seam. `create_window` gets it right (`panel.rs:220`). Fix: trait takes
`&SurfaceFrame` (or `Arc<Vec<u8>>`), X11 stores `Arc::clone`. Impact: one 3 MB
alloc+memcpy per repaint on the presenter thread and one fewer resident copy per panel.
Confidence high. Found by three audits.

**3.3 Frame cache holds wallpaper frames (32 MB each) in its six count-based slots.**
`src/render/cache.rs:26,52` `max_size(6)`; `worker.rs:145` routes `Now` and `Repaint`
through `cache.frame`, wallpapers included. A wallpaper whose content changes leaves up
to 6 × 32 MB of stale frames resident and evicts the small panel frames the cache
actually helps. Fix: weight by `pixels.len()` with a byte budget (32–64 MB), or cap
wallpaper-sized entries at one. `cached` has no weighted LRU; a small `VecDeque` with a
running byte total does it. Impact: up to ~160 MB reclaimed when wallpapers repaint; no
per-frame CPU change. Confidence high on mechanism, medium on how often real wallpapers
repaint.

**3.4 A wallpaper repaint re-renders every Panel over it even when its slice did not
change.** `src/backdrop/mod.rs:62-65,123` bumps `generation` per publish;
`src/surface/mod.rs:237-239` treats a generation change as `render_changed`; the frame
key includes the generation. Fix inside the existing key: in `crop_for`, after cropping,
memcmp (or xxh3) the new slice against the cached crop for the same rect; if identical,
return the previous generation for that surface so both `FrameKey` and
`SurfaceState.backdrop` hit. ADR 0011's guarantee ("stops a stale hit after the backdrop
moves") is preserved because slice equality is exactly "did not move under me". Impact:
eliminates 40–90 ms repaints per static-region panel per wallpaper change. Confidence
medium.

**3.5 Send only the changed rectangle per repaint (X11).** The previous frame is already
retained (`panel.bgrx`), so `update_image` can memcmp row bands and `put_image` only the
changed sub-rectangle; full frame on first paint/size change; Expose keeps sending the
retained whole frame. Impact: socket bytes per typical bar update −10× to −100× (a clock
digit vs a 700 KB–3 MB frame). Confidence medium-high; depends on repaints being
localized, which text updates are and backdrop changes are not.

**3.6 MIT-SHM for X11 uploads (optional, after 3.2 and 3.5).** x11rb has the `shm`
feature; `allow-unsafe-code`/`libc` are already on. Per-panel memfd segment,
`shm_attach_fd` + `shm_put_image`, fallback to `put_image_chunked` when absent. Removes
the kernel and server copies; 3.5 gets most of the win with less machinery. Confidence
medium.

**3.7 Frame key canonical-serializes the whole content tree per render request.**
`src/render/cache.rs:40-48` `json_canon::to_string` on every `frame()` call including
hits, and each of the six retained keys holds the full string. Fix: structural 128-bit
digest of the `Value` (sorted keys), or hash the string and drop it. Impact: 50–500 µs
and a 20–100 KB allocation per request. Confidence high on mechanism; low value alone.

**3.8 Per-render Tailwind parsing and style-JSON clone in the tree walk.**
`src/layout/html.rs:255` `TailwindValues::from_str(class)` per node per render, `:261`
`serde_json::from_value(style.clone())`. Fix: `Style::deserialize(&Value)` without the
clone; worker-local memo of `class → TailwindValues` (it is `Clone`), cleared on
`FontsChanged`/reload. Sub-millisecond per render; also paid by every click (hit-test
builds the same tree). Confidence low-medium on magnitude.

**3.9 Hit-testing rebuilds the takumi tree and runs a full layout per press.**
`src/hit_test.rs:105-165`, called from `src/app.rs:1138`; `painted_boxes` does the same
for a11y and `<Workspaces>`. With `Arc<Value>` content (2.4), a memo keyed by (panel id,
content pointer, w, h, dpr) → paint list turns repeated presses on unchanged content into
a lookup. Confidence medium-high. Not a drag cost — drags use the rect snapshotted at
press per ADR 0022.

---

## 4. Memory

- **Frame cache byte budget** — 3.3 above; the only tens-of-MB item found.
- **Third frame copy per panel** — 3.2 above.
- **`Arc<Value>` content** — 2.4 above; one copy of each panel's tree instead of 3–4.
- **QuickJS runtime is never tuned or observed.** `optative-script` `engine.rs:904`
  `Runtime::new()` with defaults; `JsxEvaluator` never calls `set_gc_threshold`,
  `set_memory_limit`, `run_gc` or `memory_usage`. Bundled QuickJS starts the GC threshold
  at 256 KiB and sets it to 1.5× live size after each GC, so a tick that allocates a whole
  node tree plausibly triggers a full mark-and-sweep every 1–3 ticks. Fix: after
  `build_runtime`, `set_gc_threshold(8–16 MiB)` on the render runtime; log
  `memory_usage()` at debug on reload; measure with `JS_DUMP_GC` first. Also `JS_FreeContext`
  shows up at 1.5 % self time in the `eval_only` profile — worth a look at what context is
  torn down per tick. Confidence medium-low on magnitude, high that nothing is tuned.
- **tauler-notify store is unbounded for never-expiring notifications** (`expire_timeout
  = 0` → no timer, `tauler-notify/src/store.rs:31-42` appends with no cap), and every emit
  builds a `json!` tree cloning every string then `to_string`s it (`main.rs:15-20`). Fix:
  cap at N≈50–100 newest (emit `NotificationClosed` reason 3 on eviction) and serialize a
  `#[derive(Serialize)] Payload<'a>` directly. Confidence high; behaviour change for the cap.
- **tauler-notify runs the multi-thread tokio runtime** for a daemon handling a few events
  a minute (`tauler-notify/src/main.rs:52`). `#[tokio::main(flavor = "current_thread")]`,
  drop `rt-multi-thread`. −(cores−1) idle threads and stacks. Confidence high.
- **Thread names.** 62 of the 67 live threads are unnamed (pool readers, bridges,
  presenter, worker). Naming them via `Builder::new().name()` costs nothing and makes every
  future `perf`/`top -H` reading legible. Not a perf win in itself.

---

## 5. Helper binaries (idle cost of the whole system)

**5.1 tauler-i3 re-fetches and re-emits the whole workspace payload every second even
when nothing changed.** `tauler-i3/src/scheduler.rs:7` `HEARTBEAT = 1s`;
`refresh_worker.rs:33-51` does a full `GET_TREE`, builds JSON, `println!`s unconditionally,
and publishes to a `tree_cache` its own doc says has no consumer. tauler dedupes the value
(`src/app.rs:1283`) so no eval or rasterization follows, but it costs i3 tree
serialization, swayipc deserialization, a pipe write and ~5 thread wakeups per second,
forever — ~86 000 useless lines a day.
**The heartbeat is currently load-bearing:** a layout reload keeps the subprocess (same
identity) but empties `stream_values` (`src/app.rs:1035`) and deliberately does not re-send
identical props to a kept process, so the workspaces panel is blank until tauler-i3's next
emit. A naive "skip the println if unchanged" would leave it blank forever. Fix, both
halves required: (1) dedupe output against the last emitted string in `refresh_worker`,
resetting the memo whenever the stdin thread receives any line from tauler; (2) in tauler,
re-send init/props to kept subprocesses on a successful reload. With both in place,
`HEARTBEAT` can rise to 5–10 s as a pure safety net for i3 4.25's silently dropped
replies. Confidence high on mechanism, medium on magnitude. Risk medium.

**5.2 tauler-i3 rewrites all four gaps on every workspace event.** `subscribe.rs:81-85`
enqueues `ReconcileGaps` for every `Event::Workspace` (urgent hints, renames, `empty`,
`init`); `ipc.rs` `reconcile_gaps` then does `GET_WORKSPACES` + a four-command
`RUN_COMMAND` even when focused workspace, output and config are unchanged. Fix: memoize
`(focused ws, output, cfg)` from the last successful apply in `command_worker`; skip
`run_command` when equal; clear on `UpdateConfig` and on subscribe reconnect (i3 restarts
drop runtime gaps). This is memoization, not event filtering, so the unsoundness the module
comment warns about is not reintroduced. Confidence medium. Risk medium-low (user-changed
gaps emit no event; repair defers to the next focus change).

**5.3 tauler-accumulate deep-clones the whole window and builds an intermediate String per
input line.** `tauler-accumulate/src/main.rs:58-66`. Fix: `serde_json::to_writer(&mut
output, &window)` — `VecDeque<Value>` is `Serialize`. 2–3× less work per line; identical
bytes. Confidence high.

**5.4 tauler-aerospace forks two `aerospace` processes per event with no coalescing.**
`tauler-aerospace/src/main.rs:97-99`, `aerospace.rs:22-47`. Fix: drain `hint_rx` with
`try_recv` before each refresh (or a 50 ms debounce like tauler-i3). A burst of 5 events
goes from 10 spawns to 2. Confidence high.

**5.5 Minor:** tauler-i3 parses each stdin line twice (`main.rs:104-119`, `events.rs:25`);
tauler-notify builds a `DBusProxy` per `Notify` and clones `OwnedValue`s to read `urgency`
(`server.rs:41-55,98-103`).

---

## 6. Latency (Hops on interactive paths)

- **Pointer: 0–8 ms poll Hop** — 1.1.
- **Layout reload can block the tick thread for the length of a running hook.**
  `src/app.rs:942` drops the `Reconciler`; `src/units.rs:411-424` `Drop` joins the
  reconciler thread, waiting out a Sweep in flight (ADR 0034 explicitly allows 40 s
  hooks). This is the one place the render loop waits on the reconciler, contrary to ADR
  0034's text. Fix: set `stop`, hand the `JoinHandle` to a detached reaper thread. Old and
  new runtimes may briefly coexist — ADR 0034 already treats a one-wake-old Sweep as
  normal; document it. Confidence high.
- **RandR change: serial round-trips, run twice per hotplug, on the presenter thread.**
  `src/presenter/x11.rs:110-145` handles `RandrScreenChangeNotify` and
  `RandrNotify(OUTPUT_CHANGE)` separately, each running `build_output_map` (1 + 2N serial
  `.reply()`s, including disconnected outputs, `src/x11/outputs.rs:94-132`) +
  `resolve_primary_output_name` + `context_dpi_dpr` (re-interns `RESOURCE_MANAGER` each
  time), and the tick thread does a full eval + reconcile per event. Fix: pipeline the
  cookies (3 round-trips regardless of N), coalesce both event kinds into one rebuild after
  the drain, intern the atom once. Non-interactive class, so mostly about how long the
  presenter is unavailable during a reconfigure. Confidence high on mechanism.
- **Startup: unconditional 50 ms sleep in `build_output_map_settled`**
  (`src/x11/outputs.rs:41,53-56`) before the first window. Keep the mechanism (issue #537),
  shorten to ~20 ms, or create windows after the first read and let the settled second read
  flow through the existing `OutputsChanged` → `Move` path. −30–50 ms startup. Confidence
  medium.
- **Per-press whole-content clone** (`src/app.rs:1137`, `:1247`) — borrow-checker
  workaround; dissolved by 2.4.

---

## 7. Build, dependencies, allocator

Already optimal: `opt-level=3`, fat LTO, `codegen-units=1`, `panic=abort`, `strip`;
default `INFO` filter with all `debug!`/`trace!` short-circuiting before argument
evaluation; no INFO+ log site on the per-pass path; no `Regex::new`, `Command::new` or
`fs::read` on the tick path.

- **`-C target-cpu=native`** would help tiny-skia/takumi raster but breaks the
  binstall-distributed binary. Offer as a local `just` recipe / `RUSTFLAGS`, never in
  `.cargo/config.toml`.
- **takumi default features:** `rayon` is only used by takumi-raster's animated-frame
  encoder, never reached by tauler (pool never initialized); `svg-backend` is only for
  `tauler-screenshot`. `default-features = false` + explicit list, and `svg-backend`
  behind a tauler cargo feature the screenshot crate enables. Binary size only.
- **`serde_json/preserve_order`** is forced by `oxc_resolver` via `optative-script`; not
  actionable, and `IndexMap` is fine here.
- **Duplicate crates** (hashbrown ×3, phf ×2, syn ×2, num-bigint ×2, miniz_oxide ×2) all
  live inside third parties; not fixable from tauler's manifest.
- **Allocator:** allocation is genuinely hot (`js_malloc_rt` 5 %, `malloc`/`cfree` ~3 %
  in the profiles; `Value` trees per tick, takumi node trees per render, MB-sized frame
  `Vec`s). mimalloc might give 5–10 % on eval/parse for ~100 KB of binary. Measure with
  `benches/pipeline.rs` before adopting. Low confidence.
- **Hashing:** every runtime `HashMap` has tiny keys except `stream_values` (2.10); FxHash
  would not help, interning would.

---

## 8. Upstream (takumi) — not fixable in this repo

- `snapshot_with_fallbacks` (`takumi-core-0.25.0/src/resources/font.rs:444-510`) clones
  the parley `FontContext` and calls `set_fallbacks` for every script sample per render;
  the profile shows ~10 % of `eval_to_pixels` in font matching (`match_font`, `load_font`,
  `table_data`, `CachedFamily` drops).
- `ShapeCache`/`MeasureCache` are per-render and `pub(crate)`; `render_with_context` and
  `render_node` are private, so shaped text cannot be reused frame to frame (glyph masks
  already are, via an 8 MiB process-global cache). A public `RenderOptions::shape_cache`
  would let unchanged strings skip shaping.
- `Bitmap` only exposes demultiplied data, so tauler pays `demultiply_rgba_in_place`
  (~2 % of the pipeline) and then discards alpha anyway; premultiplied output would be both
  cheaper and correct for the opaque window.

---

## 9. Checked and found already fine

Notifier design (capacity-1 `SyncSender` + `try_send` coalescing, blocking wait, no
spin); `changed` dedupe per key; render worker blocks on `recv` when idle, one slot per
target, floor is a delay not a discard; frames travel as `Arc` through channels (except
3.2); backdrop crop cached per `(id, generation, rect)`; wallpaper registry holds the
frame's own Arc; glyph masks cached process-wide; fonts loaded once; images decoded once
per `src`; presets and icon map are `LazyLock`; JSX transform once per load, render
function kept as `Persistent`; evaluator/runtime freed on reload; motion compressed per
pass, no hit-test per motion, drags measured from the press rect; pool I/O and bridge
threads all block; x11rb writes ≥16 KiB `PutImage` bodies directly with `writev`, one
flush per command; Wayland `SlotPool` reuses slots; tauler-i3 has no polling loops and one
persistent timed IPC query per worker; all helper stdouts are line-buffered; `src/pkg` is
off the startup and tick paths (git only on cache miss, on a background thread).

---

## 10. Suggested order

Ranked by payoff per unit of work, each step independently shippable and measurable with
the existing bench plus the wakeup counter below.

1. **Idle wakeups** — 1.1, 1.2, 1.3 (three files; verify with the command below, target
   ≤ 5/s).
2. **Per-repaint copies** — 3.1 and 3.2 (two small edits; verify with
   `cargo bench --bench pipeline`, `draw_only` should drop ~8 %).
3. **a11y clones + extra pass** — 2.1 and 2.2 (main-thread idle and interaction path).
4. **Tree clones and `Arc<Value>`** — 2.3, 2.4, 2.5 (mechanical; unlocks 3.9).
5. **`<I3Layout>` children and JS-only `h`** — 2.6, 2.7 (largest per-tick win; verify with
   `eval_only`).
6. **Theme memo, `<Workspaces>` memo, stream-key interning** — 2.8, 2.9, 2.10.
7. **Frame cache byte budget, wallpaper slice equality** — 3.3, 3.4.
8. **Helpers** — 5.1 (with its tauler-side half), 5.3, 5.2, 5.4.
9. **Latency** — reload reaper, RandR pipelining, startup settle.
10. Then measure QuickJS GC, mimalloc, SHM, damage rects before deciding.

Wakeup counter (run against the live process):

```sh
P=$(pgrep -x tauler); sw() { for t in /proc/$P/task/*; do grep -E '^(voluntary|nonvoluntary)' $t/status; done | awk '{s+=$2} END{print s}'; }; a=$(sw); sleep 10; b=$(sw); echo "$(( (b-a)/10 )) wakeups/s"
```
