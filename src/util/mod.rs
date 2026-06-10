//! Small shared utilities: filesystem helpers and output-size estimation.

pub mod estimate;
pub mod file_utils;

pub use file_utils::{disk_full_error, ensure_dir_exists, is_disk_full};
