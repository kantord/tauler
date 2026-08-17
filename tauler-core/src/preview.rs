//! The canvas a single component is rendered into for documentation.
//!
//! Three producers must agree exactly: `tauler-screenshot` builds it as JSON, `tauler-docgen`
//! as JSX, and `tauler-web-e2e` reproduces its crop. Disagree by a pixel and the comparison
//! is between two different pictures — so the numbers live here, not in three places.

/// Padding around the component, in logical pixels. Also the crop margin.
pub const PAD: u32 = 16;

/// Render width, including the padding.
pub const WIDTH: u32 = 400;

/// The padded outer canvas.
pub const CANVAS_CLASS: &str = "bg-background w-full flex flex-col p-[16px]";

/// The full-width frame inside it. This is what the crop measures.
pub const FRAME_CLASS: &str = "w-full flex flex-col";

#[cfg(test)]
mod tests {
    use super::*;

    /// `p-[16px]` is written out in the class string, so it cannot be derived from `PAD` —
    /// this is what stops the two drifting.
    #[test]
    fn the_canvas_class_pads_by_pad() {
        assert!(CANVAS_CLASS.contains(&format!("p-[{PAD}px]")));
    }
}
