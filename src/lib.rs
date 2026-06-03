pub mod config;
pub mod error;
pub mod ffmpeg;
pub mod image;
pub mod logging;
pub mod media;
pub mod pipeline;
pub mod preflight;
pub mod setup;
pub mod transform;
pub mod util;
pub mod video;

pub use error::{Result, SlideshowError};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
