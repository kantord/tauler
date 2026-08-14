# A rice fixture is a dotfiles repo

A Rice demonstrates a desktop, and a desktop is configured by more programs than tauler.
So a rice's fixture is a `home/` tree — `~/.config/tauler`, `~/.config/i3`,
`~/.Xresources`, `~/.local/bin`, and whatever else the rice configures — copied over
`$HOME` before the session starts, rather than a layout file with a few settings smuggled
in beside it. Contract scenarios keep the one-file shape; the entrypoint understands both.

It is *shaped* like a dotfiles repo. It is not one you could install. The rofi theme
hard-codes `/root/.config/rofi/plate.png` because rofi will not expand `~` inside a
`url()`, kitty's config hard-codes its plate for the same reason, and every data module
emits frozen JSON so that two runs photograph the same. Making one of these genuinely
installable is worthwhile and is not what this decision claims.

## Why not one shape for everything

Two shapes is a cost, and the alternative was to convert all six fixtures. It was
rejected because a contract scenario's fixture being one file is the claim it makes: a
tauler desktop needs one file. Wrapping `sidebar`'s twelve-line layout in four levels of
directory would have said the opposite, in the two fixtures a reader is most likely to
open first.

`showcase` is the loose end. It is a Rice by the glossary — its point is how it looks and
it carries an i3 config of its own — but it predates this and still uses the flat shape.
That is deliberate for now rather than an oversight: it is the fixture most often copied
from, and it was left alone while the shape was being proved on three new ones. Converting
it is the obvious follow-up.

## What the shape bought

The restructure was expected to be tidying and turned out to be load-bearing.

`theme.file` had been an absolute path into the harness's bind mount, because there was no
`~/.config` to point at. It is now `~/.config/tauler/theme.yaml`, which is what a real user
would write.

Terminals are *configured* rather than invoked. Font, palette, border and
pseudo-transparency moved from urxvt command-line flags in `startup` into `~/.Xresources`,
so `startup` says only which window it is and what to run in it.

And giving dunst, rofi and kitty somewhere to keep a config meant they had to be run —
which is how three of the source brief's open questions stopped being questions. They are
now on screen beside tauler's own versions of the same surfaces, so the comparison is a
screenshot rather than a claim.

## Consequences

`~/.local/bin` is on `PATH` in the image, and a module's `bin` now expands a leading `~/`
the way `theme.file` always has — otherwise a layout file could not name its own scripts
and the tree would be a costume.

The `home/` tree is copied, not symlinked: the fixture side of the bind mount is not
writable and mode bits on `~/.local/bin` have to survive.

`.Xresources` is merged with `-nocpp`. xrdb pipes its input through the C preprocessor by
default, and an apostrophe in a `!` comment then warns on every session start.

This supersedes the second half of [ADR 0015](./0015-the-showcase-is-a-scenario.md)'s
consequences. A fixture's own `i3.config` and `startup` were introduced there as two
escape hatches; for a rice, the i3 config is now simply one of the files in its home
directory, and `startup` is the only thing left that is a harness hook rather than a
dotfile. The warning in 0015 about per-side outer gaps still applies wherever that config
lives.
