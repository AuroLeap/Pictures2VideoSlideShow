//! Segment-rolling frame sink (SR-037 spike foundation): encodes the mixer's
//! output frame stream into per-clip MPEG-TS segments, starting a fresh
//! encoder at every transition midpoint the mixer signals — the plan §4
//! boundary ("second half of transition in + body + first half of transition
//! out") that the future warm-build planner (LLR-055) will key segments by.

// lib-API: the SR-037 segmented path, exercised via tests/concat_seam.rs; the
// binary wires it up with the warm-build planner (LLR-055, Round 5b).
#![allow(dead_code)]

use super::FrameSink;
use crate::error::Result;
use crate::ffmpeg::encoder_args::EncoderSettings;
use crate::ffmpeg::FfmpegEncoder;
use std::path::{Path, PathBuf};

/// Encodes an ordered frame stream into `<name>_seg_NNNN.ts` files, one per
/// segment, each promoted atomically when its encoder finishes. The mixer
/// drives [`FrameSink::roll`] at each transition midpoint; everything between
/// two rolls (or stream edges) lands in one segment.
// Implements: LLR-056, SR-037, SR-011, SR-013
pub struct SegmentedEncoderSink {
    dir: PathBuf,
    base: String,
    width: u32,
    height: u32,
    fps: u32,
    enc: EncoderSettings,
    timeout_secs: u64,
    /// The encoder for the segment currently being written.
    current: Option<FfmpegEncoder>,
    /// Index of the segment currently being written (0-based).
    seg_index: usize,
    /// Total frames written across all segments so far.
    frames: u64,
    /// Finished (promoted) segment paths, in output order.
    segments: Vec<PathBuf>,
    /// Output frame index at which each segment after the first begins.
    boundaries: Vec<u64>,
}

impl SegmentedEncoderSink {
    /// Open the sink and start segment 0's encoder in `dir` (which must
    /// exist). `enc`/`timeout_secs` are passed to every per-segment
    /// [`FfmpegEncoder::start_segment`] unchanged.
    pub fn start(
        dir: &Path,
        base: &str,
        width: u32,
        height: u32,
        fps: u32,
        enc: EncoderSettings,
        timeout_secs: u64,
    ) -> Result<Self> {
        let mut sink = Self {
            dir: dir.to_path_buf(),
            base: base.to_string(),
            width,
            height,
            fps,
            enc,
            timeout_secs,
            current: None,
            seg_index: 0,
            frames: 0,
            segments: Vec::new(),
            boundaries: Vec::new(),
        };
        sink.current = Some(sink.open_segment()?);
        Ok(sink)
    }

    /// The on-disk path of segment `seg_index`.
    fn segment_path(&self, index: usize) -> PathBuf {
        self.dir.join(format!("{}_seg_{:04}.ts", self.base, index))
    }

    /// Start the encoder for the current `seg_index`.
    fn open_segment(&self) -> Result<FfmpegEncoder> {
        FfmpegEncoder::start_segment(
            &self.segment_path(self.seg_index),
            self.width,
            self.height,
            self.fps,
            &self.enc,
            self.timeout_secs,
        )
    }

    /// Finish (promote) the in-flight segment and record its path.
    fn close_current(&mut self) -> Result<()> {
        if let Some(enc) = self.current.take() {
            enc.finish()?;
            self.segments.push(self.segment_path(self.seg_index));
        }
        Ok(())
    }

    /// Close the last segment and return `(segment paths, boundaries)` —
    /// boundaries are the output frame indices where segments 1.. begin.
    pub fn finish_all(mut self) -> Result<(Vec<PathBuf>, Vec<u64>)> {
        self.close_current()?;
        Ok((
            std::mem::take(&mut self.segments),
            std::mem::take(&mut self.boundaries),
        ))
    }
}

impl FrameSink for SegmentedEncoderSink {
    fn write(&mut self, frame: &[u8]) -> Result<()> {
        self.current
            .as_mut()
            .expect("segment encoder open while frames are written")
            .write_frame(frame)?;
        self.frames += 1;
        Ok(())
    }

    /// Transition midpoint: promote the finished segment and roll to the next
    /// — the next written frame becomes the new segment's first frame.
    fn roll(&mut self) -> Result<()> {
        self.close_current()?;
        self.seg_index += 1;
        self.boundaries.push(self.frames);
        self.current = Some(self.open_segment()?);
        Ok(())
    }
}
