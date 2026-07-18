//! Encoder-writer thread (Phase 3 pipeline overlap, plan §5): the mixer emits
//! frames into a bounded channel and a dedicated thread owns the downstream
//! [`FrameSink`] (ffmpeg stdin, or the segment-rolling sink), so rendering
//! never blocks on encoder pipe writes and vice versa — while failures keep
//! the exact serial semantics (SR-011/SR-013/SR-015).

use super::FrameSink;
use crate::error::{Result, SlideshowError};
use crate::util::timing::StageTimings;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

/// Ceiling on the bytes the mixer→writer channel (and the render batch
/// buffer, sized to the same depth) may hold. Kept at the old
/// `RANGE_MEMORY_BUDGET` value deliberately: it preserves the pre-overlap
/// in-flight frame footprint exactly (PB-005 stays on its baseline) while
/// the channel still decouples pipe writes; at the bench-canonical 1080p it
/// caps depth at 10 frames (below the full `2×fade` 30 — the LLR-063 "~").
/// Whether a deeper channel buys throughput is an open quiet-host tuning
/// question (Round-6 fps numbers were host-load-skewed; see status.md).
const MAX_CHANNEL_BYTES: usize = 64 * 1024 * 1024;

/// Channel/batch depth for one output: `2 × fade_frames` (LLR-063), floored
/// at 4 so overlap exists even with `fade = 0`, and capped so
/// `depth × frame_bytes` stays near [`MAX_CHANNEL_BYTES`] (for pathological
/// frame sizes the 4-frame floor wins over the byte cap and is then the
/// documented bound). This depth —
/// not the deleted `RANGE_MEMORY_BUDGET` — is what bounds in-flight frame
/// memory now: the render batch buffer and the channel each hold at most
/// `depth` frames, so peak in-flight bytes ≤ `2 × depth × frame_bytes`
/// (+ the mixer's `fade_frames` tail ring), verified by PB-005.
// Implements: LLR-063, SR-036
pub fn writer_channel_depth(fade_frames: usize, frame_bytes: usize) -> usize {
    let cap = (MAX_CHANNEL_BYTES / frame_bytes.max(1)).max(4);
    (2 * fade_frames).clamp(4, cap)
}

/// One queued instruction for the writer thread: a frame to write, or the
/// mixer's transition-midpoint segment-roll marker (order-preserving, so the
/// segmented sink rolls at exactly the frame the serial path would).
enum WriterMsg {
    Frame(Vec<u8>),
    Roll,
}

/// Owns a [`FrameSink`] on a dedicated thread behind a bounded channel.
///
/// The mixer (or any producer) calls [`FrameSink::write`]/[`FrameSink::roll`]
/// on this handle; the thread drains messages into the inner sink — for the
/// streaming path that inner sink is the [`crate::ffmpeg::FfmpegEncoder`]
/// whose `write_frame` remains the single watchdog-activity bump (SR-013).
/// A sink failure ends the thread, which drops the inner sink there (encoder
/// Drop kills ffmpeg and removes the `.part` — SR-011) and disconnects the
/// channel so a blocked sender unblocks immediately instead of deadlocking;
/// the failing `SlideshowError` is then re-raised verbatim by the next
/// [`FrameSink::write`] or by [`EncoderWriter::join`] (LLR-064).
// Implements: LLR-063, LLR-064, SR-011, SR-013, SR-015, SR-036
pub struct EncoderWriter<S: FrameSink + Send + 'static> {
    tx: Option<SyncSender<WriterMsg>>,
    /// The thread's result: the inner sink handed back on success, or the
    /// first sink error — the LLR-064 result back-channel.
    handle: Option<JoinHandle<Result<S>>>,
}

