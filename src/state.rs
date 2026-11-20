//! State a layout file keeps between ticks, owned here and not by any JavaScript runtime.
//!
//! The design of issue #271 and the store it ended in. [`StateStore`] below is what
//! `store(default)` in a layout file reads and writes: a layout file imports `store`
//! from the module `"tauler"`, the host functions are in `src/jsx.rs`, the Proxy and
//! that module's source in `tauler-core/src/globals.rs`, and the compile pass that
//! names a Store in `tauler-core/src/store_pass.rs`. An evaluator takes its store as a
//! constructor argument ([`SharedState`], `JsxEvaluator::new_with_state`), so one
//! store outlives a rebuilt evaluator. `App` (`src/app.rs`) owns the one store of the
//! running bar: it hands it to the render evaluator of every load, so state survives a
//! reload, and to the reconciler and draw runtimes, which read it and cannot write it.
//! After a reload that succeeded, a state the new layout did not declare is dropped.
//! `globals`, the state a layout file had before, is removed. A page keeps its Stores
//! in JavaScript (`tauler-web/js/state.js`; the `DECISION` in
//! `tauler-core/src/web.rs`).
//! Not built: nothing wakes the reconciler or redraws a D3Figure when a Store changes
//! (the last `RISK:` below). Every `RISK:` left here is open.
//!
//! TERM: Store — a JSON object a layout file declares with `store(...)`. Declared at
//! module scope it is shared by every component that imports it, and that is the Store
//! this file's design is about; declared inside a function it is a local Store
//! (`tauler-core/src/local_store.rs`). This module holds both, and an identity is an
//! opaque string here but for one rule: a write to a local Store that is not held is
//! ignored ([`LOCAL_IDENTITY_PREFIX`]).
//! Avoid: globals, atom, slice, context.
//!
//! TERM: State identity — the string that finds the same Store again on the next tick,
//! after a reload, and from another runtime: `<module>#<name>`, written by the compile
//! pass, for a shared Store; `local://` and a chain of names for a local one. No
//! person writes one.
//! Avoid: key (that is one top-level key of a state, or the JSX `key` prop), id, name.
//!
//! A Store declared inside a function is local to a component instance:
//! `tauler-core/src/local_store.rs` holds that design.
//!
//! ## Paths
//!
//! P1  A handler writes a Store; the write survives; one tick shows it.
//!     Needs: writes reach this module; a changed write pings the Notifier.
//! P2  A render writes its own state (workspace memory, a scroll clamp); it survives and
//!     causes no tick.
//! P3  A Unit hook (reconciler runtime) and a D3Figure (draw runtime) read a Store, and
//!     cannot write it.
//! P4  A reload or a layout edit keeps every state whose identity still matches.
//!     Needs: this module outlives the `JsxEvaluator`, which is rebuilt on reload.
//! P5  A Package ships a Store; it works when imported and cannot collide with the
//!     importer's state.
//! P6  The same layout file runs on the browser's own engine (ADR 0027) with no Rust.
//!
//! ## Constraints
//!
//! - #271: an unchanged state costs no encode or decode per tick.
//! - A tick stays a function `(streams, state) -> (layout, changed state)` with every
//!   input and output held in Rust, so a tick can be replayed, tested or skipped.
//! - A second scripting language and Rust components can read and write the same state.
//! - No hand-written identity anywhere: a name chosen by a person collides across
//!   Packages and the importer cannot rename it.
//!
//! ## Shape
//!
//! DECISION: this module owns the state; a runtime holds a cache of it. Reason: the three
//! constraints above. Rejected: the JS object as the truth with Rust as a backup — it
//! makes a tick depend on values Rust cannot see.
//!
//! ```text
//! struct StateStore { entries: Map<Identity, Map<Key, Slot>>, clock: u64 }
//! struct Slot       { value: serde_json::Value, version: u64 }
//!
//! get(identity, key)          -> Value      // one top-level key of one state
//! set(identity, key, value)   -> changed?   // bumps version only when the value differs
//! declare(identity, default)                // first sight: store the default
//!                                           // seen before: keep the saved value, fill
//!                                           // top-level keys the default has and it lacks
//! begin() / rollback()                      // around one handler or one render
//! ```
//!
//! The three SPECs on `set`, on a throwing handler or render, and on `declare` are now
//! tests: `state_*` below and `store_*` in `src/jsx.rs`. A state whose identity the
//! layout no longer declares is dropped after a reload that succeeded
//! ([`StateStore::retain_identities`]; `drop_undeclared_states` in `src/app.rs` holds
//! the DECISION on whose declarations count, and
//! `a_reload_drops_the_states_the_new_layout_did_not_declare` there is the test).
//!
//! DECISION: a version belongs to one top-level key, not to one state, and comes from one
//! clock for the whole store that never repeats a value (a rollback takes new ones).
//! Reason: with one version per state, a write to `desk.count` makes the runtime convert
//! `desk.items` again; and the clock is what lets a runtime ask "did anything change
//! anywhere" with no call at all (below). Rejected: `Entry { value, version }` as first
//! sketched.
//!
//! All figures below are medians in µs, one session, shape `realistic` unless named.
//! A Store's figures are the groups `eval_with_store/..` of `benches/store.rs`. "Bulk
//! copy" is what `globals` cost: the whole object converted into JavaScript before
//! every render and back after it, 112.8 when the render read nothing and 163.5 when
//! it walked every key. Those are the groups `eval_with_bulk_copy/read_none` and
//! `eval_with_bulk_copy/read_all` of the same bench, which does the copy itself now
//! that the product does not; a run of them after the removal gave 115.0 and 169.8,
//! and every bulk copy figure below within 8% of what is quoted. The figures for a
//! rejected alternative (a host call per read, values handed out deep-frozen) cannot
//! be run again: the alternative was not kept.
//!
//! DECISION: reads go through the Proxy one key at a time and nothing is copied before a
//! render. Reason and evidence: the Kill (costlier than the bulk copy on `realistic`) is
//! not met. A render that reads nothing costs 3.9 and one that reads one primitive 4.2,
//! on every shape, against 112.8; a render that walks every key costs 108.5 against
//! 163.5 for the same walk over `globals`.
//! DECISION: a runtime answers a read from its own mirror while the store's clock stands
//! (the host sets `__tauler_state_now` before each handler and render), and asks the host
//! only after it moves. This is the versioned copy the Kill named as the fallback, made
//! lazily per key. Reason: it is never slower than a host call per read on the shapes
//! where either matters and it does not convert a string again per read. Evidence, walk
//! of every key, mirror against host call per read: `realistic` 108.5 / 111.7, `flat_20`
//! 24.0 / 32.6, `flat_2000` 2295 / 3203, `long_strings_50x10k` 1063 / 2625 (bulk copy
//! with the same walk: 2551). Rejected: a host call per read, the first design.
//!
//! RISK: a state of many primitives that is walked whole (`Object.keys`, spread) is slower
//! through the Proxy than the bulk copy: `flat_20` 24.0 against 17.1, `flat_2000` 2295
//! against 1289. The cost is the Proxy's traps, about 0.9 µs for each key enumerated
//! (`ownKeys`, then `getOwnPropertyDescriptor` and `get` per key), not the host; one
//! named read (`desk.count`) costs 0.3 to 0.5 µs.
//! Kill: a real layout enumerates a Store of more than about 50 keys on every tick —
//! then give enumeration its own path (hand out one frozen plain object per clock).
//! Cost: small.
//!
//! The Kill first set for comparing nested values (more than 20% of the bulk copy) is
//! met, so the alternative was built and measured before the DECISION below. Comparing
//! every nested value a render was handed costs 50.8 (handing out every key and looking
//! at nothing: 61.5 compared, 10.7 with the values handed out deep-frozen and never
//! compared), which is 45% of the bulk copy. The compare is a `JSON.stringify` of the
//! value and a string comparison in the runtime, so its cost is that of the render
//! walking the value once more; it is paid only for nested values a handler or render
//! was handed, never for the rest.
//! Rejected: comparing in Rust (decode the value, compare with the stored one) — the
//! decode is the half of the bulk copy this design removes.
//!
//! DECISION: nested in-place writes are kept (`desk.items.push(x)` works). Every nested
//! value handed out is compared after each handler and each render, and written back
//! when it differs. Reason: layouts change nested values in place (`push`), and it is
//! still no slower than `globals` was: 108.5 against 112.8 per tick with everything
//! read. Rejected: handing nested values out deep-frozen and requiring
//! `desk.items = [...desk.items, x]` — measured cheaper (59.1 against 108.5 with
//! everything read; 158.7 against 189.4 when one nested value changes), but a nested
//! write would throw.
//!
//! RISK: "equal" is `serde_json`'s, so the integer `1` and the float `1.0` differ. A
//! handler that writes back a number it computed as a float where an integer was stored
//! reports a change and causes a tick although nothing shows.
//! Kill: a real handler does this on a hot path (a drag) — then compare numbers by value.
//! Cost: small.
//!
//! DECISION: the draw and reconciler runtimes are handed `App`'s store as a constructor
//! argument (`new_draw`, `new_reconciler` in `src/jsx.rs`) and read it live (P3). They
//! cannot change it: there the host function behind a write from a Unit hook or from
//! `draw` code throws an error that names the runtime, the one behind `store(...)`
//! stores no default, and no transaction is opened (`StateAccess` in `src/jsx.rs`
//! holds the mechanism and what was rejected). Tests: `store_*` in `src/jsx.rs`
//! (`..._is_read_by_the_reconciler_runtime`, `store_write_from_a_unit_hook_changes_nothing`,
//! `..._is_read_by_the_draw_runtime_which_cannot_write_it`). Rejected: a store of
//! their own each, which is what they had — a Unit hook could not see what a handler
//! wrote.
//!
//! DECISION: these two runtimes run the layout's render as well as its hooks and
//! `draw`s, and a write that render makes (P2) is kept there for that evaluation only:
//! the rest of the render reads it, the store never gets it, and the next evaluation
//! starts from the store. The same goes for a write at module scope. Reason: the Kill
//! of the risk this replaces was met — a real layout writes its state during render
//! and declares Units and D3Figures, and a throw there left the Units unreconciled
//! and the figures undrawn. `StateAccess` in `src/jsx.rs` holds the mechanism, the
//! tests and what was rejected.
//!
//! RISK: a Store change wakes only the render runtime (`StateWake` in `src/app.rs`).
//! The reconciler runtime evaluates on its own schedule: with a Unit declared, the
//! next Sweep reads the change, up to that Unit's refresh interval later (5 s by
//! default); with none declared it stays idle until a stream value changes
//! (`Reconciler::spawn` in `src/units.rs`), so a Unit that a Store alone switches on
//! is not seen until then. A D3Figure is drawn again only when its key or size
//! changes (`src/d3_figure/requests.rs`), so a `draw` that reads a Store shows the
//! old value until then. And the draw runtime reads the store live but takes
//! stream values from the eval that asked, so the two can disagree about a figure's
//! key; a key the draw runtime's eval does not name is not drawn
//! (`DrawError::UnknownKey`). A render that changes a Store on every Tick and builds a
//! figure's key from it is the plain case: the draw runtime reads the store after
//! that render wrote it, so its own render is one step ahead and names the next key.
//! `globals` did not have this: a draw was evaluated with a copy of them from before
//! the render that asked.
//! Kill: a real layout drives a Unit or a `draw` from a Store and the delay shows —
//! then a changed Store's clock is one more thing the reconciler's idle check
//! compares, and a figure's request carries the versions it read.
//! Cost: medium (a new wake path into two threads).
//!
//! DECISION: `globals` is removed in the same release that ships Stores. Two state
//! systems is the complexity this design exists to avoid, and the copy of `globals` into
//! and out of every evaluation is the cost issue #271 is about. Rejected: keeping it
//! beside Stores for a while. No evaluator holds or copies it any more, and a layout
//! file that names it fails to load with a message that points at `store`: the
//! compile pass refuses it (`tauler-core/src/store_pass.rs`; test
//! `a_layout_that_names_globals_fails_to_load_and_points_at_store` in `src/jsx.rs`).
//!
//! DECISION: a write made during a render is kept but does not ping the Notifier, so it
//! shows on the next Tick that something else causes, as `globals` behaved. Only a
//! handler's write causes a Tick. Rejected: a render-time write waking a render — it
//! loops whenever the value never settles (a timestamp).
//!
//! DECISION: this design overrides ADR 0012 (controls never self-hold), ADR 0007's
//! "nothing survives a tick except `globals`", and the `Tick` and `Control component`
//! entries of `CONTEXT.md`. `AGENTS.md` requires those to be kept true, so the
//! superseding ADR (`docs/adr/0051-a-layout-file-keeps-state-in-stores-and-rust-owns-them.md`)
//! and the glossary edits land in the same change as `App` owning the store.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// What the State identity of every local Store starts with, and no other identity
/// does (`__tauler_local_store` in `tauler-core/src/local_store.rs` writes it).
///
/// DECISION: a write to a local Store this store does not hold is ignored: no entry is
/// made, nothing is recorded, and the handler that wrote it does not throw. A local
/// Store is held from the render that declares it to the render that drops it; a
/// handler a press captured can run after that drop, and an entry made then would
/// have no render to drop it. Reason for ignoring: the handler's other writes (to a
/// shared Store, to a held local Store) are kept and its intents are still
/// dispatched; a throw would roll those back for a press on something that is no
/// longer on screen. A shared identity is written whether or not it is held, as
/// before. A page has the same rule in `__tauler_state_set` in
/// `tauler-web/js/state.js`. Tests:
/// `state_set_to_a_local_identity_that_is_not_held_stores_nothing` below,
/// `local_store_a_handler_held_past_its_render_writes_nothing_to_a_dropped_store` in
/// `src/jsx.rs`, and
/// `a_handler_held_past_its_render_writes_nothing_to_a_dropped_local_store` in
/// `tests/page_state_test.rs`. Rejected: throwing in the handler.
pub const LOCAL_IDENTITY_PREFIX: &str = "local://";

