pub mod config;
pub mod error;
pub mod logging;
pub mod media;
pub mod image;
pub mod video;
pub mod transform;
pub mod ffmpeg;
pub mod pipeline;
pub mod util;

pub use error::{SlideshowError, Result};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
