use std::os::unix::net::UnixStream;

use swayipc::{Connection, Fallible, Node, Workspace};

/// Default timeout for request/reply i3 IPC queries.
pub const I3_IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Connect to the i3 socket with read/write timeouts so a wedged server
/// cannot block the caller forever.
///
/// `swayipc::Connection::new()` sets no timeout of its own (plain
/// `UnixStream::connect`), so we construct the stream ourselves and wrap it
/// via `Connection::from(..)` to keep full timeout control.
pub fn connect_with_timeout(
    socket: &str,
    timeout: std::time::Duration,
) -> std::io::Result<UnixStream> {
    let s = UnixStream::connect(socket)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    Ok(s)
}

/// Persistent request/reply connection to i3/sway.
///
/// Reuses one cached `swayipc::Connection` across requests and transparently
/// reconnects (and retries the request once) on any error.
pub struct I3Query {
    socket: String,
    timeout: std::time::Duration,
    conn: Option<Connection>,
}

impl I3Query {
    pub fn new(socket: impl Into<String>, timeout: std::time::Duration) -> Self {
        Self {
            socket: socket.into(),
            timeout,
            conn: None,
        }
    }

    /// GET_TREE: fetch the full node layout tree.
    pub fn get_tree(&mut self) -> Fallible<Node> {
        self.with_retry(|conn| conn.get_tree())
    }

    /// GET_WORKSPACES: fetch the flat workspace list.
    pub fn get_workspaces(&mut self) -> Fallible<Vec<Workspace>> {
        self.with_retry(|conn| conn.get_workspaces())
    }

    /// RUN_COMMAND: run one or more sway/i3 commands.
    pub fn run_command(&mut self, cmd: &str) -> Fallible<Vec<Fallible<()>>> {
        self.with_retry(|conn| conn.run_command(cmd))
    }

    /// Run `f` against the cached connection, reusing it across calls. On
    /// any error: drop the cached connection, reconnect once, retry `f`
    /// once; if that also fails, return Err with no cached connection left
    /// (so the next call starts fresh).
    fn with_retry<T>(&mut self, mut f: impl FnMut(&mut Connection) -> Fallible<T>) -> Fallible<T> {
        // Two attempts: the second gets a fresh connection after any error.
        for attempt in 0..2 {
            let result = self.ensure_connected().and_then(|()| {
                // A connection was just ensured above (or already present).
                f(self.conn.as_mut().expect("connection just ensured"))
            });
            match result {
                Ok(v) => return Ok(v),
                Err(e) => {
                    // A connection that errored (e.g. timed out mid-reply)
                    // is in an undefined state and must never be reused.
                    self.conn = None;
                    if attempt == 1 {
                        return Err(e);
                    }
                }
            }
        }
        unreachable!()
    }

    fn ensure_connected(&mut self) -> Fallible<()> {
        if self.conn.is_none() {
            let stream = connect_with_timeout(&self.socket, self.timeout)?;
            self.conn = Some(Connection::from(stream));
        }
        Ok(())
    }
}

