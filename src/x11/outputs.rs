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

/// An output's density, in device pixels per CSS pixel.
///
/// Deliberately rotation-invariant: RandR's CRTC `width`/`height` are
/// *post*-rotation pixel dimensions, but `mm_width`/`mm_height` stay the
/// output's *native*, pre-rotation physical size — so dividing the "natural"
/// pixel axis by the "natural" mm axis (as if rotation never happened) is
/// permanently wrong on any 90/270-rotated output (issue #525 bug #2).
/// Rotation only swaps which named field holds which value; taking the max
/// of each pair cancels that swap out, since the pixel axis paired with the
/// larger physical dimension is the same one before and after rotation.
///
/// This also mostly subsumes bug #1 (a CRTC mid-mode-change can transiently
/// report `mm_height == 0`): the max survives as long as at least one mm
/// axis is nonzero. `.max(1.0)` only guards the much rarer case of both
/// axes reading zero at once, so the result stays finite instead of a
/// bogus 1.0 or a division by zero.
fn compute_dpr(crtc_width: f32, crtc_height: f32, mm_width: f32, mm_height: f32) -> f32 {
    let px = crtc_width.max(crtc_height);
    let mm = mm_width.max(mm_height).max(1.0);
    (px / mm) / (96.0 / MM_PER_INCH)
}

/// How many RandR readings [`build_output_map_settled`] takes before giving up
/// and trusting whatever it last read, and how long it waits between them.
const SETTLE_MAX_READS: u32 = 5;
const SETTLE_INTERVAL: Duration = Duration::from_millis(50);

/// Poll `read` until two consecutive readings agree, or `max_reads` is spent —
/// then return the last reading regardless.
///
/// Split out from [`build_output_map_settled`] so the retry logic itself can
/// be unit-tested without a live X server.
fn settle_output_map(
    mut read: impl FnMut() -> HashMap<String, OutputInfo>,
    max_reads: u32,
    interval: Duration,
) -> HashMap<String, OutputInfo> {
    let mut prev = read();
    for _ in 1..max_reads {
        thread::sleep(interval);
        let next = read();
        if next == prev {
            return next;
        }
        prev = next;
    }
    prev
}

/// [`build_output_map`], but re-read and compared against a second (and if
/// needed a few more) RandR reading before being trusted.
///
/// Right after a reboot or relogin, RandR can transiently report a wrong CRTC
/// `y` before external-monitor negotiation with the display server has fully
/// settled — and unlike DPR or primary-output selection, a panel's position is
/// resolved once at window-creation time and never re-queried unless a later
/// output-change event fires, which may never happen if RandR's state settles
/// silently (issue #537). Call this instead of `build_output_map` wherever the
/// result feeds straight into creating windows, i.e. at startup.
pub fn build_output_map_settled(conn: &RustConnection, root: u32) -> HashMap<String, OutputInfo> {
    settle_output_map(
        || build_output_map(conn, root),
        SETTLE_MAX_READS,
        SETTLE_INTERVAL,
    )
}

/// True when `context_dpr` (Xft.dpi/96 — what `ctx.screen_width` and an
/// implicit-primary panel are computed against) and `output_dpr` (an
/// output's own RandR-mm density) disagree by more than `threshold`,
/// relative to `output_dpr`. Purely diagnostic — issue #525 bug #6 already
/// fixed the behavior that made this divergence matter; this just surfaces
/// it in logs so a squished-panel report can be triaged without re-deriving
/// both numbers by hand.
pub fn dprs_diverge(context_dpr: f32, output_dpr: f32, threshold: f32) -> bool {
    output_dpr > 0.0 && ((context_dpr - output_dpr).abs() / output_dpr) > threshold
}

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
                                let dpr = compute_dpr(
                                    crtc.width as f32,
                                    crtc.height as f32,
                                    info.mm_width as f32,
                                    info.mm_height as f32,
                                );
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

