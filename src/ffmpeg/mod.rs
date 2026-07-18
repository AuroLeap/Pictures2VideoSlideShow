//! FFmpeg coordination: spawn an encoder process and stream raw `rgb24` frames
//! to it over stdin, producing an H.264 MP4.
//!
//! Each output is encoded to a distinguishable `<name>.mp4.part` temp file and
//! atomically renamed to the final `<name>.mp4` only after ffmpeg exits
//! successfully (LLR-015, SR-011). Any failure or early drop removes the temp
//! so a killed run never leaves a complete-looking final file.

pub mod audio;
pub mod concat;
pub mod encoder_args;
pub mod probe;
pub mod resolve;

use crate::error::{Result, SlideshowError};
use crate::util::{disk_full_error, is_disk_full};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Decide whether the encoder has been inactive past its timeout (SR-013).
/// `timeout_secs == 0` disables the watchdog and always returns false; otherwise
/// returns true once `now - last_activity` exceeds the timeout. Saturating
/// subtraction keeps a clock that jumps backwards from spuriously firing.
// Implements: LLR-008, SR-013
fn timed_out(last_activity_ms: u64, now_ms: u64, timeout_secs: u64) -> bool {
    if timeout_secs == 0 {
        return false;
    }
    now_ms.saturating_sub(last_activity_ms) > timeout_secs.saturating_mul(1000)
}

/// Current wall-clock time in epoch milliseconds (0 if the clock is before the
/// epoch, which only the watchdog timing relies on — never correctness).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A running FFmpeg encoder fed raw `rgb24` frames via stdin.
pub struct FfmpegEncoder {
    /// The ffmpeg child, shared with the watchdog so it can kill on a stall.
    child: Arc<Mutex<Child>>,
    /// ffmpeg stdin, taken out of the child so frame writes never contend with
    /// the watchdog for the child lock. Dropped in `finish`/`Drop` to signal EOF.
    stdin: Option<ChildStdin>,
    /// The `<name>.mp4.part` file ffmpeg writes to.
    temp_path: PathBuf,
    /// The final `<name>.mp4` to promote to on success.
    final_path: PathBuf,
    /// Human-facing name of the output, used in timeout/error messages.
    output_name: String,
    /// Set once finalize/abort has run so Drop does not double-clean.
    finished: bool,
    /// Inactivity watchdog handle (absent when the timeout is disabled).
    watchdog: Option<Watchdog>,
}

/// Background inactivity watchdog (SR-013): wakes ~1s, kills ffmpeg if no frame
/// has been written within the timeout, and flips `killed` so `finish` can turn
/// the resulting ffmpeg failure into a clear timeout error rather than a false
/// success.
// Implements: LLR-008, SR-013
struct Watchdog {
    handle: std::thread::JoinHandle<()>,
    /// Epoch-ms of the last successful frame write; bumped by `write_frame`.
    last_activity: Arc<AtomicU64>,
    /// Set by `finish`/`Drop` to tell the watchdog to stop and exit.
    finished: Arc<AtomicBool>,
    /// Set by the watchdog when it kills ffmpeg for inactivity.
    killed: Arc<AtomicBool>,
}

impl FfmpegEncoder {
    /// Spawn FFmpeg to read `width`x`height` `rgb24` frames at `fps` from stdin
    /// and encode them to a temp file alongside `output`. Call [`finish`] to
    /// atomically promote it to `output`.
    ///
    /// `enc` selects the video encoder and quality (SR-034/SR-035); the
    /// output-side arg block comes verbatim from [`encoder_args::encoder_args`]
    /// so the SR-005 profile holds for every choice.
    /// `timeout_secs` arms the inactivity watchdog (SR-013); `0` disables it.
    // Implements: LLR-015, LLR-008, LLR-045, SR-011, SR-013, SR-034
    pub fn start(
        output: &Path,
        width: u32,
        height: u32,
        fps: u32,
        enc: &encoder_args::EncoderSettings,
        timeout_secs: u64,
    ) -> Result<Self> {
        Self::spawn_encoder(output, width, height, fps, enc.args(), timeout_secs)
    }

    /// Like [`start`], but encodes to an intermediate MPEG-TS **segment**
    /// (SR-037): same rawvideo stdin, watchdog, `.part` temp, and atomic
    /// [`finish`] promotion, with the output-side args from
    /// [`encoder_args::segment_encoder_args`] (self-contained timestamps for
    /// stream-copy concat; `+faststart` is applied at final assembly by
    /// [`concat::concat_segments`]).
    // Implements: LLR-056, SR-037, SR-011, SR-013
    pub fn start_segment(
        output: &Path,
        width: u32,
        height: u32,
        fps: u32,
        enc: &encoder_args::EncoderSettings,
        timeout_secs: u64,
    ) -> Result<Self> {
        Self::spawn_encoder(output, width, height, fps, enc.segment_args(), timeout_secs)
    }

