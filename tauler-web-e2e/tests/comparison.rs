//! The web renderer, run against the documentation site a reader actually gets.
//!
//! Against the built Starlight page rather than a purpose-made harness, deliberately: the
//! mount's isolation from the site's CSS is a scoped `all: revert` in a cascade layer
//! (ADR 0024), and a test page without Starlight's stylesheet would not exercise the one
//! thing most likely to be wrong about it.
//!
//! These are not **Scenarios** in `CONTEXT.md`'s sense — there is no fixture, no
//! reservation and no desktop, and the expected values are derived rather than written by
//! hand. They are comparisons. `#[ignore]` by default for the reason ADR 0006 gives: they
//! need a built site and a browser.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tauler_web_e2e::browser::{read_takumi_boxes, Session};
use tauler_web_e2e::{
    compare_geometry, difference, ink_share, rendered_at_all, server::Server, CHANNEL_TOLERANCE,
};

/// Hand-listed, so a component silently dropping out of the page is a failure rather than
/// a shorter run. `icon` is absent on purpose: its Nerd Font resolves through fontconfig,
/// so the takumi side depends on the host (ADR 0026).
const COMPONENTS: &[&str] = &[
    "badge",
    "card",
    "datatable",
    "knob",
    "progress",
    "slider",
    "table",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR has no parent")
        .to_path_buf()
}

/// The built site, or an error naming the command that builds it.
fn site_dir() -> PathBuf {
    let dir = workspace_root().join("docs/dist");
    assert!(
        dir.join("components/index.html").is_file(),
        "the documentation site is not built at {}.\n\
         Build it with:  just docs && cd docs && pnpm run build",
        dir.display()
    );
    dir
}

/// takumi's boxes for every component, written by `tauler-screenshot --geometry-out`.
fn takumi_geometry_dir() -> PathBuf {
    let dir = workspace_root().join("docs/.tauler/geometry");
    assert!(
        dir.is_dir(),
        "takumi's geometry is missing at {}.\n\
         Generate it with:  just docs",
        dir.display()
    );
    dir
}

/// Every box takumi painted must be where the browser puts it, to within a pixel.
///
/// The gate (ADR 0026).
#[test]
#[ignore = "needs a built docs site and a Chrome; run with `just web-e2e`"]
fn every_box_is_where_takumi_put_it() {
    let server = Server::start(site_dir()).expect("serve the built site");
    let session = Session::launch().expect("launch Chrome");
    session
        .open(&server.url("/components/"))
        .expect("open the components page");

    let mut failures: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for component in COMPONENTS {
        let takumi = read_takumi_boxes(&takumi_geometry_dir().join(format!("{component}.json")))
            .unwrap_or_else(|e| panic!("reading takumi geometry for {component}: {e}"));
        assert!(
            !takumi.is_empty(),
            "takumi painted nothing for {component}; the comparison would pass vacuously"
        );
        let browser = session
            .boxes_in(component)
            .unwrap_or_else(|e| panic!("measuring {component} in the browser: {e}"));

        let moved = compare_geometry(&takumi, &browser);
        if !moved.is_empty() {
            failures.insert(
                component,
                moved.iter().map(ToString::to_string).collect::<Vec<_>>(),
            );
        }
    }

    assert!(
        failures.is_empty(),
        "the browser and takumi disagree about {} component(s):\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(c, rows)| format!("\n{c}:\n  {}", rows.join("\n  ")))
            .collect::<String>()
    );
}

