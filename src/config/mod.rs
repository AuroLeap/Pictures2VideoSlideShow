//! Configuration schema of record: load and validate the TOML config
//! (`Config` → input/output/processing + per-output `OutputDef`), including
//! Ken Burns, focus/ROI, and audio fields with their serde defaults.

pub mod location;

use crate::error::{Result, SlideshowError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub input: InputConfig,
    pub output: OutputConfig,
    pub processing: ProcessingConfig,
    pub outputs: Vec<OutputDef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InputConfig {
    pub media_root: PathBuf,
    pub ignore_patterns: Vec<String>,
    pub exception_pattern: Option<String>,
    pub exception_threshold: Option<i32>,

    /// Optional JSON region-of-interest database (see [`crate::roi`]). Maps an
    /// image's path relative to `media_root` to a focus point that anchors the
    /// Ken Burns zoom (e.g. a face from prior recognition). Unit: filesystem path.
    // Implements: SR-031, LLR-038
    #[serde(default)]
    pub roi_db: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputConfig {
    pub base_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessingConfig {
    pub temp_dir: Option<PathBuf>,
    pub max_workers: Option<usize>,
    pub use_parallelism: bool,
    pub dry_run: bool,
    pub verbose: bool,

    /// Seconds of no encoder progress (no frame written to ffmpeg) after which a
    /// stalled encode is aborted and the run fails non-zero. `0` disables the
    /// watchdog. Default 120s. Unit: seconds.
    // Implements: LLR-008, SR-013
    #[serde(default = "default_ffmpeg_timeout_secs")]
    pub ffmpeg_timeout_secs: u64,

    /// Optional explicit path to an FFmpeg executable. When set, it takes
    /// priority over `PATH` and the per-user cache — the offline / use-existing
    /// FFmpeg fallback so no download is needed. Unit: filesystem path.
    // Implements: SR-027, LLR-032
    #[serde(default)]
    pub ffmpeg_path: Option<PathBuf>,

    /// Optional default Ken Burns focus point `[x, y]` in normalized `[0,1]`
    /// image coordinates, applied to images that have no entry in the ROI
    /// database. When unset, images use the default two-point pan.
    // Implements: SR-031, LLR-037
    #[serde(default)]
    pub default_focus: Option<[f32; 2]>,
}

/// Default FFmpeg inactivity timeout (SR-013): 120s with no encoder progress.
// Implements: LLR-008, SR-013
fn default_ffmpeg_timeout_secs() -> u64 {
    120
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputDef {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pic_display_time_secs: f32,
    pub fade_time_secs: f32,
    pub max_rotation_degrees: f32,
    pub bulk_video_time_min: u32,
    pub quality_crf: u32,

    /// Preserve audio from source **video** clips in this output (images are
    /// silent). When false the output has no audio track.
    // Implements: SR-032, LLR-042
    pub enable_audio: bool,

    /// AAC bitrate (kbps) for the muxed audio track when `enable_audio` is set.
    // Implements: SR-032, LLR-042
    #[serde(default = "default_audio_bitrate_kbps")]
    pub audio_bitrate_kbps: u32,

    /// Audio sample rate (Hz) for the muxed audio track when `enable_audio` is set.
    // Implements: SR-032, LLR-042
    #[serde(default = "default_audio_sample_rate")]
    pub audio_sample_rate: u32,

    /// Ken Burns zoom amount as a fraction (e.g. 0.12 = up to 12% zoom over the
    /// clip). Direction (zoom in vs out) is randomized per image.
    #[serde(default = "default_zoom_amount")]
    pub zoom_amount: f32,

    /// Enable the Ken Burns pan/zoom effect. When false, images are shown
    /// statically (cover-fit) with only the fade applied.
    #[serde(default = "default_true")]
    pub ken_burns: bool,
}

fn default_zoom_amount() -> f32 {
    0.12
}

fn default_audio_bitrate_kbps() -> u32 {
    192
}

fn default_audio_sample_rate() -> u32 {
    48_000
}

fn default_true() -> bool {
    true
}

impl OutputDef {
    /// Total number of frames for one image's clip.
    pub fn total_frames(&self) -> u32 {
        ((self.pic_display_time_secs * self.fps as f32).round() as u32).max(1)
    }

    /// Number of frames spent fading in (and, symmetrically, fading out).
    pub fn fade_frames(&self) -> u32 {
        (self.fade_time_secs * self.fps as f32).round() as u32
    }

    /// Configured width/height normalized so both are even (rounded down via
    /// `x & !1`), and never zero. yuv420p H.264 rejects odd dimensions, so odd
    /// values must never reach ffmpeg.
    // Implements: LLR-010, SR-006, SR-005
    pub fn even_dims(&self) -> (u32, u32) {
        let w = (self.width & !1).max(2);
        let h = (self.height & !1).max(2);
        (w, h)
    }
}

impl Config {
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SlideshowError::Config(format!("Failed to read config: {}", e)))?;

        if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
            toml::from_str(&content)
                .map_err(|e| SlideshowError::Config(format!("TOML parsing error: {}", e)))
        } else {
            serde_json::from_str(&content)
                .map_err(|e| SlideshowError::Config(format!("JSON parsing error: {}", e)))
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !self.input.media_root.exists() {
            return Err(SlideshowError::InvalidConfig(format!(
                "Input media root does not exist: {}",
                self.input.media_root.display()
            )));
        }

        if self.outputs.is_empty() {
            return Err(SlideshowError::InvalidConfig(
                "At least one output definition is required".to_string(),
            ));
        }

        for output in &self.outputs {
            if output.width == 0 || output.height == 0 {
                return Err(SlideshowError::InvalidConfig(format!(
                    "Output '{}' has invalid dimensions: {}x{}",
                    output.name, output.width, output.height
                )));
            }

            if output.fps == 0 {
                return Err(SlideshowError::InvalidConfig(format!(
                    "Output '{}' has invalid fps: {}",
                    output.name, output.fps
                )));
            }

            if output.quality_crf > 51 {
                return Err(SlideshowError::InvalidConfig(format!(
                    "Output '{}' has invalid quality CRF: {} (must be 0-51)",
                    output.name, output.quality_crf
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_output(width: u32, height: u32) -> OutputDef {
        OutputDef {
            name: "t".into(),
            width,
            height,
            fps: 30,
            pic_display_time_secs: 6.0,
            fade_time_secs: 0.5,
            max_rotation_degrees: 15.0,
            bulk_video_time_min: 20,
            quality_crf: 28,
            enable_audio: false,
            audio_bitrate_kbps: 192,
            audio_sample_rate: 48_000,
            zoom_amount: 0.12,
            ken_burns: true,
        }
    }

    // Verifies: LLR-010, SR-006
    #[test]
    fn even_dims_rounds_down_sr006() {
        assert_eq!(sample_output(1441, 901).even_dims(), (1440, 900));
        assert_eq!(sample_output(1920, 1080).even_dims(), (1920, 1080));
        assert_eq!(sample_output(1025, 769).even_dims(), (1024, 768));
    }

    // Verifies: LLR-010, SR-006
    #[test]
    fn even_dims_never_zero_sr006() {
        // Odd 1 rounds down to 0 naively; must be clamped up to the even minimum.
        assert_eq!(sample_output(1, 1).even_dims(), (2, 2));
    }

    // Verifies: SR-032, LLR-042 — audio config fields default when omitted from
    // the TOML and explicit values round-trip through serialize/deserialize.
    #[test]
    fn audio_fields_default_and_roundtrip_sr032() {
        let toml_in = r#"
name = "f"
width = 320
height = 240
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 0.0
bulk_video_time_min = 20
quality_crf = 28
enable_audio = true
"#;
        let od: OutputDef = toml::from_str(toml_in).expect("parse");
        assert!(od.enable_audio);
        assert_eq!(od.audio_bitrate_kbps, 192);
        assert_eq!(od.audio_sample_rate, 48_000);

        let mut od2 = od.clone();
        od2.audio_bitrate_kbps = 128;
        od2.audio_sample_rate = 44_100;
        let s = toml::to_string(&od2).expect("serialize");
        let back: OutputDef = toml::from_str(&s).expect("reparse");
        assert_eq!(back.audio_bitrate_kbps, 128);
        assert_eq!(back.audio_sample_rate, 44_100);
    }
}
