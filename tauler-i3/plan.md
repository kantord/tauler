# tauler-i3 IPC integration redesign

Design for reworking tauler-i3's i3 IPC integration, worked out via a `/grilling`
session on 2026-07-17/18. Long-term motivation: tauler-i3 is meant to become the
foundation of a "meta window manager" (extra functionality layered on top of i3),
so the IPC layer needs to be genuinely correct, not just patched around one bug.

## Problems diagnosed this session

1. **Click hang (~5-10s)**: main.rs's event loop is single-threaded and fully
   synchronous. Both "an i3 workspace/window event fired, re-fetch the tree"
   (GET_TREE) and "user clicked, switch workspace now" (RUN_COMMAND) run inline
   on the same thread through the same shared `I3Query` connection. If a
   GET_TREE call stalls, the loop can't get back to reading an already-queued
   click event until it returns.

2. **The stall itself**: `I3Query::request()` has a 5s read/write timeout with
   one reconnect+retry. Live instrumentation showed the *query* connection
   (never the *subscribe* connection, which stayed rock-solid all session —
   zero drops) occasionally gets `EAGAIN`/"Resource temporarily unavailable" on
   GET_TREE specifically, taking the full ~5s before erroring/retrying.
   Observed twice in an 11-minute undisturbed window. Root cause inside i3 is
   **not understood**. i3 stayed reactive to keybindings/other clients
   throughout, ruling out the old global-freeze issues (i3/i3#2280, #1876 —
   both closed ~2015-16, i3 v4.10-4.12, long before our i3 4.25.1). i3/i3#2999
   (closed 2018) is a more relevant follow-up; PR #3263 "kill misbehaving
   subscribed clients instead of hanging" (merged 2018-08-08) addresses
   *subscribed* clients being disconnected instead of hanging — but our
   subscribe connection never dropped, so that specific mechanism doesn't
   appear to be what's happening here.

3. **GET_TREE cost measured directly** (9 workspaces, 21 windows on this
   machine): ~39KB JSON reply; i3's own CPU cost is ~0.26ms per call (measured
   via `/proc/PID/stat` utime+stime delta across 500 back-to-back calls,
   isolating i3's process from the calling `i3-msg` subprocess overhead). So
   CPU/payload-size is **not** the cause of the rare stall, and is not a
   meaningful constraint on refresh frequency at any reasonable rate.

4. `fetch_workspaces` uses GET_TREE (not the cheaper GET_WORKSPACES)
   specifically because it also extracts per-workspace window titles via
   `collect_window_titles_in_focus_order` — GET_WORKSPACES alone wouldn't
   provide that.

5. Minor robustness gaps found reading `tauler-i3/src/{ipc.rs,main.rs,
   events.rs,workspace.rs}` in full: the subscribe thread's
   reconnect-after-mid-stream-drop path has no backoff (only the
   initial-connect-failure path sleeps 1s); `i3_socket_path()` silently
   swallows a failed `i3 --get-socketpath` into an empty string; main.rs's
   event loop/batching/click-dispatch logic has zero test coverage (only
   `I3Query` itself is unit-tested).

## Refined design (post-grilling)

Four threads, **no central hub** — each producer talks directly to whichever
consumer it needs, rather than routing through a shared main-loop dispatcher.

1. **Stdin thread** (mostly unchanged) — reads stdin lines, parses click
   events, sends `SwitchWorkspace(name)` directly to the command-worker's
   channel. EOF still means `process::exit(0)`.

2. **Subscribe thread** (mostly unchanged) — persistent subscribe connection
   to `["workspace","window"]`. Per event: always sends a lightweight
   "something changed" hint to the refresh-worker; if it's a workspace-focus
   event matching our output, also sends `ApplyBarGap` directly to the
   command-worker. **Fix**: the reconnect-after-mid-stream-drop path gets the
   same simple 1s flat backoff as the initial-connect-failure path (currently
   has none — theoretical busy-loop risk, never observed in practice).

3. **Command-worker** (new) — owns one dedicated `I3Query` connection, used
   only for RUN_COMMAND (`switch_workspace`, `apply_bar_gap`). Receives from
   both the stdin thread and subscribe thread (mpsc, multiple senders).
   Fire-and-forget from the senders' side — no reply needed for current uses;
   a future caller wanting confirmation can add its own reply channel without
   blocking anyone else, since the waiting happens on the caller's own
   thread/call site, never on a shared thread.

4. **Refresh-worker** (new) — owns one dedicated `I3Query` connection, used
   only for GET_TREE. Runs a debounce-with-max-wait scheduler:
   - Decision logic is a **pure, testable function**:
     `next_wakeup(now, last_refresh, pending_since) -> Instant` (or an enum
     `{RefreshNow, WaitUntil(Instant)}`), with a heartbeat ceiling of
     `last_refresh + 1s` (fires even with zero events, self-heals missed
     events) and a debounce of `pending_since + 50ms`, capped at that
     ceiling. `pending_since` is fixed at the *first* hint since the last
     refresh, not re-armed by later hints in the same burst (confirmed
     2026-07-18): this bounds refresh latency to 50ms after the first sign
     of change even under a sustained event stream, rather than waiting for
     activity to fully quiet down — either way, the heartbeat ceiling still
     caps the wait at 1s since the last refresh.
   - The thread loop is a thin shell: computes the wakeup via the pure
     function, does one `recv_timeout` covering both "wait for a hint" and
     "wait for the deadline," and acts on the result.
   - On firing (success **or** failure both reset `last_refresh_time` and
     clear `pending_since` — no extra retry layer on top of `I3Query`'s
     existing internal reconnect+retry-once): builds the workspace list,
     prints it to stdout directly (this thread becomes the sole stdout
     writer for workspace updates — the old separate one-time startup emit
     collapses into the same path, since the very first scheduling deadline
     is essentially "now"), and publishes into a shared `TreeCache`
     (`Mutex<(Instant, Arc<Snapshot>)>` + `Condvar`) exposing
     `get_tree(max_age)` for future consumers. Documented as **best-effort**,
     not a hard deadline, since GET_TREE can rarely stall.

