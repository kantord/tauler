# Two RandR connections, one shared resolution

Two threads poll RandR: the X11 presenter thread (`src/presenter/x11.rs`, on the
`Arc<RustConnection>` shared app-wide since `src/main.rs`'s `init_x11()`) and
`outputs_thread` (`src/x11/outputs.rs`), which opens its own separate
`RustConnection`. Issue #525 flagged this as redundant and asked whether the two
should be consolidated into one connection owned by one thread.

They stay separate. What issue #525 actually objected to — "the same knowledge,
reimplemented inconsistently, several times" — is already fixed, by a different
change: `build_output_map`, `resolve_primary_output_name`, and `context_dpi_dpr`
(`src/x11/outputs.rs`, `src/x11/panel.rs`) are the one place output geometry, primary
resolution, and DPR are computed, and every call site — `init_x11`, the presenter
thread, `outputs_thread` — calls the same functions. Only the physical socket is
duplicated, not the resolution logic.

## Why not merge the sockets too

`outputs_thread` is not a fixed part of the running app — it exists only because a
layout file's JSX declared a data call to `"tauler:outputs"` (the monitor-list JSON
stream). `DataLoop`'s `BuiltInSource` mechanism (`src/data/data_loop/builtin.rs`)
starts and stops it dynamically as layouts change, and its `func` field is a bare
`fn(Sender<StreamItem>, String, Arc<AtomicBool>)` — a plain function pointer, not a
closure, so it cannot capture anything from the rest of the app. Feeding it the
presenter thread's already-resolved output state instead of its own RandR polling
would mean changing `BuiltInSource::func` to something capture-capable (e.g.
`Box<dyn Fn(...) + Send>`) across the whole generic data-loop mechanism — every
built-in source pays for that, not just this one — to save one idle X11 socket that
only exists while some layout is actually asking for monitor data.

## Consequence

A layout that subscribes to `tauler:outputs` costs one extra X11 connection for as
long as it's subscribed. That is the accepted cost. If `BuiltInSource` ever needs to
carry closures for an unrelated reason, revisit sharing the connection then — don't
add that capability speculatively for this alone.
