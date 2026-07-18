<#
.SYNOPSIS
  Canonical performance-bench runner (SR-036, LLR-052; TC-074): builds the
  release binary, runs `bench --full` over the fixed corpus with the canonical
  1920x1080 bench output (PB-001 is defined at 1080p), then compares the
  emitted docs/test/perf-metrics.json against performance-budgets.csv and the
  committed baseline via Scripts/check_perf.py.

.DESCRIPTION
  This wrapper — not an ad-hoc `bench --full` with a user config — is the
  documented producer of PB-comparable numbers: it pins the output definition
  (resolution/fps/crf/fade) so runs differ only by code and hardware. Record
  the resulting numbers plus the host hardware in docs/status.md per plan §2.

  Corpus: -Corpus DIR when given, else the binary's default (TestInput/ if
  present in the repo root, else synthesized PNGs — plumbing-valid only).

.EXAMPLE
  pwsh Scripts/bench.ps1                 # build release + bench + check_perf
  pwsh Scripts/bench.ps1 -SkipBuild      # reuse the existing release binary
#>
[CmdletBinding()]
param(
    # Corpus directory override (default: TestInput/ in the repo root).
    [string]$Corpus,
    # Frame rate for the canonical bench output.
    [int]$Fps = 30,
    # Reuse an already-built release binary.
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# Ensure cargo is reachable even when not on the session PATH (see CLAUDE.md).
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }

if (-not $SkipBuild) {
    Write-Host '==> cargo build --release' -ForegroundColor Cyan
    cargo build --release
    if ($LASTEXITCODE) { throw 'cargo build --release failed' }
}

# Canonical bench config: fixed 1920x1080 output so PB numbers are comparable
# across runs. media_root is a placeholder — bench --full resolves the corpus
# itself (explicit --corpus, else TestInput/, else synthesized).
$cfg = Join-Path ([IO.Path]::GetTempPath()) 'slideshow_bench_config.toml'
@"
[input]
media_root = "TestInput"
ignore_patterns = ["DNP"]

[output]
base_dir = "TestOut/BenchOut"

[processing]

[[outputs]]
name = "bench-1920x1080"
width = 1920
height = 1080
fps = $Fps
pic_display_time_secs = 6.0
fade_time_secs = 0.5
max_rotation_degrees = 15.0
bulk_video_time_min = 20
quality_crf = 28
enable_audio = false
"@ | Set-Content -Encoding UTF8 $cfg

$exe = Join-Path $repo 'target\release\make_video_slideshow.exe'
if (-not (Test-Path $exe)) { throw "release binary not found: $exe (run without -SkipBuild)" }

Write-Host '==> bench --full' -ForegroundColor Cyan
$benchArgs = @('--config', $cfg, '--non-interactive', 'bench', '--full')
if ($Corpus) { $benchArgs += @('--corpus', $Corpus) }
& $exe @benchArgs
if ($LASTEXITCODE) { throw "bench --full failed (exit $LASTEXITCODE)" }

Write-Host '==> check_perf (budgets + baseline comparison)' -ForegroundColor Cyan
python (Join-Path $PSScriptRoot 'check_perf.py') --tier release
if ($LASTEXITCODE) { throw 'check_perf reported a hard-gated breach' }

Write-Host 'Bench complete. Metrics: docs/test/perf-metrics.json; report: docs/test/perf-report.md' -ForegroundColor Green