    /// Shared spawn path for [`start`]/[`start_segment`]: rawvideo-in from
    /// stdin, `out_args` verbatim as the output-side block, watchdog armed.
    fn spawn_encoder(
        output: &Path,
        width: u32,
        height: u32,
        fps: u32,
        out_args: Vec<String>,
        timeout_secs: u64,
    ) -> Result<Self> {
        let final_path = output.to_path_buf();
        let temp_path = part_path(output);
        let output_name = output
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| final_path.display().to_string());

        // Remove any stale temp from a prior killed run so we start clean.
        let _ = std::fs::remove_file(&temp_path);

        let child = Command::new("ffmpeg")
            .arg("-y")
            .arg("-loglevel")
            .arg("error")
            // Raw input description.
            .arg("-f")
            .arg("rawvideo")
            .arg("-pixel_format")
            .arg("rgb24")
            .arg("-video_size")
            .arg(format!("{}x{}", width, height))
            .arg("-framerate")
            .arg(fps.to_string())
            .arg("-i")
            .arg("pipe:0")
            // Output encoding: the pure per-encoder arg block (`-an` through
            // `-f <container>`; the temp `.part` extension can't infer a muxer,
            // so the container is pinned there). Implements: LLR-045, SR-034
            .args(out_args)
            .arg(&temp_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| {
                SlideshowError::Ffmpeg(format!("Failed to spawn ffmpeg (is it on PATH?): {}", e))
            })?;

        let mut child = child;
        let stdin = child.stdin.take();
        let child = Arc::new(Mutex::new(child));
        let watchdog = Watchdog::spawn(Arc::clone(&child), timeout_secs);

