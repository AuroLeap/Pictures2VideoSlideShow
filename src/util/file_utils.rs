use crate::error::Result;
use std::path::Path;

pub fn ensure_dir_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(crate::error::SlideshowError::Io)?;
    }
    Ok(())
}
