# Repaints are superseded, not cancelled

One thread rasterizes. Everything else asks it for pictures: a repaint is a request the
worker draws when it gets to it, and the renders the pipeline cannot continue without —
a window's first frame, a resize, a wallpaper paint — are the same request with a reply
channel attached, drawn ahead of anything pending while the caller waits.

The worker holds one slot per render target: the latest frame nobody has painted yet.
Asking twice for the same target overwrites the slot. That overwrite is the whole of what
"cancelling a render" means here — a request that has not started is replaced, and one
already being drawn runs to completion.

## Why

takumi's `render` is a single opaque call. There is no cancellation token, no progress
callback, and no point inside it where tauler regains control. A render can only be stopped
by killing what runs it, and Rust has no safe thread kill — it would take a subprocess, for
work that is already finished by the time the plumbing is in place.

What that constraint buys is worth more than what it costs. The two failure modes an
interruptible renderer must defend against cannot arise:

- **Starvation.** Updates arriving faster than a target can be drawn still produce frames,
  because every started render finishes. The rate settles at one render per render, and
  what gets drawn is always the newest snapshot available when that render began.
- **Renders cancelling each other forever.** There is no cancellation to loop on. The
  scheduler holds at most one request per target, and nothing a render does enqueues
  another.

The cost is wasted work: a superseded request was cropped for nothing. That is bounded by
one crop per target, and the worker also holds a floor on how often a target may be
redrawn (`MIN_REPAINT_INTERVAL`) — a delay, never a drop, because the request stays in its
slot until it is drawn.

**One rasterizer, not two.** The blocking cases could have stayed on the tick thread, which
is where they used to run. They did not, because a second rasterizer means a second frame
cache, or a shared one behind a lock. The tick thread blocks on these renders either way;
the only thing that changed is which thread holds the pixels while it does.

## Consequences

The frame cache is the worker's own state (`src/render/cache.rs`), not a process-global. No
lock around it, and nothing that has no business drawing can evict from it. `render_frame`
and friends draw unconditionally now — deciding *not* to draw is the cache's job. A font
reload therefore has to tell the worker to forget what it has, rather than reaching into a
static.

Because a repaint's pixels arrive later than the reconciliation that asked for them, a frame
could reach the presenter for a target that has since been resized. Two things prevent it: a
blocking render drops whatever was pending for that target, and the presenter drops any
frame whose dimensions disagree with the window's. The first makes it structural; the second
is there because the invariant is quiet when it breaks.

Two channels run from the pipeline: lifecycle commands to the presenter, render jobs to the
worker. Sending everything through the worker would restore a single writer and with it a
deterministic order, but a `Move` would then wait behind whatever is being drawn.

The wallpaper crop travels inside the request, cropped on the tick thread. The worker never
reads the wallpaper registry, so it cannot draw against a wallpaper newer than the one
`SurfaceState` recorded it drawing against.

The render context stays global and behind an `Arc` a render snapshots, because hit-testing
measures text on the tick thread and must not wait out a 90ms draw.

**Where ordering will come from.** Targets are drawn in a deterministic but arbitrary order,
by id, which holds only while every target is independent. `<BufferBoundary>` (#395) makes
one target's pixels an input to another's, and the order stops being arbitrary — a boundary
has to be drawn before whatever composites it, exactly as a wallpaper is painted before the
panels that sample it. That belongs in the worker's choice of which pending target to draw
next, and nowhere else.
