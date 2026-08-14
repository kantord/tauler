//! One test per scenario: run the fixture on a real desktop, assert the
//! reservation contract, and leave a screenshot behind for CI to publish.
//!
//! `#[ignore]` by default. These need Docker and a built image, and a
//! contributor running `cargo test --workspace` should not have to care. Run
//! them with `just e2e`.
//!
//! Every expected number below is written by hand from the fixture. Computing
//! them from the edge-stack arithmetic would make the test agree with the
//! implementation by construction, including when the implementation is wrong.

use std::path::PathBuf;

use anyhow::Result;
use tauler_e2e::i3::{self, Gaps};
use tauler_e2e::{pixel_at, wait_for, Desktop, Rect, Screen};

/// What a fixture is supposed to produce on a 1920×1080 screen.
struct Expected {
    gaps: Gaps,
    panels: Vec<Rect>,
}

fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn sidebar_reserves_the_left_edge() -> Result<()> {
    run(
        "sidebar",
        Expected {
            gaps: Gaps {
                left: 272,
                right: 0,
                top: 0,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 272, 1080)],
        },
    )?;
    Ok(())
}

#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn three_edge_stack_reserves_each_edge_in_order() -> Result<()> {
    run(
        "three-edge",
        Expected {
            gaps: Gaps {
                left: 272,
                right: 0,
                top: 26,
                bottom: 26,
            },
            // The sidebar is declared first, so it is full height and the two
            // bars start 272px in — they span the space *beside* it, not above.
            panels: vec![
                rect(0, 0, 272, 1080),
                rect(272, 0, 1648, 26),
                rect(272, 1054, 1648, 26),
            ],
        },
    )?;
    Ok(())
}

/// The showcase: one floating bar over a wallpaper tauler painted itself.
///
/// Held to exactly the same contract as the two above — the extra assertions
/// after `run` are additions, not a substitute. See docs/adr/0015.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn showcase_floats_a_bar_over_its_own_wallpaper() -> Result<()> {
    let (desktop, screenshot) = run(
        "showcase",
        Expected {
            gaps: Gaps {
                left: 0,
                right: 0,
                top: 58,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 1920, 58)],
        },
    )?;

    // The clients start late by design: they wait for tauler to publish
    // _XROOTPMAP_ID before launching, so that they have a wallpaper to read.
    // Without waiting here, `assert_clients_respect` inside `run` can assert
    // over an empty list and pass without checking anything.
    wait_for("both clients on the focused workspace", || {
        let count = i3::focused_workspace_client_rects(&desktop)?.len();
        if count == 2 {
            Ok(())
        } else {
            anyhow::bail!("{count} managed windows, expected 2")
        }
    })?;

    // The panel is 58px tall and its content is inset by 12, so y=6 is inside
    // the margin, where the only thing that can be showing is the root-bg crop.
    //
    // These two points are chosen against fixtures/showcase/wallpaper.png,
    // whose top band runs from near-black on the left to a lit iris on the
    // right — change the art and these change with it. A panel that painted a
    // flat tint, or never bound root-bg at all, gives two equal samples.
    let [lr, lg, lb, _] = pixel_at(&screenshot, 100, 6)?;
    let [rr, rg, rb, _] = pixel_at(&screenshot, 1800, 6)?;
    let spread = (lr as i32 - rr as i32).abs()
        + (lg as i32 - rg as i32).abs()
        + (lb as i32 - rb as i32).abs();
    assert!(
        spread > 60,
        "the panel's margin is the same colour at x=100 and x=1800 \
         ({lr},{lg},{lb} vs {rr},{rg},{rb}) — root-bg is showing a flat fill \
         rather than a crop of the wallpaper"
    );

    // An unreadable `theme.file` is a warning, not an error: tauler falls back
    // to the shipped greyscale default and renders a perfectly correct grey
    // bar. Every other assertion in this file would still pass.
    let [br, bg, bb, _] = pixel_at(&screenshot, 960, 29)?;
    let chroma = br.abs_diff(bg).max(bg.abs_diff(bb)).max(br.abs_diff(bb));
    assert!(
        chroma >= 6,
        "the bar's own pixel is neutral ({br},{bg},{bb}) — theme.file did not \
         load and the default greyscale palette is being used"
    );

    Ok(())
}

