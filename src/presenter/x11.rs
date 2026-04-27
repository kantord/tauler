use std::sync::{mpsc, Arc};

use costae::layout::OutputInfo;
use costae::presentation::{PanelCommand, PresentationThread, PresenterEvent};
use costae::x11::outputs::build_output_map;
use costae::x11::panel::X11PanelContext;
use x11rb::connection::Connection as _;
use x11rb::protocol::randr::{ConnectionExt as RandrExt, NotifyMask};
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};

use super::drain_commands;

fn apply_x11_cmd(
    pt: &mut PresentationThread<X11PanelContext>,
    cmd: PanelCommand,
) {
    let PresentationThread { ref mut dm, ref mut presenter } = pt;
    if let Err(e) = presenter.apply(cmd, dm) {
        tracing::error!(error = %e, "x11 presenter apply failed");
    }
}

pub(crate) fn run_x11_presenter_thread(
    mut pt: PresentationThread<X11PanelContext>,
    command_rx: mpsc::Receiver<PanelCommand>,
    event_tx: mpsc::Sender<PresenterEvent>,
) {
    let _ = pt.dm.conn.randr_select_input(pt.dm.root, NotifyMask::SCREEN_CHANGE);
    let _ = pt.dm.conn.flush();

    loop {
        if drain_commands(&command_rx, |cmd| apply_x11_cmd(&mut pt, cmd)) { return; }

        while let Some(event) = pt.dm.conn.poll_for_event().unwrap_or(None) {
            match event {
                x11rb::protocol::Event::RandrScreenChangeNotify(_) => {
                    let new_map = build_output_map(&pt.dm.conn, pt.dm.root);
                    let outputs: Vec<OutputInfo> = new_map.values().cloned().collect();
                    pt.dm.output_map = Arc::new(new_map);
                    let _ = event_tx.send(PresenterEvent::OutputsChanged { outputs });
                }
                x11rb::protocol::Event::Expose(e) => {
                    if let Some(panel) = pt.presenter.panels.values().find(|p| p.win_id == e.window) {
                        let _ = pt.dm.conn.put_image(ImageFormat::Z_PIXMAP, panel.win_id, panel.gc,
                            panel.phys_width as u16, panel.phys_height as u16, 0, 0, 0, pt.dm.depth, &panel.bgrx[..]);
                        let _ = pt.dm.conn.flush();
                    }
                }
                x11rb::protocol::Event::ButtonPress(e) => {
                    if let Some(panel) = pt.presenter.panels.values().find(|p| p.win_id == e.event) {
                        let _ = event_tx.send(PresenterEvent::Click {
                            panel_id: panel.id.clone(),
                            x: e.event_x as f32,
                            y: e.event_y as f32,
                            phys_width: panel.phys_width,
                            phys_height: panel.phys_height,
                            dpr: pt.dm.dpr,
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
