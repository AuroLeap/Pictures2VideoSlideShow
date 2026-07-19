//! A uniform pull-based frame source so images and videos can be driven through
//! the same streaming cross-fade mixer.

use crate::error::Result;
use crate::image::ClipRenderer;
use crate::util::timing::StageTimings;
use crate::video::VideoFrameReader;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

/// Yields raw `rgb24` frames one at a time until the clip is exhausted.
pub trait FrameSource {
    /// The next frame, or `None` when the clip is finished.
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>>;
}

/// Image clip: renders frames in batches and serves them one at a time,
/// preserving renderer throughput while exposing a pull interface. Holds the
/// backend-blind [`ClipRenderer`] (LLR-066): CPU or GPU, the frame shape and
/// everything downstream are identical.
// Implements: LLR-066, SR-039
pub struct ImageFrameSource {
    renderer: Box<dyn ClipRenderer>,
    total: u32,
    next: u32,
    batch: u32,
    buf: VecDeque<Vec<u8>>,
    /// Per-output stage timers; render batches accumulate into the render stage.
    timings: Arc<StageTimings>,
}

impl ImageFrameSource {
    pub fn new(renderer: Box<dyn ClipRenderer>, batch: u32, timings: Arc<StageTimings>) -> Self {
        let total = renderer.total_frames();
        Self {
            renderer,
            total,
            next: 0,
            batch: batch.max(1),
            buf: VecDeque::new(),
            timings,
        }
    }
}

impl FrameSource for ImageFrameSource {
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        if self.buf.is_empty() && self.next < self.total {
            let end = (self.next + self.batch).min(self.total);
            // One Instant pair per batch, not per frame (LLR-050 cheapness).
            // Implements: LLR-050, SR-036
            let t = Instant::now();
            self.buf.extend(self.renderer.render_range(self.next, end));
            self.timings.add_render(t.elapsed());
            self.next = end;
        }
        Ok(self.buf.pop_front())
    }
}

/// Video clip: pulls decoded frames straight from the ffmpeg reader.
pub struct VideoFrameSource {
    reader: VideoFrameReader,
}

impl VideoFrameSource {
    pub fn new(reader: VideoFrameReader) -> Self {
        Self { reader }
    }
}

impl FrameSource for VideoFrameSource {
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        self.reader.read_frame()
    }
}
