#!/bin/bash
# Stage-0 onboarder (macOS) — the first rung of the onboarding ladder
# (process.md §7):
#
#     Stage 0  ->  dev-setup  ->  setup  ->  check
#     (this)       workstation     deps       gates
#
# Takes someone from a bare machine to an editable, viewable checkout: it
# ensures git, picks a folder, clones over HTTPS, then hands off to dev-setup.
# You do NOT need to know git to use it — an AI agent can drive git for you
# afterward (see the end banner).
#
# The .command extension makes this double-clickable in Finder. Read it first
# (it is meant to be read); it installs nothing without asking and never pipes
# code from the internet into a shell. macOS may warn about an unsigned script
# (Gatekeeper) — that is expected for a file you downloaded and read yourself.
#
# Linux contributors: use onboard.sh.  Windows: use onboard.cmd.

# --- EDIT FOR YOUR PROJECT ----------------------------------------------------
# bootstrap.py copies this file as-is; fill in your repo's HTTPS clone URL. The
# project may then attach onboard.command to a GitHub Release.
REPO_URL="https://github.com/OWNER/REPO.git"
# -----------------------------------------------------------------------------

set -eu

say()  { printf '%s\n' "$*"; }
rule() { say "------------------------------------------------------------------"; }

# 1. Explain, then get explicit consent.
rule
say "This will set up a working copy of:"
say "    $REPO_URL"
say
say "Steps (each shown before it runs):"
say "  1. make sure 'git' is installed"
say "  2. ask you to pick a folder to put the project in"
say "  3. download (clone) the project into that folder over HTTPS"
say "  4. offer to check your developer workstation (dev-setup)"
rule
printf 'Continue? [y/N] '
read -r reply
case "$reply" in
  [Yy]*) ;;
  *) say "Cancelled — nothing was changed."; exit 0 ;;
esac

# 2. Ensure git. On macOS the Xcode Command Line Tools ship git; Homebrew is the
#    common alternative. Offer whichever is available.
if ! command -v git >/dev/null 2>&1; then
  say
  say "'git' is not installed."
  if command -v brew >/dev/null 2>&1; then
    printf 'Run "brew install git" now? [y/N] '
    read -r ok
    case "$ok" in
      [Yy]*) brew install git ;;
      *) say "Install git yourself, then re-run."; exit 1 ;;
    esac
  else
    printf 'Install the Xcode Command Line Tools (includes git) now? [y/N] '
    read -r ok
    case "$ok" in
      [Yy]*) xcode-select --install || true
             say "Finish the popup installer, then re-run this script."; exit 0 ;;
      *) say "Install git (xcode-select --install or Homebrew), then re-run."; exit 1 ;;
    esac
  fi
fi

# 3. Native folder picker via AppleScript, else ask on the CLI.
DEST_PARENT=$(osascript -e 'try' \
  -e 'POSIX path of (choose folder with prompt "Pick a folder to put the project in")' \
  -e 'end try' 2>/dev/null || true)
if [ -z "$DEST_PARENT" ]; then
  printf 'Folder to clone into [%s]: ' "$HOME"
  read -r DEST_PARENT
  [ -n "$DEST_PARENT" ] || DEST_PARENT="$HOME"
fi

# Derive "<parent>/<repo-name>" from the URL's last path segment.
name=$(basename "$REPO_URL")
case "$name" in *.git) name=$(basename "$name" .git) ;; esac
DEST="${DEST_PARENT%/}/$name"

# 4. Clone over HTTPS. (Private repo or pushing later: authenticate with the
#    host CLI — e.g. `gh auth login` — not hand-rolled SSH keys.)
say
say "Cloning into: $DEST"
if [ -e "$DEST" ]; then
  say "That folder already exists — using it as-is (skipping clone)."
else
  git clone "$REPO_URL" "$DEST"
fi

# 5. End banner: checkout dir + agent handoff, then offer dev-setup.
say
rule
say "  Your checkout is ready at:"
say "      $DEST"
say
say "  If you'd like an AI agent to manage your changes (commits, pushes,"
say "  reviews) for you, point it at this directory."
rule
say

DEV_SETUP="$DEST/scripts/dev-setup.sh"
if [ -f "$DEV_SETUP" ]; then
  printf 'Check your developer workstation now (runs dev-setup --check, installs nothing)? [Y/n] '
  read -r go
  case "$go" in
    [Nn]*) say "Skipped. Run it later with: sh \"$DEV_SETUP\" --check" ;;
    *) sh "$DEV_SETUP" --check || true
       say
       say "To install the baseline workstation: sh \"$DEV_SETUP\" --baseline" ;;
  esac
else
  say "Next: cd \"$DEST\" and run scripts/dev-setup.sh --check"
fi
