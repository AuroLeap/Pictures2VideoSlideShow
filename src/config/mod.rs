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
    // These three are documented in Quick Reference §4 with defaults
    // (true / false / false), so omitting them must parse (SR-003).
    // Implements: SR-003
    #[serde(default = "default_true")]
    pub use_parallelism: bool,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
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

    /// H.264 encoder for this output (SR-034 set): `software` (libx264, the
    /// default — behavior unchanged), `auto` (probe hardware at preflight,
    /// fall back to software), or an explicit `h264_nvenc`/`h264_qsv`/
    /// `h264_amf` (probe-checked, falls back with a logged warning).
    /// Per-output so one build can target different encoders (SR-017).
    // Implements: SR-034, LLR-047
    #[serde(default = "default_encoder")]
    pub encoder: String,

    /// libx264 speed preset, software-encoder path only (SR-035). Default
    /// `medium` keeps the FFmpeg arg list byte-identical to before; values
    /// outside [`ALLOWED_X264_PRESETS`] are rejected at validation.
    // Implements: SR-035, LLR-049
    #[serde(default = "default_x264_preset")]
    pub x264_preset: String,

    /// Ken Burns zoom amount as a fraction (e.g. 0.12 = up to 12% zoom over the
    /// clip). Direction (zoom in vs out) is randomized per image.
    #[serde(default = "default_zoom_amount")]
    pub zoom_amount: f32,

    /// Enable the Ken Burns pan/zoom effect. When false, images are shown
    /// statically (cover-fit) with only the fade applied.
    #[serde(default = "default_true")]
    pub ken_burns: bool,
}

/// The `encoder` values Config::validate accepts (SR-034).
// Implements: SR-034, LLR-047
pub const ALLOWED_ENCODERS: [&str; 5] = ["software", "auto", "h264_nvenc", "h264_qsv", "h264_amf"];

/// The `x264_preset` values Config::validate accepts: the libx264 speed
/// ladder (SR-035; `placebo` deliberately excluded — never a practical trade).
// Implements: SR-035, LLR-049
pub const ALLOWED_X264_PRESETS: [&str; 9] = [
    "ultrafast",
    "superfast",
    "veryfast",
    "faster",
    "fast",
    "medium",
    "slow",
    "slower",
    "veryslow",
];

fn default_encoder() -> String {
    "software".into()
}

