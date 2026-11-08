//! Run tauler against a real desktop and photograph the result.
//!
//! There are two desktops, chosen by [`Backend`]: X11 + i3 in one image, headless
//! Sway in another. What is shared is starting the container, deciding the screen
//! has stopped changing, and capturing it — the capture tool differs (`scrot` or
//! `grim`), the settle logic does not. What is not shared is deliberately not
//! hidden behind that seam: [`Desktop::root_windows`] and [`Desktop::record_motion`]
//! read X11 and fail on Sway, and anything that knows what i3 is lives in [`i3`].
//!
//! The container images are not built here. `just e2e-image` builds both, because
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

pub const IMAGE_TAG: &str = "local";

/// Which desktop a scenario runs on. Each has its own image, built from one
/// Dockerfile by `just e2e-image-i3` / `just e2e-image-sway`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Xvfb + i3. The original desktop; every scenario that reserves space does.
    I3,
    /// Headless Sway, which implements `wlr-layer-shell` and so exercises the
    /// Wayland backend.
    Sway,
}

impl Backend {
    pub const ALL: [Backend; 2] = [Backend::I3, Backend::Sway];

    pub fn image_name(self) -> &'static str {
        match self {
            Backend::I3 => "tauler-e2e-i3",
            Backend::Sway => "tauler-e2e-sway",
        }
    }

    /// The `just` recipe that builds this backend's image.
    pub fn image_recipe(self) -> &'static str {
        match self {
            Backend::I3 => "just e2e-image-i3",
            Backend::Sway => "just e2e-image-sway",
        }
    }

    /// The directory under `target/e2e/` this backend's runs write to.
    ///
    /// i3 keeps the bare scenario name it always had, so CI's screenshot
    /// publishing is unchanged; Sway gets a suffix so one fixture can run on
    /// both desktops without the second run overwriting the first's screenshot.
    /// The Sway entrypoint writes to the same name.
    pub fn out_name(self, scenario: &str) -> String {
        match self {
            Backend::I3 => scenario.to_string(),
            Backend::Sway => format!("{scenario}-sway"),
        }
    }

    /// The in-container command that writes a screenshot to `path`.
    fn capture_command(self, path: &str) -> Vec<String> {
        match self {
            Backend::I3 => vec!["scrot".into(), "-o".into(), path.into()],
            Backend::Sway => vec!["grim".into(), path.into()],
        }
    }
}

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

/// A running desktop with tauler in it, for one fixture.
pub struct Desktop {
    container: Container<GenericImage>,
    backend: Backend,
    scenario: String,
    /// `target/e2e/<out_name>`, where the container's `/out/<out_name>` lands.
    out_name: String,
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
        Self::start_on(Backend::I3, scenario, screen)
    }

    /// As [`Desktop::start`], on a chosen desktop.
    pub fn start_on(backend: Backend, scenario: &str, screen: Screen) -> Result<Self> {
        Self::start_on_with_env(backend, scenario, screen, &[])
    }

    /// As [`Desktop::start`], with extra environment variables for the entrypoint.
    ///
    /// Used by the a11y scenario, which asks the entrypoint to bring up a D-Bus
    /// session and the AT-SPI bus.
    pub fn start_with_env(
        scenario: &str,
        screen: Screen,
        extra_env: &[(&str, &str)],
    ) -> Result<Self> {
        Self::start_on_with_env(Backend::I3, scenario, screen, extra_env)
    }

    /// As [`Desktop::start_with_env`], on a chosen desktop.
    pub fn start_on_with_env(
        backend: Backend,
        scenario: &str,
        screen: Screen,
        extra_env: &[(&str, &str)],
    ) -> Result<Self> {
        let fixtures = crate_dir().join("fixtures");
        if !fixtures.join(scenario).is_dir() {
            bail!("no fixture named {scenario} in {}", fixtures.display());
        }

        let out_dir = out_root()?;
        let out_name = backend.out_name(scenario);
        std::fs::create_dir_all(out_dir.join(&out_name))?;

        let mut image = GenericImage::new(backend.image_name(), IMAGE_TAG)
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
        let container = image.start().with_context(|| {
            format!(
                "starting {}:{IMAGE_TAG} — is Docker running, and has `{}` been run?",
                backend.image_name(),
                backend.image_recipe()
            )
        })?;

        Ok(Self {
            container,
            backend,
            scenario: scenario.to_string(),
            out_name,
            screen,
            out_dir,
        })
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn backend(&self) -> Backend {
        self.backend
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

        if self.backend != Backend::I3 {
            bail!("record_motion records with x11grab and only works on Backend::I3");
        }
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
        if self.backend != Backend::I3 {
            bail!("root_windows reads the X server and only works on Backend::I3");
        }
        let out = self.exec(&["xwininfo", "-root", "-children"])?;
        Ok(parse_xwininfo_children(&out))
    }

    /// Capture until two consecutive frames are pixel-identical, then keep the
    /// second. A window that is mapped is not necessarily a window that has
    /// painted; this is what tells the two apart.
    pub fn capture_stable(&self) -> Result<PathBuf> {
        let dir = self.out_dir.join(&self.out_name);
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

    /// One frame, now, without waiting for the screen to settle.
    ///
    /// For watching something that is *supposed* to keep changing — a figure
    /// redrawn every few seconds never gives [`Desktop::capture_stable`] the
    /// picture a test is waiting for, only whichever one it happened to settle on.
    pub fn capture_now(&self) -> Result<PathBuf> {
        let name = "now.png";
        self.grab(&format!("/out/{}/{}", self.out_name, name))?;
        Ok(self.out_dir.join(&self.out_name).join(name))
    }

    fn capture_frame(&self, dir: &Path, slot: usize) -> Result<PathBuf> {
        let name = format!("frame-{slot}.png");
        self.grab(&format!("/out/{}/{}", self.out_name, name))?;
        Ok(dir.join(name))
    }

    /// Write one screenshot to `path` (a path inside the container).
    fn grab(&self, path: &str) -> Result<()> {
        let argv = self.backend.capture_command(path);
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        self.exec(&argv).map(|_| ())
    }

    /// The tauler log for this scenario, for putting in a failure message.
    pub fn tauler_log(&self) -> String {
        std::fs::read_to_string(self.out_dir.join(&self.out_name).join("tauler.log"))
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
/// `target/e2e/<scenario>/`, created. The container sees it as `/out/<scenario>`, so a test can
/// leave a file there for a fixture's `startup` to read before the desktop exists.
pub fn scenario_out_dir(scenario: &str) -> Result<PathBuf> {
    let dir = out_root()?.join(scenario);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

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
