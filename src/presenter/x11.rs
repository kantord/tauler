use std::sync::{mpsc, Arc};

use tauler::layout::OutputInfo;
use tauler::presentation::{
    PointerEvent, PointerPhase, PresentationThread, PresenterEvent, SurfaceCommand,
};
use tauler::x11::outputs::build_output_map;
use tauler::x11::panel::{put_image_chunked, resolve_panel_dpr, X11PanelContext};
use x11rb::connection::Connection as _;
use x11rb::protocol::randr::{ConnectionExt as RandrExt, NotifyMask};

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
    event_tx: &mpsc::Sender<PresenterEvent>,
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
        dpr: resolve_panel_dpr(panel.output.as_deref(), &pt.dm.output_map, pt.dm.dpr),
        phase,
        buttons,
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
    event_tx: mpsc::Sender<PresenterEvent>,
) {
    let _ = pt
        .dm
        .conn
        .randr_select_input(pt.dm.root, NotifyMask::SCREEN_CHANGE);
    let _ = pt.dm.conn.flush();

    loop {
        if drain_commands(&command_rx, |cmd| apply_x11_cmd(&mut pt, cmd)) {
            return;
        }

        while let Some(event) = pt.dm.conn.poll_for_event().unwrap_or(None) {
            match event {
                x11rb::protocol::Event::RandrScreenChangeNotify(e) => {
                    // The root screen itself can grow or shrink here, not just the
                    // monitors on it — strut math and the wallpaper pixmap are both
                    // sized against it.
                    pt.dm.root_screen_width = e.width as u32;
                    pt.dm.root_screen_height = e.height as u32;
                    let new_map = build_output_map(&pt.dm.conn, pt.dm.root);
                    let outputs: Vec<OutputInfo> = new_map.values().cloned().collect();
                    pt.dm.output_map = Arc::new(new_map);
                    let _ = event_tx.send(PresenterEvent::OutputsChanged { outputs });
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
}