/// Monolith III: an engraved rail, plus a notification and a launcher that are
/// ordinary free-floating panels.
///
/// The two floats are declared outside `<I3Layout>`, so they must appear at
/// their stated geometry while reserving nothing — that is the claim worth
/// checking, and `run` checks it by listing them as expected panels while the
/// gaps stay at the rail's 60.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn monolith_floats_a_launcher_and_a_slip_without_reserving_for_them() -> Result<()> {
    let (_desktop, screenshot) = run(
        "monolith",
        Expected {
            gaps: Gaps {
                left: 60,
                right: 0,
                top: 0,
                bottom: 0,
            },
            panels: vec![
                rect(0, 0, 60, 1080),
                rect(1480, 40, 400, 132),
                rect(620, 300, 680, 360),
            ],
        },
    )?;

    // The rail is a ruled plate: a 1px gold rule every 6px over #1B1924. Two
    // rows 3px apart inside the rail's empty lower margin must therefore differ.
    // A rail that fell back to a flat fill — `repeating-linear-gradient`
    // unparsed, or dropped — gives two equal samples and passes everything else.
    let [ar, ag, ab, _] = pixel_at(&screenshot, 6, 900)?;
    let [br, bg, bb, _] = pixel_at(&screenshot, 6, 903)?;
    let spread = (ar as i32 - br as i32).abs()
        + (ag as i32 - bg as i32).abs()
        + (ab as i32 - bb as i32).abs();
    assert!(
        spread > 4,
        "the rail is the same colour at y=900 and y=903 ({ar},{ag},{ab} vs \
         {br},{bg},{bb}) — the ruled plate rendered as a flat fill"
    );

    Ok(())
}

/// Signal: one generative flag per workspace, seeded from that workspace's
/// window names.
///
/// The claim worth checking is that the seed reaches the pixels — four
/// workspaces with different contents must not fly four identical flags. A
/// hash that collapsed (the `Math.imul` note in the fixture) renders a bar that
/// looks deliberate and is telling you nothing.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn signal_flies_a_different_flag_per_workspace() -> Result<()> {
    let (_desktop, screenshot) = run(
        "signal",
        Expected {
            gaps: Gaps {
                left: 0,
                right: 0,
                top: 92,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 1920, 92)],
        },
    )?;

    // Flags are 96 wide with a 12px gap, starting at the panel's 18px inset,
    // and 64 tall starting 6px down. These four points sit well inside the
    // upper-left quadrant of each flag, away from every division boundary.
    let mut swatches = Vec::new();
    for i in 0..4u32 {
        let x = 18 + i * (96 + 12) + 20;
        let [r, g, b, _] = pixel_at(&screenshot, x, 22)?;
        swatches.push([r, g, b]);
    }

    let distinct: std::collections::HashSet<[u8; 3]> = swatches.iter().copied().collect();
    assert!(
        distinct.len() >= 3,
        "the four flags show only {} distinct colours at their hoist corner \
         ({swatches:?}) — the seed is not reaching the pixels",
        distinct.len()
    );

    Ok(())
}

