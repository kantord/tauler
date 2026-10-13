---
title: Themes
description: Choosing a built-in theme, writing your own, and the colour names a layout file can use.
---

A theme is one set of colours for one appearance — dark or light. Layout files never name a
colour directly. They name what a colour is **for** (`bg-card`, `text-muted-foreground`,
`border-border`), and the active theme says what that colour is.

## Choosing a theme

Set it in the layout file's frontmatter:

```jsx
---
theme:
  name: tokyonight-night
---
```

With no `theme:` block at all, tauler draws with `default-dark`.

| Theme                                  | Mode  | Notes                                               |
| -------------------------------------- | ----- | --------------------------------------------------- |
| `default-dark`, `default-light`        | dark, light | Neutral greys. What you get when you name nothing. |
| `tokyonight-night`                     | dark  | From the Tokyo Night colour scheme.                 |
| `tokyonight-storm`, `tokyonight-moon`  | dark  | The other two Tokyo Night dark styles.              |
| `tokyonight-day`                       | light | Tokyo Night's light style.                          |

An unknown name stops tauler at startup and lists the names it knows. If you reload a layout
file whose theme can't be loaded, the theme already on screen stays and the reason is logged.

The Tokyo Night themes are made from [tokyonight.nvim](https://github.com/folke/tokyonight.nvim)
(Apache-2.0).

### Mode

Every theme says whether it is dark or light. A theme named `tokyonight-night` is dark, and
`tokyonight-day` is light; there is no theme that holds both.

`theme.mode` is optional:

- With **no theme named**, `mode: light` picks `default-light` instead of `default-dark`.
- With a theme named or given as a file, it **overrides** the mode that theme says. Only the
  label changes: the colours stay the theme's. What it decides is which
  `dark:` / `light:` class variants in your layout apply (see below).

### Your own theme

Point `theme.file` at a YAML file instead of naming a built-in:

```jsx
---
theme:
  file: ~/.config/tauler/my-theme.yaml
---
```

Name a built-in **or** a file, never both: that stops tauler rather than choosing one.

## Writing a theme file

```yaml
name: my-theme
mode: dark # or light
colors:
  background: "#1a1b26"
  foreground: "#c0caf5"
  card: "#14141e"
  card-foreground: "#c0caf5"
  # … every colour listed below
radius:
  lg: "0.625rem"
fonts:
  heading: "Lora"
```

`name` and `mode` are required. Colours are CSS colours: `#rrggbb`, `#rrggbbaa` or `oklch(…)`.
`radius` and `fonts` are optional.

A theme that leaves a required colour out does not load — tauler stops at startup and lists
what is missing, instead of drawing elements with no colour.

### Colours every theme defines

| Colour                                                       | What it is for                                    |
| ------------------------------------------------------------ | ------------------------------------------------- |
| `background`, `foreground`                                   | The page and its default text                     |
| `card`, `card-foreground`                                    | Raised panels and the text on them                |
| `popover`, `popover-foreground`                              | Floating plates (menus, launchers, notifications) |
| `primary`, `primary-foreground`                              | The accent and the text on it                     |
| `secondary`, `secondary-foreground`                          | A quieter filled surface and its text             |
| `muted`, `muted-foreground`                                  | Tracks and fills; de-emphasised text              |
| `accent`, `accent-foreground`                                | Selected or hovered rows and their text           |
| `destructive`, `destructive-foreground`                      | Errors and the text on them                       |
| `border`, `input`, `ring`                                    | Outlines, input outlines, the focus ring          |
| `ansi-black` … `ansi-white`, `ansi-bright-black` … `ansi-bright-white` | The 16 terminal colours             |

These names follow [shadcn/ui](https://ui.shadcn.com/docs/theming).

### Colours a theme may add

`chart-1` to `chart-5` (series colours) and the `sidebar`, `sidebar-foreground`,
`sidebar-primary`, `sidebar-primary-foreground`, `sidebar-accent`,
`sidebar-accent-foreground`, `sidebar-border` and `sidebar-ring` set (a sidebar's own surface
and accents). A layout using one a theme doesn't define gets no colour from it.

### Contrast

For each text colour and the surface it is drawn on — `foreground` on `background`,
`card-foreground` on `card`, `primary-foreground` on `primary`, and so on — aim for at least
**3:1**. A theme file with a pair below that still loads; tauler logs a warning naming the
pair. Every built-in theme clears it.

## Using theme colours in a layout

A colour name works in any colour utility:

```jsx
<div class="bg-card text-card-foreground border border-border rounded-lg">
  <span class="text-muted-foreground">Memory</span>
  <div class="bg-ansi-red w-4 h-4" />
</div>
```

`rounded-<name>` reads `radius`, and `font-<name>` reads `fonts` (see
[Font roles](/docs/elements/#font-roles)). A name the theme doesn't define passes through as
an ordinary class.

### `dark:` and `light:`

A class prefixed `dark:` applies when the active theme's mode is dark, and `light:` when it
is light; the other is dropped.

```jsx
<span class="text-sm light:font-semibold">12:45</span>
```
