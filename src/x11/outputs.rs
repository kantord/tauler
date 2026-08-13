use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::randr::{self, ConnectionExt as RandrExt};
use x11rb::rust_connection::RustConnection;

use crate::data::data_loop::{StreamItem, StreamKind};
use crate::layout::OutputInfo;

const MM_PER_INCH: f32 = 25.4;

pub fn build_output_map(conn: &RustConnection, root: u32) -> HashMap<String, OutputInfo> {
    let mut map = HashMap::new();
    if let Ok(cookie) = conn.randr_get_screen_resources_current(root) {
        if let Ok(resources) = cookie.reply() {
            for &out_id in &resources.outputs {
                if let Ok(info_cookie) = conn.randr_get_output_info(out_id, 0) {
                    if let Ok(info) = info_cookie.reply() {
                        if info.crtc == 0 {
                            continue;
                        }
                        if let Ok(crtc_cookie) = conn.randr_get_crtc_info(info.crtc, 0) {
                            if let Ok(crtc) = crtc_cookie.reply() {
                                let name = String::from_utf8_lossy(&info.name).into_owned();
                                let dpr = if info.mm_height > 0 {
                                    (crtc.height as f32 / info.mm_height as f32)
                                        / (96.0 / MM_PER_INCH)
                                } else {
                                    1.0
                                };
                                map.insert(
                                    name.clone(),
                                    OutputInfo {
                                        name,
                                        x: crtc.x,
                                        y: crtc.y,
                                        width: crtc.width as u32,
                                        height: crtc.height as u32,
                                        dpr,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    map
}

/// Which output to treat as the primary one when RandR names none.
///
/// A session started by a display manager always has a primary output, but a
/// bare X server — Xvfb, an X session started by hand — has none, and RandR
/// answers `GetOutputPrimary` with 0. Asking for that output's info is a
/// protocol error, so tauler cannot simply trust the reply.
///
/// Prefers the output at the origin, since that is the one whose logical size
/// matches the root window, and falls back to the first name in sorted order so
/// that two runs against the same screen agree.
pub fn fallback_output_name(outputs: &HashMap<String, OutputInfo>) -> Option<String> {
    let at_origin = outputs
        .values()
        .filter(|o| o.x == 0 && o.y == 0)
        .min_by(|a, b| a.name.cmp(&b.name));
    let chosen = at_origin.or_else(|| outputs.values().min_by(|a, b| a.name.cmp(&b.name)))?;
    Some(chosen.name.clone())
}

// Emits the current monitor layout as a JSON array of {name, x, y, width, height,
// screen_width, screen_height, dpr} objects, where screen_* are logical-pixel dimensions.
fn emit_outputs(conn: &RustConnection, root: u32, key: &str, tx: &mpsc::Sender<StreamItem>) {
    let map = build_output_map(conn, root);
    let outputs: Vec<serde_json::Value> = map
        .values()
        .map(|info| {
            serde_json::json!({
                "name": info.name,
                "x": info.x,
                "y": info.y,
                "width": info.width,
                "height": info.height,
                "screen_width":  (info.width as f32 / info.dpr).round() as u32,
                "screen_height": (info.height as f32 / info.dpr).round() as u32,
                "dpr": info.dpr,
            })
        })
        .collect();
    let line = serde_json::to_string(&outputs).unwrap_or_default();
    let _ = tx.send(StreamItem {
        key: (key.to_string(), None),
        stream: StreamKind::Stdout,
        line,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(name: &str, x: i16, y: i16) -> (String, OutputInfo) {
        (
            name.to_string(),
            OutputInfo {
                name: name.to_string(),
                x,
                y,
                width: 1920,
                height: 1080,
                dpr: 1.0,
            },
        )
    }

    #[test]
    fn no_outputs_means_no_fallback() {
        assert_eq!(fallback_output_name(&HashMap::new()), None);
    }

    #[test]
    fn prefers_the_output_at_the_origin() {
        let outputs = HashMap::from([output("DP-2", 1920, 0), output("HDMI-1", 0, 0)]);
        assert_eq!(
            fallback_output_name(&outputs).as_deref(),
            Some("HDMI-1"),
            "the output at the origin is the one whose size matches the root window"
        );
    }

    #[test]
    fn picks_the_same_output_every_time() {
        // HashMap iteration order varies per process, so an unordered pick would
        // make the bar's logical size differ between two runs on one machine.
        let outputs = HashMap::from([output("DP-2", 1920, 200), output("HDMI-1", 0, 400)]);
        assert_eq!(fallback_output_name(&outputs).as_deref(), Some("DP-2"));
    }
}

pub fn outputs_thread(tx: mpsc::Sender<StreamItem>, key: String, stop: Arc<AtomicBool>) {
    let (conn, screen_num) = match RustConnection::connect(None) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "outputs_thread: X11 connect failed");
            return;
        }
    };
    let screen = conn.setup().roots[screen_num].clone();
    let root = screen.root;

    if let Err(e) = conn.randr_select_input(root, randr::NotifyMask::SCREEN_CHANGE) {
        tracing::error!(error = %e, "outputs_thread: randr_select_input failed");
        return;
    }
    let _ = conn.flush();

    emit_outputs(&conn, root, &key, &tx);

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match conn.poll_for_event() {
            Ok(Some(event)) => {
                if matches!(event, x11rb::protocol::Event::RandrScreenChangeNotify(_)) {
                    emit_outputs(&conn, root, &key, &tx);
                }
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                tracing::error!(error = %e, "outputs_thread: X11 error");
                break;
            }
        }
    }
}
