//! Run tauler against a real desktop and photograph the result.
//!
//! Everything here is window-manager agnostic: starting the container, deciding
//! the screen has stopped changing, capturing it, and reading back the windows
//! the X server knows about. Anything that knows what i3 is lives in [`i3`].
//!
//! The container image is not built here. `just e2e-image` builds it, because
//! the cache mounts that make an in-container cargo build bearable are a
//! BuildKit feature and testcontainers builds through the classic builder.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use testcontainers::core::{Mount, WaitFor};
use testcontainers::runners::SyncRunner;
use testcontainers::{Container, GenericImage, ImageExt};

pub mod i3;
pub mod motion;

/// The display the container's Xvfb serves, matching the image's `DISPLAY`.
/// ffmpeg's `x11grab` needs it spelled out; it does not read the environment.
const DISPLAY: &str = ":99";

pub const IMAGE_NAME: &str = "tauler-e2e";
pub const IMAGE_TAG: &str = "local";

/// How long any single in-container command may take before we call it hung.
/// i3 4.25 can accept an IPC request and simply never answer, so every read
/// that reaches i3 needs one of these — an un-timed read is a hang, not a
/// failure.
const EXEC_TIMEOUT: Duration = Duration::from_secs(20);

/// A rectangle in logical pixels, as both X and i3 report geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn center(&self) -> (u32, u32) {
        (
            self.x as u32 + self.width / 2,
            self.y as u32 + self.height / 2,
        )
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        let ax2 = self.x + self.width as i32;
        let ay2 = self.y + self.height as i32;
        let bx2 = other.x + other.width as i32;
        let by2 = other.y + other.height as i32;
        self.x < bx2 && other.x < ax2 && self.y < by2 && other.y < ay2
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}+{}+{}", self.width, self.height, self.x, self.y)
    }
}

/// The screen the scenario runs on. Parameterised so a second monitor, or a
/// high-DPI screen, is an argument rather than a rewrite.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            dpi: 96,
        }
    }
}

/// A running desktop: X, i3, two clients and tauler, for one fixture.
pub struct Desktop {
    container: Container<GenericImage>,
    scenario: String,
    screen: Screen,
    out_dir: PathBuf,
}

impl Desktop {
    /// Start the container for `scenario`, which must name a directory under
    /// `tauler-e2e/fixtures/`.
    ///
    /// Fails loudly when Docker or the image is missing rather than skipping:
    /// this repo already has two tests that skip themselves into permanent
    /// greenness, and a third would be a habit.
    pub fn start(scenario: &str, screen: Screen) -> Result<Self> {
        Self::start_with_env(scenario, screen, &[])
    }

