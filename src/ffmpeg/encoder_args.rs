//! Pure mapping from an encoder choice to the FFmpeg output-side argument
//! block (SR-034/SR-035). Sibling of [`crate::ffmpeg::resolve::pick`]: no I/O,
//! exhaustively unit-tested. The SR-005 profile fields are invariant across
//! every choice; `software` reproduces the pre-SR-034 libx264 args byte for
//! byte when the preset is the default `medium`.
// Implements: LLR-045, LLR-049, SR-034, SR-035

/// A concrete H.264 encoder to drive. An `auto` request is resolved to one of
/// these by [`crate::ffmpeg::probe::select_encoder`] before args are built.
// Implements: LLR-045, SR-034
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EncoderChoice {
    /// libx264 — the default, SR-005-verified software path.
    Software,
    /// NVIDIA NVENC hardware encoder.
    H264Nvenc,
    /// Intel Quick Sync hardware encoder.
    H264Qsv,
    /// AMD AMF hardware encoder.
    H264Amf,
}

impl EncoderChoice {
    /// The ffmpeg `-c:v` codec name (also how run output names the encoder).
    pub fn codec_name(self) -> &'static str {
        match self {
            EncoderChoice::Software => "libx264",
            EncoderChoice::H264Nvenc => "h264_nvenc",
            EncoderChoice::H264Qsv => "h264_qsv",
            EncoderChoice::H264Amf => "h264_amf",
        }
    }
}

/// What the config `encoder` field asked for (SR-034): the software default,
/// automatic hardware probing, or one explicit hardware encoder.
// Implements: LLR-047, SR-034
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderRequest {
    Software,
    Auto,
    Hardware(EncoderChoice),
}

impl EncoderRequest {
    /// Parse the config `encoder` field. `Config::validate` rejects values
    /// outside the SR-034 set at the trust boundary, so an unknown string here
    /// (impossible via a validated config) degrades to `Software` — never a
    /// failure, per SR-034.
    // Implements: LLR-047, SR-034
    pub fn parse(s: &str) -> Self {
        match s {
            "auto" => EncoderRequest::Auto,
            "h264_nvenc" => EncoderRequest::Hardware(EncoderChoice::H264Nvenc),
            "h264_qsv" => EncoderRequest::Hardware(EncoderChoice::H264Qsv),
            "h264_amf" => EncoderRequest::Hardware(EncoderChoice::H264Amf),
            _ => EncoderRequest::Software,
        }
    }
}

/// FFmpeg output-side encoding argument block for `choice`.
///
/// Contract:
/// - Inputs: `choice` — the concrete encoder; `crf` — the config `quality_crf`
///   (0-51, SR-008); `x264_preset` — the libx264 speed preset (SR-035 set),
///   applied on the software path only (hardware encoders keep their own
///   preset ladders — out of SR-035 scope).
/// - Output: the argv slice from `-an` through `-f mp4`, consumed verbatim by
///   [`crate::ffmpeg::FfmpegEncoder`] between the rawvideo input args and the
///   output path.
/// - Quality mapping (SR-034): `Software` -> `-crf <crf>`; `H264Nvenc` ->
///   `-cq <crf>` plus `-b:v 0` (without a zero bitrate NVENC's default
///   bitrate target governs and `-cq` is ignored); `H264Qsv`/`H264Amf` ->
///   `-global_quality <crf>`.
/// - Profile invariants (SR-005): `-pix_fmt yuv420p`, `-movflags +faststart`,
///   `-f mp4`, `-an` appear for every choice.
// Implements: LLR-045, LLR-049, SR-034, SR-035
pub fn encoder_args(choice: EncoderChoice, crf: u32, x264_preset: &str) -> Vec<String> {
    let mut args = quality_args(choice, crf, x264_preset);
    args.extend([
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-f".into(),
        "mp4".into(),
    ]);
    args
}

