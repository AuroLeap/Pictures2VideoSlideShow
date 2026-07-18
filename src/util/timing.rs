//! Stage timers and perf-metrics emission (SR-036): cheap per-stage elapsed
//! accumulation for the build hot path (decode, prescale, prefetch-stall,
//! render, blend, encode-write-stall, ffmpeg-wall) surfaced as `--verbose`
//! summary lines,
//! plus the `PB-ID -> number` JSON consumed by `Scripts/check_perf.py` and the
//! peak-working-set probe for PB-005.

use crate::error::Result;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Per-output elapsed-time accumulators for the six SR-036 build stages.
///
/// Cheap by design: each instrumentation point is one `Instant::now()` pair
/// and one relaxed atomic add of the elapsed nanoseconds — no syscalls or
/// allocation in the frame loop, and nothing is formatted unless the
/// `--verbose` (debug-level) summary is actually emitted, so a non-verbose
/// build's output and cost are unchanged.
// Implements: LLR-050, SR-036
#[derive(Debug, Default)]
pub struct StageTimings {
    decode_ns: AtomicU64,
    prescale_ns: AtomicU64,
    /// Blocking wait on the prefetched clip at each boundary (LLR-060): the
    /// PB-002 numerator. Decode/prescale keep accruing their raw (overlapped)
    /// time above; only this wait is dead time the build actually sees.
    stall_ns: AtomicU64,
    render_ns: AtomicU64,
    blend_ns: AtomicU64,
    write_ns: AtomicU64,
    ffmpeg_wall_ns: AtomicU64,
    /// Image clips loaded — the "per photo" denominator of the PB-002
    /// boundary-stall mean (videos stream and are not boundary stalls).
    clips: AtomicU64,
}

impl StageTimings {
    pub fn new() -> Self {
        Self::default()
    }

