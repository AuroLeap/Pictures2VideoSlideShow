# dev-setup (Windows) — provision the *developer workstation* rung of the
# onboarding ladder (process.md §7):
#
#     Stage 0  ->  dev-setup  ->  setup  ->  check
#                  (this)         deps       gates
#
# This readies what a *human* needs to view, render, edit, and run this repo —
# a runtime, git, and an OFFLINE Markdown+Mermaid renderer — distinct from
# setup.ps1 (that installs the *product* toolchain: venv, ruff, pytest).
#
# Consent-first and readable: the DEFAULT tier only detects and reports; it
# installs nothing. Nothing here pipes a remote script into a shell.
#
# Usage:  powershell -ExecutionPolicy Bypass -File dev-setup.ps1 [-Check|-Baseline|-Full] [-Profile code|domain]
#   -Check     (default) detect + report what's present; install nothing.
#   -Baseline  ensure runtime + git + an offline Mermaid renderer (+ point at
#              setup.ps1 for the product test toolchain). Asks before each install.
#   -Full      baseline + an IDE and editor extensions. Opt-in; skipped headless.
#   -Profile   code (default): runtime + linter + test tools.
#              domain: git + offline renderer + a DOMAIN VIEWER you fill in below.
#
# Linux/macOS contributors: use scripts/dev-setup.sh.
param(
    [switch]$Check,
    [switch]$Baseline,
    [switch]$Full,
    [ValidateSet("code", "domain")][string]$Profile = "code"
)
$ErrorActionPreference = "Stop"

# =================== EDIT FOR YOUR STACK / DOMAIN ===========================
# The template ships only the universal baseline detectors and EMPTY install
# slots — fill the commands for your stack. Leave a slot empty to skip its
# install (detection/reporting still works). winget shown for reference.
#
#   $RuntimeInstall = "winget install --id Python.Python.3.12 -e"
#   $RendererInstall = "npm install -g @mermaid-js/mermaid-cli"   # or install VS Code + a Mermaid preview extension
#   $IdeInstall = "winget install --id Microsoft.VisualStudioCode -e"
#   $DomainViewerInstall = "winget install --id Inkscape.Inkscape -e"
$RuntimeInstall = ""
$RendererInstall = ""
$IdeInstall = ""
$DomainViewerInstall = ""
# Commands that, if present, mean the domain viewer is installed (used by -Check
# for the domain profile), e.g. @("inkscape", "kicad").
$DomainViewerCmds = @()
# ===========================================================================

$tier = if ($Full) { "full" } elseif ($Baseline) { "baseline" } else { "check" }

function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
# Interactive only with a real console and outside CI, so -Full never blocks
# an automated run on a prompt.
function Interactive { [Environment]::UserInteractive -and -not $env:CI }
function RendererPresent { (Have "code") -or (Have "mmdc") -or (Have "npx") }

$script:missing = 0
function Report($label, $present, $hint) {
    if ($present) { Write-Host "  [ok]      $label" }
    else { Write-Host "  [missing] $label  — $hint"; $script:missing++ }
}
function MaybeInstall($label, $cmd) {
    if (-not $cmd) {
        Write-Host "  - $label: no install command configured (see EDIT FOR YOUR STACK); skipping."
        return
    }
    if (-not (Interactive)) { Write-Host "  - $label: non-interactive; skipping install ($cmd)"; return }
    $ans = Read-Host "  Install $label via `"$cmd`"? [y/N]"
    if ($ans -match '^[Yy]') { Invoke-Expression $cmd } else { Write-Host "  - skipped $label" }
}

Write-Host "dev-setup — profile=$Profile tier=$tier"
Write-Host "Developer workstation (process.md §7). Product deps are scripts/setup.ps1."
Write-Host ""

# --- Detect + report ---------------------------------------------------------
$runtime = (Have "py") -or (Have "python") -or (Have "python3")
Report "runtime (python)" $runtime "install a Python 3.8+ runtime"
Report "git" (Have "git") "install git (needed to make reviewable changes)"
Report "offline Markdown+Mermaid renderer" (RendererPresent) `
    "VS Code + a Mermaid preview extension, or: npm i -g @mermaid-js/mermaid-cli"

if ($Profile -eq "domain") {
    $dv = $false
    foreach ($c in $DomainViewerCmds) { if (Have $c) { $dv = $true } }
    Report "domain viewer" $dv "fill `$DomainViewerCmds/`$DomainViewerInstall in the EDIT block"
}
if ($tier -eq "full") {
    Report "IDE (VS Code 'code')" (Have "code") "install an editor; run again with -Full to add extensions"
}

Write-Host ""
if (Test-Path ".venv") { Write-Host "Product toolchain: .venv present (run scripts/setup.ps1 to refresh)." }
else { Write-Host "Product toolchain: run scripts/setup.ps1 to create .venv + install test tools." }

# --- -Check stops here: pure report, always green ----------------------------
if ($tier -eq "check") {
    Write-Host ""
    Write-Host "$script:missing component(s) missing. Install them with: dev-setup.ps1 -Baseline"
    exit 0
}

# --- -Baseline / -Full: consent-first installs -------------------------------
Write-Host ""
Write-Host "Installing the $tier workstation (asks before each step)…"
if (-not $runtime) { MaybeInstall "runtime" $RuntimeInstall }
if (-not (RendererPresent)) { MaybeInstall "offline Mermaid renderer" $RendererInstall }
if ($Profile -eq "domain") { MaybeInstall "domain viewer" $DomainViewerInstall }
if ($tier -eq "full") {
    if (Interactive) { MaybeInstall "IDE" $IdeInstall }
    else { Write-Host "  - IDE: headless/non-interactive; skipped (opt-in, -Full only)." }
}

Write-Host ""
Write-Host "Done. Next: run scripts/setup.ps1 for the product toolchain, then"
Write-Host ".\scripts\check.ps1 --gate G3 to run the gates."
