---
title: Running Tauler
description: Where Tauler looks for its files, which display server it picks, what reloads on its own, and where it writes when something goes wrong.
---

Run `tauler` with no arguments. It reads your layout file, draws your panels, and keeps
running until you stop it.

## Where the files are

Tauler reads `$HOME/.config/tauler`. The path is fixed: Tauler does not consult
`XDG_CONFIG_HOME`.

Two layouts are recognised, and Tauler picks one when it starts:

1. `layout.op.mdx`, if it exists. Theme and fonts live in its frontmatter. This is the
   format [the layout file](/docs/layout-file/) describes.
2. Otherwise the legacy pair: `layout.jsx` for the layout and `config.yaml` beside it for
   the theme and fonts.

If neither exists, Tauler starts and draws nothing, and logs that no layout file was found.
Creating one afterwards is picked up without a restart.

The choice is made once. If you start Tauler on `layout.jsx` and later create a
`layout.op.mdx`, Tauler keeps using `layout.jsx` until you restart it.

## Which display server it uses

Tauler picks a backend when it starts. See [Backends](/docs/backends/) for how each one
behaves.

| Situation | Backend |
|---|---|
| macOS | macOS, always |
| `TAULER_BACKEND=wayland` | Wayland |
| `TAULER_BACKEND` set to any other value | X11 |
| `TAULER_BACKEND` unset, `WAYLAND_DISPLAY` set | Wayland |
| neither set | X11 |

`TAULER_BACKEND` wins over `WAYLAND_DISPLAY`. A typo such as `TAULER_BACKEND=waylnd` selects
X11, not an error.

```sh
TAULER_BACKEND=x11 tauler     # force X11 inside a Wayland session (needs an X server)
```

## What reloads on its own

Tauler watches these, and re-reads the layout when any of them changes:

- `layout.op.mdx`, `layout.jsx` and `config.yaml` in the config directory
- every file your layout imports
- the theme file, when `theme.file` names one

You do not restart Tauler for any of these. An editor that saves by replacing the file, and
`chezmoi apply`, both count as a change.

A reload is a cold start: every subprocess restarts and all stream values are cleared. See
[the layout file](/docs/layout-file/).

### A bad edit keeps what you have

A reload that fails leaves the running layout as it was and writes the reason to the log.
This covers a syntax error, an error while the layout runs, unreadable frontmatter, and an
invalid `config.yaml`. Fix the file and save again.

A theme that cannot be loaded behaves the same way on reload: the theme on screen stays, and
the reason is logged. At startup there is no theme to keep, so a bad `theme.file`, an
unknown `theme.name`, or unreadable frontmatter stops Tauler with the reason on stderr and in the log.

### A new Tauler binary restarts Tauler

Tauler also watches its own executable. When the file is replaced, Tauler shuts down its
panels and subprocesses and starts the new binary in the same process. Installing a new
version needs no restart from you.

Replace the binary atomically, by copying to a temporary name and renaming it over the old
one, so Tauler never sees a half-written file.

## Logs

Tauler logs to standard output at level `info`. Set `RUST_LOG` to change it, for example
`RUST_LOG=debug tauler`.

A startup failure that stops Tauler — an unusable theme, or unreadable frontmatter or
`config.yaml` — is also printed to standard error, so you see it at the terminal even if you
redirect standard output. To capture both streams in one file, run `tauler > tauler.log 2>&1`.

## When it crashes or freezes

Two things append a line to `$HOME/.local/share/tauler-crash.log`:

- A crash writes `PANIC:` followed by the message and location.
- A watchdog checks every 10 seconds whether the main loop is still running. If it has not
  completed a turn for more than 10 seconds, it writes `FREEZE: main loop stalled for Ns`,
  where `N` is the number of seconds.

The watchdog only reports. It does not restart or stop Tauler.

## Updating packages

`tauler pkg update` re-resolves every package your layout imports and rewrites the lockfile.
It does not start the bar and does not touch a Tauler that is already running. See
[Packages](/docs/packages/).
