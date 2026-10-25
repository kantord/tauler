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

use std::path::{Path, PathBuf};

use anyhow::Result;
use tauler_e2e::i3::{self, Gaps};
use tauler_e2e::{pixel_at, wait_for, Desktop, Rect, Screen};

/// What a fixture is supposed to produce on a 1920×1080 screen.
struct Expected {
    gaps: Gaps,
    panels: Vec<Rect>,
    /// How many managed clients belong on the focused workspace.
    ///
    /// `run` waits for exactly this many before photographing anything. A
    /// fixture's clients start in the background — they have to, since they
    /// wait on tauler and tauler starts after them — so "the panels are up"
    /// does not mean "the desktop is populated". Without this, `capture_stable`
    /// can settle on two consecutive identical frames of a half-built desktop
    /// and every assertion afterwards is made against the wrong picture.
    clients: usize,
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
        "sidebar-reserves-the-left-edge",
        Expected {
            gaps: Gaps {
                left: 272,
                right: 0,
                top: 0,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 272, 1080)],
            clients: 2,
        },
    )?;
    Ok(())
}

#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn three_edge_stack_reserves_each_edge_in_order() -> Result<()> {
    run(
        "three-edge",
        "three-edge-stack-reserves-each-edge-in-order",
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
            clients: 2,
        },
    )?;
    Ok(())
}

/// `<Workspaces>`: a sidebar, then a single frame panel spanning the rest of the
/// screen, with a 12px border (`p-3`) reserved around the tiled area inside it.
/// Nothing here restates 12px as a size — the fixture's wrapper only says `p-3`,
/// and the gaps below are what `<Workspaces>` measured from it. One panel, not
/// four strips: independently-rasterized panels seamed at every shared boundary
/// under `backdrop-filter`, so the whole free rect renders as one panel instead.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn workspaces_frames_the_area_beside_the_sidebar() -> Result<()> {
    let (_desktop, screenshot) = run(
        "workspaces-frame",
        "workspaces-frames-the-area-beside-the-sidebar",
        Expected {
            gaps: Gaps {
                left: 272 + 12,
                right: 12,
                top: 12,
                bottom: 12,
            },
            panels: vec![rect(0, 0, 272, 1080), rect(272, 0, 1648, 1080)],
            clients: 2,
        },
    )?;

    // The middle of the 12px band on the left and bottom edges, clear of the
    // windows. The frame is #ff00ff and the wallpaper under it #00ff00, so green
    // here means the panel's own `tauler:root-bg` backdrop painted over the frame.
    for (x, y) in [(272 + 6, 540), (1100, 1080 - 6)] {
        let [r, g, b, _] = pixel_at(&screenshot, x, y)?;
        assert_eq!(
            [r, g, b],
            [0xff, 0x00, 0xff],
            "the frame band at ({x}, {y}) is not the wrapper's colour"
        );
    }
    Ok(())
}

/// How many pixels of `png` inside `x0..x1`, `y0..y1` are exactly `rgb`.
fn pixels_of(
    png: &std::path::Path,
    x: std::ops::Range<u32>,
    y: std::ops::Range<u32>,
    rgb: [u8; 3],
) -> usize {
    let img = image::open(png).expect("decode screenshot").to_rgba8();
    y.flat_map(|y| x.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| img.get_pixel_checked(x, y).is_some_and(|p| p.0[..3] == rgb))
        .count()
}

/// `#rrggbb` as bytes.
fn rgb_of(theme: &tauler_core::theme::Theme, token: &str) -> [u8; 3] {
    let hex = &theme.colors[token];
    let byte = |i: usize| u8::from_str_radix(&hex[1 + 2 * i..3 + 2 * i], 16).expect("hex colour");
    [byte(0), byte(1), byte(2)]
}

/// What a built-in theme is expected to draw, written by hand from its theme file.
struct Palette {
    background: [u8; 3],
    sidebar: [u8; 3],
    popover: [u8; 3],
}

