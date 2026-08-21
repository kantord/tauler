# Lifecycle hooks run in a second runtime, on the reconciler thread

On a native build there are two QuickJS runtimes. The render runtime evaluates the layout
file on every Tick and is pure: it produces a tree, and it ignores the Items in it. The
reconciler runtime lives on its own thread, loads the same layout file, evaluates it again,
and collects the Items out of *its own* tree — where a Unit is still an object with callable
hooks. Nothing about a Unit crosses between the runtimes.

`sh`, `read`, `ls` and every other builtin that touches the world are registered only in the
reconciler runtime. In the render runtime they do not exist.

Each runtime walks the same tree shape and keeps only the half it is responsible for. The
render side strips every Item before the layout is parsed; the reconciler side ignores every
panel, button and span. Neither half is privileged, and nothing about a Unit crosses between
them.

What does cross, one way only, is the data the layout is evaluated against: the render loop's
Stream values and its `globals`. `globals` is read-only on the reconciler side — a hook that
assigns to one is trying to report, and hooks do not report
([0035](0035-observation-is-the-truth-channel.md)).

## Why

The reason is an execution budget, not a difference in kind. tauler already reconciles: five
`Lifecycle` impls drive `OptativeSet`s for Surfaces, watched paths, built-in sources and
traces, and a Unit's Items are the sixth. What separates them is that `Surface::enter` is
bounded Rust that sends a command, and `Light.enter` is the user's JavaScript shelling out
for as long as it likes. Arbitrary code with I/O cannot sit on the thing that has a frame
budget; everything else about the two is the same machinery.

Given that it must run elsewhere, `rquickjs::Ctx` is not `Send`, so "elsewhere" means its
own runtime. A hook cannot be both arbitrary JavaScript from the layout file and run
somewhere other than the runtime that parsed it. Something had to give, and there were two
candidates.

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

**A Unit needs no identity at all.** An earlier draft had the render side ship a Unit id and
the reconciler side look the object up by it. That cannot work: `unit()` draws its
`__estoId` from a process-global `AtomicU32`, so two runtimes in one process number the same
Unit differently — and inventing a name instead would have been a thing two layout files can
collide on. Collecting reconciler-side costs nothing extra, because that runtime was already
evaluating the file, and it makes identity the object itself.

**The two runtimes can disagree about what was declared.** They evaluate at different
moments, so a Unit conditional on a Stream value can be present in one tree and absent from
the other. Only the reconciler's answer decides what gets reconciled; the render side's is
about what gets drawn. Feeding the reconciler the render loop's Stream values narrows the
window but does not close it, and a Sweep that acts on a tree one wake old is the normal
case, not a bug — the next Sweep sees the newer one.

**Props still have to survive JSON**, because a batch is serialised into the hook call. A
prop holding a closure reaches neither side.

**A thread, not a process — for now.** A process would give crash isolation, a stuck hook
that can be killed, and one binary serving both `--dry-run` and production. A thread gives
none of those, and gives `globals` and Stream values for free as an `Arc` rather than as a
protocol. The thread satisfies the actual constraint — get slow JavaScript off the frame loop
— and the process buys robustness no failure has yet demanded. A hook that takes the bar
down, or an `sh` timeout that proves insufficient, is what should change this.

**One thread for now, so Units sweep serially.** A 40-second hook delays every other Unit's
Sweep behind it. The bar does not stall — the render loop never waits on the reconciler
thread — so the symptom is "converged late", not "frozen". Going to a thread per Unit is
invisible from JavaScript, so it waits for evidence rather than for a guess; the cost when
it comes is a runtime and a set of top-level side effects per Unit.

**A hook holds its Unit until it returns.** There is no cancellation, because a hook that
takes 40 seconds is legitimate — and because a stuck hook is blocked in a subprocess inside
Rust, not in the interpreter, so `rquickjs`'s interrupt handler never gets a turn. What
exists instead is a watchdog thread that says so, repeatedly, once a Sweep has been running
for a minute. It fixes nothing; it turns "nothing converges and nobody knows why" into a log
line. The realistic cause is a subprocess with no deadline, which is `sh`'s problem to solve
and lives upstream.

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