/// Thermal: one heat field across the whole screen, cropped per window.
///
/// Two claims beyond the contract. The measurement callout is a panel whose
/// position comes from a subprocess, so it must be mapped at the focused
/// client's own origin — a panel that ignored the module's geometry would still
/// be mapped, painted and green. And the field must be continuous: the pixels
/// either side of an i3 gap must match the wallpaper showing *in* that gap,
/// which is only true if the terminals are cropping one image rather than each
/// painting its own.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn thermal_crops_one_field_across_every_window() -> Result<()> {
    let (desktop, screenshot) = run(
        "thermal",
        Expected {
            gaps: Gaps {
                left: 0,
                right: 76,
                top: 58,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 1920, 58), rect(1844, 58, 76, 1022)],
        },
    )?;

    // The callout is the only 26px-tall window on the root. Its origin has to
    // be some client's origin, and the module reports the *focused* one.
    let callout = wait_for("the measurement callout to be mapped", || {
        desktop
            .root_windows()?
            .into_iter()
            .find(|r| r.height == 26)
            .ok_or_else(|| anyhow::anyhow!("no 26px-tall window on the root"))
    })?;
    // Within the 2px border, and not exactly on it: `_NET_ACTIVE_WINDOW` names
    // the *client* window and `xwininfo` reports where that sits, while i3
    // reports the frame it was reparented into. The two differ by
    // `default_border pixel 2` on every side. The module cannot correct for it
    // without knowing the border width, which is in the i3 config and not in
    // any property it reads.
    let clients = i3::focused_workspace_client_rects(&desktop)?;
    assert!(
        clients
            .iter()
            .any(|c| c.x.abs_diff(callout.x) <= 2 && c.y.abs_diff(callout.y) <= 2),
        "the callout is at {callout} but no client on the focused workspace \
         starts within 2px of there — the panel ignored the geometry module. \
         clients: {}",
        clients
            .iter()
            .map(Rect::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let step = |a: [u8; 4], b: [u8; 4]| {
        (a[0] as i32 - b[0] as i32).abs()
            + (a[1] as i32 - b[1] as i32).abs()
            + (a[2] as i32 - b[2] as i32).abs()
    };

    // The three columns, measured off the capture rather than guessed at:
    // borders sit at x=20..21 / 602..603, 624..625 / 1217..1218, and
    // 1239..1240 / 1822..1823, so the first gap is x=604..623. These samples
    // straddle it — inside the left window, in the gap, inside the right one —
    // at a y with no text in either column.
    //
    // Straddling is the whole assertion. An earlier version of this test
    // sampled 930/960/990, which are all *inside the middle window*: it
    // compared three points of one continuous gradient with itself and would
    // have passed against any wallpaper at all.
    let inside_left = pixel_at(&screenshot, 595, 520)?;
    let in_the_gap = pixel_at(&screenshot, 613, 520)?;
    let inside_right = pixel_at(&screenshot, 632, 520)?;

    let left_seam = step(inside_left, in_the_gap);
    let right_seam = step(in_the_gap, inside_right);
    assert!(
        left_seam < 15 && right_seam < 15,
        "the field jumps at the window edges (left seam {left_seam}, right \
         seam {right_seam}, samples {inside_left:?} {in_the_gap:?} \
         {inside_right:?}) — the terminals are painting their own backgrounds \
         rather than cropping the wallpaper"
    );

    // The third column is kitty, and it is the control: it carries a plate of
    // its own, pushed over remote control, so it must *not* match the field
    // beside it. If this ever gets as close as the seam above, the
    // set-background-image call silently did nothing and the whole per-window
    // half of the scenario is decoration.
    let gap_before_kitty = pixel_at(&screenshot, 1228, 520)?;
    let inside_kitty = pixel_at(&screenshot, 1500, 520)?;
    let kitty_step = step(gap_before_kitty, inside_kitty);
    assert!(
        kitty_step > 25,
        "kitty's interior ({inside_kitty:?}) matches the field beside it \
         ({gap_before_kitty:?}, step {kitty_step}) — `kitty @ \
         set-background-image` did not take, and the window is showing the \
         shared wallpaper like its neighbours"
    );

    Ok(())
}

/// Assert the reservation contract, then hand back the desktop and its
/// screenshot so a scenario can add claims of its own.
fn run(scenario: &str, expected: Expected) -> Result<(Desktop, PathBuf)> {
    let screen = Screen::default();
    let desktop = Desktop::start(scenario, screen)?;

    let gaps = wait_for("tauler-i3 to write the gaps", || {
        let actual = i3::focused_workspace_gaps(&desktop)?;
        if actual == expected.gaps {
            Ok(actual)
        } else {
            anyhow::bail!(
                "gaps are {actual}, expected {}\n--- tauler log ---\n{}",
                expected.gaps,
                desktop.tauler_log()
            )
        }
    })?;

    wait_for("every panel to be mapped at its declared geometry", || {
        let windows = desktop.root_windows()?;
        for panel in &expected.panels {
            if !windows.contains(panel) {
                anyhow::bail!(
                    "no window at {panel}; root has {}\n--- tauler log ---\n{}",
                    windows
                        .iter()
                        .map(Rect::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                    desktop.tauler_log()
                );
            }
        }
        Ok(())
    })?;

    i3::assert_clients_respect(gaps, screen, &i3::focused_workspace_client_rects(&desktop)?)?;

    let screenshot = desktop.capture_stable()?;

    // A mapped window that painted nothing is indistinguishable from a working
    // one in every check above. The desktop behind the panels is #181825, so a
    // panel whose centre still shows that colour never drew.
    for panel in &expected.panels {
        let (x, y) = panel.center();
        let [r, g, b, _] = pixel_at(&screenshot, x, y)?;
        assert_ne!(
            [r, g, b],
            [0x18, 0x18, 0x25],
            "panel at {panel} is still showing the bare desktop at its centre — \
             it was mapped but never painted"
        );
    }

    eprintln!("e2e: {scenario} → {}", screenshot.display());
    Ok((desktop, screenshot))
}
