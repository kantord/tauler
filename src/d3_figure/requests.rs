//! Which D3Figures to draw after an eval, and what each shows until its drawing
//! arrives.
//!
//! "The same figure" across Ticks is its position among the eval's
//! `figure_calls`, in document order. That order is stable while the layout's
//! structure is, and it needs nothing from the render runtime beyond the calls it
//! already reports. When the structure changes, a figure may show a neighbour's
//! Picture until its own arrives — briefly wrong, never blank.
//!
//! A key is requested once while it stays in consecutive plans: a key names one
//! Picture for good, so drawing it again could only produce the same thing. A key
//! that leaves the plan and comes back is requested again, because its Picture
//! was dropped meanwhile (the loop keeps only the Pictures of [`Requests::named`]
//! keys and the fallbacks they show). So all this remembers is the latest plan's
//! keys — bounded by the figures, not by every key ever seen. Fallback keys need
//! no entry: they are only shown, never requested. A drawing still in flight when
//! its key leaves the plan is stored when it arrives and dropped straight after —
//! one wasted drawing.
//!
//! A figure whose key changed shows its previous key's Picture until its own
//! arrives ([`Plan::fallbacks`]).
//!
//! A layout reload starts a new Draw runtime whose `draw`s may have changed, so
//! [`Requests::after_reload`] makes the next plan request every key again while
//! keeping the previous plan's keys for fallbacks. A figure whose key stayed
//! keeps showing its Picture until the new drawing replaces it; one whose key
//! changed in the reload falls back to its pre-reload key, as on any Tick.

use std::collections::HashSet;

use crate::d3_figure::draw::DrawRequest;
use crate::jsx::FigureCall;

/// What the loop remembers between evals to plan the next one.
#[derive(Debug, Default)]
pub struct Requests {
    /// The previous plan's keys, by position among its `figure_calls` — all
    /// this remembers, and every one of them was requested.
    previous: Vec<String>,
    /// Set by [`Self::after_reload`]: the next plan requests every key, even
    /// one in `previous`.
    redraw_all: bool,
}

/// What to do after one eval.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Plan {
    /// Drawings to ask the draw runtime for.
    pub draw: Vec<DrawRequest>,
    /// `(new key, key shown meanwhile)` — for `picture::set_fallback`.
    pub fallbacks: Vec<(String, String)>,
}

impl Requests {
    pub fn new() -> Self {
        Self::default()
    }

    /// Plan the draw requests and fallbacks for this eval's `calls`.
    ///
    /// A key not in the previous plan is drawn; a figure whose key differs
    /// from the key at its position in the previous plan falls back to that key.
    pub fn plan(&mut self, calls: &[FigureCall]) -> Plan {
        let mut plan = Plan::default();
        let mut requested = if std::mem::take(&mut self.redraw_all) {
            HashSet::new()
        } else {
            self.named()
        };
        for (i, call) in calls.iter().enumerate() {
            if requested.insert(call.key.clone()) {
                plan.draw.push(DrawRequest {
                    key: call.key.clone(),
                    width: call.width,
                    height: call.height,
                });
            }
            if let Some(previous) = self.previous.get(i).filter(|p| **p != call.key) {
                plan.fallbacks.push((call.key.clone(), previous.clone()));
            }
        }
        self.previous = calls.iter().map(|c| c.key.clone()).collect();
        plan
    }

    /// After a layout reload: the new Draw runtime's `draw`s may differ, so the
    /// next plan requests every key again. The previous plan's keys stay, so a
    /// figure whose key changed in the reload still falls back to its old one.
    pub fn after_reload(&mut self) {
        self.redraw_all = true;
    }

    /// The keys of the latest plan's figures.
    pub fn named(&self) -> HashSet<String> {
        self.previous.iter().cloned().collect()
    }

