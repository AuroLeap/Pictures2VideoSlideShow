<#
.SYNOPSIS
  Canonical performance-bench runner (SR-036, LLR-052; TC-074): builds the
  release binary, runs `bench --full` over the fixed corpus twice — a
  rotation-off leg (PB-006, the LLR-061 SIMD fast path) then the canonical
  rotation-15 1920x1080 leg (PB-001/002/003/005) — and compares the merged
  docs/test/perf-metrics.json against performance-budgets.csv and the
  committed baseline via Scripts/check_perf.py.

.DESCRIPTION
  This wrapper — not an ad-hoc `bench --full` with a user config — is the
  documented producer of PB-comparable numbers: it pins the output definition
  (resolution/fps/crf/fade) so runs differ only by code and hardware. Record
  the resulting numbers plus the host hardware in docs/status.md per plan §2.

  Two legs, one variable: the legs differ ONLY in `max_rotation_degrees`
  (0.0 vs the canonical 15.0). The canonical leg keeps PB-001 comparable with
  every prior baseline; the rotation-off leg exists because rotation 15 never
  exercises the LLR-061 axis-aligned SIMD resize path, so PB-006 is that
  path's regression guard (Round-4b MINOR finding).

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

# Pinned bench config: fixed 1920x1080 output so PB numbers are comparable
# across runs; only the Ken Burns rotation differs between the two legs.
# media_root is a placeholder — bench --full resolves the corpus itself
# (explicit --corpus, else TestInput/, else synthesized).
function New-BenchConfig([string]$Name, [double]$RotationDegrees) {
    $path = Join-Path ([IO.Path]::GetTempPath()) "slideshow_bench_config_$Name.toml"
    @"
[input]
media_root = "TestInput"
ignore_patterns = ["DNP"]

[output]
base_dir = "TestOut/BenchOut"

[processing]

[[outputs]]
name = "$Name"
width = 1920
height = 1080
fps = $Fps
pic_display_time_secs = 6.0
fade_time_secs = 0.5
max_rotation_degrees = $RotationDegrees
bulk_video_time_min = 20
quality_crf = 28
enable_audio = false
"@ | Set-Content -Encoding UTF8 $path
    $path
}

$exe = Join-Path $repo 'target\release\make_video_slideshow.exe'
if (-not (Test-Path $exe)) { throw "release binary not found: $exe (run without -SkipBuild)" }

function Invoke-BenchLeg([string]$ConfigPath, [string]$Label) {
    Write-Host "==> bench --full ($Label)" -ForegroundColor Cyan
    $benchArgs = @('--config', $ConfigPath, '--non-interactive', 'bench', '--full')
    if ($Corpus) { $benchArgs += @('--corpus', $Corpus) }
    & $exe @benchArgs
    if ($LASTEXITCODE) { throw "bench --full ($Label) failed (exit $LASTEXITCODE)" }
}

$metricsPath = Join-Path $repo 'docs\test\perf-metrics.json'

# Leg 1 — rotation off: the only leg where the LLR-061 SIMD fast path fires.
# Runs first; its end-to-end fps is captured as PB-006 before the canonical
# leg overwrites perf-metrics.json (the binary only emits the canonical keys —
# the two-leg protocol and the PB-006 key are owned here, by the wrapper).
Invoke-BenchLeg (New-BenchConfig 'bench-1920x1080-rot0' 0.0) 'rotation-off leg, PB-006'
$pb006 = (Get-Content $metricsPath -Raw | ConvertFrom-Json).'PB-001'

# Leg 2 — canonical rotation-15 leg: the PB-001/002/003/005 producer,
# comparable with every prior baseline.
Invoke-BenchLeg (New-BenchConfig 'bench-1920x1080' 15.0) 'canonical leg'

# Fold the rotation-off throughput back in as PB-006.
$metrics = Get-Content $metricsPath -Raw | ConvertFrom-Json
$metrics | Add-Member -NotePropertyName 'PB-006' -NotePropertyValue $pb006 -Force
$metrics | ConvertTo-Json | Set-Content -Encoding UTF8 $metricsPath
Write-Host ("PB-006 rotation-off end-to-end frames/s: {0:N1}" -f $pb006)

Write-Host '==> check_perf (budgets + baseline comparison)' -ForegroundColor Cyan
python (Join-Path $PSScriptRoot 'check_perf.py') --tier release
if ($LASTEXITCODE) { throw 'check_perf reported a hard-gated breach' }

Write-Host 'Bench complete. Metrics: docs/test/perf-metrics.json; report: docs/test/perf-report.md' -ForegroundColor Green
