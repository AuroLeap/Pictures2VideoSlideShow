//! Background clip prefetch (plan §3 1b): decode+prescale the *next* image
//! clip on a worker thread while the current clip streams, so the clip loop's
//! only boundary cost is a (usually near-zero) blocking wait — the PB-002
//! stall the timers measure.

use crate::config::OutputDef;
use crate::error::Result;
use crate::image::LoadedClip;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver};
use std::thread::JoinHandle;

/// Channel capacity: the worker may hold one finished renderer in the channel
/// while building the next, so at most two decoded clips exist ahead of the
/// consumer — the bounded lookahead of 1–2 LLR-060 specifies, which caps the
/// prefetcher's memory at two prescaled images.
// Implements: LLR-060, SR-036
const LOOKAHEAD: usize = 1;

/// One image clip to load ahead of time: its position in the media list (for
/// the in-order join with the clip loop) and the inputs
/// [`LoadedClip::load`] needs. The Ken Burns focus is resolved
/// *before* spawning — on the main thread, against the ROI database — so
/// focus resolution stays deterministic and single-homed (SR-031).
// Implements: LLR-060, SR-036
pub struct PrefetchJob {
    /// Index into the per-output media slice this job was built from.
    pub index: usize,
    pub path: PathBuf,
    /// Pre-resolved focus (ROI entry or configured default), `None` = default pan.
    pub focus: Option<(f32, f32)>,
}

/// Handle to the background loader. Results arrive strictly in job order (one
/// worker feeding a FIFO channel), so a failed load surfaces at exactly the
/// media item where the serial code would have failed and routes through the
/// same `record_skip` — SR-014 skip semantics are identical to the serial path.
/// Carries backend-blind [`LoadedClip`]s (LLR-066): the consuming clip loop
/// wraps each in the selected backend's renderer, so GPU objects never live
/// on this worker thread.
// Implements: LLR-060, LLR-066, SR-014, SR-036, SR-039
pub struct ClipPrefetcher {
    rx: Option<Receiver<(usize, Result<LoadedClip>)>>,
    worker: Option<JoinHandle<()>>,
}

impl ClipPrefetcher {
    /// Start loading `jobs` (the image items of one output, in media order)
    /// on a background thread with bounded lookahead.
    pub fn spawn(jobs: Vec<PrefetchJob>, out: OutputDef) -> Self {
        let (tx, rx) = sync_channel(LOOKAHEAD);
        let worker = std::thread::spawn(move || {
            for job in jobs {
                let result = LoadedClip::load(&job.path, &out, job.focus);
                // Send blocks while the channel is full — that block IS the
                // lookahead bound. An Err means the consumer is gone (build
                // aborted/failed): stop loading, nothing to deliver to.
                if tx.send((job.index, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            rx: Some(rx),
            worker: Some(worker),
        }
    }

    /// Blocking-receive the next prefetched clip, in job order. `None` once
    /// every job has been delivered. The caller times this wait as the
    /// boundary stall (`StageTimings::add_stall`).
    pub fn next(&mut self) -> Option<(usize, Result<LoadedClip>)> {
        self.rx.as_ref().and_then(|rx| rx.recv().ok())
    }
}

impl Drop for ClipPrefetcher {
    fn drop(&mut self) {
        // Drop the receiver first so a worker blocked in send() unblocks with
        // an error and exits, then join so no load outlives the build.
        drop(self.rx.take());
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutputDef;
    use ::image::RgbImage;

    fn out_def() -> OutputDef {
        OutputDef {
            name: "t".into(),
            width: 64,
            height: 48,
            fps: 30,
            pic_display_time_secs: 1.0,
            fade_time_secs: 0.1,
            max_rotation_degrees: 0.0,
            bulk_video_time_min: 20,
            quality_crf: 28,
            enable_audio: false,
            audio_bitrate_kbps: 192,
            audio_sample_rate: 48_000,
            encoder: "software".into(),
            x264_preset: "medium".into(),
            zoom_amount: 0.12,
            ken_burns: true,
        }
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("slideshow_prefetch_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Verifies: SR-014, SR-036, LLR-060 (TC-086) — results (including a failed
    // load) are delivered strictly in job order with the job's index, so the
    // clip loop can route the error through record_skip exactly where the
    // serial path would have; the stream ends (None) after the last job.
    #[test]
    fn prefetch_delivers_results_in_order_sr014() {
        let dir = temp_dir("order");
        let good = |name: &str| {
            let p = dir.join(name);
            RgbImage::from_pixel(96, 72, ::image::Rgb([50, 90, 130]))
                .save(&p)
                .unwrap();
            p
        };
        let a = good("a.png");
        let bad = dir.join("bad.png"); // valid header, truncated image data
        let bytes = std::fs::read(&a).unwrap();
        std::fs::write(&bad, &bytes[..bytes.len() / 2]).unwrap();
        let c = good("c.png");

        let jobs = vec![
            PrefetchJob {
                index: 0,
                path: a,
                focus: None,
            },
            PrefetchJob {
                index: 3,
                path: bad,
                focus: Some((0.5, 0.5)),
            },
            PrefetchJob {
                index: 7,
                path: c,
                focus: None,
            },
        ];
        let mut pf = ClipPrefetcher::spawn(jobs, out_def());

        let (i0, r0) = pf.next().expect("first result");
        assert_eq!(i0, 0);
        assert!(r0.is_ok(), "valid image must load");
        let (i1, r1) = pf.next().expect("second result");
        assert_eq!(i1, 3, "failed load must arrive in order, not be reordered");
        assert!(r1.is_err(), "corrupt image must deliver its load error");
        let (i2, r2) = pf.next().expect("third result");
        assert_eq!(i2, 7);
        assert!(r2.is_ok(), "a failure must not poison later loads");
        assert!(pf.next().is_none(), "stream ends after the last job");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Verifies: LLR-060 — dropping the prefetcher mid-stream (build aborted)
    // unblocks and joins the worker instead of deadlocking or leaking it.
    #[test]
    fn drop_mid_stream_joins_worker_sr036() {
        let dir = temp_dir("drop");
        let mut jobs = Vec::new();
        for i in 0..6 {
            let p = dir.join(format!("img_{i}.png"));
            RgbImage::from_pixel(96, 72, ::image::Rgb([10, 20, 30]))
                .save(&p)
                .unwrap();
            jobs.push(PrefetchJob {
                index: i,
                path: p,
                focus: None,
            });
        }
        let mut pf = ClipPrefetcher::spawn(jobs, out_def());
        let _ = pf.next(); // consume one, then abandon the rest
        drop(pf); // must return (join) without hanging
        let _ = std::fs::remove_dir_all(&dir);
    }
}
