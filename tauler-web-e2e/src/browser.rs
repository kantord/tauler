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

/// The padding `tauler-screenshot` leaves around a component, in CSS pixels.
///
/// Not a choice made here — it is the crop rule the committed screenshots were made with,
/// and the browser has to reproduce it exactly or the two images are of different things
/// (ADR 0026).
pub const CROP_PAD: f64 = 16.0;

/// How long to wait for the wasm module to load and every embed to render.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Flags without which the captured pixels are not the pixels the page described.
///
/// `--force-color-profile=srgb` is the one that matters, and it was found by measurement
/// rather than by precaution. The theme's background is `oklch(0.145 0 0)`, which converts
/// to sRGB grey 10; takumi renders `[10,10,10]` and an unflagged Chrome captures
/// `[16,15,15]` — not even achromatic. Every pixel in every component was off by that
/// amount, which reads as a rendering disagreement and is nothing of the kind: the page is
/// composited in the display's profile and the screenshot carries it out unconverted.
///
/// `--font-render-hinting=none` removes the second host dependency. Hinting decisions come
/// from fontconfig on the machine running the browser, and takumi does not hint at all.
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
    /// installed locally means nothing a month later (ADR 0026, and ADR 0004 for the same
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
            width: Some(1200.0),
            height: Some(900.0),
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
                // A fixed window, because a viewport width the embed can respond to is a
                // variable the comparison must not have.
                .window_size(Some((1200, 900)))
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

    /// Navigate, and wait until every embed on the page has rendered.
    ///
    /// Waiting on the embeds rather than on the load event: the module is fetched and the
    /// wasm instantiated after `DOMContentLoaded`, so a page that has "loaded" may still
    /// be showing nothing at all.
    pub fn open(&self, url: &str) -> Result<()> {
        self.tab.navigate_to(url).context("navigate")?;
        self.tab.wait_until_navigated().context("wait for load")?;
        self.tab
            .wait_for_element_with_custom_timeout(".tauler-embed > *", READY_TIMEOUT)
            .map_err(|e| anyhow!("no embed rendered within {READY_TIMEOUT:?}: {e}"))?;
        Ok(())
    }

    /// Any error the page logged, so a failure names its cause instead of its symptom.
    pub fn page_errors(&self) -> Result<Vec<String>> {
        let value = self.eval("JSON.stringify(globalThis.__taulerErrors ?? [])")?;
        Ok(serde_json::from_str(value.as_str().unwrap_or("[]"))?)
    }

    /// Bring one embed to the viewport origin, alone, for a capture or for a pointer.
    ///
    /// Not cosmetic. Left where the page puts it, an embed lands at whatever sub-pixel
    /// offset the prose above it happens to produce — `y = 6287.8125` in the first
    /// measurement — and `Page.captureScreenshot` then slices every row mid-pixel. The
    /// result is a uniform ~12/255 difference across the whole image, in flat background
    /// areas as much as in text, which reads exactly like a rendering disagreement and is
    /// nothing but a fractional offset.
    ///
    /// Everything else on the page is hidden for the duration, and that is not belt and
    /// braces either. `z-index` cannot lift the embed out of a stacking context an ancestor
    /// created, so a pinned embed sits at the viewport origin geometrically and is still
    /// painted *underneath* Starlight's header — the capture then shows the header. Hiding
    /// the document and making only this subtree visible removes the question.
    ///
    /// It matters for input as much as for pixels. Left in place, the embed lands at page
    /// `x = 0` where Starlight's sidebar also is, so a pointer aimed at the slider track
    /// hits `.sidebar-content` and the layout never sees a `pointerdown` at all. A hidden
    /// element takes no pointer events, so hiding the page clears the path as well as the
    /// picture.
    ///
    /// Pinning changes where the embed is, never how it is laid out: its width is a fixed
    /// 400px, so the box tree inside is identical either way — which the geometry gate,
    /// measured without any of this, is there to confirm.
    pub fn isolate(&self, example: &str) -> Result<()> {
        self.eval(&format!(
            r#"
            (() => {{
              const el = document.querySelector('.tauler-embed[data-tauler-example="{example}"]');
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

    /// Undo [`isolate`](Self::isolate), so the next embed is measured where the page puts it.
    pub fn restore(&self, example: &str) -> Result<()> {
        self.eval(&format!(
            r#"
            (() => {{
              const el = document.querySelector('.tauler-embed[data-tauler-example="{example}"]');
              if (el) el.removeAttribute('style');
              document.documentElement.style.removeProperty('visibility');
              return "";
            }})()
            "#
        ))?;
        Ok(())
    }

    /// Bring one embed into the viewport, so a pointer can reach it.
    pub fn scroll_into_view(&self, example: &str) -> Result<()> {
        self.eval(&format!(
            r#"
            (() => {{
              const el = document.querySelector('.tauler-embed[data-tauler-example="{example}"]');
              if (el) el.scrollIntoView({{ block: 'center' }});
              return "";
            }})()
            "#
        ))?;
        Ok(())
    }

    /// The viewport rectangle of the first element matching `selector` inside one embed.
    pub fn rect_of(&self, example: &str, selector: &str) -> Result<(f64, f64, f64, f64)> {
        let raw = self.eval(&format!(
            r#"
            (() => {{
              const embed = document.querySelector('.tauler-embed[data-tauler-example="{example}"]');
              const el = embed?.querySelector('{selector}');
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

    /// The text of the first element matching `selector` inside one embed.
    pub fn text_of(&self, example: &str, selector: &str) -> Result<String> {
        let raw = self.eval(&format!(
            r#"
            (() => {{
              const embed = document.querySelector('.tauler-embed[data-tauler-example="{example}"]');
              const el = embed?.querySelector('{selector}');
              return el ? el.textContent : "";
            }})()
            "#
        ))?;
        Ok(raw.as_str().unwrap_or("").to_string())
    }

    /// Press and release at a viewport point, as a real pointer.
    ///
    /// Dispatched through CDP rather than as a synthetic `PointerEvent` from JavaScript,
    /// because the thing under test is pointer capture: `setPointerCapture` rejects a
    /// pointer id the browser did not issue, so a synthetic event would exercise a path
    /// the real one never takes (ADR 0020).
    pub fn click_at(&self, x: f64, y: f64) -> Result<()> {
        self.tab
            .move_mouse_to_point(Point { x, y })
            .context("move pointer")?;
        self.tab.click_point(Point { x, y }).context("click")?;
        Ok(())
    }

    /// Evaluate an expression and return its string result. For diagnosis only.
    pub fn debug_eval(&self, expression: &str) -> Result<String> {
        Ok(self.eval(expression)?.as_str().unwrap_or("").to_string())
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

    /// Every element's box inside one embed, keyed by its render path.
    ///
    /// Read from `getBoundingClientRect` and made relative to the embed's own origin, so
    /// the numbers are comparable with takumi's — which are relative to the canvas it
    /// rendered into, not to a page that has a sidebar and a header above it.
    pub fn boxes_in(&self, example: &str) -> Result<std::collections::HashMap<String, Box2D>> {
        let script = format!(
            r#"
            (() => {{
              const embed = document.querySelector('.tauler-embed[data-tauler-example="{example}"]');
              if (!embed) return JSON.stringify(null);
              const origin = embed.getBoundingClientRect();
              const out = {{}};
              for (const el of embed.querySelectorAll('[data-tauler-path]')) {{
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
            return Err(anyhow!("no embed on the page for {example:?}"));
        }
        Ok(serde_json::from_str(text)?)
    }

    /// The clip `tauler-screenshot` would have cropped to, padded by [`CROP_PAD`].
    ///
    /// The rule is `measured.children.first()` — the **frame**, the `w-full` wrapper the
    /// screenshot tool injects, not the component inside it. That distinction is the whole
    /// difference between a 400px-wide image and a 232px one for any example that sets its
    /// own width, and getting it wrong compares two different pictures.
    ///
    /// So: embed → canvas (the padded `bg-background` div) → frame. Reproducing the rule
    /// rather than fixing a size is what makes a disagreement about how *big* something is
    /// surface as a whole-image failure instead of hiding inside two different crops.
    pub fn component_clip(&self, example: &str) -> Result<(f64, f64, f64, f64)> {
        let script = format!(
            r#"
            (() => {{
              const embed = document.querySelector('.tauler-embed[data-tauler-example="{example}"]');
              const canvas = embed?.firstElementChild;
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
    /// The final argument is `from_surface`, not `captureBeyondViewport`: it asks the
    /// compositor for the pixels rather than the renderer, and the clip is only honoured
    /// with it on. Beyond-viewport capture is left off, because it re-lays-out the page at
    /// full document size before capturing and a pinned embed is no longer where it was
    /// measured — which is why [`pin_for_capture`](Self::pin_for_capture) brings the embed
    /// into the real viewport instead of chasing it down the page.
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
