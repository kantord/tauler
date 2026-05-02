use crate::layout::PanelSpecData;
use crate::presentation::PanelFrame;

pub trait DisplayManager {
    type Panel;

    fn create_window(
        &mut self,
        spec: &PanelSpecData,
        frame: &PanelFrame,
    ) -> anyhow::Result<Self::Panel>;
    fn update_position(
        &mut self,
        panel: &mut Self::Panel,
        spec: &PanelSpecData,
    ) -> anyhow::Result<()>;
    fn update_dimensions(
        &mut self,
        panel: &mut Self::Panel,
        spec: &PanelSpecData,
    ) -> anyhow::Result<()>;
    fn update_image(&mut self, panel: &mut Self::Panel, bgrx: &[u8]) -> anyhow::Result<()>;
    fn delete_window(&mut self, panel: Self::Panel) -> anyhow::Result<()>;
    fn flush(&mut self) {}
}
