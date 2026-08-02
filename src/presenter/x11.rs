use std::sync::{mpsc, Arc};

use tauler::layout::OutputInfo;
use tauler::presentation::{PanelCommand, PresentationThread, PresenterEvent};
use tauler::x11::outputs::build_output_map;
use tauler::x11::panel::{put_image_chunked, resolve_panel_dpr, X11PanelContext};
use x11rb::connection::Connection as _;
use x11rb::protocol::randr::{ConnectionExt as RandrExt, NotifyMask};

use super::drain_commands;

fn apply_x11_cmd(pt: &mut PresentationThread<X11PanelContext>, cmd: PanelCommand) {
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
    command_rx: mpsc::Receiver<PanelCommand>,
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
                x11rb::protocol::Event::RandrScreenChangeNotify(_) => {
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
                    if let Some(panel) = pt.presenter.panels.values().find(|p| p.win_id == e.event)
                    {
                        let _ = event_tx.send(PresenterEvent::Click {
                            panel_id: panel.id.clone(),
                            x: e.event_x as f32,
                            y: e.event_y as f32,
                            phys_width: panel.phys_width,
                            phys_height: panel.phys_height,
                            dpr: resolve_panel_dpr(
                                panel.output.as_deref(),
                                &pt.dm.output_map,
                                pt.dm.dpr,
                            ),
                        });
                    }
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

    #[test]
    fn primary_mouse_buttons_are_dispatchable() {
        assert!(is_dispatchable_button(1), "button 1 (left) should dispatch");
        assert!(
            is_dispatchable_button(2),
            "button 2 (middle) should dispatch"
        );
        assert!(is_dispatchable_button(3), "button 3 (right) should dispatch");
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

    #[test]
    fn extra_mouse_buttons_are_dispatchable() {
        assert!(is_dispatchable_button(8), "button 8 (back) should dispatch");
        assert!(
            is_dispatchable_button(9),
            "button 9 (forward) should dispatch"
        );
    }
}