/// A built-in theme, selected by `theme.name` alone — no theme file in the fixture.
///
/// The desktop is a left rail, two free-floating plates (a notification and a launcher), two
/// kitty terminals and a GTK4 file manager (Nautilus), each drawn from the tokens a UI would use for that surface. Pixels where
/// nothing else is drawn are therefore exact tokens: the rail's bottom edge is the theme's
/// `sidebar`, the launcher's bottom-right padding its `popover`, and an empty stretch of a
/// terminal its `background` — which kitty only has because tauler wrote it from this theme (`KittyTheme`).
/// That proves the name resolved to the built-in, the mode picked the right palette, and that
/// each surface reached for its own token.
fn builtin_theme_scenario(
    scenario: &str,
    devtools_name: &str,
    theme_name: &str,
    expected: Palette,
) -> Result<()> {
    // Photographed at twice the density: the screen is 3840×2160 at 192 dpi, which tauler and
    // kitty both read from Xft.dpi, so every logical size below doubles in pixels. The
    // screenshot is the same desktop with four times the pixels — sharp when it is shown large,
    // where a 1920×1080 capture is enlarged and blurs.
    const K: u32 = 2;
    let screen = Screen {
        width: 1920 * K,
        height: 1080 * K,
        dpi: 96 * K,
    };
    let theme = tauler_core::theme::builtin::by_name(theme_name).expect("built-in theme");
    let k = K as i32;
    let (_desktop, screenshot) = run_on(
        screen,
        scenario,
        devtools_name,
        Expected {
            gaps: Gaps {
                left: 272 * K,
                right: 0,
                top: 0,
                bottom: 0,
            },
            panels: vec![
                rect(0, 0, 272 * K, 1080 * K),
                rect(1480 * k, 40 * k, 400 * K, 132 * K),
                rect(620 * k, 300 * k, 680 * K, 360 * K),
            ],
            clients: 3,
        },
    )?;
    let [r, g, b, _] = pixel_at(&screenshot, 136 * K, 1070 * K)?;
    assert_eq!(
        [r, g, b],
        expected.sidebar,
        "the rail's own pixel is not the theme's `sidebar`"
    );
    // Inside the launcher's 16px padding, below its footer row, inside the 1px border.
    let [r, g, b, _] = pixel_at(&screenshot, 1290 * K, 650 * K)?;
    assert_eq!(
        [r, g, b],
        expected.popover,
        "the launcher's own pixel is not the theme's `popover`"
    );
    // i3's own chrome: the two terminals' title bars sit along the top of the area right of the
    // rail, clear of the notification. One window is focused (title bar in `primary`), the other
    // not (`card`) — both only because tauler wrote i3's colours from this theme (`I3Theme`).
    let bars = (272 * K)..(1400 * K);
    let top = 0..(40 * K);
    assert!(
        pixels_of(
            &screenshot,
            bars.clone(),
            top.clone(),
            rgb_of(&theme, "primary")
        ) > 500,
        "no title bar in the theme's `primary` — i3 did not get the colours for the focused window"
    );
    assert!(
        pixels_of(&screenshot, bars, top, rgb_of(&theme, "card")) > 500,
        "no title bar in the theme's `card` — i3 did not get the colours for an unfocused window"
    );
    // An empty stretch of the left terminal, below its output and clear of the launcher.
    let [r, g, b, _] = pixel_at(&screenshot, 500 * K, 1000 * K)?;
    assert_eq!(
        [r, g, b],
        expected.background,
        "the terminal's own pixel is not the theme's `background` — kitty did not get the colours"
    );
    Ok(())
}

#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn builtin_theme_by_name_draws_the_dark_palette() -> Result<()> {
    // tokyonight-night: `background` is #1a1b26; `sidebar` and `popover` are both #16161e, the
    // scheme's own answer for a panel (`Pmenu.bg`).
    builtin_theme_scenario(
        "builtin-theme-dark",
        "builtin-theme-by-name-draws-the-dark-palette",
        "tokyonight-night",
        Palette {
            background: [0x1a, 0x1b, 0x26],
            sidebar: [0x16, 0x16, 0x1e],
            popover: [0x16, 0x16, 0x1e],
        },
    )
}

#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn builtin_theme_by_name_draws_the_light_palette() -> Result<()> {
    // tokyonight-day: `background` is #e1e2e7; `sidebar` and `popover` are #d0d5e3, the scheme's
    // own answer for a floating window (`NormalFloat.bg`, which this workload asks for).
    builtin_theme_scenario(
        "builtin-theme-light",
        "builtin-theme-by-name-draws-the-light-palette",
        "tokyonight-day",
        Palette {
            background: [0xe1, 0xe2, 0xe7],
            sidebar: [0xd0, 0xd5, 0xe3],
            popover: [0xd0, 0xd5, 0xe3],
        },
    )
}