    fn add(cell: &AtomicU64, d: Duration) {
        cell.fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Add one clip's image-decode time (`image::open`).
    pub fn add_decode(&self, d: Duration) {
        Self::add(&self.decode_ns, d);
    }

    /// Add one clip's prescale (resize-to-plan) time.
    pub fn add_prescale(&self, d: Duration) {
        Self::add(&self.prescale_ns, d);
    }

    /// Add one clip boundary's blocking wait on the prefetched renderer
    /// (LLR-060) — the PB-002 stall.
    pub fn add_stall(&self, d: Duration) {
        Self::add(&self.stall_ns, d);
    }

    /// Add one render batch's Ken Burns warp time.
    pub fn add_render(&self, d: Duration) {
        Self::add(&self.render_ns, d);
    }

    /// Add one transition frame's blend/scale time.
    pub fn add_blend(&self, d: Duration) {
        Self::add(&self.blend_ns, d);
    }

    /// Add one frame's encoder-stdin write (stall) time.
    pub fn add_write(&self, d: Duration) {
        Self::add(&self.write_ns, d);
    }

    /// Count one loaded image clip (the PB-002 mean denominator).
    pub fn add_clip(&self) {
        self.clips.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the encoder process wall time (spawn -> exit) for this output.
    pub fn set_ffmpeg_wall(&self, d: Duration) {
        self.ffmpeg_wall_ns
            .store(d.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Plain-value copy of the totals for reporting and the bench runner.
    pub fn snapshot(&self) -> StageSnapshot {
        let ms = |cell: &AtomicU64| cell.load(Ordering::Relaxed) as f64 / 1e6;
        StageSnapshot {
            decode_ms: ms(&self.decode_ns),
            prescale_ms: ms(&self.prescale_ns),
            stall_ms: ms(&self.stall_ns),
            render_ms: ms(&self.render_ns),
            blend_ms: ms(&self.blend_ns),
            encode_write_stall_ms: ms(&self.write_ns),
            ffmpeg_wall_ms: ms(&self.ffmpeg_wall_ns),
            clips: self.clips.load(Ordering::Relaxed),
        }
    }

    /// Emit the per-output totals as one debug line per SR-036 stage — visible
    /// with `--verbose`, filtered out (and nearly free) otherwise.
    // Implements: LLR-050, SR-036
    pub fn log_summary(&self, output: &str) {
        let s = self.snapshot();
        log::debug!("'{}' stage decode: {:.1} ms", output, s.decode_ms);
        log::debug!("'{}' stage prescale: {:.1} ms", output, s.prescale_ms);
        log::debug!("'{}' stage prefetch-stall: {:.1} ms", output, s.stall_ms);
        log::debug!("'{}' stage render: {:.1} ms", output, s.render_ms);
        log::debug!("'{}' stage blend: {:.1} ms", output, s.blend_ms);
        log::debug!(
            "'{}' stage encode-write-stall: {:.1} ms",
            output,
            s.encode_write_stall_ms
        );
        log::debug!("'{}' stage ffmpeg-wall: {:.1} ms", output, s.ffmpeg_wall_ms);
    }
}

/// Plain (non-atomic) per-output stage totals in milliseconds, readable by the
/// bench runner (LLR-052) and the `--verbose` summary.
// Implements: LLR-050, SR-036
#[derive(Debug, Clone, Copy, Default)]
pub struct StageSnapshot {
    pub decode_ms: f64,
    pub prescale_ms: f64,
    /// Blocking wait on prefetched clips (the PB-002 numerator, LLR-060).
    pub stall_ms: f64,
    pub render_ms: f64,
    pub blend_ms: f64,
    pub encode_write_stall_ms: f64,
    pub ffmpeg_wall_ms: f64,
    /// Image clips loaded for this output.
    pub clips: u64,
}

impl StageSnapshot {
    /// Mean dead time per image clip boundary in ms (PB-002): the blocking
    /// wait on the prefetched renderer (LLR-060) — decode/prescale that
    /// overlapped the build does not count, only the wait the loop saw.
    /// Zero clips yields a defined 0, never a division by zero.
    pub fn boundary_stall_ms_per_photo(&self) -> f64 {
        if self.clips == 0 {
            0.0
        } else {
            self.stall_ms / self.clips as f64
        }
    }
}

/// Write the `{PB-ID -> number}` metrics JSON consumed by
/// `Scripts/check_perf.py` against `performance-budgets.csv`. Keys must be
/// PB-IDs from that registry; a PB row without an entry is skipped by the
/// comparator (how PB-004 stays honestly unmeasured until SR-037). Called by
/// the bench path only (LLR-052) — a normal build never writes metrics.
// Implements: LLR-051, SR-036
pub fn write_perf_metrics(path: &Path, pairs: &[(&str, f64)]) -> Result<()> {
    let map: BTreeMap<&str, f64> = pairs.iter().copied().collect();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_string_pretty(&map)?;
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

/// Peak working-set memory of this process in MB (PB-005): Windows process
/// memory counters on Windows, `VmHWM` on Linux, `None` elsewhere (documented
/// approximation gap — the PB-005 entry is then omitted and check_perf skips
/// the row rather than faking a number).
// Implements: LLR-052, SR-036
pub fn peak_working_set_mb() -> Option<f64> {
    peak_working_set_bytes().map(|b| b as f64 / (1024.0 * 1024.0))
}

/// Windows: `K32GetProcessMemoryInfo(GetCurrentProcess())`
/// `.PeakWorkingSetSize` — the exact counter Task Manager reports as peak
/// working set.
#[cfg(windows)]
fn peak_working_set_bytes() -> Option<u64> {
    // Field names mirror the Win32 PROCESS_MEMORY_COUNTERS layout; only the
    // layout (repr(C), field order/sizes) matters for the FFI call.
    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[allow(non_snake_case)]
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }
    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        ..Default::default()
    };
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing;
    // the counters struct matches the documented PROCESS_MEMORY_COUNTERS layout
    // and cb is its exact size.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (ok != 0).then_some(counters.peak_working_set_size as u64)
}

/// Linux: `VmHWM` ("high water mark" of the resident set) from
/// `/proc/self/status`, in kB.
#[cfg(target_os = "linux")]
fn peak_working_set_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: u64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// Other platforms: unmeasured (PB-005 omitted from the metrics).
#[cfg(not(any(windows, target_os = "linux")))]
fn peak_working_set_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique temp file path for a metrics write (lib and bin test binaries
    /// both compile this module, so pid alone is not unique enough).
    fn temp_metrics_path() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "slideshow_perf_metrics_{}_{}",
            std::process::id(),
            nanos
        ))
    }

