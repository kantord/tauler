//! Driving Chrome over CDP, and reading back the two things the comparison needs.
//!
//! `Page.captureScreenshot` is the reason this is CDP rather than WebDriver: it takes a
//! clip rectangle in CSS pixels with an explicit scale, which is what reproducing
//! `tauler-screenshot`'s crop requires. WebDriver's element screenshot decides the box for
//! you.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use headless_chrome::browser::tab::point::Point;
use headless_chrome::protocol::cdp::Page::{CaptureScreenshotFormatOption, Viewport};
use headless_chrome::{Browser, LaunchOptions, Tab};

use crate::{Box2D, PathBox};

/// The crop margin, from the one place all three producers read it.
const CROP_PAD: f64 = tauler_core::preview::PAD as f64;

/// A fixed window, because a viewport width the mount can respond to is a variable the
/// comparison must not have.
const WINDOW: (u32, u32) = (1200, 900);

/// How long to wait for the wasm module to load and every mount to render.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Flags without which the captured pixels are not the pixels the page described.
///
/// Hinting otherwise comes from the host's fontconfig, and takumi does not hint at all.
fn colour_determinism_args() -> Vec<&'static std::ffi::OsStr> {
    ["--force-color-profile=srgb", "--font-render-hinting=none"]
        .into_iter()
        .map(std::ffi::OsStr::new)
        .collect()
}

/// Where a browser is, when one is not being launched locally.
///
/// Set by `just web-e2e` and by CI to the container's CDP endpoint. Its absence is what
/// selects the development fallback — see [`Session::launch`].
pub const CHROME_WS_ENV: &str = "TAULER_CHROME_WS";

pub struct Session {
    // Held so the browser outlives the tab.
    _browser: Browser,
    tab: Arc<Tab>,
}

impl Session {
    /// Connect to the pinned browser, or launch the host's.
    ///
    /// `TAULER_CHROME_WS` names the container's CDP endpoint, and that is the arrangement
    /// the thresholds were measured under: text advance widths depend on the browser's
    /// font stack, and Chrome updates monthly, so a gate measured against whatever is
    /// installed locally means nothing a month later (ADR 0028, and ADR 0004 for the same
    /// argument about glibc).
    ///
    /// Without it the host's Chrome is launched instead. That is a development
    /// convenience — fast to iterate against, and legitimate for finding out *whether*
    /// something renders — but a pass or a failure it produces is not the project's
    /// answer. The warning says so rather than leaving the distinction to be remembered.
    pub fn launch() -> Result<Self> {
        match std::env::var(CHROME_WS_ENV) {
            Ok(ws) if !ws.trim().is_empty() => Self::connect(&ws),
            _ => {
                eprintln!(
                    "warning: {CHROME_WS_ENV} is not set, so this ran against the host's \
                     Chrome. The geometry and liveness thresholds were measured against the \
                     pinned browser in tauler-web-e2e/Dockerfile; a result from here is a \
                     development signal, not the gate. Use `just web-e2e`."
                );
                Self::launch_local()
            }
        }
    }

    /// Attach to an already-running browser over CDP.
    pub fn connect(debug_ws_url: &str) -> Result<Self> {
        let browser = Browser::connect(debug_ws_url.to_string())
            .with_context(|| format!("could not connect to Chrome at {debug_ws_url}"))?;
        let tab = browser.new_tab().context("could not open a tab")?;
        // The window size is set here rather than by a flag, because a browser that was
        // already running did not read our flags.
        tab.set_bounds(headless_chrome::types::Bounds::Normal {
            left: Some(0),
            top: Some(0),
            width: Some(WINDOW.0 as f64),
            height: Some(WINDOW.1 as f64),
        })
        .context("could not size the window")?;
        Ok(Self {
            _browser: browser,
            tab,
        })
    }

    fn launch_local() -> Result<Self> {
        let browser = Browser::new(
            LaunchOptions::default_builder()
                .headless(true)
                .window_size(Some(WINDOW))
                .sandbox(false)
                .args(colour_determinism_args())
                .build()
                .map_err(|e| anyhow!("could not build launch options: {e}"))?,
        )
        .context("could not launch Chrome")?;
        let tab = browser.new_tab().context("could not open a tab")?;
        Ok(Self {
            _browser: browser,
            tab,
        })
    }