/// Output-side args for an intermediate per-clip segment (SR-037): identical
/// codec/quality/`yuv420p` stream as [`encoder_args`] — so a concat of
/// segments still carries the SR-005 stream profile — but muxed as MPEG-TS
/// (self-contained timestamps, concat-friendly; the LLR-054 segment
/// container). `+faststart` is an MP4 muxer flag and is applied at final
/// concat assembly instead ([`crate::ffmpeg::concat::concat_segments`]).
// Implements: LLR-056, SR-037, SR-005
// lib-API: SR-037 segmented path (tests/concat_seam.rs; bin wiring 5b).
#[allow(dead_code)]
pub fn segment_encoder_args(choice: EncoderChoice, crf: u32, x264_preset: &str) -> Vec<String> {
    let mut args = quality_args(choice, crf, x264_preset);
    args.extend([
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-f".into(),
        "mpegts".into(),
    ]);
    args
}

/// The container-independent `-an -c:v <codec>` + per-encoder quality block
/// shared by [`encoder_args`] (final MP4) and [`segment_encoder_args`]
/// (MPEG-TS segments) so the SR-034 quality mapping lives once.
// Implements: LLR-045, SR-034
fn quality_args(choice: EncoderChoice, crf: u32, x264_preset: &str) -> Vec<String> {
    let mut args: Vec<String> = vec!["-an".into(), "-c:v".into(), choice.codec_name().into()];
    match choice {
        EncoderChoice::Software => {
            args.extend(["-preset".into(), x264_preset.into()]);
            args.extend(["-crf".into(), crf.to_string()]);
        }
        EncoderChoice::H264Nvenc => {
            args.extend(["-cq".into(), crf.to_string(), "-b:v".into(), "0".into()]);
        }
        EncoderChoice::H264Qsv | EncoderChoice::H264Amf => {
            args.extend(["-global_quality".into(), crf.to_string()]);
        }
    }
    args
}

/// The resolved encoding parameters [`crate::ffmpeg::FfmpegEncoder::start`]
/// consumes: the probe-resolved encoder choice plus the per-output quality
/// knobs. Groups what was previously a bare `crf` argument so the encoder's
/// watchdog/part/promote logic stays untouched by SR-034/SR-035.
// Implements: LLR-045, LLR-049, SR-034, SR-035
#[derive(Debug, Clone)]
pub struct EncoderSettings {
    pub choice: EncoderChoice,
    /// Config `quality_crf` (0-51, SR-008), mapped per encoder by
    /// [`encoder_args`].
    pub crf: u32,
    /// Config `x264_preset` (SR-035); applied on the software path only.
    pub x264_preset: String,
}

impl EncoderSettings {
    /// The pre-SR-034 default: libx264 at `crf` with the `medium` preset.
    #[allow(dead_code)] // lib-API convenience; used by the integration tests
    pub fn software(crf: u32) -> Self {
        Self {
            choice: EncoderChoice::Software,
            crf,
            x264_preset: "medium".into(),
        }
    }

    /// The [`encoder_args`] block for these settings.
    pub fn args(&self) -> Vec<String> {
        encoder_args(self.choice, self.crf, &self.x264_preset)
    }

