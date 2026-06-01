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
    pub enable_audio: bool,

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
