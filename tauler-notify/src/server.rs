use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::sync::mpsc;
use zbus::interface;
use zbus::zvariant::OwnedValue;

use crate::model::{Event, Notification};

pub struct NotifyServer {
    pub tx: mpsc::UnboundedSender<Event>,
    pub next_id: AtomicU32,
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
        let id = if replaces_id == 0 {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        } else {
            replaces_id
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
        let _ = self.tx.send(Event::Remove(id));
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
}
