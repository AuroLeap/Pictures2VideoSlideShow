//! A uniform pull-based frame source so images and videos can be driven through
//! the same streaming cross-fade mixer.

use crate::error::Result;
use crate::image::FrameRenderer;
use crate::video::VideoFrameReader;
use std::collections::VecDeque;

/// Yields raw `rgb24` frames one at a time until the clip is exhausted.
pub trait FrameSource {
    /// The next frame, or `None` when the clip is finished.
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>>;
}

/// Image clip: renders frames in parallel batches and serves them one at a time,
/// preserving rayon throughput while exposing a pull interface.
pub struct ImageFrameSource {
    renderer: FrameRenderer,
    total: u32,
    next: u32,
    batch: u32,
    buf: VecDeque<Vec<u8>>,
}

impl ImageFrameSource {
    pub fn new(renderer: FrameRenderer, batch: u32) -> Self {
        let total = renderer.total_frames();
        Self {
            renderer,
            total,
            next: 0,
            batch: batch.max(1),
            buf: VecDeque::new(),
        }
    }
}

impl FrameSource for ImageFrameSource {
    fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        if self.buf.is_empty() && self.next < self.total {
            let end = (self.next + self.batch).min(self.total);
            self.buf.extend(self.renderer.render_range(self.next, end));
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
