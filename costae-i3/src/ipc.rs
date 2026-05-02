use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

pub const I3_MAGIC: &[u8; 6] = b"i3-ipc";

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

pub fn apply_bar_gap(socket: &str, dpi: f32, bar_width: u32, outer_gap: u32) {
    if let Ok(mut s) = UnixStream::connect(socket) {
        let cmd = bar_gap_command(dpi, bar_width, outer_gap);
        let _ = i3_send(&mut s, 0, cmd.as_bytes());
        let _ = i3_recv(&mut s);
    }
}

pub fn switch_workspace(socket: &str, name: &str) {
    tracing::debug!(name, socket, "switch_workspace");
    match UnixStream::connect(socket) {
        Ok(mut s) => {
            let escaped = name.replace('"', "\\\"");
            let cmd = format!("workspace \"{}\"", escaped);
            let send_ok = i3_send(&mut s, 0, cmd.as_bytes()).is_ok();
            let recv_ok = i3_recv(&mut s).is_ok();
            tracing::debug!(send_ok, recv_ok, "switch_workspace done");
        }
        Err(e) => tracing::warn!(error = %e, "switch_workspace connect failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
