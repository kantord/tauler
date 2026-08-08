use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::sync::mpsc;
use zbus::interface;
use zbus::zvariant::OwnedValue;

use crate::model::{CloseReason, Event, Notification};

pub struct NotifyServer {
    pub tx: mpsc::UnboundedSender<Event>,
    pub next_id: AtomicU32,
    /// Id currently assigned to each stack tag. Lives here rather than in the
    /// store because `notify` has to answer the client with the id
    /// synchronously, before the event loop has seen the notification.
    pub tags: std::sync::Mutex<HashMap<String, u32>>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotifyServer {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        _app_icon: &str,
        summary: &str,
        body: &str,
        _actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> u32 {
        let tag = stack_tag(&hints);
        let id = {
            let mut tags = self.tags.lock().expect("tag map poisoned");
            resolve_id(replaces_id, tag.as_deref(), &mut tags, &self.next_id)
        };

        let urgency = hints
            .get("urgency")
            .and_then(|v| u8::try_from(v.clone()).ok())
            .unwrap_or(1);

        let enwiro_env = async {
            let sender = header.sender()?;
            let dbus = zbus::fdo::DBusProxy::new(connection).await.ok()?;
            let pid = dbus
                .get_connection_unix_process_id(zbus::names::BusName::Unique(sender.clone()))
                .await
                .ok()?;
            read_enwiro_env(pid)
        }
        .await;

        let _ = self.tx.send(Event::Add(
            Notification {
                id,
                app_name: app_name.to_string(),
                summary: summary.to_string(),
                body: body.to_string(),
                urgency,
                enwiro_env,
            },
            expire_timeout,
        ));

        id
    }

    async fn close_notification(&self, id: u32) {
        let _ = self.tx.send(Event::Close {
            id,
            reason: CloseReason::Closed,
        });
    }

    async fn get_capabilities(&self) -> Vec<String> {
        vec!["body".to_string()]
    }

    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            "tauler-notify".to_string(),
            "tauler".to_string(),
            "0.1.0".to_string(),
            "1.3".to_string(),
        )
    }
}

/// The client's "replace whatever already carries this tag" marker, used by
/// volume/brightness OSDs so repeated keypresses update one popup instead of
/// stacking. Two spellings are in the wild and mean the same thing:
/// `x-canonical-private-synchronous` (Ubuntu/GNOME) and `x-dunst-stack-tag`.
/// A wrongly-typed hint reads as untagged rather than failing the call.
fn stack_tag(hints: &HashMap<String, OwnedValue>) -> Option<String> {
    ["x-canonical-private-synchronous", "x-dunst-stack-tag"]
        .iter()
        .filter_map(|key| hints.get(*key))
        .find_map(|v| String::try_from(v.clone()).ok())
}

/// Picks the id a notification should occupy, remembering it under `tag`.
///
/// An explicit `replaces_id` is the client's own instruction and outranks our
/// bookkeeping, but the tag still follows it so the next tagged call lands on
/// the same notification.
///
/// Tags are never evicted when a notification closes, deliberately: reusing the
/// id for a dismissed tag is correct, since `store.upsert` appends a fresh
/// entry under that id and `store.remove` already dropped the old generation
/// stamp, so no stale expiry timer can match. An OSD keeping one stable id
/// across its lifetime is the intended behaviour.
fn resolve_id(
    replaces_id: u32,
    tag: Option<&str>,
    tags: &mut HashMap<String, u32>,
    next_id: &AtomicU32,
) -> u32 {
    let Some(tag) = tag else {
        return match replaces_id {
            0 => next_id.fetch_add(1, Ordering::Relaxed),
            id => id,
        };
    };

    let id = match (replaces_id, tags.get(tag)) {
        (0, Some(&known)) => known,
        (0, None) => next_id.fetch_add(1, Ordering::Relaxed),
        (replaces_id, _) => replaces_id,
    };
    tags.insert(tag.to_string(), id);
    id
}

pub fn read_enwiro_env(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/environ", pid);
    let data = std::fs::read(path).ok()?;
    parse_enwiro_env(&data)
}

