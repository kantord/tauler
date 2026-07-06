use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

pub const I3_MAGIC: &[u8; 6] = b"i3-ipc";

/// Default timeout for request/reply i3 IPC queries.
pub const I3_IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Connect to the i3 socket with read/write timeouts so a wedged server
/// cannot block the caller forever.
pub fn connect_with_timeout(
    socket: &str,
    timeout: std::time::Duration,
) -> std::io::Result<UnixStream> {
    let s = UnixStream::connect(socket)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    Ok(s)
}

pub fn i3_send(s: &mut UnixStream, msg_type: u32, payload: &[u8]) -> std::io::Result<()> {
    s.write_all(I3_MAGIC)?;
    s.write_all(&(payload.len() as u32).to_le_bytes())?;
    s.write_all(&msg_type.to_le_bytes())?;
    s.write_all(payload)
}

pub fn i3_recv(s: &mut UnixStream) -> std::io::Result<(u32, Vec<u8>)> {
    let mut hdr = [0u8; 14];
    s.read_exact(&mut hdr)?;
    let len = u32::from_le_bytes(hdr[6..10].try_into().unwrap()) as usize;
    let typ = u32::from_le_bytes(hdr[10..14].try_into().unwrap());
    let mut buf = vec![0u8; len];
    s.read_exact(&mut buf)?;
    Ok((typ, buf))
}

/// Persistent request/reply connection to i3.
///
/// Reuses one cached connection across requests and transparently
/// reconnects (and retries the request once) on any I/O error.
pub struct I3Query {
    socket: String,
    timeout: std::time::Duration,
    conn: Option<UnixStream>,
}

impl I3Query {
    pub fn new(socket: impl Into<String>, timeout: std::time::Duration) -> Self {
        Self {
            socket: socket.into(),
            timeout,
            conn: None,
        }
    }

    /// Send one request and read its reply, reusing the cached connection.
    /// On any I/O error (including timeout): drop the cached connection,
    /// reconnect once, retry the request once; if that also fails, return
    /// Err with no cached connection left (so the next call starts fresh).
    pub fn request(&mut self, msg_type: u32, payload: &[u8]) -> std::io::Result<(u32, Vec<u8>)> {
        // Two attempts: the second gets a fresh connection after any error.
        for attempt in 0..2 {
            match self.try_request(msg_type, payload) {
                Ok(reply) => return Ok(reply),
                Err(e) => {
                    // A stream that errored (e.g. timed out mid-reply) is in
                    // an undefined state and must never be reused.
                    self.conn = None;
                    if attempt == 1 {
                        return Err(e);
                    }
                }
            }
        }
        unreachable!()
    }

    fn try_request(&mut self, msg_type: u32, payload: &[u8]) -> std::io::Result<(u32, Vec<u8>)> {
        if self.conn.is_none() {
            self.conn = Some(connect_with_timeout(&self.socket, self.timeout)?);
        }
        let s = self.conn.as_mut().unwrap();
        i3_send(s, msg_type, payload)?;
        i3_recv(s)
    }
}

pub fn i3_socket_path() -> String {
    if let Ok(path) = std::env::var("I3SOCK") {
        return path;
    }
    if let Ok(path) = std::env::var("SWAYSOCK") {
        return path;
    }
    std::process::Command::new("i3")
        .arg("--get-socketpath")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
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

pub fn bar_gap_command(dpi: f32, bar_width: u32, outer_gap: u32) -> String {
    let left = scale_gap(dpi, bar_width);
    let og = scale_gap(dpi, outer_gap);
    if og == 0 {
        format!("gaps left current set {left}")
    } else {
        format!(
            "gaps left current set {left}; gaps top current set {og}; gaps right current set {og}; gaps bottom current set {og}"
        )
    }
}

/// Returns true when gap commands should be sent — only in X11/i3 mode where the WM
/// needs IPC gap commands to reserve sidebar space. In Wayland mode the layer-shell
/// exclusive zone handles this, so output is always "".
pub fn should_apply_bar_gap(output: &str) -> bool {
    !output.is_empty()
}

pub fn apply_bar_gap(query: &mut I3Query, dpi: f32, bar_width: u32, outer_gap: u32) {
    let cmd = bar_gap_command(dpi, bar_width, outer_gap);
    if let Err(e) = query.request(0, cmd.as_bytes()) {
        tracing::warn!(error = %e, "apply_bar_gap failed");
    }
}

pub fn switch_workspace(query: &mut I3Query, name: &str) {
    tracing::debug!(name, "switch_workspace");
    let escaped = name.replace('"', "\\\"");
    let cmd = format!("workspace \"{}\"", escaped);
    match query.request(0, cmd.as_bytes()) {
        Ok(_) => tracing::debug!("switch_workspace done"),
        Err(e) => tracing::warn!(error = %e, "switch_workspace failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Serve framed request/reply cycles on one connection until EOF/error.
    /// Echoes the request type back with payload `{}`.
    fn serve_connection(s: &mut UnixStream) {
        while let Ok((typ, _payload)) = i3_recv(s) {
            if i3_send(s, typ, b"{}").is_err() {
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
        let (typ1, payload1) = q.request(1, b"").expect("first request should succeed");
        assert_eq!(typ1, 1);
        assert_eq!(payload1, b"{}");
        let (typ2, payload2) = q.request(4, b"").expect("second request should succeed");
        assert_eq!(typ2, 4);
        assert_eq!(payload2, b"{}");

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
                if let Ok((typ, _)) = i3_recv(&mut s) {
                    let _ = i3_send(&mut s, typ, b"{}");
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
        let (typ1, _) = q.request(1, b"").expect("first request should succeed");
        assert_eq!(typ1, 1);
        // Server dropped the connection; this must transparently
        // reconnect and retry.
        let (typ2, payload2) = q
            .request(4, b"")
            .expect("second request should succeed via reconnect");
        assert_eq!(typ2, 4);
        assert_eq!(payload2, b"{}");

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
            let res = q.request(1, b"");
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
    fn should_apply_bar_gap_returns_false_for_empty_output() {
        assert!(!should_apply_bar_gap(""));
    }

    #[test]
    fn should_apply_bar_gap_returns_true_for_named_output() {
        assert!(should_apply_bar_gap("X11-1"));
    }

    #[test]
    fn should_apply_bar_gap_returns_true_for_randr_output() {
        assert!(should_apply_bar_gap("DP-2"));
    }

    #[test]
    fn bar_gap_command_sets_only_left_when_outer_gap_zero() {
        let cmd = bar_gap_command(96.0, 200, 0);
        assert_eq!(cmd, "gaps left current set 200");
    }

    #[test]
    fn bar_gap_command_sets_all_four_gaps_when_outer_gap_nonzero() {
        let cmd = bar_gap_command(96.0, 200, 8);
        assert_eq!(
            cmd,
            "gaps left current set 200; gaps top current set 8; gaps right current set 8; gaps bottom current set 8"
        );
    }

    #[test]
    fn bar_gap_command_scales_gaps_for_high_dpi() {
        // At DPI 192 (dpr=2.0), i3 scales gaps itself, so we divide back by dpr
        let cmd = bar_gap_command(192.0, 400, 16);
        assert_eq!(
            cmd,
            "gaps left current set 200; gaps top current set 8; gaps right current set 8; gaps bottom current set 8"
        );
    }
}
