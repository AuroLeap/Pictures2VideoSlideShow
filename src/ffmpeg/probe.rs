//! Preflight hardware-encoder probe and fallback selection (SR-034): list the
//! resolved FFmpeg's encoders, trial-encode one frame, classify the outcome,
//! memoize per (ffmpeg, encoder), and pure-select the encoder actually used.
//! A missing/failing hardware encoder degrades to libx264 with a reason —
//! never a build failure.
// Implements: LLR-046, LLR-048, SR-034

use crate::ffmpeg::encoder_args::{EncoderChoice, EncoderRequest};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

/// Result of probing one hardware encoder against one FFmpeg build.
// Implements: LLR-046, SR-034
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// Listed by `-encoders` and the 1-frame trial encode succeeded.
    Pass,
    /// Listed but the trial encode failed (reason from ffmpeg stderr).
    Fail(String),
    /// Not present in this FFmpeg build's `-encoders` listing.
    Absent,
}

impl ProbeOutcome {
    /// Short human summary for fallback-reason strings.
    fn summary(&self) -> String {
        match self {
            ProbeOutcome::Pass => "ok".into(),
            ProbeOutcome::Fail(r) => format!("probe failed ({r})"),
            ProbeOutcome::Absent => "not in this FFmpeg build".into(),
        }
    }
}

/// Hardware candidates `auto` probes, in preference order (NVENC first: the
/// most common working GPU encoder on this product's Windows audience).
// Implements: LLR-048, SR-034
pub const AUTO_CANDIDATES: [EncoderChoice; 3] = [
    EncoderChoice::H264Nvenc,
    EncoderChoice::H264Qsv,
    EncoderChoice::H264Amf,
];

/// Pure: extract the supported-encoder name set from `ffmpeg -encoders`
/// listing text. Encoder lines are ` <6-char flags> <name> <description>`;
/// legend lines (`V..... = Video`) and headers are ignored. Only video (`V`)
/// encoders are collected — that is the only kind SR-034 selects.
// Implements: LLR-046, SR-034
pub fn parse_encoders(listing: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for line in listing.lines() {
        let mut tokens = line.split_whitespace();
        let (Some(flags), Some(name)) = (tokens.next(), tokens.next()) else {
            continue;
        };
        let is_flags = flags.len() == 6
            && flags.starts_with('V')
            && flags.chars().all(|c| "VASFXBD.".contains(c));
        if is_flags && name != "=" {
            set.insert(name.to_string());
        }
    }
    set
}

/// Pure: classify a completed trial encode from its exit success and stderr.
/// The reason is the first non-empty stderr line so the fallback warning stays
/// one line, not an ffmpeg spew.
// Implements: LLR-046, SR-034
pub fn classify_trial(success: bool, stderr: &str) -> ProbeOutcome {
    if success {
        return ProbeOutcome::Pass;
    }
    let reason = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("trial encode failed")
        .to_string();
    ProbeOutcome::Fail(reason)
}

/// Pure decision leg of the probe: an unlisted encoder is `Absent` (the trial
/// closure is never invoked); a listed one is classified from its trial
/// result via [`classify_trial`].
// Implements: LLR-046, SR-034
pub fn classify_probe<F: FnOnce() -> (bool, String)>(listed: bool, trial: F) -> ProbeOutcome {
    if !listed {
        return ProbeOutcome::Absent;
    }
    let (ok, stderr) = trial();
    classify_trial(ok, &stderr)
}

/// Probe `encoder` against the **resolved** FFmpeg (LLR-032 — the binary
/// preflight resolved, not bare PATH): `-encoders` listing, then a 1-frame
/// rawvideo trial encode to the null muxer. Never errors — any I/O failure
/// classifies as `Fail(reason)` so a broken probe degrades, per SR-034.
// Implements: LLR-046, SR-034
pub fn probe_encoder(ffmpeg: &Path, encoder: &str) -> ProbeOutcome {
    let listing = match Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(e) => return ProbeOutcome::Fail(format!("could not run ffmpeg -encoders: {e}")),
    };
    let listed = parse_encoders(&listing).contains(encoder);
    classify_probe(listed, || trial_encode(ffmpeg, encoder))
}