    /// As [`Desktop::start`], with extra environment variables for the entrypoint.
    ///
    /// Used by the a11y scenario, which asks the entrypoint to bring up a D-Bus
    /// session and the AT-SPI bus.
    pub fn start_with_env(scenario: &str, screen: Screen, extra_env: &[(&str, &str)]) -> Result<Self> {
        let fixtures = crate_dir().join("fixtures");
        if !fixtures.join(scenario).is_dir() {
            bail!("no fixture named {scenario} in {}", fixtures.display());
        }

        let out_dir = out_root()?;
        std::fs::create_dir_all(out_dir.join(scenario))?;

        let mut image = GenericImage::new(IMAGE_NAME, IMAGE_TAG)
            .with_wait_for(WaitFor::message_on_stdout("e2e: desktop up"))
            .with_env_var("SCENARIO", scenario)
            .with_env_var("SCREEN", format!("{}x{}x24", screen.width, screen.height))
            .with_env_var("DPI", screen.dpi.to_string())
            .with_mount(Mount::bind_mount(
                path_str(&fixtures)?,
                "/fixtures".to_string(),
            ))
            .with_mount(Mount::bind_mount(path_str(&out_dir)?, "/out".to_string()));
        for &(key, value) in extra_env {
            image = image.with_env_var(key, value);
        }
        let container = image
            .start()
            .with_context(|| {
                format!(
                    "starting {IMAGE_NAME}:{IMAGE_TAG} — is Docker running, and has \
                     `just e2e-image` been run?"
                )
            })?;

        Ok(Self {
            container,
            scenario: scenario.to_string(),
            screen,
            out_dir,
        })
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// Record the screen as PNG frames while the fixture's `motion` script
    /// drives it.
    ///
    /// Both halves run inside one `exec` so the recorder is definitely up
    /// before anything is driven and definitely still up when the last event
    /// lands: ffmpeg is started, the script runs against a screen that is
    /// already being watched, and the exec returns when ffmpeg has written its
    /// last frame.
    ///
    /// `seconds` bounds the recording. Make it longer than the script, or the
    /// last event happens off camera.
    pub fn record_motion(&self, seconds: u32) -> Result<motion::Motion> {
        const FPS: u32 = 30;

        let script = format!("/fixtures/{}/motion", self.scenario);
        let out = format!("/out/{}", self.scenario);

        // `-draw_mouse 0`: the pointer is at 0,0 under Xvfb and its cursor would
        // sit inside the top-left of any region sampled there, changing pixels
        // for reasons that have nothing to do with the desktop.
        let recorder = format!(
            "mkdir -p {out}/motion && rm -f {out}/motion/*.png && \
             ffmpeg -loglevel error -y -f x11grab -draw_mouse 0 \
                    -framerate {FPS} -video_size {}x{} -i {} -t {seconds} \
                    {out}/motion/%04d.png",
            self.screen.width, self.screen.height, DISPLAY
        );

        // The script drives; the recorder decides when this is over.
        let both = format!("{recorder} & FF=$!; sleep 0.6; {script}; wait $FF");

        self.exec_for(seconds + 30, &["sh", "-c", &both])
            .context("recording motion")?;

        motion::collect(&self.out_dir.join(&self.scenario), FPS)
    }

    /// Run a command inside the desktop and return its stdout.
    ///
    /// Bounded by coreutils `timeout` on the container side rather than by a
    /// deadline out here: a hung `i3-msg` would otherwise block this thread
    /// forever, and there is no way to abandon an in-flight exec from Rust.
    /// Killed commands come back as an error, which the polling loops treat as
    /// "not ready yet" and retry.
    pub fn exec(&self, argv: &[&str]) -> Result<String> {
        self.exec_for(EXEC_TIMEOUT.as_secs() as u32, argv)
    }

    /// As [`Desktop::exec`], with a longer leash.
    ///
    /// Recording outlives the default timeout by design, and a recording that
    /// is killed halfway leaves a frame directory that looks merely short.
    pub fn exec_for(&self, secs: u32, argv: &[&str]) -> Result<String> {
        let bounded = std::iter::once("timeout".to_string())
            .chain(std::iter::once(secs.to_string()))
            .chain(argv.iter().map(|s| s.to_string()));

        let mut result = self
            .container
            .exec(testcontainers::core::ExecCommand::new(bounded))
            .with_context(|| format!("exec {argv:?} failed"))?;

        // Draining stdout runs to EOF, which is the command exiting.
        let stdout = result.stdout_to_vec()?;

        match result.exit_code()? {
            Some(0) | None => Ok(String::from_utf8_lossy(&stdout).into_owned()),
            Some(124) => bail!("{argv:?} did not finish within {secs}s"),
            Some(code) => bail!("{argv:?} exited {code}"),
        }
    }

    /// Every direct child of the root window, with its geometry.
    ///
    /// tauler's panels are override-redirect, so i3 never manages them and they
    /// do not appear in its tree at all — the X server is the only thing that
    /// knows where a panel actually is.
    pub fn root_windows(&self) -> Result<Vec<Rect>> {
        let out = self.exec(&["xwininfo", "-root", "-children"])?;
        Ok(parse_xwininfo_children(&out))
    }

    /// Capture until two consecutive frames are pixel-identical, then keep the
    /// second. A window that is mapped is not necessarily a window that has
    /// painted; this is what tells the two apart.
    pub fn capture_stable(&self) -> Result<PathBuf> {
        let dir = self.out_dir.join(&self.scenario);
        let final_path = dir.join("desktop.png");

        let mut previous: Option<Vec<u8>> = None;
        for attempt in 0..40 {
            let frame = self.capture_frame(&dir, attempt % 2)?;
            let pixels = image::open(&frame)
                .with_context(|| format!("decoding {}", frame.display()))?
                .to_rgba8();
            let pixels = pixels.into_raw();

            if previous.as_ref() == Some(&pixels) {
                std::fs::copy(&frame, &final_path)?;
                return Ok(final_path);
            }
            previous = Some(pixels);
            std::thread::sleep(Duration::from_millis(250));
        }

        bail!(
            "screen never settled for scenario {}: 40 consecutive frames differed",
            self.scenario
        )
    }

    fn capture_frame(&self, dir: &Path, slot: usize) -> Result<PathBuf> {
        let name = format!("frame-{slot}.png");
        self.exec(&["scrot", "-o", &format!("/out/{}/{}", self.scenario, name)])?;
        Ok(dir.join(name))
    }

    /// The tauler log for this scenario, for putting in a failure message.
    pub fn tauler_log(&self) -> String {
        std::fs::read_to_string(self.out_dir.join(&self.scenario).join("tauler.log"))
            .unwrap_or_else(|e| format!("<no tauler log: {e}>"))
    }
}

/// Poll `f` until it returns `Ok`, then return its value.
///
/// The last error is what gets reported — "gaps were 0/0/0/0" is a diagnosis,
/// "timed out" is not.
pub fn wait_for<T>(what: &str, mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let last = match f() {
            Ok(value) => return Ok(value),
            Err(e) => e,
        };
        if std::time::Instant::now() >= deadline {
            return Err(anyhow!("timed out waiting for {what}: {last}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Read one pixel out of a captured frame, to prove a panel painted rather than
/// merely existing.
pub fn pixel_at(png: &Path, x: u32, y: u32) -> Result<[u8; 4]> {
    let img = image::open(png)
        .with_context(|| format!("decoding {}", png.display()))?
        .to_rgba8();
    let px = img
        .get_pixel_checked(x, y)
        .ok_or_else(|| anyhow!("({x}, {y}) is outside {}", png.display()))?;
    Ok(px.0)
}

/// Parse the geometry of each child in `xwininfo -root -children` output.
///
/// The lines look like:
///   `0x400003 "tauler": ()  272x1080+0+0  +0+0`
/// Children of the root window have root as their parent, so the relative
/// geometry is already absolute.
fn parse_xwininfo_children(output: &str) -> Vec<Rect> {
    output
        .lines()
        .filter(|line| line.trim_start().starts_with("0x"))
        .filter_map(|line| line.split_whitespace().find_map(parse_geometry))
        .collect()
}

/// `WxH+X+Y`, where X and Y may be negative.
fn parse_geometry(token: &str) -> Option<Rect> {
    let (size, position) = token.split_once('+')?;
    let (width, height) = size.split_once('x')?;
    let width: u32 = width.parse().ok()?;
    let height: u32 = height.parse().ok()?;

    // "0+0" or "-12+34": the first coordinate's sign was consumed as the
    // separator, so only the second can carry one here.
    let (x, y) = position.split_once('+')?;
    let x: i32 = x.parse().ok()?;
    let y: i32 = y.parse().ok()?;

    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Where captures land on the host, and what gets bind-mounted at `/out`.
fn out_root() -> Result<PathBuf> {
    let dir = crate_dir()
        .parent()
        .ok_or_else(|| anyhow!("tauler-e2e has no parent directory"))?
        .join("target/e2e");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn path_str(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{} is not valid UTF-8", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_child_geometry() {
        let output = "\
xwininfo: Window id: 0x2e (the root window) (has no name)

  Root window id: 0x2e (the root window) (has no name)
  Parent window id: 0x0 (none)
     2 children:
     0x400003 (has no name): ()  272x1080+0+0  +0+0
     0x600005 \"e2e-client-1\": (\"xterm\" \"XTerm\")  1640x1076+274+2  +274+2
";
        assert_eq!(
            parse_xwininfo_children(output),
            vec![
                Rect {
                    x: 0,
                    y: 0,
                    width: 272,
                    height: 1080
                },
                Rect {
                    x: 274,
                    y: 2,
                    width: 1640,
                    height: 1076
                },
            ]
        );
    }

    #[test]
    fn parses_negative_offsets() {
        assert_eq!(
            parse_geometry("100x50+-4+7"),
            Some(Rect {
                x: -4,
                y: 7,
                width: 100,
                height: 50
            })
        );
    }

    #[test]
    fn rects_touching_at_an_edge_do_not_intersect() {
        let panel = Rect {
            x: 0,
            y: 0,
            width: 272,
            height: 1080,
        };
        let client = Rect {
            x: 272,
            y: 0,
            width: 100,
            height: 100,
        };
        assert!(!panel.intersects(&client));
        assert!(panel.intersects(&Rect { x: 271, ..client }));
    }
}
