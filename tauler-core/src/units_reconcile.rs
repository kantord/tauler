//! What one Sweep's diff does, with the QuickJS-specific half cut away.
//!
//! `sweep_unit` in the desktop's `units.rs` calls `observe`/`key`/`value` through
//! a `JsxEvaluator`, because those are user hooks and the desktop's only JS
//! engine is QuickJS. Everything *after* that — reconciling declared Items
//! against observed ones and sorting the result into enter/update/exit batches
//! — never touches QuickJS at all: it is `optative::OptativeSet` working over
//! plain `serde_json::Value`. That part lives here so a browser build (which has
//! its own JS engine already, ADR 0027, and calls `key`/`value`/`observe`
//! directly rather than through QuickJS) gets the identical diff behaviour
//! through `tauler_core::web`, not a second implementation to keep in sync.
//!
//! See ADR 0037.

use optative::{Lifecycle, OptativeSet, Reconcile};
use serde::Deserialize;
use serde_json::Value;

/// One Item, already projected: `key`/`value` computed, `props` is the whole
/// declared or observed object, `order` is its position (desired: layout
/// order; observed: the order `observe` reported it).
///
/// `Deserialize` is for the wasm boundary — a browser Unit's `key`/`value`
/// projections run as plain JS, so by the time an Item crosses into Rust it is
/// already in this shape.
#[derive(Debug, Clone, Deserialize)]
pub struct SweepItem {
    pub key: String,
    pub value: Value,
    pub props: Value,
    pub order: usize,
}

/// What the set carries between Sweeps: the value the world last showed, the
/// props to hand `exit` once the Item is gone from the layout and only the
/// store remembers it, and where `observe` reported it — an exiting Item has
/// no position in the layout, so its batch is ordered by the observation
/// instead.
type ItemState = (Value, Value, usize);

/// The three batches a diff fills. They are flushed to JavaScript after
/// `reconcile` returns rather than during it, because `optative`'s lifecycle is
/// per-Item and tauler's hooks take a batch (ADR 0033) — the diff decides
/// *which* Items, one call decides *when*.
///
/// `update` carries pairs rather than bare Items: `esto`'s `update(new, old)`
/// hands the hook the previous Item so it can compute a delta, and dropping
/// that would make tauler's `update` strictly weaker. Pairs rather than two
/// aligned arrays, so nothing depends on two `Vec`s staying the same length.
/// `pub` only because `Lifecycle`'s associated `Context` type must be as visible
/// as the type implementing it (`SweepItem`) — its fields stay private, so
/// nothing outside this module can build or read one.
#[derive(Default)]
pub struct Batches {
    enter: Vec<(usize, Value)>,
    update: Vec<(usize, (Value, Value))>,
    exit: Vec<(usize, Value)>,
}

impl Batches {
    /// Drop the ordering keys once they have done their job.
    fn ordered<T>(mut batch: Vec<(usize, T)>) -> Vec<T> {
        batch.sort_by_key(|(order, _)| *order);
        batch.into_iter().map(|(_, item)| item).collect()
    }
}

impl Lifecycle for SweepItem {
    type Key = String;
    type State = ItemState;
    type Context = Batches;
    type Output = ();
    type Error = std::convert::Infallible;

    fn key(&self) -> String {
        self.key.clone()
    }

    fn enter(self, ctx: &mut Batches, _: &mut ()) -> Result<ItemState, Self::Error> {
        ctx.enter.push((self.order, self.props.clone()));
        Ok((self.value, self.props, self.order))
    }

    fn reconcile_self(
        self,
        state: &mut ItemState,
        ctx: &mut Batches,
        _: &mut (),
    ) -> Result<(), Self::Error> {
        if state.0 != self.value {
            ctx.update
                .push((self.order, (self.props.clone(), state.1.clone())));
            *state = (self.value, self.props, self.order);
        }
        Ok(())
    }

    fn exit(state: ItemState, ctx: &mut Batches, _: &mut ()) -> Result<(), Self::Error> {
        ctx.exit.push((state.2, state.1));
        Ok(())
    }
}

/// The payload a batch hook receives: the Items themselves, in the order the
/// layout declared them.
fn json_items(items: &[Value]) -> Value {
    Value::Array(items.to_vec())
}