        Ok(Self {
            child,
            stdin,
            temp_path,
            final_path,
            output_name,
            finished: false,
            watchdog,
        })
    }

    /// Write one raw `rgb24` frame to the encoder. Each successful write records
    /// activity so the inactivity watchdog does not fire on a healthy run.
    // Implements: LLR-008, SR-013
    pub fn write_frame(&mut self, data: &[u8]) -> Result<()> {
        let temp_path = self.temp_path.clone();
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| SlideshowError::Ffmpeg("ffmpeg stdin not available".into()))?;
        stdin.write_all(data).map_err(|e| {
            // A full output volume can surface as a write/broken-pipe failure.
            if is_disk_full(&e) {
                disk_full_error(&temp_path)
            } else {
                SlideshowError::Ffmpeg(format!("Failed writing frame to ffmpeg: {}", e))
            }
        })?;
        if let Some(w) = &self.watchdog {
            w.last_activity.store(now_ms(), Ordering::Relaxed);
        }
        Ok(())
    }

    /// Close stdin, wait for FFmpeg, and on success atomically rename the temp
    /// file to the final output path. On any failure the temp is removed so no
    /// complete-looking final file is left. If the inactivity watchdog killed a
    /// stalled ffmpeg, this returns a timeout error naming the output so the
    /// killed run never reports success.
    // Implements: LLR-015, LLR-008, SR-011, SR-013, SR-015
    // Direct silent finalize: used for SR-037 segments (each `.ts` appears
    // atomically) and by the lib/integration tests; final outputs go through
    // finish_to_part + promote/mux so audio can be muxed first.
    pub fn finish(self) -> Result<()> {
        let final_path = self.final_path.clone();
        let part = self.finish_to_part()?;
        // Atomic promote temp -> final on the same volume.
        promote(&part, &final_path)
    }

    /// Like [`finish`], but stops at the validated `.part` file and returns its
    /// path **without** renaming to the final name. Used when a second pass
    /// (audio mux) must consume the silent video before the final output appears
    /// atomically. The `.part` is left in place on success (the caller promotes
    /// or consumes it) and removed on any ffmpeg failure/timeout.
    // Implements: LLR-041, LLR-015, LLR-008, SR-011, SR-013
    pub fn finish_to_part(mut self) -> Result<PathBuf> {
        self.finished = true;
        // Drop stdin to signal EOF.
        drop(self.stdin.take());
        let status = self
            .child
            .lock()
            .unwrap()
            .wait()
            .map_err(|e| SlideshowError::Ffmpeg(format!("Failed waiting for ffmpeg: {}", e)))?;

        // Stop and join the watchdog before classifying the result; `was_killed`
        // tells us whether a non-success status is actually a timeout abort.
        let timed_out = self.stop_watchdog();

        if timed_out {
            let _ = std::fs::remove_file(&self.temp_path);
            return Err(SlideshowError::Ffmpeg(format!(
                "encoding timed out for '{}': no ffmpeg progress within the inactivity timeout; aborted",
                self.output_name
            )));
        }
        if !status.success() {
            let _ = std::fs::remove_file(&self.temp_path);
            return Err(SlideshowError::Ffmpeg(format!(
                "encoding failed for '{}': ffmpeg exited with status {}",
                self.final_path.display(),
                status
            )));
        }

        Ok(self.temp_path.clone())
    }

    /// Signal the watchdog to stop, join it, and report whether it killed ffmpeg
    /// for inactivity. Safe to call more than once (Drop also calls it).
    // Implements: LLR-008, SR-013
    fn stop_watchdog(&mut self) -> bool {
        match self.watchdog.take() {
            Some(w) => {
                w.finished.store(true, Ordering::Relaxed);
                let killed = w.killed.clone();
                // Join always returns: the watchdog loop checks `finished` each
                // wake, so it cannot block forever.
                let _ = w.handle.join();
                killed.load(Ordering::Relaxed)
            }
            None => false,
        }
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        // Stop the watchdog first so it can't race the explicit kill below.
        self.stop_watchdog();
        // If we never finished (early return / panic / kill), kill ffmpeg and
        // remove the temp so no partial output bearing a real name survives.
        if !self.finished {
            drop(self.stdin.take());
            if let Ok(mut child) = self.child.lock() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

impl Watchdog {
    /// Arm an inactivity watchdog over `child`. Returns `None` when disabled
    /// (`timeout_secs == 0`) so a healthy run carries no background thread.
    // Implements: LLR-008, SR-013
    fn spawn(child: Arc<Mutex<Child>>, timeout_secs: u64) -> Option<Self> {
        if timeout_secs == 0 {
            return None;
        }
        let last_activity = Arc::new(AtomicU64::new(now_ms()));
        let finished = Arc::new(AtomicBool::new(false));
        let killed = Arc::new(AtomicBool::new(false));

        let t_last = Arc::clone(&last_activity);
        let t_finished = Arc::clone(&finished);
        let t_killed = Arc::clone(&killed);
        let handle = std::thread::spawn(move || {
            // Wake roughly once a second so a stall is detected within ~1s of the
            // timeout while normal runs pay almost nothing.
            while !t_finished.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                if t_finished.load(Ordering::Relaxed) {
                    break;
                }
                if timed_out(t_last.load(Ordering::Relaxed), now_ms(), timeout_secs) {
                    t_killed.store(true, Ordering::Relaxed);
                    if let Ok(mut c) = child.lock() {
                        let _ = c.kill();
                    }
                    break;
                }
            }
        });

        Some(Self {
            handle,
            last_activity,
            finished,
            killed,
        })
    }
}

/// Atomically promote an in-progress file (`.part`) to its final path on the
/// same volume. On failure the source is removed and a disk-full error is mapped
/// so a wedged finalize never leaves a complete-looking final file.
// Implements: LLR-015, LLR-041, SR-011, SR-015
pub fn promote(src: &Path, final_path: &Path) -> Result<()> {
    std::fs::rename(src, final_path).map_err(|e| {
        let _ = std::fs::remove_file(src);
        if is_disk_full(&e) {
            disk_full_error(final_path)
        } else {
            SlideshowError::Ffmpeg(format!(
                "Failed finalizing '{}': {}",
                final_path.display(),
                e
            ))
        }
    })
}

/// The distinguishable in-progress temp path for `output`: `<output>.part`.
// Implements: LLR-015, SR-011
fn part_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().map(|n| n.to_owned()).unwrap_or_default();
    name.push(".part");
    output.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::timed_out;

    // Verifies: LLR-008, SR-013
    #[test]
    fn inactivity_timeout_decision_sr013() {
        // Disabled (0) never fires, even with a huge gap.
        assert!(!timed_out(0, 10_000_000, 0));

        // Within the window: no timeout. 100s elapsed, 120s limit.
        assert!(!timed_out(1_000_000, 1_100_000, 120));

        // Exactly at the boundary is not yet "exceeded".
        assert!(!timed_out(0, 120_000, 120));

        // Past the boundary fires.
        assert!(timed_out(0, 120_001, 120));
        assert!(timed_out(1_000_000, 1_130_000, 120));

        // A backwards clock jump (now < last) must not fire (saturating sub).
        assert!(!timed_out(2_000_000, 1_000_000, 120));
    }
}
