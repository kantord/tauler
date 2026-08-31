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

/// Assert the reservation contract, then hand back the desktop and its
/// screenshot so a scenario can add claims of its own.
///
/// `devtools_name` is separate from `scenario`: several tests share a fixture (`signal`
/// runs twice), so keying the devtools gallery on the fixture name would let the second
/// test's screenshot silently overwrite the first's.
fn run(scenario: &str, devtools_name: &str, expected: Expected) -> Result<(Desktop, PathBuf)> {
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

    let with_bus = "sh -c '. /out/a11y/a11y.env && $1' _";

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
    desktop.exec(&["sh", "-c", &format!("{with_bus} a11y-probe --activate Mute")])?;

    wait_for("the module to receive the activation intent", || {
        let log = desktop.exec(&["sh", "-c", "cat /out/a11y/a11y-module.log"])?;
        if log.contains("\"type\":\"activated\"") {
            Ok(())
        } else {
            anyhow::bail!("module has not seen the activation yet:\n{log}")
        }
    })?;

    Ok(())
}
