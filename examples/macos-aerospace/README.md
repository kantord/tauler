# macOS + AeroSpace

A left rail showing [AeroSpace](https://nikitabobko.github.io/AeroSpace/) workspaces, with
click-to-switch. Shaped like a dotfiles repo, because a desktop is configured by more than
tauler — the reservation lives in `aerospace.toml`, not in the layout file.

```
home/.config/tauler/layout.op.mdx      the rail, theme and font
home/.config/aerospace/aerospace.toml  the space it sits in
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
| `layout.op.mdx` | `RAIL = 148`, `RAIL_PAD = 10`, `MENU_BAR = 38` |
| `aerospace.toml` | `outer.left = 176` — that is `RAIL - RAIL_PAD + GAP` |

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

## The top inset is not the menu bar

AeroSpace lays windows out inside `NSScreen.visibleFrame`, so the rail has to start at the
same place or the two will not line up. On a notched Mac that inset is the camera housing,
reserved whether the menu bar is hidden or not:

```sh
osascript -l JavaScript -e 'ObjC.import("AppKit"); var s=$.NSScreen.mainScreen;
  s.frame.size.height - s.visibleFrame.size.height'
```

`MENU_BAR` in the layout has to equal that number. It changes when you change display
scaling — the notch is a fixed physical size, so more logical points fit inside it — and
`GAP` mirrors it, so `gaps.outer.*` moves too. tauler should read `visibleFrame` itself and
make the constant unnecessary: [#416](https://github.com/kantord/tauler/issues/416).

`gaps.outer.top` stays `0`. The inset is already excluded from `visibleFrame`, so anything
there is added on top of it and doubles the margin.

## Why the left reservation subtracts the padding

The rail has no plate of its own, so it appears to end where the pills end, not where the
window does. Reserving `RAIL + GAP` therefore reads as a wider margin on the left than on
the other three sides. Subtracting the rail's own padding puts them back in agreement — a
bar with a visible background would not need this.

## The menu bar

The rail cannot be drawn *over* a visible menu bar: winit maps its topmost level to
`kCGFloatingWindowLevel` (3) and the menu bar sits at 24. Auto-hiding the menu bar frees
that strip on an unnotched display; on a notched one the safe area is reserved anyway, so
it buys almost nothing.

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
