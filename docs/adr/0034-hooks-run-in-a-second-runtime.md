# Lifecycle hooks run in a second runtime, on the reconciler thread

On a native build there are two QuickJS runtimes. The render runtime evaluates the layout
file on every Tick and is pure: it produces a tree, and the Items in that tree cross to the
other side as plain JSON — a Unit name and the Item's props, nothing else. The reconciler
runtime lives on its own thread, loads the same layout file, holds the real `unit()`
objects, and calls the real hooks.

`sh`, `read`, `ls` and every other builtin that touches the world are registered only in the
reconciler runtime. In the render runtime they do not exist.

## Why

`rquickjs::Ctx` is not `Send`. A hook cannot be both arbitrary JavaScript from the layout
file and run somewhere other than the runtime that parsed it. Something had to give, and
there were two candidates.

**Hooks as command descriptions.** Evaluation stays pure, hooks return strings, Rust runs
them on a worker. It keeps one runtime — and it cannot host `observe`. `meta.op`'s observe
is TypeScript with a memoised GitHub walk shared with the JSX descent; as a command printing
lines it is a different feature. It also forks `sh` from esto's meaning on the first line of
every hook, which throws away the reason for adopting `unit()` at all
([0033](0033-reconcilers-are-esto-units.md)).

**A second runtime.** Costs an evaluation and a runtime, and buys hooks that are ordinary
JavaScript with ordinary I/O — the same hooks that run under `esto run`.

The evaluation is not the expensive part: the layout file evaluates in about 2ms, not the
100–200μs [0007](0007-every-tick-re-renders-everything.md) claims. What is expensive is
whatever a file does at module scope, and `meta.op` runs a `cargo build` through `sh` there.

**Why the render runtime gets no effectful builtins.** An eager `sh` during a Tick blocks
the loop for as long as the command takes. `gaming_mode_exit.sh` on this desktop polls for
up to 40 seconds — which is correct behaviour for that script, and the glossary calls
anything over 1200ms **Lagged**, "a defect, never a budget." Leaving the builtins
unregistered makes that unbreakable rather than merely discouraged.

## Consequences

**The layout file is evaluated twice, in two runtimes.** Module top-level side effects run
once per runtime. A layout file that reads something nondeterministic at module scope will
have two answers to it, and nothing reconciles them.

**Items must survive JSON.** Props crossing the boundary are plain data, which is the rule
data already lives under ([0013](0013-data-stays-plain-json-and-accessors-point-at-it.md)).
A prop holding a function or a closure does not cross.

**A Unit needs an identity that survives serialisation.** The render side ships a name; the
reconciler side looks up a `unit()` object by it. Neither `esto` nor the authoring-model
sketch specifies what that name is, so tauler picks one, and it is a thing two files can
collide on.

**One thread for now, so Units sweep serially.** A 40-second hook delays every other Unit's
Sweep behind it. The bar does not stall — the render loop never waits on the reconciler
thread — so the symptom is "converged late", not "frozen". Going to a thread per Unit is
invisible from JavaScript, so it waits for evidence rather than for a guess; the cost when
it comes is a runtime and a set of top-level side effects per Unit.

**A hook holds its Unit until it returns.** There is no grace period and no timeout, because
a hook that takes 40 seconds is legitimate. A hook that never returns stops its Unit
permanently, and nothing detects that.

## What this means on the web

None of it applies. QuickJS never reaches the web
([0027](0027-the-browsers-own-engine-runs-the-layout-file.md)): a layout file is transformed
ahead of time and evaluated by the browser's own engine, so there is no second runtime to
put hooks in, no thread to run a Sweep on, and no `std::process::Command` for `sh` to be.
A Dom surface reconciles nothing.

That makes Units a native-only feature, and the reason is a property of the platform rather
than a gap someone forgot to fill. A page that wants desired-state anything has to reach a
machine that has one, which is a different feature with a different trust boundary.

It also adds to a pile 0027 already named. That ADR wants a feature flag in
`optative-script` splitting the transform from the runtime, so the web can use the oxc half
without `rquickjs`. The reconciler builtins land on the runtime side of that split, and one
of them spawns processes — which does not exist on `wasm32` at all. Whoever writes that flag
has one more thing to put behind it, and a compile error waiting if they don't.
