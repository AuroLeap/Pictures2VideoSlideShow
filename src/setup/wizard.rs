//! First-run GUI wizard launcher. Runs the embedded WinForms script
//! (`wizard.ps1`) under PowerShell `-STA`, captures its `key=value` output, and
//! returns parsed [`WizardValues`]. The value→config mapping lives in
//! [`super::config_builder`] (unit-tested); this launcher is Demonstration-only
//! (it needs a desktop session and is never reached in automation per SR-028).
// Implements: LLR-029, SR-026

use super::config_builder::{parse_wizard_output, WizardValues};
use crate::error::{Result, SlideshowError};
use std::io::Write;
use std::process::Command;

/// The WinForms wizard script, embedded so the single binary is self-contained.
const WIZARD_PS1: &str = include_str!("wizard.ps1");

/// Launch the first-run wizard and return the collected values, or an error if
/// the user cancelled or the dialog could not run.
// Implements: LLR-029, SR-026
pub fn run_wizard() -> Result<WizardValues> {
    // Write the embedded script to a temp file and run it with -STA (WinForms).
    let script = std::env::temp_dir().join("mvs_setup_wizard.ps1");
    {
        let mut f = std::fs::File::create(&script)
            .map_err(|e| SlideshowError::Config(format!("cannot stage setup wizard: {}", e)))?;
        f.write_all(WIZARD_PS1.as_bytes())
            .map_err(|e| SlideshowError::Config(format!("cannot write setup wizard: {}", e)))?;
    }

    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-STA", "-File"])
        .arg(&script)
        .output()
        .map_err(|e| SlideshowError::Config(format!("cannot launch setup wizard: {}", e)))?;

    let _ = std::fs::remove_file(&script);

    if !output.status.success() {
        return Err(SlideshowError::Config(
            "setup cancelled — no configuration was created".into(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_wizard_output(&stdout)
}
