# One theme is one mode

A theme is one palette for one appearance, labelled `dark` or `light` in the file; a colour
scheme that ships a dark and a light style is two themes (`tokyonight-night`, `tokyonight-day`).
This replaces the earlier shape, one theme with a `colors.light` and a `colors.dark` map and the
mode chosen by `theme.mode` in the config.

The two-palette shape let a theme and a mode disagree: a theme defined for dark and shown in
light had no colours, and nothing said so. Software that left Vim's single `background` option
behind (VS Code's per-appearance themes, tinted-theming's `variant`) ships one theme per
appearance and labels it, so a theme can neither be missing the palette for the mode it is shown
in, nor be shown on the wrong background by accident. It also makes a built-in theme one scheme
in one mode, derived the same way for every scheme, instead of a hand-paired dark and light.

`theme.mode` stays, optional, and wins when set: it picks the default theme's mode when no theme
is named and overrides the label of a named one — the colours do not move, so it only changes
which `dark:` / `light:` class variants apply. `auto` is gone; following the OS would need two
theme names, not a mode.

This supersedes the paragraph in ADR 0033 saying mode comes from `config.yaml` rather than the
theme file. The reload policy there is unchanged.
