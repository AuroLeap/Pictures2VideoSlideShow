//! Small shared utilities: filesystem helpers, output-size estimation, and
//! stage-timer/perf-metrics instrumentation.

pub mod estimate;
pub mod file_utils;
pub mod timing;

pub use file_utils::{disk_full_error, ensure_dir_exists, is_disk_full};
