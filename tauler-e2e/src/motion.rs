//! A desktop that moves, and the two numbers a still photograph cannot give.
//!
//! [`crate::Desktop::capture_stable`] is defined as the frame after the screen
//! stopped changing, so every question about *how long* something took, or
//! whether anything flickered on the way, is structurally outside it. This
//! module records the screen as PNG frames while a fixture's `motion` script
//! drives it with `xdotool`, and then measures regions across those frames.
//!
//! Frames, not a video file: no encode, no `yuv420p` colour conversion, and the
//! comparisons below are of exact pixels rather than of something that has been
//! through a lossy codec twice.
//!
//! # Measuring without a shared clock
//!
//! Aligning "when xdotool ran" to "which frame" means reconciling two clocks
//! across a process boundary and an unknown ffmpeg startup latency. The number
//! that actually matters does not need it: pick a **trigger** region where the
//! cause becomes visible (the tiling area, when a window opens) and a
//! **readout** region that is supposed to react (the bar), and count the frames
//! between the first change in one and the first change in the other. That is
//! event-to-repaint as a person experiences it — how long after the thing
//! happened did the bar admit it — and it is immune to when recording started.
//!
//! `events.tsv` is still written by the driving script and read back, but as
//! context for a human rather than as the basis of the measurement.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use image::RgbaImage;

use crate::Rect;

/// One recorded run.
pub struct Motion {
    /// Frames in capture order.
    pub frames: Vec<PathBuf>,
    /// What the recorder was asked for. Frame *n* is at `n / fps` seconds.
    pub fps: u32,
    /// What the driving script said it did, as `(milliseconds, label)`.
    pub events: Vec<(u64, String)>,
}

/// A region's pixels across every frame, extracted in one pass.
///
/// Decoding a 1920×1080 PNG is the expensive part and there are hundreds of
/// them, so a caller that wants three regions should ask for three series
/// rather than scanning the frames three times per question.
pub struct Series {
    region: Rect,
    per_frame: Vec<Vec<u8>>,
}

impl Motion {
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Milliseconds for a count of frames, at the recorded rate.
    pub fn ms(&self, frames: usize) -> f64 {
        frames as f64 * 1000.0 / self.fps as f64
    }

    /// Extract `region` from every frame.
    pub fn series(&self, region: Rect) -> Result<Series> {
        let mut per_frame = Vec::with_capacity(self.frames.len());
        for path in &self.frames {
            per_frame.push(crop(path, region)?);
        }
        Ok(Series { region, per_frame })
    }
}

impl Series {
    pub fn region(&self) -> Rect {
        self.region
    }

    pub fn len(&self) -> usize {
        self.per_frame.len()
    }

    pub fn is_empty(&self) -> bool {
        self.per_frame.is_empty()
    }

    /// Mean absolute per-channel difference between two frames of this region.
    ///
    /// Mean rather than max: a single stray pixel from a cursor or an
    /// antialiasing seam should not read as "the bar redrew".
    pub fn delta(&self, a: usize, b: usize) -> f64 {
        let (x, y) = (&self.per_frame[a], &self.per_frame[b]);
        if x.len() != y.len() || x.is_empty() {
            return f64::INFINITY;
        }
        let total: u64 = x
            .iter()
            .zip(y)
            .map(|(p, q)| p.abs_diff(*q) as u64)
            .sum::<u64>();
        total as f64 / x.len() as f64
    }

    /// The first frame at or after `from` that differs from frame `from` by
    /// more than `threshold`.
    pub fn first_change(&self, from: usize, threshold: f64) -> Option<usize> {
        (from + 1..self.len()).find(|&i| self.delta(from, i) > threshold)
    }

    /// The last frame of the run, as the settled "new" content.
    pub fn last(&self) -> usize {
        self.len().saturating_sub(1)
    }

    /// Frames between `before` and `after` that match neither the old content
    /// nor the new one — the map/unmap flash, if there is one.
    ///
    /// A surface that is unmapped and remapped shows something that is neither:
    /// the desktop behind it, or nothing. A surface that simply repaints goes
    /// from old to new in one frame, or through frames that resemble one or the
    /// other. This reports the candidates; whether a candidate is a flash or a
    /// legitimate intermediate render is a judgement, and the frame numbers are
    /// here so it can be made by looking.
    pub fn transitional(&self, before: usize, after: usize, threshold: f64) -> Vec<usize> {
        ((before + 1)..after)
            .filter(|&i| self.delta(before, i) > threshold && self.delta(after, i) > threshold)
            .collect()
    }
}

/// Read one region out of one frame as raw RGBA bytes.
fn crop(path: &Path, region: Rect) -> Result<Vec<u8>> {
    let img: RgbaImage = image::open(path)
        .with_context(|| format!("decoding {}", path.display()))?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let x1 = (region.x as u32 + region.width).min(w);
    let y1 = (region.y as u32 + region.height).min(h);
    let x0 = (region.x as u32).min(x1);
    let y0 = (region.y as u32).min(y1);

    let mut out = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            out.extend_from_slice(&img.get_pixel(x, y).0);
        }
    }
    if out.is_empty() {
        return Err(anyhow!("region {region} is outside {}", path.display()));
    }
    Ok(out)
}

/// Collect the frames the recorder wrote, in order, plus whatever the driving
/// script logged.
pub(crate) fn collect(dir: &Path, fps: u32) -> Result<Motion> {
    let frame_dir = dir.join("motion");
    let mut frames: Vec<PathBuf> = std::fs::read_dir(&frame_dir)
        .with_context(|| format!("no frames in {}", frame_dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    frames.sort();

    if frames.is_empty() {
        return Err(anyhow!(
            "the recorder wrote no frames to {}",
            frame_dir.display()
        ));
    }

    let events = std::fs::read_to_string(dir.join("events.tsv"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (ms, label) = line.split_once('\t')?;
            Some((ms.trim().parse().ok()?, label.trim().to_string()))
        })
        .collect();

    Ok(Motion {
        frames,
        fps,
        events,
    })
}