fn parse_enwiro_env(environ: &[u8]) -> Option<String> {
    environ
        .split(|&b| b == b'\0')
        .filter_map(|entry| entry.strip_prefix(b"ENWIRO_ENV="))
        .next()
        .and_then(|val| std::str::from_utf8(val).ok())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_enwiro_env_extracts_value() {
        let environ = b"HOME=/home/user\0ENWIRO_ENV=liro\0PATH=/usr/bin\0";
        assert_eq!(parse_enwiro_env(environ), Some("liro".to_string()));
    }

    #[test]
    fn parse_enwiro_env_returns_none_when_absent() {
        let environ = b"HOME=/home/user\0PATH=/usr/bin\0";
        assert_eq!(parse_enwiro_env(environ), None);
    }

    fn hints(pairs: &[(&str, zbus::zvariant::Value<'static>)]) -> HashMap<String, OwnedValue> {
        pairs
            .iter()
            .map(|(k, v)| {
                let owned = OwnedValue::try_from(v.try_clone().expect("hint value must clone"))
                    .expect("hint value must convert to OwnedValue");
                (k.to_string(), owned)
            })
            .collect()
    }

    // Both spellings mean the same thing on the wire: "replace whatever already
    // carries this tag". Ubuntu/GNOME clients send the first, dunst's own send
    // the second, so a notification daemon has to honour both.
    #[test]
    fn stack_tag_reads_the_canonical_synchronous_hint() {
        let h = hints(&[(
            "x-canonical-private-synchronous",
            zbus::zvariant::Value::new("i3wm.set-light-brightness.notification"),
        )]);
        assert_eq!(
            stack_tag(&h),
            Some("i3wm.set-light-brightness.notification".to_string())
        );
    }

    #[test]
    fn stack_tag_reads_the_dunst_hint() {
        let h = hints(&[("x-dunst-stack-tag", zbus::zvariant::Value::new("volume"))]);
        assert_eq!(stack_tag(&h), Some("volume".to_string()));
    }

    #[test]
    fn stack_tag_is_none_without_either_hint() {
        let h = hints(&[("urgency", zbus::zvariant::Value::new(1u8))]);
        assert_eq!(stack_tag(&h), None);
    }

    // A client is free to send garbage; a wrongly-typed hint must degrade to
    // "untagged", never panic the daemon.
    #[test]
    fn stack_tag_ignores_a_non_string_hint() {
        let h = hints(&[(
            "x-canonical-private-synchronous",
            zbus::zvariant::Value::new(7u32),
        )]);
        assert_eq!(stack_tag(&h), None);
    }

    #[test]
    fn resolve_id_without_tag_allocates_fresh_ids() {
        let next_id = AtomicU32::new(1);
        let mut tags = HashMap::new();

        let first = resolve_id(0, None, &mut tags, &next_id);
        let second = resolve_id(0, None, &mut tags, &next_id);

        assert_ne!(
            first, second,
            "untagged notifications must stack, not replace each other"
        );
        assert!(tags.is_empty());
    }

    // The whole point of the feature: repeated brightness keypresses update one
    // popup instead of piling up.
    #[test]
    fn resolve_id_reuses_the_id_for_the_same_tag() {
        let next_id = AtomicU32::new(1);
        let mut tags = HashMap::new();

        let first = resolve_id(0, Some("brightness"), &mut tags, &next_id);
        let second = resolve_id(0, Some("brightness"), &mut tags, &next_id);

        assert_eq!(first, second);
        assert_eq!(tags.get("brightness"), Some(&first));
    }

    #[test]
    fn resolve_id_keeps_different_tags_separate() {
        let next_id = AtomicU32::new(1);
        let mut tags = HashMap::new();

        let brightness = resolve_id(0, Some("brightness"), &mut tags, &next_id);
        let volume = resolve_id(0, Some("volume"), &mut tags, &next_id);

        assert_ne!(brightness, volume);
        assert_eq!(tags.get("brightness"), Some(&brightness));
        assert_eq!(tags.get("volume"), Some(&volume));
    }

    // replaces_id is the client's explicit instruction and outranks our tag
    // bookkeeping — but the tag must follow the id, so the next tagged call
    // still lands on the same notification.
    #[test]
    fn resolve_id_prefers_replaces_id_over_the_tag() {
        let next_id = AtomicU32::new(1);
        let mut tags = HashMap::new();

        let first = resolve_id(0, Some("volume"), &mut tags, &next_id);
        let replaced = resolve_id(42, Some("volume"), &mut tags, &next_id);

        assert_eq!(replaced, 42);
        assert_ne!(replaced, first);
        assert_eq!(tags.get("volume"), Some(&42));
        assert_eq!(resolve_id(0, Some("volume"), &mut tags, &next_id), 42);
    }

    #[test]
    fn resolve_id_returns_replaces_id_when_untagged() {
        let next_id = AtomicU32::new(1);
        let mut tags = HashMap::new();

        assert_eq!(resolve_id(9, None, &mut tags, &next_id), 9);
        assert!(tags.is_empty());
    }
}