/// Each mount rendered, and how far its pixels are from the committed screenshot.
///
/// The liveness half of ADR 0026: it catches what the geometry gate cannot, because a
/// mount that renders an empty tree passes every box comparison vacuously. The pixel
/// difference is measured and written out beside the shots, never gated — that is the
/// "reviewed, not gated" half of ADR 0005, and it is what makes ADR 0026's numbers
/// reproducible.
#[test]
#[ignore = "needs a built docs site and a Chrome; run with `just web-e2e`"]
fn every_mount_still_renders_its_component() {
    let server = Server::start(site_dir()).expect("serve the built site");
    let session = Session::launch().expect("launch Chrome");
    session
        .open(&server.url("/components/"))
        .expect("open the components page");

    let shots_dir = workspace_root().join("docs/.tauler/web-shots");
    std::fs::create_dir_all(&shots_dir).expect("create the output directory");

    let mut failures = Vec::new();
    let mut report = String::from("component,pixels_differing,mean_channel_delta,ink_ratio\n");
    for component in COMPONENTS {
        session
            .isolate(component)
            .unwrap_or_else(|e| panic!("pinning {component}: {e}"));
        let clip = session
            .component_clip(component)
            .unwrap_or_else(|e| panic!("measuring the crop for {component}: {e}"));
        let png = session
            .screenshot_clip(clip, 1.0)
            .unwrap_or_else(|e| panic!("screenshotting {component}: {e}"));
        std::fs::write(shots_dir.join(format!("{component}.png")), &png).expect("write the shot");
        session
            .restore(component)
            .unwrap_or_else(|e| panic!("restoring {component}: {e}"));

        let browser = image::load_from_memory(&png)
            .unwrap_or_else(|e| panic!("decoding the {component} shot: {e}"))
            .into_rgba8();
        let takumi = image::open(workspace_root().join(format!("docs/src/assets/{component}.png")))
            .unwrap_or_else(|e| panic!("opening the committed {component} screenshot: {e}"))
            .into_rgba8();

        let diff = difference(&browser, &takumi);
        let ink = ink_share(&browser, CHANNEL_TOLERANCE) / ink_share(&takumi, CHANNEL_TOLERANCE);
        report.push_str(&format!(
            "{component},{:.4},{:.2},{ink:.3}\n",
            diff.share, diff.mean
        ));
        if let Err(why) = rendered_at_all(&browser, &takumi) {
            failures.push(format!("{component}: {why}"));
        }
    }

    std::fs::write(shots_dir.join("difference.csv"), &report).expect("write the report");
    println!("{report}");

    assert!(
        failures.is_empty(),
        "{} mount(s) did not render:\n  {}\nShots and difference.csv written to {}",
        failures.len(),
        failures.join("\n  "),
        shots_dir.display()
    );
}

/// The happy path, stated on its own so a total failure says so plainly rather than
/// arriving as a hundred box discrepancies.
#[test]
#[ignore = "needs a built docs site and a Chrome; run with `just web-e2e`"]
fn the_page_runs_every_component_in_the_browser() {
    let server = Server::start(site_dir()).expect("serve the built site");
    let session = Session::launch().expect("launch Chrome");
    session
        .open(&server.url("/components/"))
        .expect("open the components page");

    for component in COMPONENTS {
        let boxes = session
            .boxes_in(component)
            .unwrap_or_else(|e| panic!("{component} did not render: {e}"));
        assert!(
            boxes.len() > 1,
            "{component} rendered {} node(s); the mount is empty or the layout threw",
            boxes.len()
        );
    }

    let errors = session.page_errors().unwrap_or_default();
    assert!(errors.is_empty(), "the page reported errors: {errors:?}");
}

/// Dragging the slider changes the value, all the way round the loop.
///
/// This is the scenario that exercises the architecture rather than the renderer. A press
/// on the track goes: pointer → `data-tauler-on` lookup → the captured handler → an intent
/// on a channel → a **Transport that is four lines of JavaScript in the page** → a Stream
/// value → a tick → new markup. On a desktop every one of those steps is identical except
/// that the Transport is a subprocess, and the layout file cannot tell which it got.
///
/// It also checks the thing ADR 0012 insists on: the control holds nothing. If the slider
/// remembered its own value this would pass with the Transport unplugged, so the label —
/// which is rendered from the Stream, not from the slider — is what is asserted.
#[test]
#[ignore = "needs a built docs site and a Chrome; run with `just web-e2e`"]
fn dragging_the_slider_moves_the_value_through_a_transport() {
    let server = Server::start(site_dir()).expect("serve the built site");
    let session = Session::launch().expect("launch Chrome");
    session
        .open(&server.url("/components/"))
        .expect("open the components page");
    session.isolate("slider").expect("isolate the slider");

    let before = session
        .text_of("slider", "span + span")
        .expect("read the label");
    assert_eq!(
        before, "40%",
        "the example starts at its default, because the Stream has produced nothing yet"
    );

    // The track is the element carrying the drag handler. Pressing counts as the first
    // drag event, so a click at 80% across it is a complete gesture.
    let (x, y, width, height) = session
        .rect_of("slider", "[data-tauler-on~=\"drag\"]")
        .expect("find the track");
    session
        .click_at(x + width * 0.8, y + height / 2.0)
        .expect("press the track");

    let after = session
        .text_of("slider", "span + span")
        .expect("read the label");
    assert_ne!(
        after, before,
        "the label still reads {before}: the intent never reached the Transport, or the \
         Stream value never came back"
    );
    assert_eq!(
        after, "80%",
        "the value should be the position under the pointer, snapped to the step"
    );

    session.restore("slider").expect("restore the page");
}