fn default_x264_preset() -> String {
    "medium".into()
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

            // Implements: SR-034, LLR-047 — encoder restricted to the SR-034 set.
            if !ALLOWED_ENCODERS.contains(&output.encoder.as_str()) {
                return Err(SlideshowError::InvalidConfig(format!(
                    "Output '{}' has invalid encoder: '{}' (allowed values: {})",
                    output.name,
                    output.encoder,
                    ALLOWED_ENCODERS.join(", ")
                )));
            }

            // Implements: SR-035, LLR-049 — x264_preset restricted to the
            // libx264 speed ladder.
            if !ALLOWED_X264_PRESETS.contains(&output.x264_preset.as_str()) {
                return Err(SlideshowError::InvalidConfig(format!(
                    "Output '{}' has invalid x264_preset: '{}' (allowed values: {})",
                    output.name,
                    output.x264_preset,
                    ALLOWED_X264_PRESETS.join(", ")
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
            encoder: default_encoder(),
            x264_preset: default_x264_preset(),
            zoom_amount: 0.12,
            ken_burns: true,
        }
    }

    /// A Config whose non-output parts pass validation (media_root = "." always
    /// exists), so output-field validation is exercised in isolation.
    fn config_with_outputs(outputs: Vec<OutputDef>) -> Config {
        Config {
            input: InputConfig {
                media_root: PathBuf::from("."),
                ignore_patterns: vec![],
                exception_pattern: None,
                exception_threshold: None,
                roi_db: None,
            },
            output: OutputConfig {
                base_dir: PathBuf::from("out"),
            },
            processing: toml::from_str("").expect("default processing"),
            outputs,
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

    // Verifies: SR-003 — the [processing] flags documented with defaults in
    // Quick Reference §4 (use_parallelism/dry_run/verbose) may be omitted from
    // the TOML and take those documented defaults (true/false/false).
    #[test]
    fn processing_flags_default_when_omitted_sr003() {
        let p: ProcessingConfig = toml::from_str("").expect("empty [processing] should parse");
        assert!(p.use_parallelism, "documented default: true");
        assert!(!p.dry_run, "documented default: false");
        assert!(!p.verbose, "documented default: false");
        assert_eq!(p.ffmpeg_timeout_secs, 120, "documented default: 120s");
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

    /// Minimal OutputDef TOML with extra lines appended (for the new fields).
    fn output_toml(extra: &str) -> String {
        format!(
            r#"
name = "f"
width = 320
height = 240
fps = 24
pic_display_time_secs = 1.0
fade_time_secs = 0.2
max_rotation_degrees = 0.0
bulk_video_time_min = 20
quality_crf = 28
enable_audio = false
{extra}
"#
        )
    }

    // Verifies: SR-034, LLR-047 (TC-066) — `encoder` defaults to software when
    // omitted, every SR-034 value validates, an invalid value is rejected with
    // a plain-language error naming the field and allowed set, and the field
    // is per-output (two [[outputs]] may differ).
    #[test]
    fn encoder_field_defaults_validates_and_rejects_sr034() {
        // Omitted -> software, so existing configs parse unchanged.
        let od: OutputDef = toml::from_str(&output_toml("")).expect("parse");
        assert_eq!(od.encoder, "software");

        // Every allowed value passes validation.
        for value in ALLOWED_ENCODERS {
            let od: OutputDef =
                toml::from_str(&output_toml(&format!("encoder = \"{value}\""))).expect("parse");
            config_with_outputs(vec![od])
                .validate()
                .unwrap_or_else(|e| panic!("'{value}' must validate: {e}"));
        }

        // An invalid value is rejected, naming the field and the allowed set.
        let od: OutputDef =
            toml::from_str(&output_toml("encoder = \"hevc_nvenc\"")).expect("parse");
        let err = config_with_outputs(vec![od])
            .validate()
            .expect_err("invalid encoder must be rejected")
            .to_string();
        assert!(err.contains("encoder"), "names the field: {err}");
        assert!(err.contains("hevc_nvenc"), "names the bad value: {err}");
        for allowed in ALLOWED_ENCODERS {
            assert!(
                err.contains(allowed),
                "lists allowed value {allowed}: {err}"
            );
        }

        // Per-output: two outputs with different encoders both validate.
        let a: OutputDef = toml::from_str(&output_toml("encoder = \"h264_nvenc\"")).expect("parse");
        let b: OutputDef = toml::from_str(&output_toml("encoder = \"software\"")).expect("parse");
        assert_ne!(a.encoder, b.encoder);
        config_with_outputs(vec![a, b])
            .validate()
            .expect("per-output encoders validate");
    }

    // Verifies: SR-035, LLR-049 (TC-070) — `x264_preset` defaults to medium
    // when omitted; an unsupported value is rejected with a plain-language
    // error naming the field and the allowed values.
    #[test]
    fn x264_preset_rejects_unsupported_sr035() {
        let od: OutputDef = toml::from_str(&output_toml("")).expect("parse");
        assert_eq!(od.x264_preset, "medium");

        for value in ALLOWED_X264_PRESETS {
            let od: OutputDef =
                toml::from_str(&output_toml(&format!("x264_preset = \"{value}\""))).expect("parse");
            config_with_outputs(vec![od])
                .validate()
                .unwrap_or_else(|e| panic!("'{value}' must validate: {e}"));
        }

        let od: OutputDef = toml::from_str(&output_toml("x264_preset = \"turbo\"")).expect("parse");
        let err = config_with_outputs(vec![od])
            .validate()
            .expect_err("invalid preset must be rejected")
            .to_string();
        assert!(err.contains("x264_preset"), "names the field: {err}");
        assert!(err.contains("turbo"), "names the bad value: {err}");
        assert!(
            err.contains("veryfast") && err.contains("slow"),
            "lists allowed values: {err}"
        );
    }
}
