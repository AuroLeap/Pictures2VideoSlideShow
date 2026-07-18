//! Interaction gating: prompts and the (future) first-run GUI wizard must only
//! appear in genuine interactive use. Automation, CI, scheduled runs, and the
//! test harness pass `--non-interactive` (or simply have no TTY) and must NEVER
//! be blocked by a prompt or dialog.
// Implements: LLR-033, SR-028

use std::io::IsTerminal;

/// Pure decision: should the tool open the first-run setup prompt/wizard?
///
/// Prompt only when there is no usable config yet AND the user did not request
/// non-interactive AND stdin is an interactive terminal. Any automation path
/// (no TTY or `--non-interactive`) returns `false` so the run never blocks.
// Implements: LLR-033, SR-028
pub fn should_prompt(config_present: bool, non_interactive: bool, stdin_is_tty: bool) -> bool {
    !config_present && !non_interactive && stdin_is_tty
}

/// Whether this process may interact with a human at all: honors
/// `--non-interactive` and requires a real terminal on stdin.
// Implements: LLR-033, SR-028
pub fn is_interactive(non_interactive: bool) -> bool {
    !non_interactive && std::io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies: SR-028, LLR-033 — prompt only with no config + interactive + TTY.
    #[test]
    fn prompts_only_when_interactive_and_unconfigured_sr028() {
        assert!(should_prompt(false, false, true));
    }

    // Verifies: SR-028, LLR-033 — the never-block guardrail across automation states.
    #[test]
    fn never_prompts_in_noninteractive_or_no_tty_or_configured_sr028() {
        // --non-interactive set: never prompt, even with a TTY and no config.
        assert!(!should_prompt(false, true, true));
        // No TTY (piped/CI): never prompt.
        assert!(!should_prompt(false, false, false));
        // Config already present: nothing to prompt for.
        assert!(!should_prompt(true, false, true));
        // Fully non-interactive + configured: never prompt.
        assert!(!should_prompt(true, true, false));
    }
}
