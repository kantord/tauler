#[cfg(target_os = "linux")]
pub mod wayland;

#[derive(Debug)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Other(u32),
}

#[derive(Debug)]
pub enum WindowEvent {
    Click {
        panel_id: String,
        x_logical: f32,
        y_logical: f32,
        button: MouseButton,
    },
    OutputsChanged,
}

#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("connection lost")]
    ConnectionLost,
}

pub trait DisplayServer {
    fn as_raw_fd(&self) -> std::os::fd::RawFd;
    fn dispatch(&mut self) -> Result<Vec<WindowEvent>, DispatchError>;
}
