use tauler::windowing::wayland::{
    build_dispatch_result, WaylandConnectError, WaylandDisplayServer,
};
use tauler::windowing::DisplayServer;
use tauler::windowing::{DispatchError, WindowEvent};

// ---------------------------------------------------------------------------
// Test 1: connect() fails gracefully when WAYLAND_DISPLAY is unset.
// In CI (X11-only, no compositor) WAYLAND_DISPLAY is absent; connect() must
// return the connection-failure variant rather than panicking or hanging.
// ---------------------------------------------------------------------------

#[test]
fn connect_without_wayland_display_returns_err() {
    // Ensure both connection paths are closed — WAYLAND_SOCKET is a fallback
    // that connect_to_env() also checks (used in nested-compositor setups).
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("WAYLAND_SOCKET");

    let result = WaylandDisplayServer::connect();

    assert!(
        result.is_err(),
        "WaylandDisplayServer::connect() must return Err when no compositor is reachable"
    );
    assert!(
        matches!(result.unwrap_err(), WaylandConnectError::Connect(_)),
        "error must be the Connect variant"
    );
}

// ---------------------------------------------------------------------------
// Test 2: WaylandConnectError::Connect implements std::error::Error.
// Proved by calling .to_string() (requires Display, which std::error::Error
// mandates) and by binding to `&dyn std::error::Error`.
// ---------------------------------------------------------------------------

#[test]
fn wayland_connect_error_implements_std_error() {
    // Construct the variant with an inner message.
    let err = WaylandConnectError::Connect("no compositor".to_string());

    // std::error::Error requires Display + Debug; .to_string() exercises Display.
    let msg = err.to_string();
    assert!(
        !msg.is_empty(),
        "WaylandConnectError::Connect must produce a non-empty Display message"
    );

    // Confirm the type can be used as a trait object.
    let _as_error: &dyn std::error::Error = &err;
}

// ---------------------------------------------------------------------------
// Test 3: WaylandDisplayServer implements the DisplayServer trait.
// This is a compile-time check only — the function below refuses to compile
// if WaylandDisplayServer does not implement DisplayServer.
// ---------------------------------------------------------------------------

fn _needs_display_server(_: &dyn DisplayServer) {}

#[test]
fn wayland_display_server_implements_display_server() {
    // We cannot actually obtain a WaylandDisplayServer without a compositor,
    // so we prove the impl exists via compile-time coercion: a function that
    // takes `&dyn DisplayServer` must accept `&WaylandDisplayServer`.
    // The compile-time check is expressed as an explicit function pointer cast.
    let _coerce: fn(&WaylandDisplayServer) -> () =
        |s| _needs_display_server(s as &dyn DisplayServer);

    // The test body itself is trivially true; the interesting assertion is the
    // static type-check above that prevents this file from compiling if the
    // trait impl is absent.
    assert!(
        true,
        "WaylandDisplayServer implements DisplayServer (compile-time proven)"
    );
}

// ---------------------------------------------------------------------------
// Tests for build_dispatch_result — a pure helper that maps (dispatch_ok,
// flush_ok, pending) → Result<Vec<WindowEvent>, DispatchError>.
// ---------------------------------------------------------------------------

// Test 4: Both flags true + non-empty pending → Ok with exact events returned.
// The function must consume (not clone) pending and hand ownership to the caller.
#[test]
fn build_dispatch_result_both_ok_returns_pending_events() {
    let events = vec![WindowEvent::OutputsChanged, WindowEvent::OutputsChanged];
    let result = build_dispatch_result(true, true, events);
    assert!(result.is_ok(), "expected Ok when both flags are true");
    let returned = result.unwrap();
    assert_eq!(
        returned.len(),
        2,
        "returned vec must contain all pending events"
    );
    assert!(
        matches!(returned[0], WindowEvent::OutputsChanged),
        "first event must be OutputsChanged"
    );
}

// Test 5: dispatch_ok = false → Err(ConnectionLost) regardless of flush_ok.
#[test]
fn build_dispatch_result_dispatch_false_returns_connection_lost() {
    let result_flush_true = build_dispatch_result(false, true, vec![]);
    assert!(
        matches!(result_flush_true, Err(DispatchError::ConnectionLost)),
        "dispatch_ok=false flush_ok=true must yield ConnectionLost"
    );

    let result_flush_false = build_dispatch_result(false, false, vec![]);
    assert!(
        matches!(result_flush_false, Err(DispatchError::ConnectionLost)),
        "dispatch_ok=false flush_ok=false must yield ConnectionLost"
    );
}

// Test 6: flush_ok = false → Err(ConnectionLost) regardless of dispatch_ok.
#[test]
fn build_dispatch_result_flush_false_returns_connection_lost() {
    let result = build_dispatch_result(true, false, vec![]);
    assert!(
        matches!(result, Err(DispatchError::ConnectionLost)),
        "dispatch_ok=true flush_ok=false must yield ConnectionLost"
    );
}

// Test 7: Both flags true + empty pending → Ok(vec![]) — no events, no error.
#[test]
fn build_dispatch_result_both_ok_empty_pending_returns_ok_empty() {
    let result = build_dispatch_result(true, true, vec![]);
    assert!(
        result.is_ok(),
        "expected Ok when both flags are true and pending is empty"
    );
    assert!(
        result.unwrap().is_empty(),
        "returned vec must be empty when no events were pending"
    );
}
