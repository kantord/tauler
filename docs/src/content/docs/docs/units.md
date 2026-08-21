---
title: Units
description: Declaring things outside the bar — lights, themes, modes — and letting tauler make the world match.
---

A layout file usually describes a bar. A **Unit** lets it describe something else: which
workspace an app belongs on, a theme a program should be using, a light that should be on.
You say what should be true; tauler keeps looking at the world and does what it takes.

```jsx
const WindowPlacement = unit({
  key: (a) => a.class,
  value: (a) => a.workspaces,
  reconciler: optativeSet({ observe: () => currentPlacement() }),
  updateOne: (a) => moveToWorkspace(a.class, a.workspace),
})

export default function render() {
  return (
    <root>
      <WindowPlacement class="Chromium" workspace={1} />
      <panel id="bar" anchor="top" width={1920} height={32}>…</panel>
    </root>
  )
}
```

Chromium now lives on workspace 1. Drag it somewhere else and it comes back; open it after
a reboot and it lands where you said.

`unit()` returns a component. Using it — `<WindowPlacement class="Chromium" workspace={1} />`
— declares one **Item**: one thing that should be true, with one desired value. Items draw
nothing.

## The four parts

`key` names an Item. Two Items with the same key are the same thing; the key is what lets
tauler match what it sees in the world against what you asked for.

`value` is what decides "changed". If the observed value and the declared value differ, the
Item needs an `update`. Return whatever comparison you want — a string, a number, an object.

`observe` reports what the world actually holds, as an array of Items in the same shape you
declare them. This is the only thing tauler believes. A hook that says it succeeded proves
nothing; the next `observe` does.

The hooks act. Each comes in two spellings and you pick one:

| batch | per Item |
| --- | --- |
| `enter(items)` | `enterOne(item)` |
| `update(pairs)` | `updateOne(item, old)` |
| `exit(items)` | `exitOne(item)` |

The batch form is handed **all** the Items that need that transition, so a Unit that talks
to an API can make one request for ten lights instead of ten requests. Items arrive in the
order the layout declared them. `update`'s batch form gets `{item, old}` pairs, so you can
see what the world had before:

```jsx
update: (pairs) => pairs.forEach(({ item, old }) => fade(old.state, item.state)),
```

A hook you define neither spelling of is a transition you are not managing, which is fine
and costs nothing.

:::caution
Defining both `enter` and `enterOne` is an error. So is writing a per-Item hook under the
batch name — `enter: (light) => …` gets an array, and you'll be told so:

```
TypeError: `enter` receives an array of Items, not one Item. Did you mean `enterOne`?
```
:::

## What a Sweep does

One **Sweep** is: run `observe`, compare it against the Items the layout declared, call the
hooks the comparison asks for.

| the world | the layout | hook |
| --- | --- | --- |
| absent | declared | `enter` |
| present, different `value` | declared | `update` |
| present | not declared | `exit` |

Sweeps run on their own thread, off the render loop. A hook that takes forty seconds makes
its Unit converge late; it never drops a frame.

A Unit sweeps on a fixed interval. What the last Sweep did makes no difference to when the
next one runs:

```jsx
const WindowPlacement = unit({
  refreshInterval: 5000, // ms; 5000 is also the default
  …
})
```

That interval is how quickly a change made outside tauler — you dragging the window
somewhere else — gets undone. It is also your blast radius: a Unit that can never converge,
because its hook is failing or because it declares `state: true` where the world says
`"on"`, retries exactly this often and no faster. Short enough to feel immediate, long
enough not to hammer whatever `observe` talks to.

:::caution
`observe` should report only the Items this Unit manages — or define no hook for the
transitions you do not want. An `observe` that lists every window on the machine will hand
`exit` every one you did not declare, and a Unit with no `exit` quietly ignores them.
:::

## Hooks run somewhere else

The hooks and `observe` run in a second JavaScript runtime, on the reconciler thread, and
that one has a shell:

```jsx
observe: () => JSON.parse(sh`some-command --json`)
```

`sh`, `read`, `ls`, `exists` and `hash` exist there and **do not exist** while your layout
is being rendered. That is deliberate: a shell command during a render would block the bar
for as long as the command takes. If you reach for `sh` outside a hook, you get
`sh is not defined`.

The practical consequence: your layout file is evaluated twice, in two runtimes. Anything
at module top level runs once in each. Keep the expensive things inside hooks.

Units are a native-only feature. Nothing on this page applies to a browser-hosted layout.

## Worked example: apps on the right workspace

i3 can tell you where every window is and can move them, so a Unit that keeps apps on the
workspaces you want is a shell command each way.

