use crate::error::Result;

pub struct FFmpegProcess;

impl FFmpegProcess {
    pub fn new() -> Self {
        Self
    }

    pub async fn spawn(&self) -> Result<()> {
        // TODO: Implement FFmpeg process spawning and frame streaming
        Ok(())
    }
}