/// Resolve the i3/sway IPC socket path: `I3SOCK`, then `SWAYSOCK`, then
/// `i3 --get-socketpath`, then `sway --get-socketpath`. Returns `Err` with a
/// clear message (rather than silently falling back to an empty path, which
/// would surface downstream as a confusing raw OS connect error) if none of
/// these resolve.
pub fn i3_socket_path() -> Result<String, String> {
    if let Ok(path) = std::env::var("I3SOCK") {
        return Ok(path);
    }
    if let Ok(path) = std::env::var("SWAYSOCK") {
        return Ok(path);
    }
    for wm in ["i3", "sway"] {
        if let Ok(output) = std::process::Command::new(wm)
            .arg("--get-socketpath")
            .output()
        {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }
    Err("could not determine i3/sway IPC socket path: I3SOCK and SWAYSOCK are unset, and neither `i3 --get-socketpath` nor `sway --get-socketpath` succeeded".to_string())
}

const I3_DPI_SCALE_THRESHOLD: f32 = 1.25;

// i3 only scales gaps if dpi/96 >= 1.25 (logical_px threshold in libi3/dpi.c)
fn scale_gap(dpi: f32, px: u32) -> u32 {
    if (dpi / 96.0) < I3_DPI_SCALE_THRESHOLD {
        px
    } else {
        (px as f32 * 96.0 / dpi).floor() as u32
    }
}

/// `gaps ... current set` writes to the focused workspace, so the focused
/// workspace — not the one named by the triggering event — decides what is
/// correct.
pub fn focused_output(workspaces: &[Workspace]) -> Option<&str> {
    workspaces
        .iter()
        .find(|w| w.focused)
        .map(|w| w.output.as_str())
}

/// Bar facts from the init event. Physical pixels; `right` is 0 with no
/// right-anchored panel, `output` is empty in Wayland mode.
#[derive(Debug, Clone)]
pub struct BarConfig {
    pub output: String,
    pub dpi: f32,
    pub left: u32,
    pub right: u32,
    pub outer_gap: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gaps {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

impl Gaps {
    pub const ZERO: Self = Self {
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
    };
}

/// i3 stores gaps on the workspace, so a workspace that moves off the bar's
/// output keeps its reservation unless something explicitly revokes it.
pub fn desired_gaps(focused_output: &str, cfg: &BarConfig) -> Gaps {
    if focused_output != cfg.output {
        return Gaps::ZERO;
    }
    Gaps {
        left: cfg.left,
        right: if cfg.right > 0 {
            cfg.right
        } else {
            cfg.outer_gap
        },
        top: cfg.outer_gap,
        bottom: cfg.outer_gap,
    }
}

/// Zero sides are emitted explicitly: the command can only target `current`
/// or `all`, so omitting a side leaves its stale value in place.
pub fn gaps_command(dpi: f32, gaps: &Gaps) -> String {
    [
        ("left", gaps.left),
        ("right", gaps.right),
        ("top", gaps.top),
        ("bottom", gaps.bottom),
    ]
    .iter()
    .map(|(side, px)| format!("gaps {side} current set {}", scale_gap(dpi, *px)))
    .collect::<Vec<_>>()
    .join("; ")
}

/// Wayland reserves panel space via the layer-shell exclusive zone instead,
/// and sends an empty output.
pub fn gap_management_enabled(bar_output: &str) -> bool {
    !bar_output.is_empty()
}

/// Writes unconditionally. Caching what was applied would be unsound: i3
/// emits no event when gaps change, so the cache silently goes stale after an
/// i3 restart or any external `gaps` command.
pub fn reconcile_gaps(query: &mut I3Query, cfg: &BarConfig) {
    let workspaces = match query.get_workspaces() {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!(error = %e, "reconcile_gaps: GET_WORKSPACES failed");
            return;
        }
    };
    let Some(focused) = focused_output(&workspaces) else {
        return;
    };
    let cmd = gaps_command(cfg.dpi, &desired_gaps(focused, cfg));
    if let Err(e) = query.run_command(&cmd) {
        tracing::warn!(error = %e, "reconcile_gaps: apply failed");
    }
}

pub fn switch_workspace(query: &mut I3Query, name: &str) {
    tracing::debug!(name, "switch_workspace");
    let escaped = name.replace('"', "\\\"");
    let cmd = format!("workspace \"{}\"", escaped);
    match query.run_command(&cmd) {
        Ok(_) => tracing::debug!("switch_workspace done"),
        Err(e) => tracing::warn!(error = %e, "switch_workspace failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    /// Unique socket path under the system temp dir.
    fn temp_sock(name: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "tauler-i3-{name}-{}-{nanos}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }

    /// Read one framed i3-ipc request off the wire: magic bytes, native-endian
    /// u32 length, native-endian u32 type, then the JSON payload. This mirrors
    /// `swayipc`'s own (private) `receive_from_stream`, since the crate offers
    /// no test-server utility of its own.
    fn read_i3_frame(s: &mut UnixStream) -> std::io::Result<(u32, Vec<u8>)> {
        let mut hdr = [0u8; 14];
        s.read_exact(&mut hdr)?;
        let len = u32::from_ne_bytes(hdr[6..10].try_into().unwrap()) as usize;
        let typ = u32::from_ne_bytes(hdr[10..14].try_into().unwrap());
        let mut buf = vec![0u8; len];
        s.read_exact(&mut buf)?;
        Ok((typ, buf))
    }

    /// Write one framed i3-ipc reply, matching the same wire format.
    fn write_i3_frame(s: &mut UnixStream, typ: u32, payload: &[u8]) -> std::io::Result<()> {
        s.write_all(&swayipc::MAGIC)?;
        s.write_all(&(payload.len() as u32).to_ne_bytes())?;
        s.write_all(&typ.to_ne_bytes())?;
        s.write_all(payload)
    }

    /// Serve framed request/reply cycles on one connection until EOF/error.
    /// Every request is treated as RUN_COMMAND (the only op these tests
    /// exercise) and answered with a single successful command outcome.
    fn serve_connection(s: &mut UnixStream) {
        while let Ok((typ, _payload)) = read_i3_frame(s) {
            if write_i3_frame(s, typ, b"[{\"success\":true}]").is_err() {
                break;
            }
        }
    }

    #[test]
    fn i3query_reuses_single_connection() {
        let path = temp_sock("reuse");
        let listener = UnixListener::bind(&path).unwrap();
        let accepts = Arc::new(AtomicUsize::new(0));

        let a = Arc::clone(&accepts);
        // Server: accept connections and serve multiple request/reply
        // cycles on each. Thread exits are not joined; reads end on EOF.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                a.fetch_add(1, Ordering::SeqCst);
                serve_connection(&mut s);
            }
        });

        let mut q = I3Query::new(path, Duration::from_secs(2));
        let outcomes1 = q
            .run_command("nop first")
            .expect("first request should succeed");
        assert!(outcomes1.into_iter().all(|o| o.is_ok()));
        let outcomes2 = q
            .run_command("nop second")
            .expect("second request should succeed");
        assert!(outcomes2.into_iter().all(|o| o.is_ok()));

        assert_eq!(
            accepts.load(Ordering::SeqCst),
            1,
            "both requests should reuse a single connection"
        );
    }

    #[test]
    fn i3query_reconnects_after_server_drops_connection() {
        let path = temp_sock("reconnect");
        let listener = UnixListener::bind(&path).unwrap();
        let accepts = Arc::new(AtomicUsize::new(0));

        let a = Arc::clone(&accepts);
        std::thread::spawn(move || {
            // First connection: serve exactly one request, then close it.
            if let Ok((mut s, _)) = listener.accept() {
                a.fetch_add(1, Ordering::SeqCst);
                if let Ok((typ, _)) = read_i3_frame(&mut s) {
                    let _ = write_i3_frame(&mut s, typ, b"[{\"success\":true}]");
                }
                drop(s);
            }
            // Later connections: serve requests in a loop.
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                a.fetch_add(1, Ordering::SeqCst);
                serve_connection(&mut s);
            }
        });

        let mut q = I3Query::new(path, Duration::from_secs(2));
        let outcomes1 = q
            .run_command("nop first")
            .expect("first request should succeed");
        assert!(outcomes1.into_iter().all(|o| o.is_ok()));
        // Server dropped the connection; this must transparently
        // reconnect and retry.
        let outcomes2 = q
            .run_command("nop second")
            .expect("second request should succeed via reconnect");
        assert!(outcomes2.into_iter().all(|o| o.is_ok()));

        assert_eq!(
            accepts.load(Ordering::SeqCst),
            2,
            "client should have reconnected exactly once"
        );
    }

    #[test]
    fn i3query_times_out_when_server_never_replies() {
        let path = temp_sock("timeout");
        let listener = UnixListener::bind(&path).unwrap();
        let accepts = Arc::new(AtomicUsize::new(0));

        let a = Arc::clone(&accepts);
        // Server: accept connections, read requests, never reply.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                a.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                loop {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            }
        });

        // Guard the call in a thread so a hang fails the test instead of
        // blocking the whole binary. Do not join: if it hangs, drop it.
        let (tx, rx) = mpsc::channel();
        let client_path = path.clone();
        std::thread::spawn(move || {
            let mut q = I3Query::new(client_path, Duration::from_millis(200));
            let start = Instant::now();
            let res = q.run_command("nop");
            let _ = tx.send((res, start.elapsed()));
        });

        let (res, elapsed) = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("request hung: no result within 5s");
        assert!(res.is_err(), "request should time out with Err");
        // Reconnect + retry means up to ~2x the 200ms timeout, plus slack.
        assert!(
            elapsed < Duration::from_secs(3),
            "request should fail within a bounded time, took {elapsed:?}"
        );
        assert!(
            accepts.load(Ordering::SeqCst) >= 1,
            "client should have actually connected to the server"
        );
    }

    #[test]
    fn gap_management_is_disabled_for_empty_output() {
        assert!(!gap_management_enabled(""));
    }

    #[test]
    fn gap_management_is_enabled_for_named_output() {
        assert!(gap_management_enabled("X11-1"));
    }

    #[test]
    fn gap_management_is_enabled_for_randr_output() {
        assert!(gap_management_enabled("DP-2"));
    }

    /// Build a `swayipc::Workspace` fixture. Like `Node` and `WorkspaceEvent`
    /// it is `#[non_exhaustive]`, so it can only be built by deserialization
    /// from this crate, not a struct literal.
    fn workspace(name: &str, output: &str, focused: bool) -> swayipc::Workspace {
        let value = serde_json::json!({
            "id": 1,
            "num": 1,
            "name": name,
            "visible": focused,
            "focused": focused,
            "urgent": false,
            "representation": null,
            "rect": {"x": 0, "y": 0, "width": 0, "height": 0},
            "output": output,
        });
        serde_json::from_value(value).expect("valid Workspace fixture")
    }

    #[test]
    fn focused_output_returns_the_output_of_the_focused_workspace() {
        let ws = [
            workspace("1", "DP-4", false),
            workspace("2", "DP-3", true),
            workspace("3", "HDMI-0", false),
        ];
        assert_eq!(focused_output(&ws), Some("DP-3"));
    }

    #[test]
    fn focused_output_is_none_when_nothing_is_focused() {
        let ws = [workspace("1", "DP-4", false), workspace("2", "DP-3", false)];
        assert_eq!(focused_output(&ws), None);
    }

    #[test]
    fn focused_output_is_none_for_an_empty_workspace_list() {
        assert_eq!(focused_output(&[]), None);
    }

    fn cfg(right: u32, outer_gap: u32) -> BarConfig {
        BarConfig {
            output: "DP-4".into(),
            dpi: 96.0,
            left: 272,
            right,
            outer_gap,
        }
    }

    #[test]
    fn desired_gaps_reserve_panel_widths_on_the_bar_output() {
        assert_eq!(
            desired_gaps("DP-4", &cfg(60, 8)),
            Gaps {
                left: 272,
                right: 60,
                top: 8,
                bottom: 8
            }
        );
    }

    /// The defect this fixes: a panel-less output must have the reservation
    /// revoked, not merely left alone.
    #[test]
    fn desired_gaps_are_zero_on_an_output_without_a_panel() {
        assert_eq!(desired_gaps("DP-3", &cfg(60, 8)), Gaps::ZERO);
    }

    #[test]
    fn desired_gaps_fall_back_to_outer_gap_when_there_is_no_right_panel() {
        assert_eq!(desired_gaps("DP-4", &cfg(0, 8)).right, 8);
    }

    #[test]
    fn desired_gaps_right_panel_width_takes_precedence_over_outer_gap() {
        assert_eq!(desired_gaps("DP-4", &cfg(87, 8)).right, 87);
    }

    #[test]
    fn gaps_command_always_emits_all_four_sides() {
        assert_eq!(
            gaps_command(96.0, &Gaps::ZERO),
            "gaps left current set 0; gaps right current set 0; \
             gaps top current set 0; gaps bottom current set 0"
        );
    }

    #[test]
    fn gaps_command_scales_every_side_for_high_dpi() {
        let g = Gaps {
            left: 400,
            right: 0,
            top: 16,
            bottom: 16,
        };
        assert_eq!(
            gaps_command(192.0, &g),
            "gaps left current set 200; gaps right current set 0; \
             gaps top current set 8; gaps bottom current set 8"
        );
    }

    /// Serve GET_WORKSPACES with one workspace focused on `focused_on`, and
    /// return the command string that `reconcile_gaps` writes back.
    fn reconcile_against_fake_i3(focused_on: &str, cfg: &BarConfig) -> String {
        let path = temp_sock("reconcile");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        let reply = serde_json::to_vec(&serde_json::json!([{
            "id": 1, "num": 1, "name": "1", "visible": true, "focused": true,
            "urgent": false, "representation": null,
            "rect": {"x": 0, "y": 0, "width": 0, "height": 0},
            "output": focused_on,
        }]))
        .unwrap();

        std::thread::spawn(move || {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            while let Ok((typ, payload)) = read_i3_frame(&mut s) {
                // 1 = GET_WORKSPACES, 0 = RUN_COMMAND.
                let body: &[u8] = if typ == 1 {
                    &reply
                } else {
                    let _ = tx.send(String::from_utf8_lossy(&payload).into_owned());
                    b"[{\"success\":true}]"
                };
                if write_i3_frame(&mut s, typ, body).is_err() {
                    break;
                }
            }
        });

        let mut q = I3Query::new(path, Duration::from_secs(2));
        reconcile_gaps(&mut q, cfg);
        rx.recv_timeout(Duration::from_secs(5))
            .expect("reconcile_gaps should have issued a RUN_COMMAND")
    }

    #[test]
    fn reconcile_writes_panel_widths_when_focus_is_on_the_bar_output() {
        let cmd = reconcile_against_fake_i3("DP-4", &cfg(60, 8));
        assert_eq!(
            cmd,
            "gaps left current set 272; gaps right current set 60; \
             gaps top current set 8; gaps bottom current set 8"
        );
    }

    /// End-to-end form of the fix: focus on an output with no panel must
    /// produce an explicit all-zero write.
    #[test]
    fn reconcile_revokes_gaps_when_focus_is_on_another_output() {
        let cmd = reconcile_against_fake_i3("DP-3", &cfg(60, 8));
        assert_eq!(
            cmd,
            "gaps left current set 0; gaps right current set 0; \
             gaps top current set 0; gaps bottom current set 0"
        );
    }
}
