//! The crate-wide error type: every failure maps to a `SlideshowError`
//! variant with a plain-language message (no panics/stack traces for the
//! end user).

use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SlideshowError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("FFmpeg error: {0}")]
    Ffmpeg(String),

    #[error("Transform calculation error: {0}")]
    #[allow(dead_code)] // part of the error taxonomy; reserved for future use
    Transform(String),

    #[error("Media loading error: {0}")]
    Media(String),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Processing error: {0}")]
    #[allow(dead_code)] // part of the error taxonomy; reserved for future use
    Processing(String),

    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SlideshowError>;