/// The showcase: one floating bar over a wallpaper tauler painted itself.
///
/// Held to exactly the same contract as the two above — the extra assertions
/// after `run` are additions, not a substitute. See docs/adr/0015.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn showcase_floats_a_bar_over_its_own_wallpaper() -> Result<()> {
    let (_desktop, screenshot) = run(
        "showcase",
        "showcase-floats-a-bar-over-its-own-wallpaper",
        Expected {
            gaps: Gaps {
                left: 0,
                right: 0,
                top: 58,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 1920, 58)],
            // These two start late by design: they wait for tauler to publish
            // _XROOTPMAP_ID before launching, so that they have a wallpaper to
            // read. `run` waits for them.
            clients: 2,
        },
    )?;

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
        "monolith-floats-a-launcher-and-a-slip-without-reserving-for-them",
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
            // Workspace 2 holds the two terminals. The launcher, the slip, the
            // real rofi and the real dunst notification are not managed by i3
            // and are not counted here.
            clients: 2,
        },
    )?;

    // The rail is a ruled plate: a 1px gold rule every 6px over #1B1924. Sampling a
    // full period of rows inside the rail's empty lower margin must therefore find
    // more than one colour. A rail that fell back to a flat fill —
    // `repeating-linear-gradient` unparsed, or dropped — gives one colour throughout
    // and passes everything else.
    //
    // A window, not two fixed rows: where the rules land depends on where the
    // gradient is anchored (a `0deg` gradient starts at the bottom edge), so a pair
    // of rows that straddled a rule under one takumi can both miss it under another.
    // The window is 12 rows because the e2e desktop renders at twice the CSS size.
    let [fr, fg, fb, _] = pixel_at(&screenshot, 6, 900)?;
    let mut spread = 0;
    for y in 901..912 {
        let [r, g, b, _] = pixel_at(&screenshot, 6, y)?;
        spread = spread.max(
            (fr as i32 - r as i32).abs()
                + (fg as i32 - g as i32).abs()
                + (fb as i32 - b as i32).abs(),
        );
    }
    assert!(
        spread > 4,
        "the rail is the same colour ({fr},{fg},{fb}) over y=900..912 — \
         the ruled plate rendered as a flat fill"
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
        "signal-flies-a-different-flag-per-workspace",
        Expected {
            gaps: Gaps {
                left: 0,
                right: 0,
                top: 92,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 1920, 92)],
            clients: 2,
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

/// Signal, moving: how long after a window opens does its band admit it?
///
/// This is the number a settled capture cannot produce, and the first one this
/// suite has ever had. It is measured without aligning any clocks: the tiling
/// area is the *trigger*, the band is the *readout*, and the answer is the
/// frames between the first change in one and the first change in the other.
/// That is also the number a person experiences — how long after the window
/// appeared did the bar react — rather than how long after a key was pressed.
///
/// It prints rather than asserts a latency bound. A threshold here would be a
/// number invented today and defended forever; the deliverable is the
/// measurement, and the assertions below are only that the mechanism fired at
/// all.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn signal_reseeds_a_band_when_a_window_opens() -> Result<()> {
    let (desktop, _) = run(
        "signal",
        "signal-reseeds-a-band-when-a-window-opens",
        Expected {
            gaps: Gaps {
                left: 0,
                right: 0,
                top: 92,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 1920, 92)],
            clients: 2,
        },
    )?;

    let motion = desktop.record_motion(11)?;

    // Workspace 2's band. Flags are 96 wide with a 12px gap from an 18px inset,
    // 64 tall starting 6px down, and workspace 2 is the second of them.
    let readout = motion.series(rect(18 + 108, 6, 96, 64))?;
    // A strip of the tiling area well below any text, where a retile is the
    // only thing that changes anything.
    let trigger = motion.series(rect(0, 780, 1920, 200))?;

    // Threshold in mean absolute per-channel difference. Frames are exact PNGs
    // with no codec between them, so an unchanged region differs by 0 — this is
    // set well above nothing and well below any real repaint.
    const CHANGED: f64 = 1.0;

    let t_first = trigger
        .first_change(0, CHANGED)
        .ok_or_else(|| anyhow::anyhow!("the tiling area never changed — nothing was driven"))?;
    let r_first = readout
        .first_change(0, CHANGED)
        .ok_or_else(|| anyhow::anyhow!("the band never changed — the hoist is not reactive"))?;

    let lag = r_first.saturating_sub(t_first);
    eprintln!(
        "\n=== signal · event to repaint ===\n         frames captured      {}\n         window visible at    frame {t_first}\n         band reacted at      frame {r_first}\n         lag                  {lag} frames ({:.0}ms at 30fps)\n",
        motion.frame_count(),
        motion.ms(lag),
    );
    for (ms, label) in &motion.events {
        eprintln!("  event {ms}  {label}");
    }

    // The map/unmap flash the brief asks about: across *one* reseed, is there
    // any frame where the band is neither its old self nor its new one?
    //
    // The span has to stop while the probe window is still open. The script
    // opens it and then closes it again, so the last frame of the recording is
    // back in the *original* state — measuring to there would mark every frame
    // of the open period as neither-old-nor-new, which is correct arithmetic
    // and a useless answer. 45 frames is 1.5s after the window appeared, well
    // inside the 3s it stays up.
    let before = t_first.saturating_sub(1);
    let settled = (t_first + 45).min(readout.last());
    let flashes = readout.transitional(before, settled, CHANGED);
    eprintln!(
        "flash check · band across one reseed, frames {before}..{settled}\n\
         frames matching neither old nor new: {} {:?}\n",
        flashes.len(),
        &flashes[..flashes.len().min(12)],
    );

    assert!(
        r_first >= t_first,
        "the band changed at frame {r_first}, before the window was visible at \
         {t_first} — the regions are not measuring what they claim to"
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
        "thermal-crops-one-field-across-every-window",
        Expected {
            gaps: Gaps {
                left: 0,
                right: 76,
                top: 58,
                bottom: 0,
            },
            panels: vec![rect(0, 0, 1920, 58), rect(1844, 58, 76, 1022)],
            // Three, and the third is kitty. It is the slowest thing on the
            // desktop to appear, and a capture taken before it arrives shows a
            // two-column workspace in which every sample below lands somewhere
            // else entirely.
            clients: 3,
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

/// The two figures in `fixtures/d3-figure`, by their centres: 16px of padding,
/// 120×40 figures 16px apart, centred in a 64px bar.
const D3_STATIC: (u32, u32) = (16 + 60, 32);
const D3_LIVE: (u32, u32) = (16 + 120 + 16 + 60, 32);

/// The flat fills the fixture draws — chosen because nothing else on this
/// desktop is any of them, so a match can only be the figure.
const MAGENTA: [u8; 3] = [0xff, 0x00, 0xff];
const YELLOW: [u8; 3] = [0xff, 0xff, 0x00];
const CYAN: [u8; 3] = [0x00, 0xff, 0xff];
/// What the reload scenario rewrites `MAGENTA` to in the fixture.
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];

fn d3_figure_expected() -> Expected {
    Expected {
        gaps: Gaps {
            left: 0,
            right: 0,
            top: 64,
            bottom: 0,
        },
        panels: vec![rect(0, 0, 1920, 64)],
        clients: 2,
    }
}

/// Poll fresh frames until the pixel at `at` is exactly `rgb`.
///
/// Fresh frames, not `capture_stable`: the live figure flips every 2 seconds,
/// so a settled capture shows whichever colour it settled on, not the one the
/// test is waiting for.
fn wait_for_colour(desktop: &Desktop, what: &str, at: (u32, u32), rgb: [u8; 3]) -> Result<()> {
    let (x, y) = at;
    wait_for(what, || {
        let frame = desktop.capture_now()?;
        let [r, g, b, _] = pixel_at(&frame, x, y)?;
        if [r, g, b] == rgb {
            Ok(())
        } else {
            anyhow::bail!(
                "({x}, {y}) is {:?}, expected {rgb:?}\n--- tauler log ---\n{}",
                [r, g, b],
                desktop.tauler_log()
            )
        }
    })
}

/// `<D3Figure>`: drawn off the loop in the Draw runtime, shown as a Picture,
/// and drawn again when its key changes (ADR 0044).
///
/// The first figure is also the first use of d3 on this machine, so it is the
/// download path too: d3 and linkedom are fetched from npm and GitHub over the
/// container's network and checked against their pins before anything draws
/// (ADR 0045). A figure that never appears here is as likely a failed download
/// as a failed draw — the tauler log in the failure message says which.
///
/// The live figure's key comes from a stream that flips every 2 seconds, and
/// its colour with it. Seeing both colours at the same pixel is the claim:
/// a figure whose redraw was skipped, or whose Picture was never swapped,
/// shows one colour for good.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn d3_figure_draws_and_redraws_when_its_key_changes() -> Result<()> {
    let (desktop, _) = run(
        "d3-figure",
        "d3-figure-draws-and-redraws-when-its-key-changes",
        d3_figure_expected(),
    )?;

    // The download, the reload it triggers and the first draw all happen
    // after the bar is up, so `run`'s screenshot may well show an empty bar.
    wait_for_colour(
        &desktop,
        "the first figure to be drawn (d3 + linkedom downloaded, then drawn)",
        D3_STATIC,
        MAGENTA,
    )?;
    wait_for_colour(
        &desktop,
        "the live figure in its first colour",
        D3_LIVE,
        YELLOW,
    )?;
    wait_for_colour(
        &desktop,
        "the live figure to be redrawn under its next key",
        D3_LIVE,
        CYAN,
    )?;

    // Photograph the bar with both figures drawn, replacing `run`'s early
    // capture as this scenario's desktop.png and gallery shot.
    let screenshot = desktop.capture_stable()?;
    let [r, g, b, _] = pixel_at(&screenshot, D3_STATIC.0, D3_STATIC.1)?;
    assert_eq!(
        [r, g, b],
        MAGENTA,
        "the static figure stopped showing its drawing while the live one was redrawn"
    );
    save_devtools_shot(
        "d3-figure-draws-and-redraws-when-its-key-changes",
        &screenshot,
    )?;
    Ok(())
}

