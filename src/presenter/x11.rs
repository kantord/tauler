use std::sync::{mpsc, Arc};

use tauler::layout::OutputInfo;
use tauler::presentation::{
    PointerEvent, PointerPhase, PresentationThread, PresenterEvent, PresenterEvents, SurfaceCommand,
};
use tauler::x11::outputs::{build_output_map, randr_event_mask, resolve_primary_output_name};
use tauler::x11::panel::{context_dpi_dpr, put_image_chunked, X11PanelContext};
use x11rb::connection::Connection as _;
use x11rb::protocol::randr::ConnectionExt as RandrExt;

use super::drain_commands;

/// X11 reports which buttons are held as a modifier mask; the DOM numbers them
/// differently, and handlers are given the DOM's numbering (`docs/adr/0020`).
///
/// The mask describes the state *before* the event, so a press has to add the button
/// going down and a release has to take away the one coming up — otherwise pressing
/// the primary button reports nothing held.
fn dom_buttons(state: x11rb::protocol::xproto::KeyButMask) -> u16 {
    const X11_BUTTON1: u16 = 1 << 8;
    const X11_BUTTON2: u16 = 1 << 9;
    const X11_BUTTON3: u16 = 1 << 10;
    let state = u16::from(state);
    let mut buttons = 0;
    if state & X11_BUTTON1 != 0 {
        buttons |= 1; // primary
    }
    if state & X11_BUTTON3 != 0 {
        buttons |= 2; // secondary — X11's third button is the DOM's second bit
    }
    if state & X11_BUTTON2 != 0 {
        buttons |= 4; // auxiliary
    }
    buttons
}

/// The DOM bit for the button this event is about.
fn dom_button(detail: u8) -> u16 {
    match detail {
        1 => 1,
        2 => 4,
        3 => 2,
        _ => 0,
    }
}

/// Send one pointer event for whichever panel owns `win`.
///
/// Press, motion and release differ only in `phase` — same geometry, same routing.
/// What they mean apart is decided by the capture state machine in `app`.
fn send_pointer(
    event_tx: &PresenterEvents,
    pt: &PresentationThread<X11PanelContext>,
    win: u32,
    x: i16,
    y: i16,
    phase: PointerPhase,
    buttons: u16,
) {
    let Some(panel) = pt.presenter.panels.values().find(|p| p.win_id == win) else {
        return;
    };
    let _ = event_tx.send(PresenterEvent::Pointer(PointerEvent {
        panel_id: panel.id.clone(),
        x: x as f32,
        y: y as f32,
        phys_width: panel.phys_width,
        phys_height: panel.phys_height,
        dpr: panel.dpr,
        phase,
        buttons,
        trace: tauler::trace::Trace::begin(),
    }));
}

fn apply_x11_cmd(pt: &mut PresentationThread<X11PanelContext>, cmd: SurfaceCommand) {
    let PresentationThread {
        ref mut dm,
        ref mut presenter,
    } = pt;
    if let Err(e) = presenter.apply(cmd, dm) {
        tracing::error!(error = %e, "x11 presenter apply failed");
    }
}

/// X11 has no separate scroll event: wheel motion arrives as button presses
/// 4-7 (up, down, left, right). Only real buttons should become clicks.
fn is_dispatchable_button(detail: u8) -> bool {
    !matches!(detail, 4..=7)
}

