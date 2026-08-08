# tauler

A status bar for Linux whose layout is written as a `.jsx` file. The file returns a
tree of `<panel>` nodes; tauler evaluates it on every data tick and renders the result
natively into X11 windows or Wayland layer surfaces.

Data comes from ordinary subprocesses that write to stdout, so anything you can script
can drive the bar. Both the layout and the config are hot-reloaded on save.

📖 **[Documentation](https://kantord.github.io/tauler/)** — including a
[gallery of built-in components](https://kantord.github.io/tauler/components/).

## Requirements

- Linux, running X11 or Wayland (tauler picks the backend from `WAYLAND_DISPLAY`;
  override with `TAULER_BACKEND=x11` or `TAULER_BACKEND=wayland`)
- Rust 1.95 or newer
- fontconfig at runtime — `fc-list` and `fc-match` are used to resolve font families

Build dependencies on Debian/Ubuntu:

```sh
sudo apt-get install -y libwayland-dev libxkbcommon-dev libfontconfig-dev libxcb1-dev pkg-config
```

## Install

```sh
cargo install tauler
```

Optionally, the data source binaries:

```sh
cargo install tauler-i3      # i3/sway workspaces
cargo install tauler-notify  # desktop notifications
```

## Quick start

Write `~/.config/tauler/layout.jsx`:

```jsx
export default function render() {
  const time = useStringStream("/bin/sh", "while true; do date +%H:%M; sleep 1; done");

  return (
    <root>
      <panel anchor="top" height={32} width={ctx.screen_width}>
        <container tw="flex h-full w-full items-center justify-end px-3">
          <text tw="text-[13px] text-white">{time}</text>
        </container>
      </panel>
    </root>
  );
}
```

Then run `tauler`. Editing the file re-renders the bar immediately — no restart.

The module has to `export default` a render function. `ctx` is injected before each
evaluation and carries `output`, `dpi`, `screen_width` and `screen_height`. Styling
uses Tailwind-style classes in the `tw` prop.

`~/.config/tauler/config.yaml` is optional and holds the theme and font settings:

```yaml
theme:
  mode: dark        # or light
fonts:
  primary: "Inter"
```

## Companion crates

| crate | what it does |
|---|---|
| [`tauler-i3`](https://crates.io/crates/tauler-i3) | i3/sway workspace state and click-to-switch |
| [`tauler-notify`](https://crates.io/crates/tauler-notify) | a freedesktop notification daemon that feeds the bar |
| [`tauler-screenshot`](https://crates.io/crates/tauler-screenshot) | renders a layout to a PNG through the same pipeline as the bar |
| [`tauler-ui-macro`](https://crates.io/crates/tauler-ui-macro) | proc macro for writing built-in components in Rust |

## The rendering model vs React

tauler uses JSX syntax but the execution model is deliberately simpler than React's.

**React** is incremental: components have local state, effects manage subscriptions, and
re-renders are triggered by state or prop changes. The framework tracks what changed and
re-renders the minimum necessary subtree.

**tauler** is a pure function called on every tick:

```
(all stream values) → UI tree
```

There is no component state, no effects, no virtual DOM diffing, no lifecycle. The
entire render function runs from top to bottom on each tick and produces a fresh layout
tree. This is closer to a spreadsheet than to React.

### Streams replace useState + useEffect

In React you'd subscribe to external data with `useEffect` + `useState`:

```jsx
const [time, setTime] = useState('');
useEffect(() => {
  const id = setInterval(() => setTime(new Date().toISOString()), 1000);
  return () => clearInterval(id);
}, []);
```

In tauler you declare the data source inline and get the latest value synchronously:

```jsx
const time = useStringStream("/usr/bin/bash", "while true; do date; sleep 1; done");
```

The runtime manages the subprocess lifecycle. You never write subscription or cleanup code.

### Joining streams is just closures

In React, sharing data between unrelated parts of the tree requires context providers,
prop drilling, or external state managers.

In tauler, all stream values are computed at the top of the render function and are in
scope everywhere — including inside Module render-prop callbacks:

```jsx
const notifications = useJSONStream("...tauler-notify")?.notifications ?? [];

<Module bin="...tauler-i3">
  {(data, events) => {
    // notifications is in scope here — no context, no prop drilling
    const urgent = notifications.some(n => data.workspaces.find(...));
    return <WorkspaceList urgent={urgent} />;
  }}
</Module>
```

### When you would need explicit state

The pure-function model cannot express state that persists *across ticks* — for example
"this workspace received a notification since you last visited it." That kind of memory
lives in the data sources themselves (a module process that tracks read/unread state),
not in the render function.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

### Third-party assets

`assets/fonts/inter/InterVariable.ttf` is the [Inter](https://github.com/rsms/inter)
typeface by The Inter Project Authors, licensed under the SIL Open Font License 1.1
(see `assets/fonts/inter/LICENSE.txt`). It is used only to render documentation
screenshots and is not part of the published crates.
