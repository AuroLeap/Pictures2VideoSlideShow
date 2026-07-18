//! Runtime-prerequisite checks shared by the `validate` command and `build`.
//!
//! Each essential RUNTIME prerequisite is probed independently and reported as
//! a plain-language pass/fail line. There is deliberately NO Rust/build-
//! toolchain check — FFmpeg is the sole external runtime dependency (SR-002).
// Implements: LLR-004, LLR-005, LLR-006, SR-002, SR-012

use crate::config::Config;
use std::path::Path;
use std::process::{Command, Stdio};

/// Result of one prerequisite probe.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    /// Whether failing this check should block `build` / fail `validate`.
    pub essential: bool,
    pub detail: String,
}

impl CheckResult {
    fn pass(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: true,
            essential: true,
            detail: detail.into(),
        }
    }

    fn fail(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: false,
            essential: true,
            detail: detail.into(),
        }
    }
}

/// True when every essential check passed.
// Implements: LLR-004, LLR-005, SR-002
pub fn all_essential_passed(results: &[CheckResult]) -> bool {
    results.iter().all(|r| !r.essential || r.passed)
}

/// Process exit code implied by a set of check results: 0 when all essential
/// checks passed, 1 otherwise.
// Implements: LLR-004, LLR-005, SR-002
pub fn exit_code(results: &[CheckResult]) -> i32 {
    if all_essential_passed(results) {
        0
    } else {
        1
    }
}

/// Run every runtime-prerequisite check for `config`.
// Implements: LLR-004, LLR-006, SR-002, SR-012
pub fn run_checks(config: &Config) -> Vec<CheckResult> {
    let mut checks = vec![
        check_ffmpeg(config),
        check_program("ffprobe", "ffprobe"),
        check_config_valid(config),
        check_media_root(&config.input.media_root),
        check_output_writable(&config.output.base_dir),
    ];
    checks.extend(check_encoders(config));
    checks
}

/// One probe-result line per output requesting a non-software encoder
/// (SR-034). Deliberately never essential: an unavailable hardware encoder
/// degrades to software at build time with a logged reason — it must not fail
/// `validate` or block `build`. `software` outputs (and invalid values, which
/// the essential config check already rejects) get no line — software is never
/// probed.
// Implements: SR-034, LLR-046, LLR-048
fn check_encoders(config: &Config) -> Vec<CheckResult> {
    use crate::ffmpeg::encoder_args::EncoderRequest;
    config
        .outputs
        .iter()
        .filter(|o| EncoderRequest::parse(&o.encoder) != EncoderRequest::Software)
        .map(|o| {
            let sel = crate::ffmpeg::probe::selection_for(
                config.processing.ffmpeg_path.as_deref(),
                &o.encoder,
            );
            let name = format!("Encoder '{}' (output '{}')", o.encoder, o.name);
            match sel.fallback_reason {
                None => CheckResult::pass(&name, format!("will use {}", sel.encoder.codec_name())),
                Some(reason) => CheckResult {
                    name,
                    passed: false,
                    essential: false,
                    detail: reason,
                },
            }
        })
        .collect()
}

/// Resolve FFmpeg per the SR-027 order (configured `ffmpeg_path` → PATH →
/// per-user cache) so the check reflects every place a usable FFmpeg may live.
// Implements: LLR-006, LLR-032, SR-012, SR-027
fn check_ffmpeg(config: &Config) -> CheckResult {
    let cache = crate::setup::ffmpeg_fetch::cache_dir();
    match crate::ffmpeg::resolve::resolve(config.processing.ffmpeg_path.as_deref(), &cache) {
        Some(p) => CheckResult::pass("FFmpeg", format!("found ({})", p.display())),
        None => CheckResult::fail(
            "FFmpeg",
            "not found on PATH, via config `ffmpeg_path`, or in the per-user cache — \
             install it / add to PATH, or set `ffmpeg_path`",
        ),
    }
}

/// Probe an external program by spawning `<prog> -version`.
// Implements: LLR-006, SR-012
fn check_program(label: &str, prog: &str) -> CheckResult {
    let spawned = Command::new(prog)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status();
    match spawned {
        Ok(s) if s.success() => CheckResult::pass(label, "found"),
        Ok(_) => CheckResult::fail(label, format!("'{} -version' did not run cleanly", prog)),
        Err(_) => CheckResult::fail(
            label,
            format!("{} not found — install it or add it to PATH", label),
        ),
    }
}

fn check_config_valid(config: &Config) -> CheckResult {
    match config.validate() {
        Ok(()) => CheckResult::pass("Config valid", "parsed and validated"),
        Err(e) => CheckResult::fail("Config valid", e.to_string()),
    }
}

fn check_media_root(path: &Path) -> CheckResult {
    if !path.exists() {
        CheckResult::fail(
            "Media root readable",
            format!("path does not exist: {}", path.display()),
        )
    } else if std::fs::read_dir(path).is_err() {
        CheckResult::fail(
            "Media root readable",
            format!("cannot read directory: {}", path.display()),
        )
    } else {
        CheckResult::pass("Media root readable", path.display().to_string())
    }
}

// Implements: LLR-019, SR-016
fn check_output_writable(path: &Path) -> CheckResult {
    match crate::util::ensure_dir_exists(path) {
        Ok(()) => CheckResult::pass("Output base_dir writable", path.display().to_string()),
        Err(e) => CheckResult::fail("Output base_dir writable", e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results(spec: &[(bool, bool)]) -> Vec<CheckResult> {
        spec.iter()
            .enumerate()
            .map(|(i, &(passed, essential))| CheckResult {
                name: format!("c{i}"),
                passed,
                essential,
                detail: String::new(),
            })
            .collect()
    }

    // Verifies: LLR-004, SR-002
    #[test]
    fn exit_zero_when_all_essential_pass_sr002() {
        let r = results(&[(true, true), (true, true), (false, false)]);
        assert!(all_essential_passed(&r));
        assert_eq!(exit_code(&r), 0);
    }

    // Verifies: LLR-005, SR-002
    #[test]
    fn exit_nonzero_when_essential_fails_sr002() {
        let r = results(&[(true, true), (false, true)]);
        assert!(!all_essential_passed(&r));
        assert_eq!(exit_code(&r), 1);
    }

    // Verifies: LLR-004, SR-002
    #[test]
    fn nonessential_failure_does_not_block_sr002() {
        let r = results(&[(false, false)]);
        assert!(all_essential_passed(&r));
        assert_eq!(exit_code(&r), 0);
    }
}
