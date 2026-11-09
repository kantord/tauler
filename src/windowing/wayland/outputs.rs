//! What the Wayland backend knows about outputs, as plain data.
//!
//! sctk hands out one `OutputInfo` per `wl_output`; the pipeline wants
//! `layout::OutputInfo` — a name, a rect in **physical** pixels, and a dpr — and
//! it divides `width`/`height` by `dpr` to get the logical screen size. The
//! conversion, the choice of primary output, and the lookup of the output a
//! surface asked for all live here as pure functions so they can be tested
//! without a compositor.
//!
//! Wayland has no primary output and no stable enumeration order, so "primary"
//! is a rule of ours: the output at the lowest `(x, y)`, ties broken by name.
//! That is the leftmost, then topmost output, and it does not change when
//! outputs are plugged in a different order.

use crate::layout::OutputInfo;

use super::compute_output_scale;

/// The fields of sctk's `OutputInfo` that matter here, detached from sctk.
#[derive(Debug, Clone, Default)]
pub(crate) struct OutputSnapshot {
    /// `wl_output` v4 / xdg-output name (`DP-1`, `HEADLESS-2`), if the
    /// compositor reports one.
    pub name: Option<String>,
    /// The `wl_output` global's registry id, for naming an unnamed output.
    pub id: u32,
    pub logical_position: Option<(i32, i32)>,
    pub logical_size: Option<(i32, i32)>,
    /// Size of the current video mode, in physical pixels.
    pub current_mode: Option<(i32, i32)>,
    pub scale_factor: i32,
}

/// The pipeline's view of one output, or `None` when the compositor has not
/// said how big it is.
///
/// `width`/`height` are the current mode's size — physical pixels, and the
/// same number `dpr` was derived from, so `width / dpr` is the logical width.
/// Without a mode they are the logical size times the integer scale.
///
/// `x`/`y` are the output's logical position times its own `dpr`, so they
/// share units with `width`/`height`. Wayland has no global physical
/// coordinate space; with mixed scales two outputs' rects can overlap in these
/// units, which is harmless because every consumer pairs a surface with its
/// output by name and measures from that output's own origin.
pub(crate) fn output_info(snap: &OutputSnapshot) -> Option<OutputInfo> {
    let (width, height) = match (snap.current_mode, snap.logical_size) {
        (Some((w, h)), _) if w > 0 && h > 0 => (w, h),
        (_, Some((w, h))) if w > 0 && h > 0 => (
            (w as f32 * snap.scale_factor as f32).round() as i32,
            (h as f32 * snap.scale_factor as f32).round() as i32,
        ),
        _ => return None,
    };
    // Derived from the size reported above, so `width / dpr` is the logical
    // width whichever branch produced it.
    let dpr = compute_output_scale(
        snap.logical_size.map_or(0, |(w, _)| w.max(0) as u32),
        width as u32,
        snap.scale_factor,
    );
    let (lx, ly) = snap.logical_position.unwrap_or((0, 0));
    let name = snap
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("wayland-{}", snap.id));
    Some(OutputInfo {
        name,
        x: to_i16((lx as f32 * dpr).round() as i32),
        y: to_i16((ly as f32 * dpr).round() as i32),
        width: width as u32,
        height: height as u32,
        dpr,
    })
}

fn to_i16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// Index of the primary output: lowest `(x, y)`, then lowest name.
pub(crate) fn primary_index<T>(entries: &[(OutputInfo, T)]) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .min_by(|(_, (a, _)), (_, (b, _))| (a.x, a.y, &a.name).cmp(&(b.x, b.y, &b.name)))
        .map(|(i, _)| i)
}

/// Which output a surface lands on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub index: usize,
    /// `wanted` named an output that does not exist, so the primary was used.
    pub fell_back: bool,
}