/// One 320x240 black rgb24 frame through `encoder` to the null muxer.
/// (320x240 comfortably clears every hardware encoder's minimum dimensions.)
/// Returns (exit success, stderr text).
fn trial_encode(ffmpeg: &Path, encoder: &str) -> (bool, String) {
    const W: usize = 320;
    const H: usize = 240;
    let mut child = match Command::new(ffmpeg)
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgb24",
            "-video_size",
            "320x240",
            "-framerate",
            "30",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-an",
            "-c:v",
            encoder,
            "-pix_fmt",
            "yuv420p",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (false, format!("could not spawn trial encode: {e}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&vec![0u8; W * H * 3]);
        // Dropping stdin signals EOF after the single frame.
    }
    match child.wait_with_output() {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(e) => (false, format!("trial encode did not complete: {e}")),
    }
}

/// Memoizes probe outcomes per (ffmpeg, encoder) so a multi-output run
/// (SR-017) probes each encoder once per resolved FFmpeg.
// Implements: LLR-046, SR-034
#[derive(Default)]
pub struct ProbeCache {
    map: HashMap<(PathBuf, String), ProbeOutcome>,
}

impl ProbeCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cached outcome for (ffmpeg, encoder), invoking `probe` only on a miss.
    /// The injectable prober keeps the memoization unit-testable without I/O.
    pub fn outcome_with<F: FnOnce(&Path, &str) -> ProbeOutcome>(
        &mut self,
        ffmpeg: &Path,
        encoder: &str,
        probe: F,
    ) -> ProbeOutcome {
        let key = (ffmpeg.to_path_buf(), encoder.to_string());
        if let Some(hit) = self.map.get(&key) {
            return hit.clone();
        }
        let outcome = probe(ffmpeg, encoder);
        self.map.insert(key, outcome.clone());
        outcome
    }

    /// [`outcome_with`](Self::outcome_with) wired to the real [`probe_encoder`].
    pub fn outcome(&mut self, ffmpeg: &Path, encoder: &str) -> ProbeOutcome {
        self.outcome_with(ffmpeg, encoder, probe_encoder)
    }
}

/// The encoder actually used for an output, plus the fallback reason when a
/// hardware request degraded to software. There is deliberately no error
/// variant: a missing/failing GPU degrades the encode, it never fails the
/// build (SR-034).
// Implements: LLR-048, SR-034
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub encoder: EncoderChoice,
    pub fallback_reason: Option<String>,
}

impl Selection {
    /// Log form of the encoder in use: `h264_nvenc`, `libx264`, or
    /// `libx264 (fallback: <reason>)` (LLR-048 reporting).
    pub fn describe(&self) -> String {
        match &self.fallback_reason {
            Some(reason) => format!("{} (fallback: {reason})", self.encoder.codec_name()),
            None => self.encoder.codec_name().to_string(),
        }
    }
}

/// Pure fallback decision (SR-034): resolve the encoder actually used from the
/// config request and the probe outcomes of the candidates consulted.
///
/// Contract:
/// - `request` — the parsed config `encoder` field.
/// - `outcomes` — (candidate, outcome) pairs in probe order: empty for
///   `Software` (the probe is never consulted), the single explicit candidate
///   for `Hardware`, the [`AUTO_CANDIDATES`] prefix actually probed for `Auto`
///   (the caller may stop at the first `Pass`).
/// - Returns the selection; `fallback_reason` is `Some` exactly when a
///   hardware request degraded to software, and names every encoder consulted
///   with why it was rejected. No input combination panics or errors.
// Implements: LLR-048, SR-034
pub fn select_encoder(
    request: EncoderRequest,
    outcomes: &[(EncoderChoice, ProbeOutcome)],
) -> Selection {
    fn fallback(why: String) -> Selection {
        Selection {
            encoder: EncoderChoice::Software,
            fallback_reason: Some(format!("{why}; falling back to libx264 software encode")),
        }
    }

    match request {
        EncoderRequest::Software => Selection {
            encoder: EncoderChoice::Software,
            fallback_reason: None,
        },
        EncoderRequest::Hardware(hw) => {
            let outcome = outcomes.iter().find(|(c, _)| *c == hw).map(|(_, o)| o);
            match outcome {
                Some(ProbeOutcome::Pass) => Selection {
                    encoder: hw,
                    fallback_reason: None,
                },
                Some(o) => fallback(format!("{}: {}", hw.codec_name(), o.summary())),
                // Defensive: an unprobed explicit candidate degrades like Absent.
                None => fallback(format!("{}: not probed", hw.codec_name())),
            }
        }
        EncoderRequest::Auto => {
            if let Some((c, _)) = outcomes.iter().find(|(_, o)| *o == ProbeOutcome::Pass) {
                return Selection {
                    encoder: *c,
                    fallback_reason: None,
                };
            }
            if outcomes.is_empty() {
                return fallback("no hardware encoder available to probe".into());
            }
            let detail: Vec<String> = outcomes
                .iter()
                .map(|(c, o)| format!("{}: {}", c.codec_name(), o.summary()))
                .collect();
            fallback(detail.join("; "))
        }
    }
}