/// A layout reload draws every figure again — its `draw` may have changed —
/// and until the new drawing lands the figure keeps showing the old one.
///
/// The edit is made inside the container, to the copy of the fixture in
/// `~/.config/tauler`, with `sed -i` — a write-and-rename, as an editor or
/// `chezmoi apply` does it. Then every frame until the new colour appears must
/// show the old one: a frame with neither means the figure blanked during the
/// reload, which is exactly what keeping the previous Picture is for.
///
/// This samples as fast as `scrot` over `docker exec` allows (a frame every
/// few hundred ms), so a blank shorter than that can slip between two samples.
/// It catches a figure that is gone for the length of a draw, not a single
/// dropped frame.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn d3_figure_keeps_its_drawing_across_a_reload() -> Result<()> {
    let (desktop, _) = run(
        "d3-figure",
        "d3-figure-keeps-its-drawing-across-a-reload",
        d3_figure_expected(),
    )?;
    wait_for_colour(&desktop, "the first figure to be drawn", D3_STATIC, MAGENTA)?;

    desktop.exec(&[
        "sed",
        "-i",
        "s/#ff00ff/#ff8000/",
        "/root/.config/tauler/layout.op.mdx",
    ])?;

    let (x, y) = D3_STATIC;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut before = 0;
    loop {
        let frame = desktop.capture_now()?;
        let [r, g, b, _] = pixel_at(&frame, x, y)?;
        match [r, g, b] {
            ORANGE => break,
            MAGENTA => before += 1,
            other => panic!(
                "after the edit the figure showed {other:?}, neither its old drawing \
                 nor its new one — it blanked during the reload\n--- tauler log ---\n{}",
                desktop.tauler_log()
            ),
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the figure never showed its new drawing after the layout file was edited \
             ({before} frames of the old one)\n--- tauler log ---\n{}",
            desktop.tauler_log()
        );
    }
    eprintln!("e2e: d3-figure reload · {before} frames of the old drawing before the new one");

    let screenshot = desktop.capture_stable()?;
    save_devtools_shot("d3-figure-keeps-its-drawing-across-a-reload", &screenshot)?;
    Ok(())
}