/// One [`StateStore`], shared between an evaluator's host functions and its owner.
pub type SharedState = Arc<Mutex<StateStore>>;

/// One top-level key of one state.
struct Slot {
    value: Value,
    /// The store's clock when this key last changed. Never reused, so a runtime that
    /// holds a converted value for version `n` may keep it exactly while this is `n`.
    version: u64,
}

/// What [`StateStore::rollback`] undoes, newest last.
enum Undo {
    /// `key` of `identity` held `previous` (`None`: it did not exist).
    Key {
        identity: String,
        key: String,
        previous: Option<Value>,
    },
    /// `identity` was first declared inside the transaction.
    Declared(String),
}

/// Every Store of one layout, by State identity.
#[derive(Default)]
pub struct StateStore {
    entries: HashMap<String, BTreeMap<String, Slot>>,
    clock: u64,
    /// `Some` between [`Self::begin`] and [`Self::commit`]/[`Self::rollback`].
    journal: Option<Vec<Undo>>,
}

impl StateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> SharedState {
        Arc::new(Mutex::new(Self::new()))
    }

    fn tick_clock(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn record(&mut self, undo: Undo) {
        if let Some(journal) = self.journal.as_mut() {
            journal.push(undo);
        }
    }

    /// First sight of `identity`: store `default`. Seen before: keep what is saved and
    /// add only the top-level keys `default` has and the saved state lacks.
    ///
    /// First sight moves the clock even when `default` is empty: a runtime that only
    /// reads answers from its own default until the identity is held (`Defaults` in
    /// `src/jsx.rs`), and must ask again from then on.
    pub fn declare(&mut self, identity: &str, default: serde_json::Map<String, Value>) {
        if !self.entries.contains_key(identity) {
            self.entries.insert(identity.to_string(), BTreeMap::new());
            self.tick_clock();
            self.record(Undo::Declared(identity.to_string()));
        }
        for (key, value) in default {
            if self.entries[identity].contains_key(&key) {
                continue;
            }
            self.set(identity, &key, value);
        }
    }

    /// Moves whenever any state changes. A runtime that mirrors values may keep all of
    /// them while this stands.
    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// One top-level key of one state, and the version it has had since it last changed.
    pub fn get(&self, identity: &str, key: &str) -> Option<(&Value, u64)> {
        let slot = self.entries.get(identity)?.get(key)?;
        Some((&slot.value, slot.version))
    }

    /// The version of one key; `0` for a key that does not exist.
    pub fn version(&self, identity: &str, key: &str) -> u64 {
        self.get(identity, key).map_or(0, |(_, version)| version)
    }

    /// Whether a state is held under `identity`: declared, and not dropped since.
    pub fn holds(&self, identity: &str) -> bool {
        self.entries.contains_key(identity)
    }

    /// The top-level keys of one state, sorted.
    pub fn keys(&self, identity: &str) -> Vec<String> {
        self.entries
            .get(identity)
            .map(|entry| entry.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// One whole state, as the JSON object a layout file sees.
    pub fn snapshot(&self, identity: &str) -> Option<serde_json::Map<String, Value>> {
        let entry = self.entries.get(identity)?;
        Some(
            entry
                .iter()
                .map(|(key, slot)| (key.clone(), slot.value.clone()))
                .collect(),
        )
    }

    /// Write one top-level key. `true` when the stored value changed; an equal value
    /// changes nothing and moves no version.
    ///
    /// A local Store that is not held (never declared, or dropped) is not written:
    /// nothing is stored and the answer is `false` ([`LOCAL_IDENTITY_PREFIX`]).
    pub fn set(&mut self, identity: &str, key: &str, value: Value) -> bool {
        if !self.entries.contains_key(identity) {
            if identity.starts_with(LOCAL_IDENTITY_PREFIX) {
                return false;
            }
            self.entries.insert(identity.to_string(), BTreeMap::new());
        }
        let previous = match self.entries[identity].get(key) {
            Some(slot) if slot.value == value => return false,
            Some(slot) => Some(slot.value.clone()),
            None => None,
        };
        let version = self.tick_clock();
        self.entries
            .get_mut(identity)
            .expect("inserted above")
            .insert(key.to_string(), Slot { value, version });
        self.record(Undo::Key {
            identity: identity.to_string(),
            key: key.to_string(),
            previous,
        });
        true
    }

    /// Delete one top-level key. `true` when it existed.
    pub fn remove(&mut self, identity: &str, key: &str) -> bool {
        let Some(slot) = self.entries.get_mut(identity).and_then(|e| e.remove(key)) else {
            return false;
        };
        self.tick_clock();
        self.record(Undo::Key {
            identity: identity.to_string(),
            key: key.to_string(),
            previous: Some(slot.value),
        });
        true
    }

    /// Forget one whole state: a local Store the render that just returned did not
    /// declare (`tauler-core/src/local_store.rs`). `true` when it existed. No rollback
    /// brings it back: the drop is the last thing a render's transaction does.
    pub fn drop_identity(&mut self, identity: &str) -> bool {
        let existed = self.entries.remove(identity).is_some();
        if existed {
            self.tick_clock();
        }
        existed
    }

    /// How many states are held.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Forget every state whose identity is not in `declared`. Not part of a
    /// transaction: a reload calls it between Ticks, and nothing undoes a reload.
    pub fn retain_identities(&mut self, declared: &[String]) {
        let before = self.entries.len();
        self.entries
            .retain(|identity, _| declared.iter().any(|d| d == identity));
        if self.entries.len() != before {
            self.tick_clock();
        }
    }

    /// Start remembering every change, for one handler or one render.
    pub fn begin(&mut self) {
        self.journal = Some(Vec::new());
    }

    /// Keep everything since [`Self::begin`]. `true` when any state changed.
    pub fn commit(&mut self) -> bool {
        self.journal
            .take()
            .is_some_and(|journal| !journal.is_empty())
    }

    /// Put every state back as it was at [`Self::begin`]. A restored key gets a new
    /// version, not its old one: a runtime may have converted the value that is being
    /// thrown away under a version in between. A state that was declared since then
    /// stops being held.
    ///
    /// A page has the same rules a second time, in the `catch` of `withState` in
    /// `tauler-web/js/state.js`. Change the two together.
    pub fn rollback(&mut self) {
        let Some(journal) = self.journal.take() else {
            return;
        };
        for undo in journal.into_iter().rev() {
            match undo {
                Undo::Declared(identity) => {
                    // The identity stops being held, which moves the clock as
                    // [`Self::drop_identity`] does.
                    self.entries.remove(&identity);
                    self.tick_clock();
                }
                Undo::Key {
                    identity,
                    key,
                    previous,
                } => {
                    let version = self.tick_clock();
                    let Some(entry) = self.entries.get_mut(&identity) else {
                        continue;
                    };
                    match previous {
                        Some(value) => {
                            entry.insert(key, Slot { value, version });
                        }
                        None => {
                            entry.remove(&key);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> serde_json::Map<String, Value> {
        value.as_object().expect("an object").clone()
    }

    #[test]
    fn state_set_of_an_equal_value_is_unchanged_and_moves_no_version() {
        let mut store = StateStore::new();
        assert!(store.set("s", "n", json!(1)));
        let version = store.version("s", "n");
        assert!(!store.set("s", "n", json!(1)));
        assert_eq!(store.version("s", "n"), version);
        assert!(store.set("s", "n", json!(2)));
        assert!(store.version("s", "n") > version);
    }

    /// A local Store is held from its declaration to its drop; a write outside that
    /// makes no entry. A shared identity is written as before.
    #[test]
    fn state_set_to_a_local_identity_that_is_not_held_stores_nothing() {
        let local = "local://a.jsx#Row > a.jsx#Row.s";
        let mut store = StateStore::new();
        store.begin();
        assert!(!store.set(local, "n", json!(1)));
        assert!(!store.remove(local, "n"));
        assert!(!store.commit(), "nothing changed");
        assert_eq!((store.len(), store.clock()), (0, 0));

        store.declare(local, object(json!({ "n": 0 })));
        assert!(store.set(local, "n", json!(1)));
        assert!(store.set(local, "new", json!(1)));
        assert!(store.drop_identity(local));
        assert!(!store.set(local, "n", json!(2)));
        assert_eq!(store.len(), 0);

        assert!(store.set("a.jsx#desk", "n", json!(1)));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn state_set_moves_only_the_version_of_the_key_it_wrote() {
        let mut store = StateStore::new();
        store.set("s", "a", json!([1]));
        store.set("s", "b", json!([2]));
        let a = store.version("s", "a");
        store.set("s", "b", json!([3]));
        assert_eq!(store.version("s", "a"), a);
    }

    #[test]
    fn state_rollback_restores_values_removals_and_declarations() {
        let mut store = StateStore::new();
        store.declare("s", object(json!({ "n": 1, "gone": true })));
        let version = store.version("s", "n");

        store.begin();
        store.set("s", "n", json!(2));
        store.set("s", "n", json!(3));
        store.set("s", "new", json!("x"));
        store.remove("s", "gone");
        store.declare("fresh", object(json!({ "a": 1 })));
        store.rollback();

        assert_eq!(
            store.snapshot("s"),
            Some(object(json!({ "n": 1, "gone": true })))
        );
        assert_eq!(store.snapshot("fresh"), None);
        assert!(
            store.version("s", "n") > version,
            "a restored key must not reuse a version a runtime may have cached under"
        );
    }

    /// A runtime that only reads answers from its own kept default while the store
    /// holds nothing under an identity (`Defaults` in `src/jsx.rs`), and keeps that
    /// answer while the clock stands. So the clock moves when an identity starts or
    /// stops being held, even when its default is empty.
    #[test]
    fn state_declare_of_a_new_identity_moves_the_clock_even_when_the_default_is_empty() {
        let mut store = StateStore::new();
        store.declare("empty", object(json!({})));
        let declared = store.clock();
        assert!(declared > 0, "a mirror of a kept default must be refilled");

        store.declare("empty", object(json!({})));
        assert_eq!(store.clock(), declared, "seen before: nothing moves");

        store.begin();
        store.declare("fresh", object(json!({})));
        let inside = store.clock();
        assert!(inside > declared);
        store.rollback();
        assert!(!store.holds("fresh"));
        assert!(
            store.clock() > inside,
            "a declaration that is undone moves the clock again"
        );
    }

    #[test]
    fn state_retain_identities_forgets_the_rest_and_moves_the_clock() {
        let mut store = StateStore::new();
        store.declare("kept", object(json!({ "n": 1 })));
        store.declare("gone", object(json!({ "n": 2 })));
        let clock = store.clock();

        store.retain_identities(&["kept".to_string()]);

        assert_eq!(store.snapshot("kept"), Some(object(json!({ "n": 1 }))));
        assert_eq!(store.snapshot("gone"), None);
        assert!(store.clock() > clock, "a mirror of `gone` must be refilled");

        let clock = store.clock();
        store.retain_identities(&["kept".to_string()]);
        assert_eq!(store.clock(), clock, "nothing forgotten moves nothing");
    }

    #[test]
    fn state_drop_identity_forgets_one_state_and_moves_the_clock() {
        let mut store = StateStore::new();
        store.declare("kept", object(json!({ "n": 1 })));
        store.declare("gone", object(json!({ "n": 2 })));
        let clock = store.clock();

        assert!(store.drop_identity("gone"));
        assert!(!store.drop_identity("never"));
        assert_eq!(store.snapshot("gone"), None);
        assert_eq!(store.snapshot("kept"), Some(object(json!({ "n": 1 }))));
        assert!(store.clock() > clock, "a mirror of `gone` must be refilled");
    }

    #[test]
    fn state_commit_reports_whether_anything_changed() {
        let mut store = StateStore::new();
        store.set("s", "n", json!(1));
        store.begin();
        store.set("s", "n", json!(1));
        assert!(!store.commit());
        store.begin();
        store.set("s", "n", json!(2));
        assert!(store.commit());
    }

    #[test]
    fn state_declare_on_a_known_identity_keeps_the_value_and_fills_only_new_keys() {
        let mut store = StateStore::new();
        store.declare("s", object(json!({ "n": 1, "items": [] })));
        store.set("s", "n", json!(5));
        store.set("s", "items", json!([1, 2]));

        store.declare(
            "s",
            object(json!({ "n": 0, "items": [], "added": "later" })),
        );

        assert_eq!(
            store.snapshot("s"),
            Some(object(json!({ "n": 5, "items": [1, 2], "added": "later" })))
        );
    }
}