```jsx
const WindowPlacement = unit({
  refreshInterval: 5000,

  key: (a) => a.class,
  // A class can hold several windows, so what matters is the set of workspaces it
  // occupies — declaring one number means "all of them, there".
  value: (a) =>
    [...new Set(a.workspaces ?? [a.workspace])].map(Number).sort((x, y) => x - y),

  reconciler: optativeSet({
    observe: () =>
      JSON.parse(sh`timeout 5 i3-msg -t get_tree | jq -c '
        [ recurse(.nodes[]?)
          | select(.type=="workspace") as $ws
          | [ $ws
              | recurse(.nodes[]?, .floating_nodes[]?)
              | select(.window_properties != null)
              | { class: .window_properties.class, num: $ws.num } ]
          | .[] ]
        | group_by(.class)
        | map({ class: .[0].class, workspaces: (map(.num) | unique) })
        | map(select(.class != null))'`),
  }),

  updateOne: (a) =>
    sh`timeout 5 i3-msg '[class="^${a.class}$"] move --no-auto-back-and-forth to workspace number ${a.workspace}' >/dev/null`,
})
```

Three things in there are worth pulling out, because each is a general lesson.

**`value` is a set, not a number.** Five Chromium windows share one class, so one key covers
all of them. If two are on different workspaces, `value` is `[1, 3]`, that differs from the
declared `[1]`, and one `move` consolidates them. Comparing a single workspace number would
have compared an arbitrary one of the five.

**There is no `enterOne`.** An app that is not running is declared but not observed, which is
an `enter` — and a Unit that defines no `enter` is a Unit that does not manage that
transition. Without this, quitting Spotify would make tauler relaunch it five seconds later.
A missing hook is a design decision you can make.

**`timeout 5` on both commands.** i3's IPC can fail to answer, and a hook that never returns
holds the reconciler thread for the life of the process. Put a deadline on anything that
talks to something else.

### A nicer way to say it

`unit()` returns a component, so the readable spelling is a component too — and it needs
nothing from tauler:

```jsx
const App = (p) => p
const Workspace = ({ num, children }) =>
  children.map((app) => <WindowPlacement class={app.class} workspace={num} />)
```

Which buys you:

```jsx
<root>
  <Workspace num={1}>
    <App class="Chromium" />
    <App class="Slack" />
  </Workspace>

  <Workspace num={10}>
    <App class="Spotify" />
  </Workspace>
</root>
```

`<App>` never becomes an Item — it is an inert `{class: "Chromium"}` that `Workspace` reads
and discards. `Workspace` builds the real Item one line later, copying its own `num` onto
each. There is no prop inheritance and no context; the parent constructs the child
explicitly, which is why this is two ordinary functions rather than a feature.

It also stays **one** Unit. Grouping is a spelling, not a scope, so a batch hook still gets
every Item at once however the layout arranged them.

## A Unit that talks to a network service

Home Assistant's REST API is two calls — `GET /api/states` to see, `POST
/api/services/light/turn_on` to act — so a Unit for a light is short. What it adds to the
example above is a secret.

Keep the token out of the process table. Anything passed as an argument is visible
to every process on the machine and lands in shell history and logs; a curl config file is
not:

```bash
install -m 600 /dev/null ~/.config/tauler/hass.curlrc
cat >> ~/.config/tauler/hass.curlrc <<'EOF'
header = "Authorization: Bearer YOUR_LONG_LIVED_TOKEN"
header = "Content-Type: application/json"
EOF
```

Then the Unit:

```jsx
const HASS = 'http://homeassistant.local:8123'
const MINE = ['light.desk', 'light.hall']

const hass = (path, body) =>
  body
    ? sh`curl -sfK "$HOME/.config/tauler/hass.curlrc" -d ${JSON.stringify(body)} ${HASS + path}`
    : sh`curl -sfK "$HOME/.config/tauler/hass.curlrc" ${HASS + path}`

const Light = unit({
  refreshInterval: 5000,

  key: (light) => light.entity,
  value: (light) => light.state,

  reconciler: optativeSet({
    observe: () =>
      JSON.parse(hass('/api/states'))
        .filter((s) => MINE.includes(s.entity_id))
        .map((s) => ({ entity: s.entity_id, state: s.state })),
  }),

  // A light that Home Assistant has never heard of and one whose state is wrong
  // need the same call, so both hooks are the same call.
  enterOne: (light) => apply(light),
  updateOne: (light) => apply(light),
})

function apply(light) {
  hass(`/api/services/light/turn_${light.state === 'on' ? 'on' : 'off'}`, {
    entity_id: light.entity,
  })
}
```

Used:

```jsx
<root>
  <Light entity="light.desk" state={working ? 'on' : 'off'} />
  <panel id="bar" anchor="top" width={1920} height={32}>…</panel>
</root>
```

`MINE` is what keeps `observe` honest — without it every other light in the house shows up
as an Item nobody declared.

Note there is no `exit`. Dropping `<Light>` from the layout means tauler stops managing that
light, not that it turns it off. If you want it off, declare it off.

## Driving a Unit from the bar

A Unit reads the same `globals` your layout does, so a button can change what a Unit
declares:

```jsx
<root>
  <Light entity="light.desk" state={globals.desk ? 'on' : 'off'} />
  <panel id="bar" anchor="top" width={1920} height={32}>
    <button on_click={() => { globals.desk = !globals.desk }}>desk</button>
  </panel>
</root>
```

The click updates `globals`; the next Sweep sees the new declaration and acts.

`globals` is **read-only inside a hook**. The bar owns it; a hook that assigns to it throws.
If a hook needs to record something, the thing to record it in is the world — and `observe`
is what reads it back.

## Where to put a Unit

Anywhere in the tree. An Item draws nothing, so declaring one next to the UI it drives is
the natural thing:

```jsx
function DeskLight({ on }) {
  return (
    <>
      <Light entity="light.desk" state={on ? 'on' : 'off'} />
      <button on_click={toggle}>desk</button>
    </>
  )
}
```

## When it isn't working

Sweeps log at debug level:

```bash
RUST_LOG=tauler::units=debug tauler
```

That gives you one line per Sweep with what entered, updated and exited, plus the exception
from any hook that threw.

## What survives a restart

Nothing is torn down when tauler exits or re-execs. The next Sweep after it comes back runs
`observe`, sees the world as it is, and carries on from there — which is the same thing it
does on the very first Sweep. A Unit does not need to know whether it has run before.
