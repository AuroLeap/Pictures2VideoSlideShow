# Thin launcher for the check harness on Windows (PowerShell). Prefers the
# project venv, then any Python on PATH. All arguments pass through to check.py,
# e.g.:  .\scripts\check.ps1 --gate G3 --tier smoke
$ErrorActionPreference = "Stop"
# Push/Pop so running the script doesn't leave the caller's shell cd'd here.
Push-Location (Join-Path $PSScriptRoot "..")
try {
    $venvPy = Join-Path ".venv" "Scripts\python.exe"
    if (Test-Path $venvPy) {
        $python = $venvPy
    } else {
        $python = $null
        foreach ($cand in @("py", "python", "python3")) {
            if (Get-Command $cand -ErrorAction SilentlyContinue) { $python = $cand; break }
        }
        if (-not $python) {
            Write-Error "Python 3 not found. Run .\scripts\setup.ps1 first."; exit 1
        }
    }

    & $python scripts/check.py @args
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