    /// How many keys this remembers as already requested.
    #[cfg(test)]
    fn remembered(&self) -> usize {
        self.previous.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(key: &str) -> FigureCall {
        FigureCall {
            key: key.to_string(),
            width: 248.0,
            height: 96.0,
        }
    }

    fn keys(plan: &Plan) -> Vec<&str> {
        plan.draw.iter().map(|r| r.key.as_str()).collect()
    }

    #[test]
    fn the_first_plan_requests_every_figure_once() {
        let mut requests = Requests::new();
        let plan = requests.plan(&[call("a"), call("b")]);
        assert_eq!(
            plan.draw,
            vec![
                DrawRequest {
                    key: "a".into(),
                    width: 248.0,
                    height: 96.0
                },
                DrawRequest {
                    key: "b".into(),
                    width: 248.0,
                    height: 96.0
                },
            ],
            "each figure is drawn once, at its own size"
        );
        assert!(
            plan.fallbacks.is_empty(),
            "nothing was on screen before, so there is nothing to fall back to"
        );
    }

    #[test]
    fn a_key_already_requested_is_not_requested_again() {
        let mut requests = Requests::new();
        let first = requests.plan(&[call("a")]);
        assert_eq!(
            keys(&first),
            ["a"],
            "precondition: the first plan asks for a"
        );

        let second = requests.plan(&[call("a")]);
        assert!(
            second.draw.is_empty(),
            "a key names one immutable Picture, so it is drawn once; got {:?}",
            keys(&second)
        );
    }

    #[test]
    fn a_figure_whose_key_changed_is_requested_and_falls_back_to_its_previous_key() {
        let mut requests = Requests::new();
        requests.plan(&[call("a"), call("city/dark")]);

        let plan = requests.plan(&[call("a"), call("city/light")]);
        assert_eq!(keys(&plan), ["city/light"], "only the new key is drawn");
        assert_eq!(
            plan.fallbacks,
            vec![("city/light".to_string(), "city/dark".to_string())],
            "the second figure keeps showing its previous Picture until the new one arrives"
        );
    }

    #[test]
    fn a_figure_at_a_new_position_gets_no_fallback() {
        let mut requests = Requests::new();
        requests.plan(&[call("a")]);

        let plan = requests.plan(&[call("a"), call("b")]);
        assert_eq!(keys(&plan), ["b"], "the new figure is drawn");
        assert!(
            plan.fallbacks.is_empty(),
            "no figure stood at that position before; got {:?}",
            plan.fallbacks
        );
    }

    #[test]
    fn named_is_exactly_the_latest_plans_keys() {
        let mut requests = Requests::new();
        requests.plan(&[call("a"), call("b")]);
        requests.plan(&[call("b"), call("c")]);

        assert_eq!(
            requests.named(),
            HashSet::from(["b".to_string(), "c".to_string()]),
            "a key from an earlier plan only is not named"
        );
    }

    #[test]
    fn a_key_that_left_the_plan_and_comes_back_is_requested_again() {
        let mut requests = Requests::new();
        requests.plan(&[call("a")]);
        requests.plan(&[call("b")]);

        // Its Picture was dropped once it was no longer named (app.rs retains
        // only the named keys and their fallbacks), so it must be drawn again.
        let plan = requests.plan(&[call("a")]);
        assert_eq!(
            keys(&plan),
            ["a"],
            "a key that left the plan has no Picture any more and must be redrawn"
        );
    }

    #[test]
    fn after_a_reload_the_same_keys_are_all_requested_again() {
        let mut requests = Requests::new();
        requests.plan(&[call("a"), call("b")]);

        requests.after_reload();
        let plan = requests.plan(&[call("a"), call("b")]);

        assert_eq!(
            keys(&plan),
            ["a", "b"],
            "the new Draw runtime's `draw` may differ, so every figure is drawn again"
        );
        assert!(
            plan.fallbacks.is_empty(),
            "a key that stayed keeps its own Picture until the new one replaces it; got {:?}",
            plan.fallbacks
        );
    }

    #[test]
    fn after_a_reload_a_figure_whose_key_changed_falls_back_to_its_pre_reload_key() {
        let mut requests = Requests::new();
        requests.plan(&[call("a"), call("city/dark")]);

        requests.after_reload();
        let plan = requests.plan(&[call("a"), call("city/light")]);

        assert_eq!(
            plan.fallbacks,
            vec![("city/light".to_string(), "city/dark".to_string())],
            "a reload must not blank a figure whose key changed in it"
        );
    }

    #[test]
    fn what_requests_remembers_is_bounded_by_the_figures_not_by_every_key_ever_seen() {
        let mut requests = Requests::new();
        for i in 0..1000 {
            requests.plan(&[call(&format!("live/{i}"))]);
        }

        assert!(
            requests.remembered() <= 2,
            "one figure whose key changes every tick must not grow the remembered keys; \
             remembered {} keys",
            requests.remembered()
        );
    }
}
