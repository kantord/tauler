use std::collections::HashMap;

use crate::model::Notification;

/// The live notification list, in display order.
///
/// Ids are not unique over time: a client may reuse one via the D-Bus
/// `replaces_id` argument (volume/brightness OSDs do this on every keypress).
/// A pending expiry timer therefore cannot identify its notification by id
/// alone — it would close whatever happens to hold that id when it fires.
/// Every `upsert` stamps the entry with a fresh generation, and `expire` only
/// acts when the stamp still matches, so superseded timers become no-ops.
pub struct Notifications {
    items: Vec<Notification>,
    /// Generation currently stamped on the entry holding each live id.
    generations: HashMap<u32, u64>,
    next_generation: u64,
}

impl Notifications {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            generations: HashMap::new(),
            next_generation: 0,
        }
    }

    /// Replaces the entry with the same id in place, or appends. Returns the
    /// generation to hand to the matching `expire` call.
    pub fn upsert(&mut self, n: Notification) -> u64 {
        let id = n.id;
        match self.items.iter().position(|x| x.id == id) {
            Some(pos) => self.items[pos] = n,
            None => self.items.push(n),
        }

        let generation = self.next_generation;
        self.next_generation += 1;
        self.generations.insert(id, generation);
        generation
    }

    /// Removes the entry only if it is still the one `generation` was issued
    /// for. Returns whether anything was removed.
    pub fn expire(&mut self, id: u32, generation: u64) -> bool {
        if self.generations.get(&id) != Some(&generation) {
            return false;
        }
        self.remove(id)
    }

    /// Removes the entry regardless of generation, as a dismissal does.
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|n| n.id != id);
        self.generations.remove(&id);
        self.items.len() != before
    }

    pub fn items(&self) -> &[Notification] {
        &self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Notification;

    fn notification(id: u32, summary: &str) -> Notification {
        Notification {
            id,
            app_name: "test-app".to_string(),
            summary: summary.to_string(),
            body: "body".to_string(),
            urgency: 1,
            enwiro_env: None,
        }
    }

    fn summaries(store: &Notifications) -> Vec<&str> {
        store.items().iter().map(|n| n.summary.as_str()).collect()
    }

    #[test]
    fn new_store_is_empty() {
        let store = Notifications::new();
        assert!(store.items().is_empty());
    }

    #[test]
    fn upsert_appends_a_new_notification() {
        let mut store = Notifications::new();
        store.upsert(notification(1, "first"));
        store.upsert(notification(2, "second"));
        assert_eq!(summaries(&store), vec!["first", "second"]);
    }

    #[test]
    fn upsert_with_existing_id_replaces_in_place() {
        let mut store = Notifications::new();
        store.upsert(notification(1, "first"));
        store.upsert(notification(2, "second"));
        store.upsert(notification(3, "third"));

        store.upsert(notification(2, "second-replaced"));

        assert_eq!(
            summaries(&store),
            vec!["first", "second-replaced", "third"],
            "replacement must keep position and must not duplicate"
        );
    }

    #[test]
    fn each_upsert_returns_a_distinct_generation() {
        let mut store = Notifications::new();
        let a = store.upsert(notification(1, "a"));
        let b = store.upsert(notification(2, "b"));
        let c = store.upsert(notification(1, "a-again"));

        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn expire_with_current_generation_removes_the_entry() {
        let mut store = Notifications::new();
        let generation = store.upsert(notification(7, "toast"));

        assert!(store.expire(7, generation));
        assert!(store.items().is_empty());
    }

    #[test]
    fn expire_of_unknown_id_returns_false() {
        let mut store = Notifications::new();
        let generation = store.upsert(notification(7, "toast"));

        assert!(!store.expire(99, generation));
        assert_eq!(summaries(&store), vec!["toast"]);
    }

    #[test]
    fn stale_expire_does_not_remove_a_replaced_notification() {
        // The volume-OSD regression: id 5 is reused via replaces_id, and the
        // first notification's expiry timer fires afterwards.
        let mut store = Notifications::new();
        let gen_a = store.upsert(notification(5, "volume 30%"));
        let gen_b = store.upsert(notification(5, "volume 40%"));

        assert!(
            !store.expire(5, gen_a),
            "the old timer must not remove the replacement"
        );
        assert_eq!(summaries(&store), vec!["volume 40%"]);

        assert!(store.expire(5, gen_b));
        assert!(store.items().is_empty());
    }

    #[test]
    fn remove_deletes_regardless_of_generation() {
        let mut store = Notifications::new();
        store.upsert(notification(1, "first"));
        store.upsert(notification(2, "second"));

        assert!(store.remove(1));
        assert_eq!(summaries(&store), vec!["second"]);
    }

    #[test]
    fn remove_of_unknown_id_returns_false() {
        let mut store = Notifications::new();
        store.upsert(notification(1, "first"));

        assert!(!store.remove(42));
        assert_eq!(summaries(&store), vec!["first"]);
    }

    #[test]
    fn expire_after_manual_remove_does_not_touch_a_reused_id() {
        // Dismissal removes id 5; a new notification then reuses id 5 before
        // the dismissed one's timer fires.
        let mut store = Notifications::new();
        let gen_a = store.upsert(notification(5, "dismissed"));
        assert!(store.remove(5));

        let gen_b = store.upsert(notification(5, "fresh"));

        assert!(!store.expire(5, gen_a));
        assert_eq!(summaries(&store), vec!["fresh"]);

        assert!(store.expire(5, gen_b));
        assert!(store.items().is_empty());
    }

    #[test]
    fn serialized_items_keep_exactly_the_public_notification_fields() {
        // The generation is internal bookkeeping: the stdout JSON contract that
        // tauler consumes must be unchanged.
        let mut store = Notifications::new();
        store.upsert(notification(3, "hello"));

        let json = serde_json::to_value(serde_json::json!({ "notifications": store.items() }))
            .expect("notifications must serialize");

        let entry = json["notifications"][0]
            .as_object()
            .expect("each notification serializes as an object");
        let mut keys: Vec<&str> = entry.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();

        assert_eq!(
            keys,
            vec!["app_name", "body", "enwiro_env", "id", "summary", "urgency"]
        );
        assert_eq!(entry["id"], 3);
        assert_eq!(entry["summary"], "hello");
    }
}
