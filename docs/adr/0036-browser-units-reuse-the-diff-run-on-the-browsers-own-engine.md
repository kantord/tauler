# Browser Units reuse the diff, run on the browser's own engine

A layout hosted in a page can declare a Unit too, through `defineUnit` (`tauler-web/js/units.js`)
rather than `unit()` — imperative, not JSX-declared, and without a shell. What it reconciles
with is not a second implementation: `key`, `value` and every hook are plain JavaScript,
called directly by the browser's own engine on a `setInterval`, and only the diff step itself
— declared Items against observed ones, sorted into enter/update/exit — crosses into wasm,
through `taulerReconcileUnit`. That function is `tauler_core::units_reconcile::reconcile`,
the exact code native's `sweep_unit` calls, relocated from `src/units.rs` down into
`tauler-core` so both builds reach the same one.

## Why native's two constraints don't apply here

**No second runtime, because there is no first one to protect.** Hooks run in a second
QuickJS runtime on the desktop only because `rquickjs::Ctx` isn't `Send`
([0034](0034-hooks-run-in-a-second-runtime.md)) — moving work off the render path requires a
whole other engine instance. A page already runs the layout file in the browser's own engine,
not QuickJS at all ([0027](0027-the-browsers-own-engine-runs-the-layout-file.md)), so the same
goal — a slow hook does not cost the render loop a frame — is met by not calling hooks inside
`Mount.tick()`'s call stack, i.e. a `setInterval` outside it. One engine, because one was
always enough; the second engine was a `Send` workaround, not a property of Units.

**No shell, because there is nothing native's `observe` was reaching for.** `sh`, `read`,
`ls`, `exists` and `hash` exist for a native Unit's `observe` to ask the real machine a
question. A browser Unit's `observe` was never going to ask a real machine anything — it
returns synthetic data, or data already fetched some other way. Dropping the shell is not a
capability a browser Unit is missing; it is what makes it a browser Unit.
[0035](0035-observation-is-the-truth-channel.md) still holds exactly as written: what a
browser Unit believes exists is whatever `observe` says, and nothing else.

## Why the diff moved, not was copied

`src/units.rs` lives in the root native crate, which `tauler-web` does not depend on —
only `tauler-core` is shared between the two builds. The diff itself
(`OptativeSet::with_initial_state` + `.reconcile`, sorting the result into batches) never
touched `JsxEvaluator`; the QuickJS coupling was entirely in the five calls *around* it
(`eval_units`, `call_unit_projection`, `observe`, `reconciler_kind`, `dispatch_unit_hook`).
Duplicating that diff into `tauler-web` would mean two implementations of "what does a Sweep
do" to keep in sync by hand — exactly the kind of drift `tauler-web` exists to avoid (its own
`lib.rs`: "holds no logic of its own"). So `SweepItem`, `Batches` and `reconcile` moved into
`tauler-core::units_reconcile`; `src/units.rs` keeps the QuickJS-specific orchestration and
calls the relocated function instead of defining it. Native's own tests
(`src/units.rs`'s `mod tests`) verify the move changed nothing.

## What Phase 1 does not do

Native Units are declared inside JSX — `unit()` returns a component, and an Item is a node
the reconciler runtime collects by evaluating the whole layout file again
([0033](0033-reconcilers-are-esto-units.md)). Recognising a Unit node the same way inside
whatever a browser's `render()` returns is a real, separate piece of work with no coverage to
lean on yet. `defineUnit` is imperative instead: `items` is a function returning the
currently-declared Items, called fresh each sweep the way `observe` already is. A future
JSX-declared form is a natural extension, not a redesign, if it turns out to be wanted.

## Consequences

**The batch/per-Item guard is real, not decorative.** `unit()`'s docs promise a Unit cannot
define both `enter` and `enterOne`, and that writing a per-Item hook under the batch name
throws rather than silently reading `undefined` off an array
([0033](0033-reconcilers-are-esto-units.md)'s `Proxy` trick). `defineUnit` enforces the first
at define time and reproduces the second with its own `Proxy`, so the failure mode reads the
same message in a browser as on a desktop.

**A batch hook with nothing to report is not called.** Neither native's nor a browser Unit's
`enter`/`update`/`exit` fires with an empty array — a sweep that found no exits does not tell
`exit` so.

**`observe` is synchronous for now.** A `Promise`-returning `observe` (an actual `fetch`) is
a reasonable next step and does not change anything decided here; it just is not built yet.

**Nothing here is wired into a real page.** This is the capability, verified on its own
(`tauler-core/src/units_reconcile.rs`'s tests, `docs/tests/units.spec.ts`). Whether and how a
specific page's layout uses `defineUnit` is a separate decision.