pub(crate) fn run_x11_presenter_thread(
    mut pt: PresentationThread<X11PanelContext>,
    command_rx: mpsc::Receiver<SurfaceCommand>,
    event_tx: PresenterEvents,
) {
    let _ = pt
        .dm
        .conn
        .randr_select_input(pt.dm.root, randr_event_mask());
    let _ = pt.dm.conn.flush();

    loop {
        if drain_commands(&command_rx, |cmd| apply_x11_cmd(&mut pt, cmd)) {
            return;
        }
        // Not every command flushes its own requests (Delete and Move don't), and
        // `poll_for_event` never flushes — without this a removed panel stays on
        // screen until some other panel's repaint pushes the buffer out.
        let _ = pt.dm.conn.flush();

        while let Some(event) = pt.dm.conn.poll_for_event().unwrap_or(None) {
            match event {
                x11rb::protocol::Event::RandrScreenChangeNotify(e) => {
                    // The root screen itself can grow or shrink here, not just the
                    // monitors on it — strut math and the wallpaper pixmap are both
                    // sized against it.
                    pt.dm.root_screen_width = e.width as u32;
                    pt.dm.root_screen_height = e.height as u32;
                    let new_map = build_output_map(&pt.dm.conn, pt.dm.root);
                    let primary_name =
                        resolve_primary_output_name(&pt.dm.conn, pt.dm.root, &new_map);
                    let (_dpi, context_dpr) = context_dpi_dpr(&pt.dm.conn, pt.dm.root);
                    let outputs: Vec<OutputInfo> = new_map.values().cloned().collect();
                    pt.dm.output_map = Arc::new(new_map);
                    let _ = event_tx.send(PresenterEvent::OutputsChanged {
                        outputs,
                        primary_name,
                        context_dpr,
                    });
                }
                x11rb::protocol::Event::RandrNotify(e)
                    if e.sub_code == x11rb::protocol::randr::Notify::OUTPUT_CHANGE =>
                {
                    // Output-only change (e.g. a bare `xrandr --primary` switch) — no
                    // root-screen-size fields on this event, so root_screen_width/height
                    // are left untouched.
                    let new_map = build_output_map(&pt.dm.conn, pt.dm.root);
                    let primary_name =
                        resolve_primary_output_name(&pt.dm.conn, pt.dm.root, &new_map);
                    let (_dpi, context_dpr) = context_dpi_dpr(&pt.dm.conn, pt.dm.root);
                    let outputs: Vec<OutputInfo> = new_map.values().cloned().collect();
                    pt.dm.output_map = Arc::new(new_map);
                    let _ = event_tx.send(PresenterEvent::OutputsChanged {
                        outputs,
                        primary_name,
                        context_dpr,
                    });
                }
                x11rb::protocol::Event::Expose(e) => {
                    if let Some(panel) = pt.presenter.panels.values().find(|p| p.win_id == e.window)
                    {
                        let _ = put_image_chunked(
                            &pt.dm.conn,
                            panel.win_id,
                            panel.gc,
                            panel.phys_width,
                            pt.dm.depth,
                            &panel.bgrx[..],
                        );
                        let _ = pt.dm.conn.flush();
                    }
                }
                x11rb::protocol::Event::ButtonPress(e) if is_dispatchable_button(e.detail) => {
                    let held = dom_buttons(e.state) | dom_button(e.detail);
                    let phase = PointerPhase::Press;
                    send_pointer(&event_tx, &pt, e.event, e.event_x, e.event_y, phase, held);
                }
                // The pointer moved with button 1 down. X11 grabs the pointer to the
                // window the press landed in, so these keep arriving even once it has
                // left the panel — which is exactly what a capture wants (ADR 0020).
                x11rb::protocol::Event::MotionNotify(e) => {
                    let phase = PointerPhase::Move;
                    let held = dom_buttons(e.state);
                    send_pointer(&event_tx, &pt, e.event, e.event_x, e.event_y, phase, held);
                }
                x11rb::protocol::Event::ButtonRelease(e) if is_dispatchable_button(e.detail) => {
                    let held = dom_buttons(e.state) & !dom_button(e.detail);
                    let phase = PointerPhase::Release;
                    send_pointer(&event_tx, &pt, e.event, e.event_x, e.event_y, phase, held);
                }
                x11rb::protocol::Event::Error(e) => {
                    tracing::error!(error = ?e, "X11 async error");
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::KeyButMask;

    #[test]
    fn primary_mouse_buttons_are_dispatchable() {
        assert!(is_dispatchable_button(1), "button 1 (left) should dispatch");
        assert!(
            is_dispatchable_button(2),
            "button 2 (middle) should dispatch"
        );
        assert!(
            is_dispatchable_button(3),
            "button 3 (right) should dispatch"
        );
    }

    #[test]
    fn scroll_wheel_buttons_are_not_dispatchable() {
        assert!(
            !is_dispatchable_button(4),
            "button 4 (wheel up) should be ignored"
        );
        assert!(
            !is_dispatchable_button(5),
            "button 5 (wheel down) should be ignored"
        );
        assert!(
            !is_dispatchable_button(6),
            "button 6 (wheel left) should be ignored"
        );
        assert!(
            !is_dispatchable_button(7),
            "button 7 (wheel right) should be ignored"
        );
    }

    /// X11's mask describes the state *before* the event, so a press reporting no
    /// buttons held is the bug this guards against.
    #[test]
    fn a_primary_press_reports_the_button_going_down() {
        let none = KeyButMask::from(0u16);
        assert_eq!(dom_buttons(none), 0, "nothing held before the press");
        assert_eq!(dom_buttons(none) | dom_button(1), 1, "primary is bit 0");
    }

    /// The DOM numbers the buttons differently from X11: X11's third button is the
    /// DOM's second bit, and X11's second is the DOM's third.
    #[test]
    fn the_secondary_and_auxiliary_buttons_are_renumbered_for_the_dom() {
        assert_eq!(dom_button(3), 2, "right-hand button");
        assert_eq!(dom_button(2), 4, "middle button");
        assert_eq!(
            dom_buttons(KeyButMask::from(1u16 << 10)),
            2,
            "button 3 held"
        );
        assert_eq!(dom_buttons(KeyButMask::from(1u16 << 9)), 4, "button 2 held");
    }

    #[test]
    fn holding_two_buttons_reports_both() {
        let held = KeyButMask::from((1u16 << 8) | (1u16 << 10));
        assert_eq!(dom_buttons(held), 1 | 2);
    }

    /// A release has to take its own button away, or the last event of a drag would
    /// still claim the button is down.
    #[test]
    fn a_release_takes_away_the_button_coming_up() {
        let held = KeyButMask::from(1u16 << 8);
        assert_eq!(dom_buttons(held) & !dom_button(1), 0);
    }

    #[test]
    fn extra_mouse_buttons_are_dispatchable() {
        assert!(is_dispatchable_button(8), "button 8 (back) should dispatch");
        assert!(
            is_dispatchable_button(9),
            "button 9 (forward) should dispatch"
        );
    }

    /// A presenter thread on the real display, plus a second connection to watch it
    /// with. Requests still sitting in the presenter's write buffer are invisible to
    /// any other client, so every observation goes through `observer`.
    struct LivePresenter {
        commands: mpsc::Sender<SurfaceCommand>,
        thread: std::thread::JoinHandle<()>,
        observer: x11rb::rust_connection::RustConnection,
        root: u32,
        _events: (mpsc::Receiver<PresenterEvent>, mpsc::Receiver<()>),
    }

    /// Tests run in parallel on one display, and `create` finds its window by size,
    /// so each test passes its own `height`.
    const PANEL_W: u32 = 137;

    fn panel_spec(id: &str, x: i32, height: u32) -> tauler::layout::SurfaceSpec {
        tauler::layout::SurfaceSpec {
            kind: tauler::layout::SurfaceKind::Panel,
            id: id.to_string(),
            anchor: None,
            width: PANEL_W,
            height,
            x,
            y: 0,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 1.0,
        }
    }

    fn blank_frame(height: u32) -> tauler::presentation::SurfaceFrame {
        tauler::presentation::SurfaceFrame {
            pixels: Arc::new(vec![0u8; (PANEL_W * height * 4) as usize]),
            width: PANEL_W,
            height,
            trace: None,
        }
    }

    fn poll_until(timeout_ms: u64, mut done: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        while std::time::Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        done()
    }

    impl LivePresenter {
        fn start() -> Option<Self> {
            let Ok(init) = crate::init_x11() else {
                println!("SKIP: no X11 display available");
                return None;
            };
            let (observer, screen) = x11rb::rust_connection::RustConnection::connect(None).ok()?;
            let root = observer.setup().roots[screen].root;
            let (commands, command_rx) = mpsc::channel();
            let (event_tx, event_rx) = mpsc::channel();
            let (notify_tx, notify_rx) = mpsc::sync_channel(1);
            let events = PresenterEvents::new(event_tx, notify_tx);
            let pt = PresentationThread::new(init.panel_ctx);
            let thread =
                std::thread::spawn(move || run_x11_presenter_thread(pt, command_rx, events));
            Some(LivePresenter {
                commands,
                thread,
                observer,
                root,
                _events: (event_rx, notify_rx),
            })
        }

        fn children(&self) -> Vec<u32> {
            use x11rb::protocol::xproto::ConnectionExt as _;
            self.observer
                .query_tree(self.root)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|t| t.children)
                .unwrap_or_default()
        }

        /// Root-relative geometry as another client sees it; `None` once the window
        /// is gone (panels are override-redirect, so the WM never reparents them).
        fn geometry(&self, win: u32) -> Option<(i16, i16, u16, u16)> {
            use x11rb::protocol::xproto::ConnectionExt as _;
            let g = self.observer.get_geometry(win).ok()?.reply().ok()?;
            Some((g.x, g.y, g.width, g.height))
        }

        /// Create a panel and return its window, found by its distinctive size among
        /// the root's new children.
        fn create(&self, spec: tauler::layout::SurfaceSpec) -> u32 {
            let before = self.children();
            let height = spec.height;
            self.commands
                .send(SurfaceCommand::Create {
                    spec,
                    frame: blank_frame(height),
                })
                .unwrap();
            let mut found = None;
            poll_until(1000, || {
                found = self.children().into_iter().find(|w| {
                    !before.contains(w)
                        && matches!(self.geometry(*w), Some((_, _, w, h))
                            if u32::from(w) == PANEL_W && u32::from(h) == height)
                });
                found.is_some()
            });
            found.expect("the created panel's window should appear on the display")
        }

        fn shutdown(self) {
            let _ = self.commands.send(SurfaceCommand::Shutdown);
            let _ = self.thread.join();
        }
    }

    /// Delete only queues a DestroyWindow; unless the loop flushes after applying
    /// commands, the panel stays on screen until some other panel happens to repaint.
    #[test]
    fn a_deleted_panel_disappears_without_another_repaint() {
        let Some(live) = LivePresenter::start() else {
            return;
        };
        let win = live.create(panel_spec("flush-delete", 0, 23));

        live.commands
            .send(SurfaceCommand::Delete {
                id: "flush-delete".into(),
            })
            .unwrap();
        let gone = poll_until(500, || live.geometry(win).is_none());

        live.shutdown();
        assert!(
            gone,
            "window {win:#x} is still on the display 500ms after Delete"
        );
    }

    /// Move only queues a ConfigureWindow; unless the loop flushes after applying
    /// commands, the panel stays where it was until some other panel repaints.
    #[test]
    fn a_moved_panel_moves_without_another_repaint() {
        let Some(live) = LivePresenter::start() else {
            return;
        };
        let win = live.create(panel_spec("flush-move", 0, 29));
        let (x0, ..) = live.geometry(win).expect("panel window should exist");

        live.commands
            .send(SurfaceCommand::Move(panel_spec("flush-move", 41, 29)))
            .unwrap();
        let moved = poll_until(
            500,
            || matches!(live.geometry(win), Some((x, ..)) if x != x0),
        );
        let x_after = live.geometry(win).map(|(x, ..)| x);

        let _ = live.commands.send(SurfaceCommand::Delete {
            id: "flush-move".into(),
        });
        live.shutdown();
        assert!(
            moved,
            "window {win:#x} still at x={x_after:?} (was {x0}) 500ms after Move"
        );
    }
}
