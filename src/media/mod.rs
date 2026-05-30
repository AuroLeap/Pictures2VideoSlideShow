use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub path: PathBuf,
    pub file_type: MediaType,
    pub dimensions: (u32, u32),
    pub duration_secs: Option<f32>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MediaType {
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "video")]
    Video,
}

#[derive(Debug, Clone)]
pub struct Album {
    pub media_files: Vec<MediaFile>,
    pub total_size: u64,
    pub created_at: SystemTime,
}

pub struct MediaLoader {
    root_path: PathBuf,
}

impl MediaLoader {
    pub fn new(root_path: PathBuf) -> Self {
        Self { root_path }
    }

    pub async fn scan_and_index(&self) -> Result<Album> {
        log::info!("Starting media scan from: {}", self.root_path.display());

        let mut media_files = Vec::new();
        let mut total_size = 0u64;

        for entry in walkdir::WalkDir::new(&self.root_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path().to_path_buf();

            // Check if file should be included
            if let Ok(Some(media_file)) = self.process_file(&path).await {
                total_size += media_file.size_bytes;
                media_files.push(media_file);
            }
        }

        log::info!(
            "Media scan complete: {} files, {} bytes",
            media_files.len(),
            total_size
        );

        Ok(Album {
            media_files,
            total_size,
            created_at: SystemTime::now(),
        })
    }

    async fn process_file(&self, path: &PathBuf) -> Result<Option<MediaFile>> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        let file_type = match extension.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "tiff" => MediaType::Image,
            "mp4" | "avi" | "mov" | "mkv" | "webm" | "flv" | "wmv" => MediaType::Video,
            _ => return Ok(None),
        };

        let metadata = std::fs::metadata(path)
            .map_err(|e| crate::SlideshowError::Media(format!("Failed to read metadata: {}", e)))?;

        let dimensions = self.get_dimensions(path, file_type).await?;

        Ok(Some(MediaFile {
            path: path.clone(),
            file_type,
            dimensions,
            duration_secs: if file_type == MediaType::Video {
                self.get_duration(path).await.ok()
            } else {
                None
            },
            size_bytes: metadata.len(),
        }))
    }

    async fn get_dimensions(
        &self,
        _path: &PathBuf,
        _file_type: MediaType,
    ) -> Result<(u32, u32)> {
        // TODO: Extract actual dimensions from image/video
        // Placeholder: return default
        Ok((1920, 1080))
    }

    async fn get_duration(&self, _path: &PathBuf) -> Result<f32> {
        // TODO: Extract video duration via ffprobe
        // Placeholder
        Ok(0.0)
    }
}
