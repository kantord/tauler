use crate::layout::SurfaceSpec;
use crate::presentation::SurfaceFrame;

pub trait DisplayManager {
    type Panel;

    fn create_window(
        &mut self,
        spec: &SurfaceSpec,
        frame: &SurfaceFrame,
    ) -> anyhow::Result<Self::Panel>;
    fn update_position(
        &mut self,
        panel: &mut Self::Panel,
        spec: &SurfaceSpec,
    ) -> anyhow::Result<()>;
    fn update_dimensions(
        &mut self,
        panel: &mut Self::Panel,
        spec: &SurfaceSpec,
    ) -> anyhow::Result<()>;
    fn update_image(&mut self, panel: &mut Self::Panel, bgrx: &[u8]) -> anyhow::Result<()>;
    fn delete_window(&mut self, panel: Self::Panel) -> anyhow::Result<()>;

    /// Paint `frame` into the desktop background of the spec's output.
    ///
    /// The frame is always exactly the output's physical size — there is no
    /// scaling or fitting here. Anything of that sort (cover, contain, tiling,
    /// gradients, solid colours) is expressed in the layout subtree and comes
    /// out of takumi already rasterized, same as for a panel.
    ///
    /// Only X11 implements this. Everywhere else the default rejects the node,
    /// which the presenter reports once, at create time.
    fn paint_wallpaper(&mut self, spec: &SurfaceSpec, _frame: &SurfaceFrame) -> anyhow::Result<()> {
        anyhow::bail!("<wallpaper id=\"{}\"> is only supported on X11", spec.id)
    }

    fn flush(&mut self) {}
}