`main()` becomes just wiring: build the channels, spawn the four threads,
block on the stdin thread's lifetime.

### Testing

The scheduler decision function gets fast, deterministic unit tests
(fabricated `Instant`s, no real sleeping/sockets) covering: heartbeat-only
firing, burst-coalescing via debounce, the cap never exceeding the heartbeat
ceiling, and failed-attempt-still-resets-schedule. `main.rs` wiring itself
stays untested as plumbing (pragmatic TDD — no tests for pure wiring); one
lightweight end-to-end smoke test (fake i3 socket) covers the rest.

### Deliberately deferred

Generalizing beyond the current subscribe event types (`["workspace",
"window"]`) or command types (`switch_workspace`, `apply_bar_gap`) — not
worth it before a second concrete feature exists to validate the shape. The
`TreeCache` primitive plus the clean thread/channel boundaries are considered
sufficient extensibility hooks for now ("if it's not hard to generalize
later, keep it as-is").

### Still genuinely unexplained

Why GET_TREE specifically stalls rarely inside i3 itself. This design fully
insulates click responsiveness from it and gives the codebase a correct
foundation regardless of the cause, but does not explain or fix the
underlying i3-side behavior.

## Deep-research pass (2026-07-18) — resolved

Ran a research pass on the six open questions below before implementing. Full
cited report ingested into the second brain:
`/home/kantord/repos/second-brain/projects/tauler-i3-ipc-redesign.md` (and its
linked `notes/`/`sources/`). Headline results:

- i3's own IPC docs and most mature client libraries (i3ipc-python, i3ipc-rs,
  i3ipcpp) already separate the event-subscribe connection from
  query/command connections — validates this design's topology, though it's
  not universal (`tokio-i3ipc` uses one connection for everything).
- **No source anywhere explains the actual GET_TREE stall.** Not i3's own
  issue tracker, not any client library's issues, not the i3 source itself.
  The one historical i3 blocking-IPC-write bug class and its 2018 fix
  (PR #3263) were confirmed to apply only to the async event-push path for
  subscribed clients, not to synchronous GET_TREE replies — so it does not
  explain this symptom. This root cause remains a **fully open mystery**;
  this design is built to be robust to it regardless of cause, not to fix it.
- No prior art exists for the debounce-with-max-wait scheduler pattern in any
  surveyed i3/sway client library — it appears to be original design here.
- No documented limits/quirks on a client holding multiple concurrent i3/sway
  IPC connections (validates having 3 separate connections).
- No direct i3-vs-sway comparison of IPC server internals exists; whether
  sway shares the same latent stall risk is unverified either way.
- The one confirmed prior art for a pure-IPC "meta window manager" is
  `autotiling` (i3ipc-python, event-driven via WINDOW/MODE subscriptions, not
  polling) — validates the general "react to events, don't poll" shape.

## Final grilling pass (2026-07-18) — adopt the `swayipc` crate

Reconciling the research findings against the locked-in design didn't change
the architecture (see above — validated, not contradicted). But a follow-up
question ("should we use a crate for this, especially for sway support?")
surfaced a real, verified improvement over the original hand-rolled-protocol
assumption:

- **Adopt [`swayipc`](https://github.com/jaycefayne/swayipc-rs)** (the
  blocking variant), replacing tauler-i3's hand-rolled wire-protocol framing
  (`i3_send`/`i3_recv`) and manual `serde_json::Value` tree-walking.
- Verified facts (read directly from crates.io and the crate's own source,
  not assumed): `swayipc` has 4.6M downloads and was last updated October
  2025 — versus the i3-specific alternatives research surfaced, `i3ipc-rs`
  (175K downloads, untouched since 2019) and `tokio-i3ipc` (36K downloads,
  untouched since 2022), both effectively abandoned. `swayipc`'s README
  explicitly states i3 compatibility is kept despite targeting sway
  primarily. Its `EventType` enum has both `Workspace` and `Window` variants,
  covering what we subscribe to today.
- **Timeout compatibility, verified by reading `blocking/src/connection.rs`
  directly**: `Connection::new()` itself sets no read/write timeout (plain
  `UnixStream::connect`) — using it as-is would reintroduce the "block
  forever" problem this whole redesign exists to fix. But the crate also
  provides `impl From<UnixStream> for Connection` (and the reverse), so we
  construct our own `UnixStream` with `set_read_timeout`/`set_write_timeout`
  exactly as today's `connect_with_timeout` already does, then wrap it via
  `Connection::from(stream)` — keeping full timeout/reconnect control while
  gaining the crate's typed API (`get_tree() -> Node`, `get_workspaces() ->
  Vec<Workspace>`, `run_command()`, `subscribe() -> EventStream`).
- Net effect on the design above: command-worker and refresh-worker each own
  one `swayipc::Connection` (constructed via our own timeout-setting
  `UnixStream`); the subscribe thread's `Connection` is consumed by
  `.subscribe(&[EventType::Workspace, EventType::Window])` into an
  `EventStream`, matching the existing subscribe-thread shape almost exactly.
  `workspace.rs`'s tree-walking gets rewritten against the typed `Node`
  struct instead of `serde_json::Value`. Everything else above (topology,
  scheduler, TreeCache, backoff fix, deferred scope) is unchanged.
