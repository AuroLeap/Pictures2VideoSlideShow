//! Config file location (SR-030): the config lives beside the executable when
//! that directory is writable, otherwise under `%APPDATA%`. The chosen path is
//! reported to the user and re-read on subsequent runs.
// Implements: LLR-035, SR-030

use std::path::{Path, PathBuf};

const APP_DIR: &str = "make_video_slideshow";
const CONFIG_NAME: &str = "config.toml";

/// Pure directory choice: the exe dir when writable; else `<appdata>/<app>`;
/// else fall back to the exe dir (last resort when no APPDATA is set).
// Implements: LLR-035, SR-030
pub fn choose_config_dir(
    exe_dir: &Path,
    exe_dir_writable: bool,
    appdata: Option<&Path>,
) -> PathBuf {
    if exe_dir_writable {
        exe_dir.to_path_buf()
    } else if let Some(ad) = appdata {
        ad.join(APP_DIR)
    } else {
        exe_dir.to_path_buf()
    }
}

/// Probe whether a directory is writable by creating and removing a temp file.
fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".mvs_write_test");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Resolve the absolute config path the first-run wizard writes and the app
/// reads. Beside the exe if writable, else under `%APPDATA%`.
// Implements: LLR-035, SR-030
pub fn resolve_config_path() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let writable = dir_writable(&exe_dir);
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    choose_config_dir(&exe_dir, writable, appdata.as_deref()).join(CONFIG_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: SR-030, LLR-035 — exe dir used when writable.
    #[test]
    fn uses_exe_dir_when_writable_sr030() {
        let exe = Path::new("C:/Program Files/App");
        let appdata = Path::new("C:/Users/u/AppData/Roaming");
        assert_eq!(
            choose_config_dir(exe, true, Some(appdata)),
            PathBuf::from("C:/Program Files/App")
        );
    }

    // Verifies: SR-030, LLR-035 — falls back to %APPDATA%/<app> when exe dir is read-only.
    #[test]
    fn falls_back_to_appdata_when_readonly_sr030() {
        let exe = Path::new("C:/Program Files/App");
        let appdata = Path::new("C:/Users/u/AppData/Roaming");
        assert_eq!(
            choose_config_dir(exe, false, Some(appdata)),
            PathBuf::from("C:/Users/u/AppData/Roaming/make_video_slideshow")
        );
    }

    // Verifies: SR-030, LLR-035 — last-resort exe dir when no APPDATA.
    #[test]
    fn falls_back_to_exe_dir_without_appdata_sr030() {
        let exe = Path::new("/opt/app");
        assert_eq!(
            choose_config_dir(exe, false, None),
            PathBuf::from("/opt/app")
        );
    }
}
