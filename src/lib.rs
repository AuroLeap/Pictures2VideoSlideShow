//! Library crate root: re-exports the engine's modules so integration tests
//! and the binary share one implementation.

pub mod cache;
pub mod config;
pub mod error;
pub mod ffmpeg;
pub mod image;
pub mod logging;
pub mod media;
pub mod pipeline;
pub mod preflight;
pub mod roi;
pub mod setup;
pub mod transform;
pub mod util;
pub mod video;

pub use error::{Result, SlideshowError};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
