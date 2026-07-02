# One-shot dev setup for Windows (PowerShell). Makes a fresh clone runnable.
# Idempotent. Edit the dependency list for your project; the reference installs
# the tools the Python check harness uses. Linux/macOS: use scripts/setup.sh.
$ErrorActionPreference = "Stop"
# Push/Pop so running the script doesn't leave the caller's shell cd'd here.
Push-Location (Join-Path $PSScriptRoot "..")
try {
    # Find a Python launcher.
    $py = $null
    foreach ($cand in @("py", "python", "python3")) {
        if (Get-Command $cand -ErrorAction SilentlyContinue) { $py = $cand; break }
    }
    if (-not $py) { Write-Error "Python 3 not found on PATH."; exit 1 }
    Write-Host "Using $(& $py --version)"

    # Create a local virtualenv so installs don't touch the system Python.
    if (-not (Test-Path ".venv")) {
        Write-Host "Creating .venv ..."
        & $py -m venv .venv
    }
    $python = Join-Path ".venv" "Scripts\python.exe"

    & $python -m pip install --upgrade pip
    # --- Edit below for your stack --------------------------------------------
    & $python -m pip install ruff pytest pytest-cov
    if (Test-Path "pyproject.toml") { & $python -m pip install -e . }
    elseif (Test-Path "requirements.txt") { & $python -m pip install -r requirements.txt }
    # ---------------------------------------------------------------------------

    # Enable the agent-neutral pre-commit hook (the process floor) if this is a
    # git repo. Opt-in + reversible: undo with `git config --unset core.hooksPath`.
    if (Test-Path ".githooks/pre-commit") {
        try {
            git rev-parse --is-inside-work-tree 2>$null | Out-Null
            if ($LASTEXITCODE -eq 0) {
                git config core.hooksPath .githooks
                Write-Host "Enabled pre-commit hook (core.hooksPath=.githooks; undo: git config --unset core.hooksPath)."
            }
        }
        catch {
            # Not a git repo yet (or git missing) — the hook is opt-in; skip quietly.
        }
    }

    Write-Host ""
    Write-Host "Setup complete. Run the harness with: .\scripts\check.ps1 --gate G3"
    Write-Host "(check.ps1 uses the venv python directly; no activation needed.)"
}
finally {
    Pop-Location
}