/// Process-wide probe cache: preflight and every output of a multi-output run
/// (SR-017) share it, so each (ffmpeg, encoder) pair is probed once per run.
// Implements: LLR-046, SR-034
fn global_cache() -> &'static Mutex<ProbeCache> {
    static CACHE: OnceLock<Mutex<ProbeCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ProbeCache::new()))
}

/// Resolve the encoder actually used for a config `encoder` field value:
/// parse the request, probe the candidates it implies against the SR-027
/// resolution of `configured_ffmpeg` (memoized process-wide), and apply the
/// pure [`select_encoder`] decision. `software` (and, defensively, any value
/// validation would have rejected) short-circuits without resolving or probing
/// anything. The single selection entry point for both the preflight report
/// and the build pipeline — the decision lives once.
// Implements: LLR-046, LLR-048, SR-034
pub fn selection_for(configured_ffmpeg: Option<&Path>, encoder_field: &str) -> Selection {
    let request = EncoderRequest::parse(encoder_field);
    let candidates: Vec<EncoderChoice> = match request {
        EncoderRequest::Software => {
            return Selection {
                encoder: EncoderChoice::Software,
                fallback_reason: None,
            }
        }
        EncoderRequest::Auto => AUTO_CANDIDATES.to_vec(),
        EncoderRequest::Hardware(hw) => vec![hw],
    };

    // Probe against the binary the run will actually use (LLR-032); if none
    // resolves, bare `ffmpeg` keeps probing total — its failure classifies as
    // Fail(reason) and degrades to software, never an error (SR-034).
    let ffmpeg = crate::ffmpeg::resolve::resolve(
        configured_ffmpeg,
        &crate::setup::ffmpeg_fetch::cache_dir(),
    )
    .unwrap_or_else(|| PathBuf::from("ffmpeg"));

    let mut cache = global_cache().lock().expect("probe cache poisoned");
    let mut outcomes = Vec::new();
    for candidate in candidates {
        let outcome = cache.outcome(&ffmpeg, candidate.codec_name());
        let passed = outcome == ProbeOutcome::Pass;
        outcomes.push((candidate, outcome));
        if passed {
            break; // auto stops at the first working hardware encoder
        }
    }
    select_encoder(request, &outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic excerpt of `ffmpeg -encoders` output: legend, separator,
    /// video/audio encoder lines.
    const LISTING: &str = "Encoders:\n V..... = Video\n A..... = Audio\n S..... = Subtitle\n .F.... = Frame-level multithreading\n ..S... = Slice-level multithreading\n ...X.. = Codec is experimental\n ....B. = Supports draw_horiz_band\n .....D = Supports direct rendering method 1\n ------\n V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC (codec h264)\n V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)\n A....D aac                  AAC (Advanced Audio Coding)\n";

    // Verifies: SR-034, LLR-046 (TC-064) — the supported set is extracted from
    // listing text: present encoders found, absent ones not, legend/audio
    // lines never mistaken for video encoder names.
    #[test]
    fn parse_encoders_extracts_supported_set_sr034() {
        let set = parse_encoders(LISTING);
        assert!(set.contains("libx264"));
        assert!(set.contains("h264_nvenc"));
        assert!(!set.contains("h264_qsv"), "absent encoder must not appear");
        assert!(!set.contains("aac"), "audio encoders are not candidates");
        assert!(!set.contains("="), "legend lines must be ignored");
    }

    // Verifies: SR-034, LLR-046 (TC-064) — trial classification: absent skips
    // the trial entirely, success passes, failure carries the stderr reason.
    #[test]
    fn trial_encode_classifies_pass_fail_absent_sr034() {
        // Absent: the trial closure must never run.
        let outcome = classify_probe(false, || panic!("trial must not run for an absent encoder"));
        assert_eq!(outcome, ProbeOutcome::Absent);

        // Listed + clean exit -> Pass.
        assert_eq!(
            classify_probe(true, || (true, String::new())),
            ProbeOutcome::Pass
        );

        // Listed + failed exit -> Fail with the first non-empty stderr line.
        let outcome = classify_probe(true, || {
            (false, "\nCannot load nvcuda.dll\nmore detail\n".to_string())
        });
        assert_eq!(outcome, ProbeOutcome::Fail("Cannot load nvcuda.dll".into()));

        // Failed exit with empty stderr still yields a named reason.
        assert_eq!(
            classify_trial(false, ""),
            ProbeOutcome::Fail("trial encode failed".into())
        );
    }

    // Verifies: SR-034, LLR-046 (TC-064) — outcomes are memoized per
    // (ffmpeg, encoder): a multi-output run probes each encoder once.
    #[test]
    fn probe_result_cached_per_encoder_sr034() {
        let mut cache = ProbeCache::new();
        let calls = std::cell::Cell::new(0u32);
        let probe = |_: &Path, _: &str| {
            calls.set(calls.get() + 1);
            ProbeOutcome::Pass
        };

        let ff = Path::new("ffmpeg");
        assert_eq!(
            cache.outcome_with(ff, "h264_nvenc", probe),
            ProbeOutcome::Pass
        );
        assert_eq!(
            cache.outcome_with(ff, "h264_nvenc", probe),
            ProbeOutcome::Pass
        );
        assert_eq!(calls.get(), 1, "second lookup must hit the cache");

        // A different encoder, or a different ffmpeg, is a distinct key.
        cache.outcome_with(ff, "h264_qsv", probe);
        cache.outcome_with(Path::new("other/ffmpeg"), "h264_nvenc", probe);
        assert_eq!(calls.get(), 3);
    }

    // Verifies: SR-034, LLR-048 (TC-065) — the encoder x probe fallback
    // matrix: software never consults the probe; auto+Pass yields the probed
    // hardware; Fail/Absent yields software plus a reason naming the encoder;
    // no combination errors or aborts (the return type has no error variant).
    #[test]
    fn select_encoder_fallback_matrix_sr034() {
        use EncoderChoice::*;
        let fail = || ProbeOutcome::Fail("no device".into());

        // software: probe outcomes (even failing ones) are ignored entirely.
        for outcomes in [
            vec![],
            vec![(H264Nvenc, fail()), (H264Qsv, ProbeOutcome::Absent)],
        ] {
            let s = select_encoder(EncoderRequest::Software, &outcomes);
            assert_eq!(s.encoder, Software);
            assert!(s.fallback_reason.is_none(), "software is never a fallback");
        }

        // auto + a passing candidate -> that hardware encoder, no fallback.
        let s = select_encoder(
            EncoderRequest::Auto,
            &[
                (H264Nvenc, ProbeOutcome::Absent),
                (H264Qsv, ProbeOutcome::Pass),
            ],
        );
        assert_eq!(s.encoder, H264Qsv);
        assert!(s.fallback_reason.is_none());

        // auto + no pass -> software, reason names every consulted encoder.
        let s = select_encoder(
            EncoderRequest::Auto,
            &[
                (H264Nvenc, fail()),
                (H264Qsv, ProbeOutcome::Absent),
                (H264Amf, ProbeOutcome::Absent),
            ],
        );
        assert_eq!(s.encoder, Software);
        let reason = s.fallback_reason.expect("auto with no pass is a fallback");
        for name in ["h264_nvenc", "h264_qsv", "h264_amf", "libx264"] {
            assert!(reason.contains(name), "reason names {name}: {reason}");
        }

        // auto with nothing probed still degrades, never errors.
        let s = select_encoder(EncoderRequest::Auto, &[]);
        assert_eq!(s.encoder, Software);
        assert!(s.fallback_reason.is_some());

        // explicit hardware: pass -> selected; fail/absent -> software with a
        // reason naming the encoder.
        for hw in [H264Nvenc, H264Qsv, H264Amf] {
            let s = select_encoder(EncoderRequest::Hardware(hw), &[(hw, ProbeOutcome::Pass)]);
            assert_eq!(s.encoder, hw);
            assert!(s.fallback_reason.is_none());

            for outcome in [fail(), ProbeOutcome::Absent] {
                let s = select_encoder(EncoderRequest::Hardware(hw), &[(hw, outcome)]);
                assert_eq!(s.encoder, Software);
                let reason = s.fallback_reason.expect("hw fallback carries a reason");
                assert!(reason.contains(hw.codec_name()), "{reason}");
                assert!(reason.contains("libx264"), "{reason}");
            }
        }

        // describe(): plain codec name vs fallback-annotated libx264.
        let s = select_encoder(
            EncoderRequest::Hardware(H264Nvenc),
            &[(H264Nvenc, ProbeOutcome::Pass)],
        );
        assert_eq!(s.describe(), "h264_nvenc");
        let s = select_encoder(EncoderRequest::Hardware(H264Nvenc), &[(H264Nvenc, fail())]);
        assert!(
            s.describe().starts_with("libx264 (fallback:"),
            "{}",
            s.describe()
        );
    }
}