    /// Navigate, and wait until every mount on the page has rendered.
    ///
    /// Waiting on the mounts rather than on the load event: the module is fetched and the
    /// wasm instantiated after `DOMContentLoaded`, so a page that has "loaded" may still
    /// be showing nothing at all.
    pub fn open(&self, url: &str) -> Result<()> {
        self.tab.navigate_to(url).context("navigate")?;
        self.tab.wait_until_navigated().context("wait for load")?;
        self.tab
            .wait_for_element_with_custom_timeout(".tauler-mount > *", READY_TIMEOUT)
            .map_err(|e| anyhow!("no mount rendered within {READY_TIMEOUT:?}: {e}"))?;
        Ok(())
    }

    /// Any error the page logged, so a failure names its cause instead of its symptom.
    pub fn page_errors(&self) -> Result<Vec<String>> {
        let value = self.eval("JSON.stringify(globalThis.__taulerErrors ?? [])")?;
        Ok(serde_json::from_str(value.as_str().unwrap_or("[]"))?)
    }

    /// Bring one mount to the viewport origin, alone, for a capture or for a pointer.
    ///
    /// Three measured reasons, none cosmetic:
    ///
    /// - Left in place it lands at a sub-pixel offset (`y = 6287.8125` when this was first
    ///   measured), and the capture then slices every row mid-pixel — a uniform ~12/255
    ///   difference everywhere, which reads exactly like a renderer disagreement.
    /// - `z-index` cannot lift it out of a stacking context an ancestor created, so a
    ///   pinned mount is still painted *under* the site header and the capture shows the
    ///   header. Hiding the document settles it.
    /// - At this window width the mount sits at page `x = 0`, under the sidebar, so a
    ///   pointer aimed at it hits `.sidebar-content` instead. Hidden elements take no
    ///   pointer events, so the same move clears the path as well as the picture.
    ///
    /// It moves the mount, never its layout: the width is fixed, so the box tree is
    /// identical either way — which the geometry gate, measured without any of this,
    /// confirms.
    pub fn isolate(&self, example: &str) -> Result<()> {
        self.eval(&format!(
            r#"
            (() => {{
              const el = document.querySelector('.tauler-mount[data-tauler-example="{example}"]');
              if (!el) return "";
              document.documentElement.style.visibility = 'hidden';
              el.style.visibility = 'visible';
              el.style.position = 'fixed';
              el.style.left = '0px';
              el.style.top = '0px';
              return "";
            }})()
            "#
        ))?;
        Ok(())
    }

    /// Undo [`isolate`](Self::isolate), so the next mount is measured where the page puts it.
    pub fn restore(&self, example: &str) -> Result<()> {
        self.eval(&format!(
            r#"
            (() => {{
              const el = document.querySelector('.tauler-mount[data-tauler-example="{example}"]');
              if (el) el.removeAttribute('style');
              document.documentElement.style.removeProperty('visibility');
              return "";
            }})()
            "#
        ))?;
        Ok(())
    }

    /// The viewport rectangle of the first element matching `selector` inside one mount.
    pub fn rect_of(&self, example: &str, selector: &str) -> Result<(f64, f64, f64, f64)> {
        let raw = self.eval(&format!(
            r#"
            (() => {{
              const mount = document.querySelector('.tauler-mount[data-tauler-example="{example}"]');
              const el = mount?.querySelector('{selector}');
              if (!el) return JSON.stringify(null);
              const r = el.getBoundingClientRect();
              return JSON.stringify([r.x, r.y, r.width, r.height]);
            }})()
            "#
        ))?;
        let parsed: Option<[f64; 4]> = serde_json::from_str(raw.as_str().unwrap_or("null"))?;
        let [x, y, width, height] =
            parsed.ok_or_else(|| anyhow!("no {selector:?} inside {example:?}"))?;
        Ok((x, y, width, height))
    }

    /// The text of the first element matching `selector` inside one mount.
    pub fn text_of(&self, example: &str, selector: &str) -> Result<String> {
        let raw = self.eval(&format!(
            r#"
            (() => {{
              const mount = document.querySelector('.tauler-mount[data-tauler-example="{example}"]');
              const el = mount?.querySelector('{selector}');
              return el ? el.textContent : "";
            }})()
            "#
        ))?;
        Ok(raw.as_str().unwrap_or("").to_string())
    }

