# Data stays plain JSON; Accessors point at it

Data in a layout file is whatever JSON the module emitted — an array, an object, a number.
There is no dataframe, cube or table type. Components are told which part of it to use with
**Accessors**: `y="usage"` for the common case, `y={r => r.load * 100}` when a value has to
be derived.

## Why not a dataframe

Issue #367 asked for one, so its absence is the part that needs explaining.

Splitting data along a dimension is the only job a dataframe would uniquely serve, and that
turns out not to be a property of the data. Nobody splits weather readings by temperature;
someone picking a weekend destination might split them by `rain > 0`. Split-worthiness lives
in the question being asked, not in the fields — so the split has to be an expression. Once
it is an expression, grouping an array by a function is a three-line `reduce`, and there is
nothing left for a dataframe to hold.

Plain JSON also keeps `Array.sort`, `.filter` and `.map` working. A type with an internal
shape would have had to re-provide every one of them, and whichever one it forgot would
strand the user with no way out.

And it keeps what a module prints identical to what the layout file sees, so `cat` is a
debugger and a shell one-liner is a valid module.

## Consequences

**Function accessors cannot reach Rust.** `UiComponent::js_fn` deserializes props with
serde, and a JS function has no serde representation — it would fail the whole props object,
not just that one prop, taking the component's render down with it. A Display component
written in Rust therefore needs a JS shim to resolve its accessors before Rust sees them:
the same split ADR 0003 describes for `<I3Layout>`.

**`DataTable` already spells it `key`.** `columns={[{key: "service", label: "SERVICE"}]}` is
an accessor under another name, and predates this decision. Left as it is for now; it should
converge on the term.

**A fourth component kind is expected but not declared.** Something that renders one copy of
its child per value along a dimension — `<Stack along={r => r.core}>` — is likely; it is
called *facet* in ggplot and Vega-Lite. It is deliberately absent from `CONTEXT.md` because
nothing implements it, and plain `.map()` covers flat fan-out today. Grouping is the only
part JS makes awkward, and that is the part such a component would earn its keep on.

**Units and thresholds are missing from the model.** Every bar widget formats a number and
colours it by range; Grafana makes both first-class and tauler has neither, so users
hand-write ternaries. Identified while surveying prior art for this decision, and out of
scope for it.
