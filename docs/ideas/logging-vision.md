# Logging Vision

> Covers the `LogSource`, `LogStore`, and `QueryableLogStore` traits, the `KeyPath`
> identity system, and layer-level metadata attachment. Connects to `health-vision.md`
> (Supervisor aggregates log streams), `lifecycle-status-vision.md` (errors flow to log
> stream), and `pipeline-vision.md` (logs are part of the pipeline's output stream).

---

## Design goals

- Attaching a log backend (Sentry, SQLite, a log file, stdout) is trivial — implement one
  trait.
- Log entries are identifiable by source: which item in the reconciler tree produced them,
  and at what level.
- Subtree filtering is natural: "all logs from panel 'sidebar' and everything below it."
- Layer-level context enrichment is optional but composable: each Supervisor level can
  attach structured metadata to every log entry it routes, without items knowing about it.
- No mandatory storage in items — logs flow through the Supervisor, not stored by items.
- The framework never interprets log metadata — only consumers do.

---

## The `KeyPath` identity system

Every item in the reconciler tree is uniquely identified by the sequence of keys from root
to that item. This is the same path used in `KeyedError` (see `health-vision.md`) and log
source filters.

### Encoding

Each key segment is independently serialised to JSON, then base64url-encoded (RFC 4648
URL-safe alphabet: `A-Z a-z 0-9 - _`, no padding). Segments are joined by `/`.

```
key "sidebar"   → base64url('"sidebar"')   → InNpZGViYXIi
key "weather"   → base64url('"weather"')   → InB3ZWF0aGVyIg
key 42          → base64url('42')          → NDI

path: InNpZGViYXIi/InB3ZWF0aGVyIg
```

**Why this encoding:**
- `/` is not in the base64url alphabet, so it is an unambiguous segment delimiter.
- No escaping needed — keys can contain any characters internally.
- Shell-friendly: the encoded path works as a CLI argument, a URL path segment, a filename,
  and a database column without quoting or special handling.
- Human-readable debug display is a separate concern: a `Display` impl renders the
  unencoded JSON values with `/` separating them.
- Subtree filter: `path.starts_with(prefix_path)` works on the raw segment slice.

### Requirements on `Lifecycle::Key`

```rust
type Key: Clone + Eq + Hash + serde::Serialize + serde::de::DeserializeOwned;
```

The `Serialize` bound is the only addition over what keys already need. Any type that
serialises to a JSON value works as a key segment.

### `KeyPath` type

```rust
/// A root-to-item path through the reconciler tree.
/// Each element is the serde_json::Value representation of one key segment.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct KeyPath(Vec<serde_json::Value>);

impl KeyPath {
    fn push(&self, key: &impl serde::Serialize) -> Self { ... }
    fn starts_with(&self, prefix: &KeyPath) -> bool { ... }
    fn encode(&self) -> String { /* base64url segments joined by "/" */ }
    fn decode(s: &str) -> Result<Self, ...> { /* split by "/", base64 decode, json parse */ }
}

impl Display for KeyPath {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        /* render as: "sidebar"/"weather" — unencoded, human-readable */
    }
}
```

Finding a leaf by `KeyPath` is O(depth × store lookup): navigate the supervisor tree one
level at a time, calling `get(key)` at each level. No special indexing needed.

---

## Log entry structure

```rust
struct LogEntry {
    timestamp:    DateTime<Utc>,
    source:       KeyPath,           // root-to-item path
    level:        LogLevel,
    message:      String,
    layer_meta:   Vec<serde_json::Value>,  // one element per ancestor level (see below)
    item_meta:    Option<serde_json::Value>, // from the item itself
}

enum LogLevel { Trace, Debug, Info, Warn, Error }
```

`layer_meta` is an ordered list of metadata values contributed by each Supervisor level as
the log entry was routed upward. Index 0 is the root level's metadata, the last element is
the immediate parent Supervisor's metadata.

---

## Layer metadata attachment

Each Supervisor level can optionally attach structured metadata to every log entry it
routes. This is the structured-logging equivalent of `tracing` spans — context that
enriches log entries without items needing to know about it.

```rust
trait AttachesLogMeta {
    type LayerMeta: serde::Serialize;

    fn log_meta(&self) -> Option<Self::LayerMeta> { None }
}
```

When a Supervisor implementing `AttachesLogMeta` routes a log entry upward, it prepends
`serde_json::to_value(self.log_meta())` to `entry.layer_meta`. The root receives an entry
with the full ancestry of metadata attached.

This trait is optional. Supervisors that don't implement it pass log entries through
unmodified. The pattern is identical to the `Metadata` associated type on `ReportsStatus`
— same design, different scope (per-log-entry vs per-health-snapshot).

---

## Traits

### `LogStore` — write-only

```rust
trait LogStore {
    type Error;
    fn write(&mut self, entry: &LogEntry) -> Result<(), Self::Error>;
    fn write_batch(&mut self, entries: &[LogEntry]) -> Result<(), Self::Error> {
        entries.iter().try_for_each(|e| self.write(e))
    }
}
```

Sentry, log files, remote sinks, and stdout implement only this. No query capability
required.

### `QueryableLogStore: LogStore` — read + write

```rust
trait QueryableLogStore: LogStore {
    fn query(
        &self,
        source:  Option<&KeyPath>,       // prefix filter — None = all sources
        since:   Option<DateTime<Utc>>,
        until:   Option<DateTime<Utc>>,
        level:   Option<LogLevel>,       // minimum level — None = all levels
    ) -> impl Iterator<Item = &LogEntry>;
}
```

SQLite, in-memory ring buffers, and log files with an index implement this. The query
interface is deliberately minimal — no full query language, no aggregations, no joins.
Callers who need richer queries read the iterator and filter in Rust.

---

## How logs flow through the system

Items do not write to a `LogStore` directly. Log events come from two sources:

1. **Process stderr streams.** Each `ProcessSource` item holds a stderr reader in its
   `State`. The Supervisor maps over items and collects stderr lines as `LogEntry` values,
   tagging them with the item's `KeyPath`.

2. **`ReconcileErrors` from lifecycle operations.** Errors returned by `enter()`,
   `reconcile_self()`, and `exit()` are wrapped as `LogEntry` values at `Error` level
   and routed to the same store.

The Supervisor:
1. Collects log events from both sources.
2. If it implements `AttachesLogMeta`, prepends its layer metadata.
3. Appends its own key segment to the source path.
4. Passes the entry to the `LogStore`.

Items never hold a reference to the `LogStore` — log routing is the Supervisor's
responsibility, not the item's.

---

## Backend implementations

| Backend | Trait | Notes |
|---|---|---|
| stdout / stderr | `LogStore` | Simplest; format as JSON lines or human-readable |
| Log file | `LogStore` | Append-only; `QueryableLogStore` if indexed |
| SQLite | `QueryableLogStore` | Natural fit; one row per entry; `KeyPath` as text column |
| In-memory ring buffer | `QueryableLogStore` | Bounded; useful for live dashboards |
| Sentry | `LogStore` | SDK call per entry; errors only in practice |
| Remote HTTP sink | `LogStore` | Batched writes via `write_batch` |

The `LogStore` trait is the only thing a backend needs to implement. Adding a new backend
is implementing one method.

---

## Connection to `tracing` crate

The Rust `tracing` crate solves a related problem: structured, hierarchical, contextual
logging via spans. `tracing` spans are hierarchical (like `KeyPath`), carry structured
fields (like `layer_meta`), and have subscriber backends (like `LogStore`).

Whether to build on `tracing` or define independent traits is an open question — see
GitHub issue #11. The key risk of adopting `tracing` is that it brings its own span
lifecycle model which may conflict with the reconciler lifecycle model.

---

## Open questions

- Should `LogStore` be part of the `Context` passed to lifecycle items, or held exclusively
  by the Supervisor? Items passing through Context can write structured log events directly;
  Supervisor-only means items can only log via stderr and errors.

- Should `layer_meta` be `Vec<serde_json::Value>` (ordered oldest-to-newest) or a merged
  flat `serde_json::Map`? Ordered preserves which level contributed what; merged is simpler
  for consumers.

- What is the right in-memory buffer size for the ring buffer backend? Should it be
  configurable per `LogStore` instance?

- Should `KeyPath` be part of the core `managed_set` crate or a separate `tauler-path`
  crate? The path type is useful independently of the reconciler.

- Should log entries be emitted as a `Stream<Item = LogEntry>` rather than via trait
  method calls? A stream fits the pipeline model more naturally and allows backpressure.
