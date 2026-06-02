<#
.SYNOPSIS
  Local test/quality harness for the Rust engine. Mirrors the GitHub Action
  (.github/workflows/ci.yml). Used by Objective 3's gate (see docs/process.md).

.DESCRIPTION
  Runs: format check, clippy, unit+integration tests, optional coverage, and the
  traceability report. Exits non-zero if any required step fails.
#>
[CmdletBinding()]
param(
    [int]$CoverageThreshold = 80,
    [switch]$SkipCoverage
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# Ensure cargo is reachable even when not on the session PATH (see project memory).
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }

$failures = @()
function Step($name, [scriptblock]$body) {
    Write-Host "==> $name" -ForegroundColor Cyan
    try { & $body; Write-Host "    OK: $name" -ForegroundColor Green }
    catch { $script:failures += $name; Write-Host "    FAIL: $name -- $_" -ForegroundColor Red }
}

Step 'cargo fmt --check' { cargo fmt --all -- --check; if ($LASTEXITCODE) { throw "fmt" } }
Step 'cargo clippy (deny warnings)' { cargo clippy --all-targets -- -D warnings; if ($LASTEXITCODE) { throw "clippy" } }
Step 'cargo test' { cargo test --all; if ($LASTEXITCODE) { throw "test" } }

if (-not $SkipCoverage) {
    $hasLlvmCov = (cargo llvm-cov --version 2>$null)
    if ($hasLlvmCov) {
        Step "coverage >= $CoverageThreshold%" {
            cargo llvm-cov --summary-only --fail-under-lines $CoverageThreshold
            if ($LASTEXITCODE) { throw "coverage below $CoverageThreshold%" }
        }
    } else {
        Write-Host "    SKIP: coverage (install with 'cargo install cargo-llvm-cov')" -ForegroundColor Yellow
    }
}

# Traceability / requirement coverage report (non-fatal here; Objective 2 gate enforces 0 orphans).
Step 'traceability report' { & (Join-Path $PSScriptRoot 'trace.ps1') }

Write-Host ""
if ($failures.Count) {
    Write-Host "HARNESS FAILED: $($failures -join ', ')" -ForegroundColor Red
    exit 1
}
Write-Host "HARNESS PASSED" -ForegroundColor Green
exit 0
