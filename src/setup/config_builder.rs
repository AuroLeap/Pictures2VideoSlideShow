//! Pure mapping from first-run wizard inputs to a valid [`Config`], plus parsing
//! of the wizard's `key=value` output. This logic is unit-tested independently
//! of the GUI (the dialog in `wizard.rs` is Demonstration-only).
// Implements: LLR-030, SR-026

use crate::config::{Config, InputConfig, OutputConfig, OutputDef, ProcessingConfig};
use crate::error::{Result, SlideshowError};
use std::path::PathBuf;

/// Values collected by the first-run wizard.
#[derive(Debug, Clone)]
pub struct WizardValues {
    pub source: PathBuf,
    pub output_dir: PathBuf,
    pub temp_dir: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality_crf: u32,
    pub pic_display_time_secs: f32,
    /// Number of frame outputs to scaffold (single vs multiple frames).
    pub frames: u32,
}

const DEFAULT_FFMPEG_TIMEOUT_SECS: u64 = 120;

/// Build a valid [`Config`] from wizard values. One `[[outputs]]` per frame
/// (named `frame-1..frame-N`), even dimensions, with sensible defaults for
/// fields the wizard does not ask about. Single vs multiple frames is driven by
/// `frames` (clamped to >= 1).
// Implements: LLR-030, SR-026
pub fn build_config(v: &WizardValues) -> Config {
    let frames = v.frames.max(1);
    let width = (v.width & !1).max(2);
    let height = (v.height & !1).max(2);

    let outputs = (1..=frames)
        .map(|i| OutputDef {
            name: if frames == 1 {
                "frame".to_string()
            } else {
                format!("frame-{i}")
            },
            width,
            height,
            fps: v.fps,
            pic_display_time_secs: v.pic_display_time_secs,
            fade_time_secs: 0.5,
            max_rotation_degrees: 15.0,
            bulk_video_time_min: 20,
            quality_crf: v.quality_crf,
            enable_audio: false,
            audio_bitrate_kbps: 192,
            audio_sample_rate: 48_000,
            // SR-034/SR-035 defaults: the wizard does not ask about encoders.
            encoder: "software".into(),
            x264_preset: "medium".into(),
            zoom_amount: 0.12,
            ken_burns: true,
        })
        .collect();

    Config {
        input: InputConfig {
            media_root: v.source.clone(),
            ignore_patterns: vec!["DNP".to_string()],
            exception_pattern: None,
            exception_threshold: None,
            roi_db: None,
        },
        output: OutputConfig {
            base_dir: v.output_dir.clone(),
        },
        processing: ProcessingConfig {
            temp_dir: v.temp_dir.clone(),
            max_workers: None,
            use_parallelism: true,
            dry_run: false,
            verbose: false,
            ffmpeg_timeout_secs: DEFAULT_FFMPEG_TIMEOUT_SECS,
            ffmpeg_path: None,
            default_focus: None,
            segment_cache_gb: 20.0,
            // SR-039 default: GPU when a usable adapter exists, else CPU.
            render_backend: "auto".into(),
        },
        outputs,
    }
}

/// Serialize a config to pretty TOML for writing beside the exe / under APPDATA.
// Implements: LLR-030, SR-026, SR-030
pub fn to_toml(config: &Config) -> Result<String> {
    toml::to_string_pretty(config)
        .map_err(|e| SlideshowError::Config(format!("failed to serialize config: {}", e)))
}

