---
title: Units
description: Declaring things outside the bar — lights, themes, modes — and letting tauler make the world match.
---

A layout file usually describes a bar. A **Unit** lets it describe something else: a light
that should be on, a theme a program should be using, a mode a machine should be in. You
say what should be true; tauler keeps looking at the world and does what it takes.

```jsx
const Light = unit({
  key: (light) => light.entity,
  value: (light) => light.state,
  reconciler: optativeSet({ observe: () => currentLights() }),
  enterOne: (light) => turnOn(light),
  updateOne: (light) => turnOn(light),
})

export default function render() {
  return (
    <root>
      <Light entity="light.desk" state="on" />
      <panel id="bar" anchor="top" width={1920} height={32}>…</panel>
    </root>
  )
}
```

`unit()` returns a component. Using it — `<Light entity="light.desk" state="on" />` — declares
one **Item**: one thing that should exist, with one desired value. Items sit under `<root>`
alongside the panels and draw nothing.

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
const Light = unit({
  refreshInterval: 5000, // ms; 5000 is also the default
  …
})
```

That interval is how quickly a change made outside tauler — someone hitting the physical
switch — gets undone. It is also your blast radius: a Unit that can never converge, because
its hook is failing or because it declares `state: true` where the world says `"on"`,
retries exactly this often and no faster. Short enough to feel immediate, long enough not
to hammer whatever `observe` talks to.

:::caution
`observe` should report only the Items this Unit manages. An `observe` that lists every
light in the house will hand `exit` every light you did not declare.
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

## Worked example: a Home Assistant light

Home Assistant's REST API is two calls — `GET /api/states` to see, `POST
/api/services/light/turn_on` to act — so a Unit for it is short.

First, keep the token out of the process table. Anything passed as an argument is visible
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
