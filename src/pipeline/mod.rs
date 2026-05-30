use crate::config::{OutputDef, ProcessingConfig};
use crate::error::Result;
use crate::media::Album;

pub struct FrameGenerationPipeline {
    album: Album,
    outputs: Vec<OutputDef>,
    processing: ProcessingConfig,
}

impl FrameGenerationPipeline {
    pub fn new(album: Album, outputs: Vec<OutputDef>, processing: ProcessingConfig) -> Self {
        Self {
            album,
            outputs,
            processing,
        }
    }

    pub async fn execute(&self) -> Result<()> {
        log::info!(
            "Starting frame generation pipeline for {} media files",
            self.album.media_files.len()
        );

        // TODO: Implement full pipeline orchestration
        // 1. For each media file
        // 2. For each output definition
        // 3. Generate frames (parallel)
        // 4. Stream to encoder

        Ok(())
    }
}