/// Assert the reservation contract, then hand back the desktop and its
/// screenshot so a scenario can add claims of its own.
///
/// `devtools_name` is separate from `scenario`: several tests share a fixture (`signal`
/// runs twice), so keying the devtools gallery on the fixture name would let the second
/// test's screenshot silently overwrite the first's.
fn run(scenario: &str, devtools_name: &str, expected: Expected) -> Result<(Desktop, PathBuf)> {
    run_on(Screen::default(), scenario, devtools_name, expected)
}

/// As [`run`], on `screen` — for a scenario photographed at a higher density.
fn run_on(
    screen: Screen,
    scenario: &str,
    devtools_name: &str,
    expected: Expected,
) -> Result<(Desktop, PathBuf)> {
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

    let clients = wait_for("every client to reach the focused workspace", || {
        let clients = i3::focused_workspace_client_rects(&desktop)?;
        if clients.len() == expected.clients {
            Ok(clients)
        } else {
            anyhow::bail!(
                "{} managed windows on the focused workspace, expected {}",
                clients.len(),
                expected.clients
            )
        }
    })?;

    i3::assert_clients_respect(gaps, screen, &clients)?;

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
    save_devtools_shot(devtools_name, &screenshot)?;
    Ok((desktop, screenshot))
}

/// Copy the screenshot into the hidden devtools gallery's tree, so it survives past this
/// test run without depending on `target/e2e`, which is gitignored and scenario-keyed.
fn save_devtools_shot(name: &str, screenshot: &Path) -> Result<()> {
    let dir = workspace_root()?.join("docs/src/assets/devtools/i3-scenarios");
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(screenshot, dir.join(format!("{name}.png")))?;
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("CARGO_MANIFEST_DIR has no parent"))
}

