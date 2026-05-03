# Structured Unix Philosophy

## Core idea

tauler's internal message-passing should follow a "structured unix philosophy": every
message is a typed Rust value that is always round-trippable to and from JSON. The wire
format at process boundaries is JSONL (one JSON object per line), but internally no
serialization happens — components communicate over typed channels.

This gives:

- **Internal type safety** — no stringly-typed routing inside the Rust runtime
- **Zero serialization cost** on the hot path between internal components
- **Trivial externalization** — any internal channel can be bridged to a TCP connection,
  a subprocess, or a Unix socket by inserting a single serialize/deserialize step at the
  boundary, without changing either endpoint
- **Composability** — because the wire format is JSONL, standard tools like `jq` can be
  inserted between two components to transform or translate messages, the same way shell
  pipelines work with text but with structure preserved

## Why "less strict" than classical Unix

Classical Unix pipes everything as byte streams / line-oriented text. That maximizes
tool interoperability but forces constant parsing. This design keeps the spirit — every
connection point is a defined, inspectable, transformable interface — while allowing
structured data to flow instead of raw strings. It is unix philosophy with JSON as the
common substrate instead of newline-delimited text.

## What this enables in practice

- An internal data source (e.g. i3 workspace events) can be moved to an external process
  communicating over TCP with no changes to consumers
- A `jq` transformation can be spliced between a data source and a consumer to adapt
  the shape without writing Rust code
- Any internal stream can be trivially logged, replayed, or mocked by capturing/injecting
  JSONL at a boundary
- Future web API surfaces (WebSocket, HTTP SSE) just add a serialization adapter at the
  edge — the rest of the system is unchanged

## Current state

tauler does not follow this yet. The current `StreamItem` type carries a `line: String`
which is raw JSONL text produced by external processes — deserialization happens
inconsistently and only in some consumers. Internal components (e.g. file watchers,
layout reload signals) use ad-hoc channel types with no defined JSON shape. There is no
explicit concept of a "boundary" where serialization is expected to occur.

The internal streaming system needs to be reworked (tracked as task #27) to introduce
typed message types with `serde::Serialize + serde::Deserialize` bounds, and to push
JSONL serialization exclusively to the edges where external processes or network
connections are involved.
