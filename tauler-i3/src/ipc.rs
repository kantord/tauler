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

/// The output of the workspace the default seat currently has focused.
///
/// `gaps ... current set` writes to the *focused* workspace, so this — not
/// the workspace named by whichever event triggered a reconcile — decides
/// which gaps are correct. Event payloads name the workspace the event is
/// *about*, which for an urgency hint or a workspace being emptied is
/// routinely a background workspace on a different output.
pub fn focused_output(workspaces: &[Workspace]) -> Option<&str> {
    workspaces
        .iter()
        .find(|w| w.focused)
        .map(|w| w.output.as_str())
}

/// Panel geometry as reported by tauler core in the init event, in physical
/// pixels. `right` is 0 when there is no right-anchored panel.
#[derive(Debug, Clone, Copy)]
pub struct BarGeometry {
    pub left: u32,
    pub right: u32,
    pub outer_gap: u32,
}

impl BarGeometry {
    pub fn new(left: u32, right: u32, outer_gap: u32) -> Self {
        Self {
            left,
            right,
            outer_gap,
        }
    }
}

/// Outer gaps for a single workspace, in unscaled physical pixels.
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

/// The gaps a workspace on `focused_output` should have, given the panels
/// live on `bar_output`.
///
/// Only the output carrying the panels reserves space; every other output
/// gets zeros. i3 stores per-workspace gaps on the workspace itself, so a
/// workspace that moves off the bar's output keeps its reservation unless
/// something explicitly revokes it — nothing else will.
pub fn desired_gaps(focused_output: &str, bar_output: &str, geom: BarGeometry) -> Gaps {
    if focused_output != bar_output {
        return Gaps::ZERO;
    }
    Gaps {
        left: geom.left,
        // A right panel's width is the real space needing reservation; the
        // decorative outer_gap only stands in when there is no right panel.
        right: if geom.right > 0 {
            geom.right
        } else {
            geom.outer_gap
        },
        top: geom.outer_gap,
        bottom: geom.outer_gap,
    }
}

/// Render `gaps` as an i3 command targeting the focused workspace.
///
/// Every side is emitted even when zero. The runtime `gaps` command can only
/// address `current` or `all` (i3 rejects named-workspace criteria), so
/// revoking a stale reservation means writing an explicit 0 — omitting the
/// side would leave the old value in place.
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

/// Returns true when tauler should manage gaps at all — only in X11/i3 mode,
/// where the WM needs IPC gap commands to reserve panel space. In Wayland
/// mode the layer-shell exclusive zone handles reservation, and the init
/// event carries an empty output.
pub fn gap_management_enabled(bar_output: &str) -> bool {
    !bar_output.is_empty()
}

/// Resolve the focused workspace's output, derive the gaps it should have,
/// and write them.
///
/// Unconditional by design. Reading the current value back to skip a
/// redundant write would buy nothing — writes cost the same as reads — and
/// caching what was last applied is unsound: i3 emits no event whatsoever
/// when gaps change, so a cache would go silently stale after an i3 restart
/// or any external `gaps` command. Re-asserting the truth on every trigger
/// is what keeps this correct.
pub fn reconcile_gaps(query: &mut I3Query, dpi: f32, bar_output: &str, geom: BarGeometry) {
    let workspaces = match query.get_workspaces() {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!(error = %e, "reconcile_gaps: GET_WORKSPACES failed");
            return;
        }
    };
    let Some(focused) = focused_output(&workspaces) else {
        tracing::debug!("reconcile_gaps: no focused workspace, nothing to write");
        return;
    };
    let cmd = gaps_command(dpi, &desired_gaps(focused, bar_output, geom));
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

    /// The bar output reserves real panel space on every side it occupies.
    #[test]
    fn desired_gaps_reserve_panel_widths_on_the_bar_output() {
        let g = desired_gaps("DP-4", "DP-4", BarGeometry::new(272, 60, 8));
        assert_eq!(
            g,
            Gaps {
                left: 272,
                right: 60,
                top: 8,
                bottom: 8
            }
        );
    }

    /// The defect this PR exists to fix: an output with no panel must have
    /// tauler's reservation revoked, not merely left alone.
    #[test]
    fn desired_gaps_are_zero_on_an_output_without_a_panel() {
        let g = desired_gaps("DP-3", "DP-4", BarGeometry::new(272, 60, 8));
        assert_eq!(g, Gaps::ZERO);
    }

    #[test]
    fn desired_gaps_fall_back_to_outer_gap_when_there_is_no_right_panel() {
        let g = desired_gaps("DP-4", "DP-4", BarGeometry::new(272, 0, 8));
        assert_eq!(g.right, 8);
    }

    #[test]
    fn desired_gaps_right_panel_width_takes_precedence_over_outer_gap() {
        let g = desired_gaps("DP-4", "DP-4", BarGeometry::new(272, 87, 8));
        assert_eq!(g.right, 87);
    }

    /// Revocation is only expressible if zero-valued sides are emitted
    /// explicitly — omitting them would leave a stale gap in place.
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
}
