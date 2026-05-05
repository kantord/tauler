# Costae as a Declarative Wayland Compositor

## Core idea

A thin reconciler layer over [smithay](https://github.com/Smithay/smithay)
that gives you the `ManagedSet` / `Lifecycle` primitives for Wayland surfaces,
windows, and running processes — but *nothing else*. No tiling policy, no
floating policy, no keybinding system. The compositor is intentionally empty:
it provides the loop, the lifecycle hooks, and the surface glue; the user
writes a **reducer** that maps current state to desired layout.

This is the same split that makes Redux and Elm powerful: the framework owns
the reconcile cycle, the user owns the logic. Write a tiling reducer and you
get an i3-style compositor. Write a declarative workload reducer and you get
Kubernetes for your desktop. The substrate is the same in both cases.

Beyond individual windows, the vision extends to enwiro environments as
namespaces: declared applications, layouts, and whole workspaces
reconciled continuously, so that switching environment brings up the right
workload and tears down the old one — exactly the same way `ManagedSet`
already manages panel lifecycles today.

## The layer stack

```
┌─────────────────────────────────────────┐
│  user reducer  (tiling / workload / ...) │  ← user writes this
├─────────────────────────────────────────┤
│  tauler-compositor                       │  ← reconciler + lifecycle
│  (ManagedSet<WindowSpec>, scene graph,   │
│   process management, enwiro integration)│
├─────────────────────────────────────────┤
│  smithay                                 │  ← Wayland protocol / DRM
└─────────────────────────────────────────┘
```

tauler-compositor is intentionally a library, not a complete window manager.
It provides:
- `ManagedSet<WindowSpec>` — reconcile a desired window set against actual surfaces
- `ProcessSet` — reconcile a desired application set against running processes
- Scene graph integration (via takumi or smithay's scene API)
- Enwiro environment change events as a first-class input stream

The user provides a **reducer**: a pure function `(State, Event) → DesiredWindowSet`
that encodes all policy. The framework drives the reconcile loop; the reducer
decides what should exist and where.

## Why this fits tauler's existing model exactly

Costae already manages panels this way: `ManagedSet<PanelSpec>` drives
`enter` (create layer shell surface), `update` (resize, rerender), `exit`
(destroy). A window is the same shape:

| Panel lifecycle          | Window lifecycle                          |
|--------------------------|-------------------------------------------|
| `enter` → open wlr surface | `enter` → spawn process, map surface    |
| `update` → rerender      | `update` → move, resize, raise/lower      |
| `exit` → destroy surface | `exit` → send close request, reap process |

The reconciler loop that drives panels today is the same loop that would drive
windows. The only new concern is matching observed Wayland surfaces back to
their declaration — and that is a solvable bookkeeping problem (app_id + pid
tracking).

## What the user actually writes

A tiling window manager reducer might look like this in outline:

```rust
fn reduce(state: &CompositorState, event: Event) -> DesiredWindowSet {
    let mut windows = DesiredWindowSet::new();
    let surfaces = state.mapped_surfaces();       // what smithay sees
    let focused = state.focused_surface();

    // tile all surfaces in the active workspace
    for (i, surface) in surfaces.iter().enumerate() {
        windows.insert(WindowSpec {
            id: surface.id(),
            geometry: tile_geometry(i, surfaces.len(), state.output_size()),
            focused: surface.id() == focused,
            decoration: DecorationSpec::border(2, if focused { BLUE } else { GRAY }),
        });
    }
    windows
}
```

A declarative workload reducer would instead compute the desired set from
enwiro environment config, then reconcile running processes against it:

```rust
fn reduce(state: &CompositorState, event: Event) -> (DesiredWindowSet, DesiredProcessSet) {
    let env = state.active_enwiro_env();
    let spec = load_env_spec(env);               // declared apps for this env
    let processes = desired_processes(&spec);    // what should be running
    let windows = desired_windows(&spec, state); // where each window should be
    (windows, processes)
}
```

The framework reconciles both sets on every tick. The reducer is pure; the
framework owns all the side effects (spawning, killing, resizing, repainting).

## What this enables that today's compositors don't

Sway, Hyprland, and Niri describe *layout rules* declaratively but manage
*workloads* imperatively (startup commands, ad-hoc keybindings). Once launched,
applications are on their own: they crash, they accumulate, they end up on the
wrong workspace. The compositor doesn't care.

A reconciling compositor cares continuously:

- **State repair**: a declared terminal crashes → the reconciler detects the
  missing item and respawns it, just like a Kubernetes Deployment restores a
  failed pod
- **Env-scoped workloads**: switch enwiro environment → the desired-state set
  changes → the reconciler brings up the new env's applications and exits the
  old env's (or quiesces them to a background set)
- **Idempotent activation**: `activate my-project` from any state produces the
  same result — the right windows in the right places, nothing more
- **Drift detection**: if a user moves a window manually, the compositor can
  either accept the override or gently restore the declared position on next
  reconcile, depending on policy

## The enwiro integration

Enwiro environments already map workspaces to project directories. A
compositing layer makes them first-class runtime scopes:

```yaml
# ~/.config/tauler/envs/my-project.yaml
env: my-project
windows:
  - app: alacritty
    cwd: "{{ env.path }}"
    layout: { x: 0, y: 0, width: 33%, height: 100% }
  - app: firefox
    url: "localhost:3000"
    layout: { x: 33%, y: 0, width: 67%, height: 100% }
  - app: spotify
    layout: { floating: true, anchor: top-right, width: 400, height: 600 }
```

This is the same JSX layout model tauler already uses, extended to describe
running processes rather than just rendered panels. Enwiro cookbooks could
generate these declarations from Git repo metadata or GitHub project configs —
open a new repo and its standard development layout is already there.

## The tauler decoration layer

Costae's existing rendering pipeline (takumi + JSX layout) would handle window
decorations natively. Title bars, borders, focus rings — all rendered by the
same JSX compositor that already draws panels. The boundary between "bar
widget" and "window decoration" dissolves: a window title is just another
takumi surface, driven by the same value streams as everything else.

This also means window decorations can be arbitrarily rich: show git branch,
test status, or resource usage directly in the title bar without any external
tool integration.

## What tauler already provides

The gap between "tauler today" and "tauler-compositor" is narrower than it
looks:

**Process lifecycle is fully solved.** `ProcessSource` already implements
`Lifecycle` with crash detection and automatic respawn built into
`reconcile_self` — if `child.try_wait()` returns `Some(_)`, the process is
respawned immediately. Kill-on-exit is handled in `exit`. This is exactly the
"state repair" behaviour described above, and it already ships.

**The pipeline fan-out architecture is already envisioned.** `pipeline-vision.md`
describes how each level of the reconciler tree fans out to child `ManagedSet`s,
with cleanup propagating structurally when a parent exits. Environments as
namespaces and windows as workloads are a direct application of that model.

**Window identity for compositor-spawned apps is trivial.** Because tauler
spawns the process itself (via `ProcessSource`), it holds the PID. A newly
mapped Wayland surface can be matched to its declaration by PID lookup against
smithay's client credentials. No `app_id` heuristics needed for managed windows.

**The decorator surface model is already built.** Takumi surfaces and the JSX
renderer already produce layer-shell surfaces that sit above other windows.
Window decorations are exactly the same thing: a surface keyed to a window ID,
rendered by the same pipeline.

## Remaining integration work

### 1. Fork a minimal smithay compositor as the base

Rather than writing compositor plumbing from scratch, fork a minimal smithay
compositor (e.g. `anvil` or an equivalent). This already provides: seat
management, DRM/KMS, XWayland opt-in, damage tracking, and output
configuration. The fork's job is to remove all hard-coded policy and expose
a surface-lifecycle hook where the tauler reconciler plugs in.

Seat management is not multi-user support — it is keyboard/pointer routing.
A single-user compositor needs it but it is a solved problem in any smithay
fork. No bespoke work required.

### 2. `WindowSurface` as a `Lifecycle` item

The new primitive is a `Lifecycle` impl that wraps a mapped `smithay`
`ToplevelSurface`: `enter` receives the surface when it maps, `update` applies
geometry changes, `exit` sends the close request. The `exit` timeout policy
(wait N ms then force-kill the process) can reuse the existing `ProcessSource`
kill path since tauler already holds the PID.

### 3. Escape hatches for ad-hoc windows

Windows not spawned by tauler have no declaration to reconcile against. These
need an "unmanaged" bucket: tracked, positioned by the user's reducer policy
(e.g. float them), but never forcibly closed by the reconciler. The
distinction is clean: managed windows have a `ProcessSource` entry; unmanaged
windows arrived from outside.

## Open questions

- Should per-environment window declarations live in tauler config, enwiro
  config, or a new shared format that both tools understand?
- Can enwiro cookbook plugins generate window declarations alongside
  environments, so that checking out a repo also defines its default layout?
- Which minimal smithay compositor is the best fork base — `anvil`
  (smithay's own reference), or a leaner community compositor closer to the
  thin-layer goal?
- How should the reconciler handle multi-monitor setups where the same
  environment spans multiple outputs? `ctx.outputs` from the multi-monitor
  work is the natural per-output fan-out point.
- The reducer function receives events and returns a desired set — but
  enwiro cookbook calls are async (network, filesystem). Does the reducer
  stay pure (caches pre-fetched desired state) or does it go async? The
  streaming pipeline model suggests the cookbook result arrives as a stream
  event, keeping the reducer synchronous.