    /// The [`segment_encoder_args`] block for these settings (SR-037 segments).
    #[allow(dead_code)] // lib-API: SR-037 segmented path (bin wiring 5b)
    pub fn segment_args(&self) -> Vec<String> {
        segment_encoder_args(self.choice, self.crf, &self.x264_preset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [EncoderChoice; 4] = [
        EncoderChoice::Software,
        EncoderChoice::H264Nvenc,
        EncoderChoice::H264Qsv,
        EncoderChoice::H264Amf,
    ];

    /// True when `args` contains the adjacent pair `flag value`.
    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    // Verifies: SR-034, LLR-045 (TC-063) — `software` yields the pre-SR-034
    // libx264 arg block byte-identically (default preset `medium`).
    #[test]
    fn software_args_byte_identical_sr034() {
        let expected: Vec<String> = [
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "28",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            encoder_args(EncoderChoice::Software, 28, "medium"),
            expected
        );
    }

    // Verifies: SR-034, LLR-045 (TC-063) — quality_crf maps to each hardware
    // encoder's quality control: -cq (nvenc), -global_quality (qsv/amf).
    #[test]
    fn hardware_quality_control_mapping_sr034() {
        let nv = encoder_args(EncoderChoice::H264Nvenc, 23, "medium");
        assert!(has_pair(&nv, "-cq", "23"), "nvenc uses -cq: {nv:?}");
        assert!(!nv.contains(&"-crf".to_string()), "nvenc must not use -crf");

        for hw in [EncoderChoice::H264Qsv, EncoderChoice::H264Amf] {
            let a = encoder_args(hw, 23, "medium");
            assert!(
                has_pair(&a, "-global_quality", "23"),
                "{} uses -global_quality: {a:?}",
                hw.codec_name()
            );
            assert!(!a.contains(&"-crf".to_string()));
        }
    }

    // Verifies: SR-034, SR-005, LLR-045 (TC-063) — the SR-005 profile fields
    // are invariant across every encoder choice.
    #[test]
    fn profile_fields_invariant_across_encoders_sr034() {
        for choice in ALL {
            let a = encoder_args(choice, 28, "medium");
            assert!(has_pair(&a, "-pix_fmt", "yuv420p"), "{choice:?}: {a:?}");
            assert!(has_pair(&a, "-movflags", "+faststart"), "{choice:?}: {a:?}");
            assert!(has_pair(&a, "-f", "mp4"), "{choice:?}: {a:?}");
            assert!(a.contains(&"-an".to_string()), "{choice:?}: {a:?}");
            assert!(
                has_pair(&a, "-c:v", choice.codec_name()),
                "{choice:?}: {a:?}"
            );
        }
    }

    // Verifies: SR-037, SR-005, LLR-056 (TC-079) — segment args share the
    // exact quality block with the final-output args (same stream for every
    // encoder choice) but pin the MPEG-TS container with no MP4-only flags.
    #[test]
    fn segment_args_share_quality_block_and_pin_mpegts_sr037() {
        for choice in ALL {
            let seg = segment_encoder_args(choice, 28, "medium");
            let full = encoder_args(choice, 28, "medium");
            // Identical prefix up to the container tail: one quality fact.
            let q = quality_args(choice, 28, "medium");
            assert!(seg.starts_with(&q), "{choice:?}: {seg:?}");
            assert!(full.starts_with(&q), "{choice:?}: {full:?}");
            // MPEG-TS container, yuv420p kept, no MP4-only faststart flag.
            assert!(has_pair(&seg, "-f", "mpegts"), "{choice:?}: {seg:?}");
            assert!(has_pair(&seg, "-pix_fmt", "yuv420p"), "{choice:?}: {seg:?}");
            assert!(
                !seg.contains(&"-movflags".to_string()),
                "faststart is applied at concat assembly, not per segment: {seg:?}"
            );
        }
    }

    // Verifies: SR-035, LLR-049 (TC-070) — a configured preset passes through
    // as `-preset <value>` on the software path only; hardware arg lists carry
    // no x264 preset.
    #[test]
    fn preset_passthrough_software_only_sr035() {
        for preset in ["veryfast", "faster", "medium", "slow"] {
            let a = encoder_args(EncoderChoice::Software, 28, preset);
            assert!(
                has_pair(&a, "-preset", preset),
                "software carries {preset}: {a:?}"
            );
        }
        for hw in [
            EncoderChoice::H264Nvenc,
            EncoderChoice::H264Qsv,
            EncoderChoice::H264Amf,
        ] {
            let a = encoder_args(hw, 28, "veryfast");
            assert!(
                !a.contains(&"-preset".to_string()),
                "{} must not carry an x264 preset: {a:?}",
                hw.codec_name()
            );
        }
    }
}