/// Resolve which output RandR currently reports as primary, by name.
///
/// Trusts RandR's live primary-output flag first, falling back to
/// [`fallback_output_name`] when RandR reports no primary at all (a bare X
/// server has none). Meant to be called again on every output-change event,
/// not just at startup — issue #525 bug #3 was a *runtime* handler trusting
/// an unordered `HashMap`'s iteration order (or, before that, a name
/// resolved once and never revisited) instead of re-asking RandR who is
/// primary now.
pub fn resolve_primary_output_name(
    conn: &RustConnection,
    root: u32,
    outputs: &HashMap<String, OutputInfo>,
) -> String {
    conn.randr_get_output_primary(root)
        .ok()
        .and_then(|c| c.reply().ok())
        .filter(|r| r.output != 0)
        .and_then(|r| conn.randr_get_output_info(r.output, 0).ok()?.reply().ok())
        .map(|info| String::from_utf8_lossy(&info.name).into_owned())
        .or_else(|| fallback_output_name(outputs))
        .unwrap_or_default()
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

/// The RandR notify mask both output-watching threads subscribe with. `OUTPUT_CHANGE`
/// catches a bare `xrandr --primary` switch or other output-only change with no
/// resolution change — `SCREEN_CHANGE` alone misses that (issue #525's proposed
/// design flagged this as a "Must").
pub fn randr_event_mask() -> randr::NotifyMask {
    randr::NotifyMask::SCREEN_CHANGE | randr::NotifyMask::OUTPUT_CHANGE
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

    #[test]
    fn dpr_formula_is_rotation_invariant() {
        // RandR reports post-rotation pixel dimensions but native (pre-rotation)
        // physical ones, so a 90-degree-rotated reading swaps only the CRTC pixel
        // axes, never mm_width/mm_height — the computed density must not change.
        let unrotated = compute_dpr(1920.0, 1080.0, 600.0, 340.0);
        let rotated_90 = compute_dpr(1080.0, 1920.0, 600.0, 340.0);
        assert!(
            (unrotated - rotated_90).abs() < 0.001,
            "rotating an output must not change its computed DPR: {unrotated} vs {rotated_90}"
        );
    }

    #[test]
    fn dpr_formula_survives_one_zero_mm_axis() {
        // A CRTC mid-mode-change can transiently report mm_height == 0. Falling
        // back to the other axis (rather than silently substituting 1.0) keeps
        // the output's real density instead of discarding it.
        let dpr = compute_dpr(1920.0, 1080.0, 600.0, 0.0);
        assert!(dpr.is_finite() && dpr > 0.0);
        assert!((dpr - compute_dpr(1920.0, 1080.0, 600.0, 340.0)).abs() < 0.001);
    }

    #[test]
    fn dprs_diverge_flags_a_large_relative_gap_but_not_a_small_one() {
        // Issue #525's worked example: Xft.dpi ~= 95 (context_dpr = 95.0 / 96.0
        // ~= 0.99) against an EDID-derived RandR-mm density of roughly 1.5 for a
        // dense panel misread at its native mm size. Relative gap here is
        // (1.5 - 0.99) / 1.5 ~= 0.34, which is well past a 10% threshold.
        assert!(dprs_diverge(0.99, 1.5, 0.1));

        // A near-equal pair: (1.02 - 1.0) / 1.0 = 0.02, under the 10% threshold.
        assert!(!dprs_diverge(1.02, 1.0, 0.1));
    }

    #[test]
    fn dpr_formula_does_not_divide_by_zero_when_both_mm_axes_are_zero() {
        let dpr = compute_dpr(1920.0, 1080.0, 0.0, 0.0);
        assert!(dpr.is_finite() && dpr > 0.0);
    }

    fn map_with_y(y: i16) -> HashMap<String, OutputInfo> {
        HashMap::from([output("HDMI-1", 0, y)])
    }

    #[test]
    fn settle_output_map_confirms_a_stable_first_reading() {
        let result = settle_output_map(|| map_with_y(0), 5, Duration::from_millis(0));
        assert_eq!(result.get("HDMI-1").unwrap().y, 0);
    }

    #[test]
    fn settle_output_map_ignores_a_transient_bad_first_reading() {
        let mut calls = 0;
        let result = settle_output_map(
            || {
                calls += 1;
                map_with_y(if calls == 1 { -10 } else { 0 })
            },
            5,
            Duration::from_millis(0),
        );
        assert_eq!(
            result.get("HDMI-1").unwrap().y,
            0,
            "a bad first reading must not stick once later readings agree"
        );
    }

    #[test]
    fn settle_output_map_gives_up_after_max_reads_if_never_stable() {
        let mut calls = 0;
        settle_output_map(
            || {
                calls += 1;
                map_with_y(calls) // a different reading every time
            },
            3,
            Duration::from_millis(0),
        );
        assert_eq!(
            calls, 3,
            "must stop reading after max_reads even if never stable"
        );
    }

    #[test]
    fn randr_event_mask_includes_screen_and_output_change() {
        let mask = randr_event_mask();
        assert_ne!(
            u16::from(mask) & u16::from(randr::NotifyMask::SCREEN_CHANGE),
            0,
            "must include SCREEN_CHANGE"
        );
        assert_ne!(
            u16::from(mask) & u16::from(randr::NotifyMask::OUTPUT_CHANGE),
            0,
            "must include OUTPUT_CHANGE"
        );
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

    if let Err(e) = conn.randr_select_input(root, randr_event_mask()) {
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
            Ok(Some(event)) => match event {
                x11rb::protocol::Event::RandrScreenChangeNotify(_) => {
                    emit_outputs(&conn, root, &key, &tx);
                }
                x11rb::protocol::Event::RandrNotify(e)
                    if e.sub_code == randr::Notify::OUTPUT_CHANGE =>
                {
                    emit_outputs(&conn, root, &key, &tx);
                }
                _ => {}
            },
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
