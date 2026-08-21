# An unusable theme file stops startup, not a reload

`theme.file` names the palette a bar is drawn in. When the file is missing, unreadable or
not valid YAML, tauler used to log a warning and draw with the shipped default instead.
It no longer does. At startup an unusable `theme.file` ends the process with the reason on
stderr; on a config reload it is reported and the palette already on screen stays.

The two halves are deliberately different, and the difference is the whole decision.

## Why not fall back

The shipped default is chroma 0 on every colour token. Substituting it does not produce a
bar that looks broken — it produces a grey bar that looks designed. A typo in the path, a
file that moved, a bad indent, all render as a deliberate monochrome rice, and the only
thing that says otherwise is a `warn!` in a log nobody has open while their bar is running.

A fall back is a recovery when the caller had no preference. Here the caller stated one.
Drawing a different palette than the one asked for is not recovering from the failure, it
is hiding it in the one place a status bar is guaranteed to be looked at.

## Why startup exits and a reload does not

Startup has nothing to keep. There is no palette on screen, no bar the user is reading, and
the process was launched from a terminal that is still there to print to — so exiting is
both the loudest available signal and the cheapest one to act on.

A reload has all three, and its trigger is a file write. Every save of a theme file fires
one, including the saves an editor makes part-way through writing the file, which are
invalid YAML for as long as it takes to finish. Exiting there would mean a bar that dies
while its own theme is being edited — a worse failure than the one the rule is meant to
prevent, and one that arrives during the exact activity the feature exists for.

So a reload keeps the last theme that loaded and logs at `error!`. That is still never a
silent substitution of the default: the palette on screen is one the user wrote, and the
next successful save replaces it.

## Consequences

`load_theme` returns a `Result` and knows nothing about which path it is on. The two
policies live in the callers — `load_theme_or_exit` for the three `App` constructors,
`theme_after_reload` for `handle_layout_reload` — so the asymmetry is stated in the names
rather than buried in a branch.

A config that names no `theme.file` at all is not a failure and never was. Reading the
config and reading the theme are separate calls (`theme_selection_from_config`, `load_theme`)
because only the first can tell those two cases apart.

Theme mode and the watched theme path come from `config.yaml`, not from the theme file, so a
reload applies both even when the theme itself failed to load. Otherwise fixing a broken
path by pointing `theme.file` somewhere new would leave the watcher on the old one, and the
fix would never fire a reload.

An image a layout file references still fails silently — a missing file and one the `image`
crate cannot decode both render as nothing. Same class of bug, different call site
(`preload_layout_images`), and not covered by this decision.