    // Verifies: SR-036, LLR-051 (TC-073) — the metrics file is a flat JSON
    // object of {PB-ID: number} with one entry per measured budget row, and
    // every key exists in performance-budgets.csv.
    #[test]
    fn perf_metrics_json_is_pbid_to_number_sr036() {
        let dir = temp_metrics_path();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("perf-metrics.json");
        let pairs = [
            ("PB-001", 60.5),
            ("PB-002", 100.0),
            ("PB-003", 2.0),
            ("PB-004", 10.0),
            ("PB-005", 2048.0),
        ];
        write_perf_metrics(&path, &pairs).unwrap();

        let text = std::fs::read_to_string(&path).expect("metrics file must be written");
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let obj = json.as_object().expect("flat JSON object");
        assert_eq!(obj.len(), pairs.len(), "one entry per measured PB row");
        for (k, v) in &pairs {
            let got = obj
                .get(*k)
                .unwrap_or_else(|| panic!("missing key {k}"))
                .as_f64()
                .unwrap_or_else(|| panic!("{k} must be a number"));
            assert!((got - v).abs() < 1e-9, "{k}: wrote {v}, read {got}");
        }

        // Every key must be a PB row in the budgets registry.
        let csv = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/requirements/performance-budgets.csv"
        ))
        .expect("budgets registry");
        let ids: Vec<&str> = csv
            .lines()
            .skip(1)
            .filter_map(|l| l.split(',').next())
            .collect();
        for (k, _) in &pairs {
            assert!(
                ids.contains(k),
                "{k} is not a PB-ID in performance-budgets.csv"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Verifies: SR-036, LLR-050, LLR-060 — nanosecond accumulation across
    // stages, and the PB-002 mean is the BLOCKING wait on each prefetched clip
    // (the number prefetch collapses), not the raw decode+prescale time (which
    // now overlaps the build and keeps accumulating separately).
    #[test]
    fn boundary_stall_is_mean_blocking_wait_per_clip_sr036() {
        let t = StageTimings::new();
        // Two clips: 30+50 ms decode, 10+10 ms prescale accrue raw (overlapped
        // work), while the loop only BLOCKED 6+4 ms waiting => mean stall 5.
        t.add_decode(Duration::from_millis(30));
        t.add_decode(Duration::from_millis(50));
        t.add_prescale(Duration::from_millis(10));
        t.add_prescale(Duration::from_millis(10));
        t.add_stall(Duration::from_millis(6));
        t.add_stall(Duration::from_millis(4));
        t.add_clip();
        t.add_clip();
        t.add_render(Duration::from_millis(200));
        t.add_blend(Duration::from_millis(5));
        t.add_write(Duration::from_millis(7));
        t.set_ffmpeg_wall(Duration::from_millis(400));

        let s = t.snapshot();
        assert!((s.decode_ms - 80.0).abs() < 1e-6);
        assert!((s.prescale_ms - 20.0).abs() < 1e-6);
        assert!((s.stall_ms - 10.0).abs() < 1e-6);
        assert!((s.render_ms - 200.0).abs() < 1e-6);
        assert!((s.blend_ms - 5.0).abs() < 1e-6);
        assert!((s.encode_write_stall_ms - 7.0).abs() < 1e-6);
        assert!((s.ffmpeg_wall_ms - 400.0).abs() < 1e-6);
        assert_eq!(s.clips, 2);
        assert!((s.boundary_stall_ms_per_photo() - 5.0).abs() < 1e-6);
        // No clips loaded => a defined 0, never a division by zero.
        assert_eq!(
            StageTimings::new().snapshot().boundary_stall_ms_per_photo(),
            0.0
        );
    }

    // Verifies: SR-036, LLR-052 — the PB-005 probe reports a plausible peak
    // working set on the supported platforms (Windows API / Linux VmHWM).
    #[test]
    fn peak_working_set_reports_positive_mb_sr036() {
        if cfg!(any(windows, target_os = "linux")) {
            let mb = peak_working_set_mb().expect("supported platform must measure");
            assert!(mb > 1.0, "a running test process uses more than 1 MB: {mb}");
        } else {
            assert!(
                peak_working_set_mb().is_none(),
                "documented approximation gap"
            );
        }
    }
}
