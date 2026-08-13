# Gaps are logical pixels, passed to i3 unconverted

Gap values travel from the layout file to i3 with no DPI scaling applied. This looks like a
missing conversion — it is not. i3's gap unit is already the logical pixel, the same unit a
layout file is written in, so any scaling on the way in would have to be undone on the way
out.

## Why

i3's `cmd_gaps` opens with `logical_px(atoi(value))` (`src/commands.c`), and `logical_px`
is `ceil(dpi / 96 * value)` above a 1.25 DPI threshold and the identity below
(`libi3/dpi.c`). i3 scales the number itself.

There is no physical-pixel path to opt into. i3's command grammar accepts a trailing `px`
keyword, but it is discarded, and `cmd_gaps` takes no unit parameter. This was checked
directly against i3's source rather than assumed.

## Consequences

We learned this the expensive way. An earlier version scaled gaps by the device pixel
ratio on the way out, which i3 then scaled again — and separately, two DPR conversions
elsewhere in the pipeline cancelled each other, so the bug was invisible on
already-created workspaces and only appeared on new ones. It survived a round of "looks
correct to me" and was only settled by a reboot.

So: exactly one component owns this scaling, and it is i3. Any DPR arithmetic reappearing
on the gaps path is a bug, not a fix. The same rule is why `<Panel size>` and `<panel
width>` are logical too — one unit for every length a user writes.
