# Reserve space with i3 gaps, not EWMH struts

The obvious way for a bar to keep tiled windows out of its way is `_NET_WM_STRUT_PARTIAL`.
tauler cannot use it, so it reserves space by telling i3 its gaps instead — an explicit
declaration in the layout file rather than something inferred from a surface's geometry.

## Considered options

**EWMH struts.** Rejected for two independent reasons, either of which is fatal on its own.
Panels are override-redirect windows, which tells X11 the window manager must not manage
them at all — tauler needs that, because it places panels itself, per-output and DPR-aware,
but an unmanaged window's properties are never read. And i3 would not honour them anyway:
it recognises only `W_DOCK_TOP` and `W_DOCK_BOTTOM` (`include/data.h`) and classifies a
dock by its top/bottom struts alone (`src/manage.c`). There is no left or right dock, so a
full-height sidebar — the most common bar there is — cannot reserve space this way even in
principle.

**Dropping override-redirect and becoming a managed dock.** This was considered and
scrapped: it would surrender per-output placement, and the missing left/right dock means it
would only ever have worked for top and bottom bars.

## Consequences

Reserving space is a decision the layout file states, not a consequence of panel geometry
that tauler can compute. That is why `gaps` is declared per side and never derived, and why
`<I3Layout>` exists — writing sizes and reservations separately is easy to get wrong and
hard to notice, since a stale gap just leaves dead space or lets windows slide under a bar.

The strut-setting code was written, found to be dead, and removed. Do not add it back
without re-reading the two reasons above.

This binds tauler's reservation story to i3. A Wayland backend gets it for free from
wlr-layer-shell's exclusive zone, which needs none of this.