/// The output named `wanted`; the primary when `wanted` is `None` or names an
/// output that is not there. `None` only when there are no outputs at all.
pub(crate) fn select_output<T>(
    entries: &[(OutputInfo, T)],
    wanted: Option<&str>,
) -> Option<Selection> {
    let primary = primary_index(entries)?;
    let Some(name) = wanted else {
        return Some(Selection {
            index: primary,
            fell_back: false,
        });
    };
    Some(match entries.iter().position(|(o, _)| o.name == name) {
        Some(index) => Selection {
            index,
            fell_back: false,
        },
        None => Selection {
            index: primary,
            fell_back: true,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(name: &str) -> OutputSnapshot {
        OutputSnapshot {
            name: Some(name.to_string()),
            id: 1,
            logical_position: Some((0, 0)),
            logical_size: Some((1920, 1080)),
            current_mode: Some((1920, 1080)),
            scale_factor: 1,
        }
    }

    fn info(name: &str, x: i16, y: i16) -> (OutputInfo, ()) {
        (
            OutputInfo {
                name: name.to_string(),
                x,
                y,
                width: 1920,
                height: 1080,
                dpr: 1.0,
            },
            (),
        )
    }

    #[test]
    fn a_scale_one_output_reports_its_logical_size_as_physical() {
        let o = output_info(&snap("DP-1")).unwrap();
        assert_eq!((o.width, o.height, o.dpr), (1920, 1080, 1.0));
    }

    #[test]
    fn a_fractional_scale_output_reports_physical_pixels_not_logical() {
        // 2560x1440 mode shown at 1.5x is a 1706x960 logical desktop. The
        // pipeline divides by dpr to get logical, so width must be the mode.
        let o = output_info(&OutputSnapshot {
            logical_size: Some((1706, 960)),
            current_mode: Some((2560, 1440)),
            scale_factor: 2,
            ..snap("DP-1")
        })
        .unwrap();
        assert_eq!((o.width, o.height), (2560, 1440));
        assert_eq!((o.width as f32 / o.dpr).round() as u32, 1706);
    }

    #[test]
    fn without_a_mode_physical_size_is_logical_times_the_scale() {
        let o = output_info(&OutputSnapshot {
            logical_size: Some((1706, 960)),
            current_mode: None,
            scale_factor: 2,
            ..snap("DP-1")
        })
        .unwrap();
        assert_eq!((o.width, o.height, o.dpr), (3412, 1920, 2.0));
    }

    #[test]
    fn position_is_in_the_same_units_as_the_size() {
        let o = output_info(&OutputSnapshot {
            logical_position: Some((1706, 0)),
            logical_size: Some((1706, 960)),
            current_mode: Some((2560, 1440)),
            ..snap("DP-2")
        })
        .unwrap();
        assert_eq!((o.x, o.y), (2560, 0));
    }

    #[test]
    fn an_output_with_no_size_at_all_is_not_reported() {
        assert!(output_info(&OutputSnapshot {
            logical_size: None,
            current_mode: None,
            ..snap("DP-1")
        })
        .is_none());
    }

    #[test]
    fn an_unnamed_output_gets_a_name_from_its_id_never_an_empty_one() {
        let o = output_info(&OutputSnapshot {
            name: None,
            id: 7,
            ..snap("")
        })
        .unwrap();
        assert_eq!(o.name, "wayland-7");
    }

    #[test]
    fn primary_is_the_leftmost_output_whatever_order_they_arrive_in() {
        let entries = vec![info("B", 1920, 0), info("A", 0, 0)];
        assert_eq!(primary_index(&entries), Some(1));
    }

    #[test]
    fn primary_breaks_a_position_tie_by_name() {
        let entries = vec![info("B", 0, 0), info("A", 0, 0)];
        assert_eq!(primary_index(&entries), Some(1));
    }

    #[test]
    fn primary_of_no_outputs_is_none() {
        assert_eq!(primary_index::<()>(&[]), None);
    }

    #[test]
    fn a_named_output_is_selected_by_name() {
        let entries = vec![info("A", 0, 0), info("B", 1920, 0)];
        assert_eq!(
            select_output(&entries, Some("B")),
            Some(Selection {
                index: 1,
                fell_back: false
            })
        );
    }

    #[test]
    fn no_name_selects_the_primary() {
        let entries = vec![info("B", 1920, 0), info("A", 0, 0)];
        assert_eq!(
            select_output(&entries, None),
            Some(Selection {
                index: 1,
                fell_back: false
            })
        );
    }

    #[test]
    fn an_unknown_name_falls_back_to_the_primary_and_says_so() {
        let entries = vec![info("A", 0, 0), info("B", 1920, 0)];
        assert_eq!(
            select_output(&entries, Some("Z")),
            Some(Selection {
                index: 0,
                fell_back: true
            })
        );
    }

    #[test]
    fn selecting_among_no_outputs_is_none() {
        assert_eq!(select_output::<()>(&[], Some("A")), None);
    }
}
