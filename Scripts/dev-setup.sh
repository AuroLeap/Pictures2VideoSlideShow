#!/usr/bin/env sh
# dev-setup (Linux/macOS) — provision the *developer workstation* rung of the
# onboarding ladder (process.md §7):
#
#     Stage 0  ->  dev-setup  ->  setup  ->  check
#                  (this)         deps       gates
#
# This readies what a *human* needs to view, render, edit, and run this repo —
# a runtime, git, and an OFFLINE Markdown+Mermaid renderer — which is distinct
# from `setup.sh` (that installs the *product* toolchain: venv, ruff, pytest).
# Separating them is the point: "no required tools" was always a claim about the
# stdlib *process* checks, never about what a person needs at a workstation.
#
# Consent-first and readable by design: the DEFAULT tier only detects and
# reports; it installs nothing. Nothing here pipes a remote script to a shell.
#
# Usage:  sh dev-setup.sh [--check|--baseline|--full] [--profile code|domain]
#   --check     (default) detect + report what's present; install nothing.
#   --baseline  ensure runtime + git + an offline Mermaid renderer (+ point at
#               `setup.sh` for the product test toolchain). Asks before each install.
#   --full      baseline + an IDE and editor extensions. Opt-in, and skipped when
#               headless / non-interactive (no TTY or $CI set).
#   --profile code    (default) a code contributor: runtime + linter + test tools.
#   --profile domain  a non-code contributor (art/UI, CAD, electronics, docs):
#                     git + offline renderer + a DOMAIN VIEWER you fill in below.
#
# Windows contributors: use scripts/dev-setup.ps1.
set -eu

# =================== EDIT FOR YOUR STACK / DOMAIN ===========================
# The template ships only the universal baseline detectors and EMPTY install
# slots — fill the commands for your OS/stack. Leave a slot empty to skip its
# install (detection/reporting still works). Debian/Ubuntu shown for reference;
# swap apt-get for brew/dnf/pacman as needed.
#
#   RUNTIME_INSTALL="sudo apt-get install -y python3"
#   RENDERER_INSTALL="npm install -g @mermaid-js/mermaid-cli"   # or install VS Code + a Mermaid preview extension
#   IDE_INSTALL="sudo snap install code --classic"
#   DOMAIN_VIEWER_INSTALL="sudo apt-get install -y inkscape"    # your domain's viewer(s)
RUNTIME_INSTALL=""
RENDERER_INSTALL=""
IDE_INSTALL=""
DOMAIN_VIEWER_INSTALL=""
# Space-separated commands that, if present, mean the domain viewer is installed
# (used by --check for the domain profile), e.g. "inkscape kicad".
DOMAIN_VIEWER_CMDS=""
# ===========================================================================

TIER="check"
PROFILE="code"
while [ $# -gt 0 ]; do
  case "$1" in
    --check)    TIER="check" ;;
    --baseline) TIER="baseline" ;;
    --full)     TIER="full" ;;
    --profile)  shift; PROFILE="${1:-code}" ;;
    --profile=*) PROFILE="${1#*=}" ;;
    -h|--help)  sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

have() { command -v "$1" >/dev/null 2>&1; }
say()  { printf '%s\n' "$*"; }

# Interactive only when there is a TTY and we are not in CI — so --full never
# blocks an automated run waiting on a prompt.
interactive() { [ -t 0 ] && [ -z "${CI:-}" ]; }

# Report one component: name, whether a satisfying command is present, and a hint.
missing=0
report() { # <label> <present:0/1> <hint>
  if [ "$2" -eq 1 ]; then
    say "  [ok]      $1"
  else
    say "  [missing] $1  — $3"
    missing=$((missing + 1))
  fi
}

# Consent-first install: show the command, ask (default no), run it. A blank
# command is treated as "not configured for this stack" and skipped.
maybe_install() { # <label> <install-cmd>
  if [ -z "$2" ]; then
    say "  - $1: no install command configured (see EDIT FOR YOUR STACK); skipping."
    return 0
  fi
  if ! interactive; then
    say "  - $1: non-interactive; skipping install ($2)"
    return 0
  fi
  printf '  Install %s via "%s"? [y/N] ' "$1" "$2"
  read -r ans
  case "$ans" in [Yy]*) sh -c "$2" ;; *) say "  - skipped $1" ;; esac
}

renderer_present() { have code || have mmdc || have npx; }

say "dev-setup — profile=$PROFILE tier=$TIER"
say "Developer workstation (process.md §7). Product deps are scripts/setup.sh."
say

# --- Detect + report (every tier does this first) ----------------------------
if have python3; then RUNTIME=1; elif have python; then RUNTIME=1; else RUNTIME=0; fi
report "runtime (python3)"          "$RUNTIME"                        "install a Python 3.8+ runtime"
report "git"                        "$(have git && echo 1 || echo 0)" "install git (needed to make reviewable changes)"
report "offline Markdown+Mermaid renderer" \
       "$(renderer_present && echo 1 || echo 0)" \
       "VS Code + a Mermaid preview extension, or: npm i -g @mermaid-js/mermaid-cli"

if [ "$PROFILE" = "domain" ]; then
  dv=0
  for c in $DOMAIN_VIEWER_CMDS; do have "$c" && dv=1; done
  report "domain viewer" "$dv" "fill DOMAIN_VIEWER_CMDS/INSTALL in the EDIT block for your artifacts"
fi
if [ "$TIER" = "full" ]; then
  report "IDE (VS Code 'code')" "$(have code && echo 1 || echo 0)" "install an editor; run again with --full to add extensions"
fi

say
if [ -d .venv ]; then
  say "Product toolchain: .venv present (run scripts/setup.sh to refresh)."
else
  say "Product toolchain: run scripts/setup.sh to create .venv + install test tools."
fi

# --- --check stops here: pure report, always green ---------------------------
if [ "$TIER" = "check" ]; then
  say
  say "$missing component(s) missing. Install them with: sh $0 --baseline"
  exit 0
fi

# --- --baseline / --full: consent-first installs -----------------------------
say
say "Installing the $TIER workstation (asks before each step)…"
[ "$RUNTIME" -eq 1 ] || maybe_install "runtime" "$RUNTIME_INSTALL"
renderer_present     || maybe_install "offline Mermaid renderer" "$RENDERER_INSTALL"
if [ "$PROFILE" = "domain" ]; then
  maybe_install "domain viewer" "$DOMAIN_VIEWER_INSTALL"
fi
if [ "$TIER" = "full" ]; then
  if interactive; then
    maybe_install "IDE" "$IDE_INSTALL"
  else
    say "  - IDE: headless/non-interactive; skipped (opt-in, --full only)."
  fi
fi

say
say "Done. Next: run scripts/setup.sh for the product toolchain, then"
say "./scripts/check.sh --gate G3 to run the gates."
