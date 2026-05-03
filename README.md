# tauler

A JSX-based Linux status bar renderer. Layout is written as a `.jsx` file that returns a tree of `<panel>` nodes; tauler evaluates it on every data tick and renders to X11 windows.

## The rendering model vs React

tauler uses JSX syntax but the execution model is deliberately simpler than React.

**React** is incremental: components have local state, effects manage subscriptions, and re-renders are triggered by state or prop changes. The framework tracks what changed and re-renders the minimum necessary subtree.

**tauler** is a pure function called on every tick:

```
(all stream values) → UI tree
```

There is no component state, no effects, no virtual DOM diffing, no lifecycle. The entire `_render()` function runs from top to bottom on each tick and produces a fresh layout tree. This is closer to a spreadsheet than to React.

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

In React, sharing data between unrelated parts of the tree requires context providers, prop drilling, or external state managers.

In tauler, all stream values are computed at the top of the render function and are in scope everywhere — including inside Module render-prop callbacks:

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

The pure-function model cannot express state that persists *across ticks* — for example "this workspace received a notification since you last visited it." That kind of memory lives in the data sources themselves (a module process that tracks read/unread state), not in the render function.