    /// Press and release at a viewport point, as a real pointer.
    ///
    /// Through CDP rather than a synthetic `PointerEvent`: `setPointerCapture` rejects a
    /// pointer id the browser did not issue, so a synthetic event exercises a path the real
    /// one never takes (ADR 0020).
    pub fn click_at(&self, x: f64, y: f64) -> Result<()> {
        self.tab
            .move_mouse_to_point(Point { x, y })
            .context("move pointer")?;
        self.tab.click_point(Point { x, y }).context("click")?;
        Ok(())
    }

    fn eval(&self, expression: &str) -> Result<serde_json::Value> {
        let result = self
            .tab
            .evaluate(expression, true)
            .with_context(|| format!("evaluating {expression}"))?;
        result
            .value
            .ok_or_else(|| anyhow!("{expression} returned nothing"))
    }

    /// Every element's box inside one mount, keyed by its render path.
    ///
    /// Read from `getBoundingClientRect` and made relative to the mount's own origin, so
    /// the numbers are comparable with takumi's — which are relative to the canvas it
    /// rendered into, not to a page that has a sidebar and a header above it.
    pub fn boxes_in(&self, example: &str) -> Result<std::collections::HashMap<String, Box2D>> {
        let script = format!(
            r#"
            (() => {{
              const mount = document.querySelector('.tauler-mount[data-tauler-example="{example}"]');
              if (!mount) return JSON.stringify(null);
              const origin = mount.getBoundingClientRect();
              const out = {{}};
              for (const el of mount.querySelectorAll('[data-tauler-path]')) {{
                const r = el.getBoundingClientRect();
                out[el.getAttribute('data-tauler-path')] = {{
                  x: r.x - origin.x, y: r.y - origin.y, width: r.width, height: r.height,
                }};
              }}
              return JSON.stringify(out);
            }})()
            "#
        );
        let raw = self.eval(&script)?;
        let text = raw.as_str().unwrap_or("null");
        if text == "null" {
            return Err(anyhow!("no mount on the page for {example:?}"));
        }
        Ok(serde_json::from_str(text)?)
    }

    /// The clip `tauler-screenshot` would have cropped to, padded by [`CROP_PAD`].
    ///
    /// The rule is `measured.children.first()` — the **frame**, not the component inside
    /// it. For an example that sets its own width that is the difference between a 400px
    /// image and a 232px one. So: mount → canvas → frame.
    pub fn component_clip(&self, example: &str) -> Result<(f64, f64, f64, f64)> {
        let script = format!(
            r#"
            (() => {{
              const mount = document.querySelector('.tauler-mount[data-tauler-example="{example}"]');
              const canvas = mount?.firstElementChild;
              const target = canvas?.firstElementChild;
              if (!target) return JSON.stringify(null);
              const r = target.getBoundingClientRect();
              return JSON.stringify([r.x, r.y, r.width, r.height]);
            }})()
            "#
        );
        let raw = self.eval(&script)?;
        let parsed: Option<[f64; 4]> = serde_json::from_str(raw.as_str().unwrap_or("null"))?;
        let [x, y, width, height] = parsed.ok_or_else(|| anyhow!("no component in {example:?}"))?;
        Ok((
            x - CROP_PAD,
            y - CROP_PAD,
            width + 2.0 * CROP_PAD,
            height + 2.0 * CROP_PAD,
        ))
    }

    /// A PNG of exactly that clip, at `scale` device pixels per CSS pixel.
    ///
    /// The final argument is `from_surface`, not `captureBeyondViewport`: the clip is only
    /// honoured with it on. Beyond-viewport capture stays off — it re-lays-out the page at
    /// full document size, after which an isolated mount is no longer where it was
    /// measured.
    pub fn screenshot_clip(&self, clip: (f64, f64, f64, f64), scale: f64) -> Result<Vec<u8>> {
        let (x, y, width, height) = clip;
        self.tab
            .capture_screenshot(
                CaptureScreenshotFormatOption::Png,
                None,
                Some(Viewport {
                    x,
                    y,
                    width,
                    height,
                    scale,
                }),
                true,
            )
            .context("capture screenshot")
    }
}

/// takumi's boxes for one component, as `tauler-screenshot --geometry-out` wrote them.
pub fn read_takumi_boxes(path: &std::path::Path) -> Result<Vec<PathBox>> {
    let body =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_str(&body)?)
}
