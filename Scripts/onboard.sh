#!/usr/bin/env sh
# Stage-0 onboarder (Linux) — the first rung of the onboarding ladder
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
# HOW TO RUN: download this file, read it (it is meant to be read), then run
#   sh onboard.sh
# Never pipe a remote script straight into a shell. Nothing here does that.
#
# macOS contributors: use onboard.command.  Windows: use onboard.cmd.

# --- EDIT FOR YOUR PROJECT ----------------------------------------------------
# bootstrap.py copies this file as-is; fill in your repo's HTTPS clone URL. The
# project may then attach onboard.sh to a GitHub Release so contributors can
# download a single file. (The kit does not run any release CI for you.)
REPO_URL="https://github.com/OWNER/REPO.git"
# -----------------------------------------------------------------------------

set -eu

say()  { printf '%s\n' "$*"; }
rule() { say "------------------------------------------------------------------"; }

# 1. Explain, then get explicit consent before doing anything.
rule
say "This will set up a working copy of:"
say "    $REPO_URL"
say
say "Steps (each shown before it runs):"
say "  1. make sure 'git' is installed"
say "  2. ask you to pick a folder to put the project in"
say "  3. download (clone) the project into that folder over HTTPS"
say "  4. offer to check your developer workstation (dev-setup)"
say
say "It installs nothing without asking first, and never pipes code from the"
say "internet into a shell."
rule
printf 'Continue? [y/N] '
read -r reply
case "$reply" in
  [Yy]*) ;;
  *) say "Cancelled — nothing was changed."; exit 0 ;;
esac

# 2. Ensure git is available (offer to install via the system package manager).
if ! command -v git >/dev/null 2>&1; then
  say
  say "'git' is not installed."
  if command -v apt-get >/dev/null 2>&1;   then GIT_INSTALL="sudo apt-get install -y git"
  elif command -v dnf >/dev/null 2>&1;     then GIT_INSTALL="sudo dnf install -y git"
  elif command -v pacman >/dev/null 2>&1;  then GIT_INSTALL="sudo pacman -S --noconfirm git"
  else GIT_INSTALL=""
  fi
  if [ -n "$GIT_INSTALL" ]; then
    printf 'Run "%s" now? [y/N] ' "$GIT_INSTALL"
    read -r ok
    case "$ok" in
      [Yy]*) sh -c "$GIT_INSTALL" ;;
      *) say "Install git yourself, then re-run this script."; exit 1 ;;
    esac
  else
    say "Please install git with your package manager, then re-run this script."
    exit 1
  fi
fi

# 3. Native folder picker (zenity if a desktop is present), else ask on the CLI.
DEST_PARENT=""
if command -v zenity >/dev/null 2>&1; then
  DEST_PARENT=$(zenity --file-selection --directory \
                --title="Pick a folder to put the project in" 2>/dev/null || true)
fi
if [ -z "$DEST_PARENT" ]; then
  printf 'Folder to clone into [%s]: ' "$HOME"
  read -r DEST_PARENT
  [ -n "$DEST_PARENT" ] || DEST_PARENT="$HOME"
fi

# Derive "<parent>/<repo-name>" from the URL's last path segment.
name=$(basename "$REPO_URL")
case "$name" in *.git) name=$(basename "$name" .git) ;; esac
DEST="$DEST_PARENT/$name"

# 4. Clone over HTTPS. (Public repos clone anonymously; for a private repo or to
#    push later, authenticate with the host CLI — e.g. `gh auth login` — rather
#    than hand-managing SSH keys. We deliberately do not touch credentials here.)
say
say "Cloning into: $DEST"
if [ -e "$DEST" ]; then
  say "That folder already exists — using it as-is (skipping clone)."
else
  git clone "$REPO_URL" "$DEST"
fi

# 5. End banner: name the checkout dir + the agent handoff, then offer dev-setup.
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
