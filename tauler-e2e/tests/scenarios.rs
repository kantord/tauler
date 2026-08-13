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
    )
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
    )
}

fn run(scenario: &str, expected: Expected) -> Result<()> {
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

    i3::assert_clients_respect(gaps, screen, &i3::client_rects(&desktop)?)?;

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
    Ok(())
}
