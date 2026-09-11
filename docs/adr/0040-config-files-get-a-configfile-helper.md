# Config files get a `ConfigFile` helper, not a new primitive

ADR 0033 named this and stopped: "Writing a config file is a Unit like any other, but a
Unit whose value is a whole rendered file wants a serialiser, and that is a separate
feature with its own decisions." This is that feature. The decision is that it needs no
new builtin and no per-format grammar — only the boilerplate factored out of writing that
Unit by hand, as a global helper next to `unit()` itself.

```jsx
globalThis.ConfigFile = ({ path, render, apply }) => {
  function write() {
    const rendered = render();
    sh`printf '%s' ${rendered} > ${path}`;
    if (apply) apply(path, rendered);
  }
  return unit({
    key: () => path,
    value: (f) => ('content' in f ? f.content : render()),
    reconciler: optativeSet({
      observe: () => (exists(path) ? [{ content: read(path) }] : []),
    }),
    enterOne: write,
    updateOne: write,
  });
};
```

Used:

```jsx
const RofiTheme = ConfigFile({
  path: "/home/you/.config/rofi/tauler.rasi",
  render: () => rasi({ "*": { ... } }),
});

<RofiTheme />
```

`ConfigFile` lives in `tauler_core::globals::JSX_GLOBALS_JS`, next to `Module`, `I3Layout`
and `useEvents` — every one of them the same bargain: no Rust changed, a global function
composes builtins that already exist. `path` is the key; `render()` computed fresh is the
declared value; `observe` reading the file back is the world's answer; a mismatch writes.
`apply`, if given, runs once after every write with the path and the text just written —
the hook a target that keeps running needs and rofi does not, since rofi reads its file
fresh on every launch.

## Why

**Writing a file needs no new builtin.** The obvious gap is that `optative-script`
registers `sh`, `read`, `ls`, `exists`, `hash` (ADR 0034) but no `write` — the missing
half of `read`. It does not need one: `sh`'s tagged template quotes every interpolated
value as a single shell *argument* (wraps it in `'...'`, escaping embedded `'` as
`'\''`), and `printf '%s' ARG > PATH` uses the rendered text as exactly that — an
argument, not a heredoc body. That quoting already handles quotes, backslashes, `%` and
newlines in the content correctly, because it was written for shell-injection safety, and
file-content safety is the same problem. A dedicated `write` builtin would only save one
`sh` call, and it would live in `optative-script`, a separately-versioned, separately-
released crate — real cost for a call this codebase can already make.

**`apply` cannot be an Item prop.** The obvious shape is `<ConfigFile path={p}
content={c} apply={reload} />`, one shared Unit, many declared Items. It does not work:
a batch is serialised into JSON before a hook is dispatched (ADR 0034), and a function
does not survive that crossing — `apply` would read back `undefined`. `ConfigFile`
sidesteps it by being a *factory*: each call returns a fresh `unit()`, and `apply` is a
variable `write` closes over, never a prop. The cost is one Unit per file rather than one
Unit shared across every file a layout declares, which is the same shape `WindowPlacement`
and `Light` already have — one Unit, many Items — traded for the one thing this case
needed that they do not.

**`value` tells declared from observed by shape, not by a flag.** Both sides call the
same `value`, so it has to answer two different questions from one argument. `observe`
always returns `{content}`; a declared `<RofiTheme/>` never has that property, because it
takes no props at all. `'content' in f` is exact for that reason, not a heuristic that
happens to work today.

**The grammar the issue asked for is not tauler's to design.** `rasi()` is a twelve-line
function from a JS object to `.rasi` text; an INI file wants different twelve lines, a
YAML file different twelve again. Making that generic — a "simple tree model" every
format serialises from — is a userland library problem with no tauler concept underneath
it. `ConfigFile` takes `render` as a plain function precisely so it stays out of that
decision: whatever `render()` returns is what gets written, and nothing about the helper
cares how.

**Reload is `apply`, and rofi's absence of one is not a special case.** A target that
runs as a daemon passes `apply: () => sh\`...\``; rofi passes nothing, because there is
nothing to signal. The debounce the issue imagined is not `ConfigFile`'s problem either —
it is a property of whatever `apply`'s command does, the same as any other Unit's hook.

## Consequences

**No new CONTEXT.md term.** `ConfigFile` is not a new domain concept the way `Module` is
— it names no new relationship between tauler and the world, only a common shape of
`unit()` call. ADR 0033 already says a new kind of reconcilable thing needs no tauler
change; this is a convenience over that kind, not another one.

**A reader who wants a `write()` builtin has been here before.** This ADR is that answer:
the gap is closed by composing `sh` with `printf`, and adding one would move a general
capability into a crate release cycle for no reason.

**One Unit per `ConfigFile()` call, always.** There is no version of this helper where
many declared files share one Unit and one `observe`, because `observe` has no way to
learn which paths matter without either an unbounded filesystem walk or a second
registration mechanism this ADR did not need to build.

**Binary or huge files are out of scope, by the same reasoning `sh`/`read` already
carry.** `read` is `read_to_string`; content that is not valid UTF-8 was never on the
table, and a shell argument has a length ceiling (`ARG_MAX`) a hand-rolled theme file will
not reach.
