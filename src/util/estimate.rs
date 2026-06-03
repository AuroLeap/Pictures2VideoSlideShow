//! Pre-run output size estimation and the FAT32 oversize threshold.
//!
//! These are coarse approximations used only to warn the user before/while a
//! long encode runs; the authoritative size is the final file on disk.
// Implements: LLR-013, SR-009

/// The ~3.5 GB soft ceiling below the 4 GB FAT32 per-file wall (UN-018).
pub const OVERSIZE_THRESHOLD_BYTES: u64 = 3_758_096_384; // 3.5 * 1024^3

/// Approximate H.264 video bitrate (bits/sec) implied by a CRF value.
///
/// CRF is not a bitrate, so this is intentionally rough: we anchor a
/// 1080p-ish encode and assume each CRF step changes size by ~12% (lower CRF =
/// larger). The anchor is ~8 Mbps at CRF 23; the result is clamped to a sane
/// band. This is documented as an estimate only — it deliberately does NOT
/// account for resolution or motion content.
fn assumed_bitrate_bps(crf: u32) -> f64 {
    const ANCHOR_CRF: f64 = 23.0;
    const ANCHOR_BPS: f64 = 8_000_000.0;
    // Each +1 CRF ~ x0.88 size; each -1 CRF ~ /0.88. `steps` is how many CRF
    // points ABOVE the anchor we are, so higher CRF -> larger `steps` -> smaller.
    let steps = crf as f64 - ANCHOR_CRF;
    let bps = ANCHOR_BPS * 0.88_f64.powf(steps);
    bps.clamp(500_000.0, 60_000_000.0)
}

/// Estimate the encoded byte size for `duration_secs` of video at `crf`.
// Implements: LLR-013, SR-009
pub fn estimate_output_bytes(duration_secs: f64, crf: u32) -> u64 {
    let bytes = assumed_bitrate_bps(crf) * duration_secs.max(0.0) / 8.0;
    bytes.round() as u64
}

/// Whether an (estimated or actual) byte size exceeds the oversize threshold.
// Implements: LLR-013, SR-009
pub fn is_oversize(bytes: u64) -> bool {
    bytes > OVERSIZE_THRESHOLD_BYTES
}

/// Human-readable byte size for log/summary lines.
pub fn human_bytes(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else {
        format!("{:.1} MB", b / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: LLR-013, SR-009
    #[test]
    fn estimate_scales_with_duration_sr009() {
        let one_min = estimate_output_bytes(60.0, 28);
        let two_min = estimate_output_bytes(120.0, 28);
        assert!(two_min > one_min);
        // Linear in duration.
        assert_eq!(two_min, one_min * 2);
    }

    // Verifies: LLR-013, SR-009
    #[test]
    fn lower_crf_estimates_larger_sr009() {
        let hi_q = estimate_output_bytes(60.0, 20); // lower CRF -> bigger
        let lo_q = estimate_output_bytes(60.0, 30); // higher CRF -> smaller
        assert!(hi_q > lo_q);
    }

    // Verifies: LLR-013, SR-009
    #[test]
    fn oversize_threshold_at_3_5gb_sr009() {
        assert!(!is_oversize(OVERSIZE_THRESHOLD_BYTES));
        assert!(is_oversize(OVERSIZE_THRESHOLD_BYTES + 1));
        assert!(!is_oversize(0));
    }

    // Verifies: LLR-013, SR-009
    #[test]
    fn long_high_quality_run_flags_oversize_sr009() {
        // ~3 hours at a low CRF should comfortably exceed 3.5 GB.
        let bytes = estimate_output_bytes(3.0 * 3600.0, 20);
        assert!(is_oversize(bytes));
    }
}
