use std::sync::mpsc;

use costae::layout::OutputInfo;
use costae::presentation::{PanelCommand, PresentationThread, PresenterEvent};
use costae::windowing::wayland::WaylandDisplayServer;
use costae::windowing::{DisplayServer, WindowEvent};
use super::drain_commands;

fn apply_wayland_cmd(
    pt: &mut PresentationThread<WaylandDisplayServer>,
    cmd: PanelCommand,
) {
    let PresentationThread { ref mut dm, ref mut presenter } = pt;
    if let Err(e) = presenter.apply(cmd, dm) {
        tracing::error!(error = %e, "wayland presenter apply failed");
    }
}

pub(crate) fn run_wayland_presenter_thread(
    mut pt: PresentationThread<WaylandDisplayServer>,
    command_rx: mpsc::Receiver<PanelCommand>,
    event_tx: mpsc::Sender<PresenterEvent>,
) {
    loop {
        if drain_commands(&command_rx, |cmd| apply_wayland_cmd(&mut pt, cmd)) { return; }
        pt.dm.flush();

        for (surface_id, new_size) in pt.dm.take_pending_configures() {
            for panel in pt.presenter.panels.values_mut() {
                if panel.surface_id != surface_id { continue; }
                if new_size.0 > 0 { panel.width = new_size.0; }
                if new_size.1 > 0 { panel.height = new_size.1; }
                panel.configured = true;
                let _ = event_tx.send(PresenterEvent::NeedsRender);
            }
        }

        match pt.dm.dispatch() {
            Ok(events) => {
                for event in events {
                    match event {
                        WindowEvent::OutputsChanged => {
                            if let Some((w, h)) = pt.dm.primary_output_size() {
                                let dpr = pt.dm.primary_output_scale();
                                let outputs = vec![OutputInfo {
                                    name: String::new(),
                                    x: 0,
                                    y: 0,
                                    width: w,
                                    height: h,
                                    dpr,
                                }];
                                let _ = event_tx.send(PresenterEvent::OutputsChanged { outputs });
                            }
                        }
                        WindowEvent::Click { panel_id, x_logical, y_logical, .. } => {
                            if let Some((id, panel)) = pt.presenter.panels.iter()
                                .find(|(_, p)| p.surface_id.to_string() == panel_id)
                            {
                                let (x, y, phys_width, phys_height) = scale_click_to_physical(
                                    x_logical, y_logical, panel.width, panel.height, panel.dpr,
                                );
                                let _ = event_tx.send(PresenterEvent::Click {
                                    panel_id: id.clone(),
                                    x,
                                    y,
                                    phys_width,
                                    phys_height,
                                    dpr: panel.dpr,
                                });
                            }
                        }
                    }
                }
            }
            Err(_) => {
                tracing::info!("Wayland compositor disconnected, exiting");
                return;
            }
        }
    }
}

fn scale_click_to_physical(x_logical: f32, y_logical: f32, logical_width: u32, logical_height: u32, dpr: f32) -> (f32, f32, u32, u32) {
    (
        x_logical * dpr,
        y_logical * dpr,
        (logical_width as f32 * dpr).round() as u32,
        (logical_height as f32 * dpr).round() as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::scale_click_to_physical;

    #[test]
    fn scale_click_to_physical_at_dpr_1_is_identity() {
        let result = scale_click_to_physical(50.0, 20.0, 200, 30, 1.0);
        assert_eq!(result, (50.0, 20.0, 200, 30));
    }

    #[test]
    fn scale_click_to_physical_at_dpr_2_doubles_all() {
        let result = scale_click_to_physical(50.0, 20.0, 200, 30, 2.0);
        assert_eq!(result, (100.0, 40.0, 400, 60));
    }

    #[test]
    fn scale_click_to_physical_fractional_dpr_rounds_dims() {
        let result = scale_click_to_physical(40.0, 10.0, 100, 30, 1.5);
        assert_eq!(result, (60.0, 15.0, 150, 45));
    }
}
