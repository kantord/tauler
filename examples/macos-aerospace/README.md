# macOS + AeroSpace

A left rail showing [AeroSpace](https://nikitabobko.github.io/AeroSpace/) workspaces, with
click-to-switch. Shaped like a dotfiles repo, because a desktop is configured by more than
tauler — the reservation lives in `aerospace.toml`, not in the layout file.

```
home/.config/tauler/layout.jsx      the rail
home/.config/tauler/config.yaml     theme and font
home/.config/aerospace/aerospace.toml   the 140px it sits in
```

## Requirements

AeroSpace **0.21.0-Beta or newer**. The rail is driven by `aerospace subscribe`, which
older builds do not have — `aerospace subscribe --help` printing a usage line is the check.

```sh
brew install --cask aerospace     # or: brew upgrade --cask aerospace
cargo install --path .            # tauler
cargo install --path tauler-aerospace
```

## Install

The tree is copied over `$HOME`, so review it first — the AeroSpace config here is a
minimal one, and it will replace yours.

```sh
cp -r home/. ~/
aerospace reload-config
tauler
```

## Reserving the rail's space

This is the one thing that does not work the way it does under i3.

`<I3Layout>` computes gaps from panel geometry and pushes them to i3 at runtime. AeroSpace
reads `gaps.outer.*` when it loads its config and offers no command to set them, so the
number is declared twice and the two have to agree:

| where | what |
| --- | --- |
| `layout.jsx` | `const RAIL = 140` — how wide the panel is drawn |
| `aerospace.toml` | `outer.left = 140` — how much space tiled windows leave for it |

Change one and you get dead space, or windows sliding under the rail. Regenerating
`aerospace.toml` and calling `aerospace reload-config` does work — reload re-applies gaps,
not just bindings — so this could be automated later. AeroSpace has no `include` directive
and no config-path override, so anything generated has to be written into the same file as
your keybindings.

## Why AeroSpace leaves the rail alone

There is no rule in `aerospace.toml` telling AeroSpace to ignore the bar, and there should
not be. AeroSpace decides what to manage from the window itself — `isWindowHeuristic` wants
a close, fullscreen, zoom or minimize button, or `subrole == kAXStandardWindowSubrole` —
and tauler runs under `NSApplicationActivationPolicy.accessory`, so it is not a Dock app
and never appears in `aerospace list-windows` at all.

That is what keeps the rail on screen across a workspace switch. AeroSpace switches
workspaces by moving the windows it manages; one it cannot see is never moved, so no sticky
or always-on-all-workspaces setting is needed — AeroSpace has neither. The same property
should keep yabai and Amethyst off it.

An `[[on-window-detected]]` rule with `layout floating` looks like the fix and is not one:
floating windows still belong to a workspace, and still disappear when you leave it.

## The menu bar

macOS keeps its menu bar at the top of every screen and will not release the space, so the
rail starts at `y = 25` rather than `y = 0`. It is also `above={true}`: a normal-level
window on macOS sits under the menu bar rather than beside it.

## What the module reports

`tauler-aerospace` emits one JSON line per change:

```json
{"workspaces": [
  {"name": "1", "focused": true, "visible": true, "urgent": false,
   "focused_windows": ["Claude", "Downloads"], "apps": ["Claude", "Finder"]}
]}
```

Field names match `tauler-i3`'s, so a workspace strip written for i3 renders here unchanged.
Two differences worth knowing:

- `urgent` is always `false`. AeroSpace has no urgency hint and macOS has nothing to derive
  one from.
- `apps` is additional — app names for the workspace, deduplicated. Per-workspace app icons
  are the usual AeroSpace bar idiom and `focused_windows` alone cannot express them.

AeroSpace declares every workspace in the config whether or not anything is on it, so the
raw list is 30-odd entries. The rail filters to occupied ones plus wherever you are.