/// A11y: an AT sees tauler's accessibility tree, and activating a button
/// dispatches the intent through the same pipeline a click would (ADR 0038).
///
/// Unlike the screenshot scenarios this makes no picture: the ground truth is
/// the module log. The probe is the real thing — a tiny libatspi client on the
/// same accessibility bus — so this is the closest the suite gets to a real
/// screen reader, without dragging one in.
///
/// This starts D-Bus + the AT-SPI bus (A11Y=1), which every other scenario
/// leaves off. The entrypoint persists the bus addresses so the probe's exec —
/// a separate process with none of that environment — can find them.
#[test]
#[ignore = "needs Docker and `just e2e-image`"]
fn a11y_button_is_visible_and_activation_dispatches_an_intent() -> Result<()> {
    let desktop = Desktop::start_with_env("a11y", Screen::default(), &[("A11Y", "1")])?;

    let with_bus = "sh -c '. /out/a11y/a11y.env && \"$@\"' _";

    // The button appears in the accessibility tree as a push button named Mute.
    let tree = wait_for("tauler's a11y tree to contain the button", || {
        let out = desktop.exec(&["sh", "-c", &format!("{with_bus} a11y-probe")])?;
        if out.contains("Mute") && out.contains("push button") {
            Ok(out)
        } else {
            anyhow::bail!("button not in the tree yet:\n{out}")
        }
    })?;
    assert!(
        tree.contains("Mute\tpush button"),
        "the button must be a named push button in the tree:\n{tree}"
    );

    // Activate it. The intent lands on the recorder module's stdin.
    desktop.exec(&[
        "sh",
        "-c",
        &format!("{with_bus} a11y-probe --activate Mute"),
    ])?;

    wait_for("the module to receive the activation intent", || {
        let log = desktop.exec(&["sh", "-c", "cat /out/a11y/a11y-module.log"])?;
        if log.contains("\"type\":\"activated\"") {
            Ok(())
        } else {
            anyhow::bail!("module has not seen the activation yet:\n{log}")
        }
    })?;

    // Every fixture produces exactly one desktop.png — the CI screenshot check
    // counts fixtures and expects an equal number of captures, so a scenario
    // that never photographs itself would silently drop the suite's only guard
    // that scenarios actually ran. This one's point is the a11y tree, not the
    // pixels, but it photographs the same bar the AT just drove.
    let _screenshot = desktop.capture_stable()?;

    Ok(())
}
