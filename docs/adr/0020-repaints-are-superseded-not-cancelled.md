# Repaints are superseded, not cancelled

A panel repaint is rasterized on a render worker rather than on the tick thread. When new
data arrives for a panel that is already being drawn, the drawing runs to completion and
the newer request is drawn after it. Nothing is ever abandoned part-way.

"Cancelling a render" therefore means exactly one thing here: replacing a request that has
not started yet. That happens in the worker's drain — it takes everything queued, keeps the
newest request per panel, and discards the rest.

## Why

takumi's `render` is a single opaque call. There is no cancellation token, no progress
callback, and no point inside it where tauler regains control. A render can only be stopped
by killing what runs it, and Rust has no safe thread kill — it would take a subprocess, for
work that is already finished by the time the plumbing is in place.

What that constraint buys is worth more than what it costs. The two failure modes an
interruptible renderer has to defend against cannot arise:

- **Starvation.** Updates arriving faster than the panel can be drawn still produce frames,
  because every started render finishes. The rate settles at one render per render, and the
  data drawn is always the newest snapshot available when that render began.
- **Renders cancelling each other forever.** There is no cancellation to loop on. The queue
  holds at most one request per panel, and nothing a render does enqueues another one —
  wallpapers, the only thing whose painting invalidates panels, are painted on the tick
  thread.

The cost is wasted CPU: a superseded render draws pixels nobody sees. That is bounded by
one render per panel, and it is why the worker also holds a floor on how often a panel may
be redrawn (`MIN_REPAINT_INTERVAL`) — a delay, never a drop.

## Consequences

Only repaints leave the tick thread. `Create` needs pixels before its window exists,
`Resize` must land with a correctly-sized frame, and a wallpaper has to be published before
the panels that sample it are rendered against it. All three still rasterize inline.

Because a repaint's pixels arrive later than the reconciliation that asked for them, a frame
can reach the presenter for a panel that has since been resized. The presenter drops any
frame whose dimensions disagree with the window's — safe precisely because the resize
repainted synchronously, so what is on screen is already newer than the frame being
dropped. That invariant is load-bearing: any future path that changes a panel's size without
repainting it inline would leave a panel blank.

Two channels run from the pipeline: lifecycle commands to the presenter, repaint requests to
the worker. Sending everything through the worker would restore a single writer and with it
a deterministic order, but a `Move` would then wait behind whatever the worker happens to be
drawing.

The wallpaper crop travels inside the request, cropped on the tick thread. The worker never
reads the wallpaper registry, so it cannot draw against a wallpaper newer than the one
`SurfaceState` recorded it drawing against.

Renders now run concurrently, which is why the render context sits behind an `Arc` a render
snapshots rather than a lock a render holds. A font reload replaces the context
copy-on-write instead of mutating one that a render in flight is reading.