/// Parse the wizard's `key=value` stdout into [`WizardValues`]. Unknown lines
/// are ignored; missing required keys are an error.
// Implements: LLR-030, SR-026
pub fn parse_wizard_output(stdout: &str) -> Result<WizardValues> {
    let mut map = std::collections::HashMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if let Some((k, val)) = line.split_once('=') {
            map.insert(k.trim().to_string(), val.trim().to_string());
        }
    }

    let get = |k: &str| -> Result<String> {
        map.get(k)
            .cloned()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| SlideshowError::Config(format!("wizard did not provide '{}'", k)))
    };
    let parse_u32 = |k: &str, s: &str| -> Result<u32> {
        s.parse::<u32>()
            .map_err(|_| SlideshowError::Config(format!("wizard '{}' is not a number: {}", k, s)))
    };

    let source = PathBuf::from(get("source")?);
    let output_dir = PathBuf::from(get("output_dir")?);
    let temp = map.get("temp_dir").map(|s| s.trim()).unwrap_or("");
    let temp_dir = if temp.is_empty() {
        None
    } else {
        Some(PathBuf::from(temp))
    };
    let ws = get("width")?;
    let hs = get("height")?;
    let fpss = get("fps")?;
    let crfs = get("quality_crf")?;
    let disp = get("pic_display_time_secs")?;
    let display = disp.parse::<f32>().map_err(|_| {
        SlideshowError::Config(format!(
            "wizard 'pic_display_time_secs' is not a number: {}",
            disp
        ))
    })?;
    let frames = map
        .get("frames")
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(1)
        .max(1);

    Ok(WizardValues {
        source,
        output_dir,
        temp_dir,
        width: parse_u32("width", &ws)?,
        height: parse_u32("height", &hs)?,
        fps: parse_u32("fps", &fpss)?,
        quality_crf: parse_u32("quality_crf", &crfs)?,
        pic_display_time_secs: display,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(frames: u32) -> WizardValues {
        WizardValues {
            source: PathBuf::from("C:/Photos"),
            output_dir: PathBuf::from("C:/Videos"),
            temp_dir: None,
            width: 1441, // odd on purpose
            height: 900,
            fps: 30,
            quality_crf: 28,
            pic_display_time_secs: 6.0,
            frames,
        }
    }

    // Verifies: SR-026, LLR-030 — single frame -> one output, even dims.
    #[test]
    fn single_frame_builds_one_output_even_dims_sr026() {
        let c = build_config(&sample(1));
        assert_eq!(c.outputs.len(), 1);
        assert_eq!(c.outputs[0].name, "frame");
        assert_eq!(c.outputs[0].width % 2, 0); // 1441 -> 1440
        assert_eq!(c.outputs[0].width, 1440);
        assert_eq!(c.input.media_root, PathBuf::from("C:/Photos"));
        assert_eq!(c.output.base_dir, PathBuf::from("C:/Videos"));
    }

    // Verifies: SR-026, LLR-030 — multiple frames -> N distinctly named outputs.
    #[test]
    fn multiple_frames_build_distinct_outputs_sr026() {
        let c = build_config(&sample(3));
        assert_eq!(c.outputs.len(), 3);
        let names: Vec<_> = c.outputs.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, vec!["frame-1", "frame-2", "frame-3"]);
    }

    // Verifies: SR-026, LLR-030 — produced config round-trips through TOML.
    #[test]
    fn build_config_roundtrips_through_toml_sr026() {
        let c = build_config(&sample(2));
        let toml_str = to_toml(&c).expect("serialize");
        let back: Config = toml::from_str(&toml_str).expect("parse back");
        assert_eq!(back.outputs.len(), 2);
        assert_eq!(
            back.processing.ffmpeg_timeout_secs,
            DEFAULT_FFMPEG_TIMEOUT_SECS
        );
    }

    // Verifies: SR-026, LLR-030 — wizard key=value output parses to values.
    #[test]
    fn parse_wizard_output_reads_keys_sr026() {
        let out = "source=C:/Photos\noutput_dir=C:/Videos\ntemp_dir=\nwidth=1440\nheight=900\nfps=30\nquality_crf=28\npic_display_time_secs=6\nframes=2\n";
        let v = parse_wizard_output(out).expect("parse");
        assert_eq!(v.source, PathBuf::from("C:/Photos"));
        assert_eq!(v.width, 1440);
        assert_eq!(v.frames, 2);
        assert!(v.temp_dir.is_none());
    }

    // Verifies: SR-026, LLR-030 — missing required keys error (no silent default).
    #[test]
    fn parse_wizard_output_errors_on_missing_required_sr026() {
        let out = "width=1440\nheight=900\n"; // no source/output_dir
        assert!(parse_wizard_output(out).is_err());
    }
}
