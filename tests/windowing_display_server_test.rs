use std::os::fd::RawFd;

use tauler::windowing::{DispatchError, DisplayServer, WindowEvent};

// ---------------------------------------------------------------------------
// In-test stub — no real Wayland or X11 connection required.
// ---------------------------------------------------------------------------

struct StubServer {
    fd: RawFd,
    events: Vec<WindowEvent>,
}

impl StubServer {
    fn new(fd: RawFd, events: Vec<WindowEvent>) -> Self {
        Self { fd, events }
    }
}

impl DisplayServer for StubServer {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }

    fn dispatch(&mut self) -> Result<Vec<WindowEvent>, DispatchError> {
        Ok(std::mem::take(&mut self.events))
    }
}

// ---------------------------------------------------------------------------
// Test 1: StubServer dispatches Ok(vec![WindowEvent::OutputsChanged]).
// Proves `OutputsChanged` variant exists and the return type is correct.
// ---------------------------------------------------------------------------

#[test]
fn stub_server_dispatch_returns_outputs_changed() {
    let mut server = StubServer::new(3, vec![WindowEvent::OutputsChanged]);
    let events = server.dispatch().expect("dispatch must succeed");
    assert_eq!(events.len(), 1);
    assert!(
        matches!(events[0], WindowEvent::OutputsChanged),
        "expected OutputsChanged, got {:?}",
        events[0]
    );
}

// ---------------------------------------------------------------------------
// Test 2: DispatchError::ConnectionLost implements std::error::Error.
// Proved by calling .to_string() (requires Display, which Error requires).
// ---------------------------------------------------------------------------

#[test]
fn dispatch_error_connection_lost_implements_error() {
    let err = DispatchError::ConnectionLost;
    // std::error::Error requires Display + Debug; .to_string() exercises Display.
    let msg = err.to_string();
    // Any non-empty message is acceptable; we just confirm it doesn't panic.
    assert!(
        !msg.is_empty(),
        "DispatchError::ConnectionLost must produce a non-empty Display message"
    );
}

// ---------------------------------------------------------------------------
// Test 3: DisplayServer is object-safe — Box<dyn DisplayServer> compiles.
// Required for runtime selection between X11 and Wayland backends.
// ---------------------------------------------------------------------------

#[test]
fn display_server_is_object_safe() {
    let server: Box<dyn DisplayServer> = Box::new(StubServer::new(5, vec![]));
    // Call through the vtable to confirm object-safe dispatch works end-to-end.
    assert_eq!(server.as_raw_fd(), 5);
}
