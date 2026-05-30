use crate::error::Result;

pub struct VideoEncoder;

impl VideoEncoder {
    pub fn new() -> Self {
        Self
    }

    pub async fn encode(&self) -> Result<()> {
        // TODO: Implement FFmpeg coordination
        Ok(())
    }
}