impl<S: FrameSink + Send + 'static> EncoderWriter<S> {
    /// Move `sink` onto a writer thread behind a channel of `depth` frames
    /// (see [`writer_channel_depth`]). Raw pipe time accrues to the
    /// `pipe-write` stage on `timings`; the *sender's* blocking time is the
    /// encode-write-stall the mixer measures (back-pressure), unchanged in
    /// meaning (SR-036).
    pub fn start(sink: S, depth: usize, timings: Arc<StageTimings>) -> Self {
        let (tx, rx) = sync_channel::<WriterMsg>(depth.max(1));
        let mut sink = sink;
        let handle = std::thread::spawn(move || -> Result<S> {
            for msg in rx {
                match msg {
                    WriterMsg::Frame(frame) => {
                        // The pipe write itself, now overlapped with
                        // rendering; errors (disk-full mapping LLR-018,
                        // broken pipe after a watchdog kill) pass through
                        // untouched. Implements: LLR-063, LLR-050, SR-036
                        let t = Instant::now();
                        sink.write(frame)?;
                        timings.add_pipe(t.elapsed());
                    }
                    WriterMsg::Roll => sink.roll()?,
                }
            }
            // Channel disconnected: the producer finished (join) or aborted
            // (Drop) — hand the sink back for finalize/cleanup by the caller.
            Ok(sink)
        });
        Self {
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    /// Close the channel, join the thread, and hand back the inner sink so
    /// the caller can finalize it (`finish_to_part`, `finish_all`). A writer
    /// error is re-raised here exactly as the serial call site would have
    /// seen it (LLR-064): no false success, and the failed sink was already
    /// dropped on the thread (`.part` removed via encoder Drop, SR-011).
    // Implements: LLR-064, SR-011, SR-013, SR-015
    pub fn join(mut self) -> Result<S> {
        drop(self.tx.take()); // disconnect => the thread drains and returns
        let handle = self
            .handle
            .take()
            .expect("join is the only consumer of the writer handle");
        match handle.join() {
            Ok(result) => result,
            Err(_) => Err(SlideshowError::Processing(
                "encoder writer thread panicked".into(),
            )),
        }
    }

    /// Retrieve the real failure after a send found the channel disconnected
    /// (the thread exited early). Never blocks: the thread is already done.
    fn take_error(&mut self) -> SlideshowError {
        match self.handle.take().map(|h| h.join()) {
            Some(Ok(Err(e))) => e,
            Some(Err(_)) => SlideshowError::Processing("encoder writer thread panicked".into()),
            // Ok(sink) with a live sender cannot happen (the thread only
            // returns Ok on disconnect) — fail loudly rather than invent.
            Some(Ok(Ok(_))) | None => {
                SlideshowError::Processing("encoder writer thread exited unexpectedly".into())
            }
        }
    }
}

impl<S: FrameSink + Send + 'static> FrameSink for EncoderWriter<S> {
    /// Queue one frame. Blocks only while the channel is full (that blocking
    /// is the encode-write back-pressure the mixer times); if the writer
    /// thread has died the disconnected send returns immediately and the
    /// thread's real error is re-raised (LLR-064 — no deadlock, no
    /// placeholder error).
    fn write(&mut self, frame: Vec<u8>) -> Result<()> {
        match &self.tx {
            Some(tx) if tx.send(WriterMsg::Frame(frame)).is_ok() => Ok(()),
            _ => Err(self.take_error()),
        }
    }

    /// Queue the transition-midpoint roll in-order with the frames (SR-037
    /// segment boundaries land on exactly the serial frame).
    fn roll(&mut self) -> Result<()> {
        match &self.tx {
            Some(tx) if tx.send(WriterMsg::Roll).is_ok() => Ok(()),
            _ => Err(self.take_error()),
        }
    }
}

impl<S: FrameSink + Send + 'static> Drop for EncoderWriter<S> {
    /// Early abort (error unwind, the zero-frames abort path): disconnect and
    /// join. The thread drains the few queued frames and returns; the inner
    /// sink drops here-after unfinalized, so encoder Drop cleanup (kill +
    /// `.part` removal) runs exactly as in the serial path (SR-011).
    // Implements: LLR-064, SR-011
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join(); // the drained sink (if Ok) is dropped here
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: LLR-063, SR-036 (TC-089 memory-bound leg) — the channel depth
    // is 2x fade_frames, floored for overlap with fade=0, and byte-capped so
    // depth x frame_bytes stays bounded for any resolution.
    #[test]
    fn writer_channel_depth_bounds_memory_sr036() {
        let hd = 1920 * 1080 * 3;
        // Bench-canonical 1080p: the 64 MB byte cap rules (10 frames), keeping
        // the pre-overlap render footprint (Round-6 measured tune).
        assert_eq!(writer_channel_depth(15, hd), 10);
        // fade 0 still overlaps (floor).
        assert_eq!(writer_channel_depth(0, hd), 4);
        // For large frames the cap clamps depth down to the 4-frame floor —
        // the floor is then the (documented) memory bound.
        let uhd4k = 3840 * 2160 * 3;
        assert_eq!(
            writer_channel_depth(120, uhd4k),
            4,
            "byte cap must clamp large-frame depth to the floor"
        );
        // Small fades under the cap keep 2x fade.
        assert_eq!(writer_channel_depth(3, hd), 6);
        // Tiny frames: cap is huge, 2x fade rules.
        assert_eq!(writer_channel_depth(60, 64 * 48 * 3), 120);
    }

    // Verifies: LLR-064 — frames and rolls arrive at the inner sink in send
    // order across the thread boundary (segment boundaries cannot drift).
    #[test]
    fn writer_preserves_frame_and_roll_order_sr037() {
        #[derive(Default)]
        struct RecordingSink {
            events: Vec<String>,
        }
        impl FrameSink for RecordingSink {
            fn write(&mut self, frame: Vec<u8>) -> Result<()> {
                self.events.push(format!("f{}", frame[0]));
                Ok(())
            }
            fn roll(&mut self) -> Result<()> {
                self.events.push("roll".into());
                Ok(())
            }
        }

        let mut w =
            EncoderWriter::start(RecordingSink::default(), 2, Arc::new(StageTimings::new()));
        w.write(vec![1]).unwrap();
        w.write(vec![2]).unwrap();
        FrameSink::roll(&mut w).unwrap();
        w.write(vec![3]).unwrap();
        let sink = w.join().expect("clean join hands the sink back");
        assert_eq!(sink.events, ["f1", "f2", "roll", "f3"]);
    }
}