/// The payload `update` receives: `{item, old}` per changed Item, so a hook can
/// diff against what the world had before without depending on two arrays
/// lining up.
fn json_pairs(pairs: &[(Value, Value)]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|(item, old)| serde_json::json!({ "item": item, "old": old }))
            .collect(),
    )
}

/// One turn of reconciliation: diff `desired` against `observed`, sort the
/// result into `(exit, update, enter)` batches, each ready to hand a hook
/// straight — no further shaping needed on the caller's side.
///
/// `desired`/`observed` are already `key`/`value`-projected. Who calls the
/// projection is deliberately not this function's business: the desktop calls
/// it through `JsxEvaluator`, a browser Unit calls it directly in JS — see
/// `units::sweep_unit` and `web::reconcile_unit` respectively.
pub fn reconcile(desired: Vec<SweepItem>, observed: Vec<SweepItem>) -> (Value, Value, Value) {
    let mut set = OptativeSet::<SweepItem>::with_initial_state(
        observed
            .into_iter()
            .map(|item| (item.key.clone(), (item.value, item.props, item.order))),
    );
    let mut acc = Batches::default();
    set.reconcile(desired, &mut acc, &mut ());

    // Exits first, so a Unit that has to free something before claiming it
    // again — a port, a lock, a single physical device — can.
    let exit = json_items(&Batches::ordered(acc.exit));
    let update = json_pairs(&Batches::ordered(acc.update));
    let enter = json_items(&Batches::ordered(acc.enter));
    (exit, update, enter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(key: &str, value: Value, order: usize) -> SweepItem {
        SweepItem {
            key: key.to_string(),
            props: json!({ "key": key, "value": value.clone() }),
            value,
            order,
        }
    }

    // Kept deliberately independent of `units.rs`'s tests: those exercise this
    // same diff through a real `JsxEvaluator`; these exercise the wasm-facing
    // entry point directly, with no QuickJS involved at all.

    #[test]
    fn declared_but_not_observed_is_an_enter() {
        let desired = vec![item("a", json!(1), 0)];
        let observed = vec![];
        let (exit, update, enter) = reconcile(desired, observed);
        assert_eq!(exit, json!([]));
        assert_eq!(update, json!([]));
        assert_eq!(enter, json!([{ "key": "a", "value": 1 }]));
    }

    #[test]
    fn observed_but_not_declared_is_an_exit() {
        let desired = vec![];
        let observed = vec![item("a", json!(1), 0)];
        let (exit, update, enter) = reconcile(desired, observed);
        assert_eq!(exit, json!([{ "key": "a", "value": 1 }]));
        assert_eq!(update, json!([]));
        assert_eq!(enter, json!([]));
    }

    #[test]
    fn same_key_different_value_is_an_update_carrying_the_old_props() {
        let desired = vec![item("a", json!(2), 0)];
        let observed = vec![item("a", json!(1), 0)];
        let (exit, update, enter) = reconcile(desired, observed);
        assert_eq!(exit, json!([]));
        assert_eq!(
            update,
            json!([{
                "item": { "key": "a", "value": 2 },
                "old": { "key": "a", "value": 1 },
            }])
        );
        assert_eq!(enter, json!([]));
    }

    #[test]
    fn same_key_same_value_makes_no_progress() {
        let desired = vec![item("a", json!(1), 0)];
        let observed = vec![item("a", json!(1), 0)];
        let (exit, update, enter) = reconcile(desired, observed);
        assert_eq!(exit, json!([]));
        assert_eq!(update, json!([]));
        assert_eq!(enter, json!([]));
    }

    #[test]
    fn batches_are_ordered_by_layout_position_not_observation_order() {
        // Observed in b, a order; declared in a, b order — enter/update should
        // come back in the declared (desired) order regardless.
        let desired = vec![item("a", json!(2), 0), item("b", json!(2), 1)];
        let observed = vec![item("b", json!(1), 0), item("a", json!(1), 1)];
        let (_, update, _) = reconcile(desired, observed);
        let keys: Vec<&str> = update
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| pair["item"]["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, vec!["a", "b"]);
    }
}
