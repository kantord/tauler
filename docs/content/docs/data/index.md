---
title: Handling dynamic data
description: Where a layout's changing values live, the four ways to reach them, and how to pick one.
---

A layout file describes what to draw. Everything that changes while Tauler runs (the time,
the volume, whether a drawer is open) is a value the layout reads from somewhere else.

## The model

A layout is a function. Tauler runs it again whenever a value it reads changes, draws
what it returns, and keeps nothing in between. A variable you set during one run is gone
in the next. Other pages call one run a tick.

So state lives in one of two places:

- **In another program.** Tauler runs the program for you and passes lines of JSON between
  it and the layout, the way a web page and a worker pass `postMessage` data. The program
  can be a shell loop, a script in any language, or one that ships with Tauler.
- **In the layout.** You declare an object that Tauler keeps for you between runs.

## Where state lives

| State lives in | The layout | Name | You call |
|---|---|---|---|
| another program | reads it | **Stream** | [`useStringStream`, `useJSONStream`](/docs/data/streams/) |
| another program | sends to it | no noun: you **dispatch** an **event** | [`useDispatch`](/docs/data/dispatch/) |
| another program | reads it and sends to it | **Service** | [`useService`](/docs/data/services/) |
| the layout | reads and writes it | **Store** | [`store`](/docs/data/shared-stores/) |

Each name, once:

- A **subprocess** is a program Tauler runs for a layout.
- A **stream** is a subprocess you read. Its value is the latest line it printed. Tauler
  also provides [streams of its own](/docs/data/built-in-streams/).
- An **event** is an object with a `type`, such as `{ type: "open", url }`. To
  **dispatch** it is to send it to a subprocess, which receives it as one line of JSON.
- A **service** is a subprocess you read and dispatch to. It holds state for you, outside
  Tauler. `const [state, dispatch] = useService(bin)` has the shape of React's
  `const [state, dispatch] = useReducer(...)`, with the reducer in another program.
- A **store** is state the layout owns. A [shared store](/docs/data/shared-stores/) is
  declared at the top level of a file. A [local store](/docs/data/local-stores/) is
  declared inside a component, and each place the component is written gets its own.

## Choose one

1. The value exists outside Tauler (the time, the CPU load, a file) and you only show it.
   [Read it as a stream](/docs/data/streams/).
2. A click must cause something outside Tauler, and there is nothing to show afterwards.
   [Dispatch an event](/docs/data/dispatch/).
3. You show a value and also change it, and other programs can change it too (the volume,
   the focused workspace). [Use a service](/docs/data/services/).
4. The value matters only to the layout (an open drawer, a selected tab).
   Keep it in a [shared store](/docs/data/shared-stores/), or in a
   [local store](/docs/data/local-stores/) when each copy of a component needs its own.
5. The value must survive a restart of Tauler. A store does not, so
   [write a program](/docs/data/programs/) that saves it, and use it as a service.

## The one rule

**A layout keeps nothing between runs.** The rest follows from it:

- The `use` functions are not React's hooks. There are no per-component state slots, no
  effects and no dependency arrays. Each call says "this run reads that program" and
  returns its latest value.
- A program is started when a run asks for it and stopped when a run no longer does, so
  the calls must not come and go. See
  [the rule on the streams page](/docs/data/streams/#rules).
- Every value is read at the top of the render function, so it is in scope everywhere
  below. Two parts of the tree share a value by reading the same variable.
- What must be remembered goes in a store or in a program.

## Related

- [Do something on a click](/docs/interaction/clicks/): handlers are where events are
  dispatched and stores are written.
- [The layout file](/docs/layout-file/#ctx): `ctx`, the screen size and theme Tauler
  passes to every run.
- [Screenshots](/docs/develop/screenshots/): streams do not run in a screenshot.
