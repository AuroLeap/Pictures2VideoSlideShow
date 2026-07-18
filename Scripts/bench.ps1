<#
.SYNOPSIS
  Canonical performance-bench runner (SR-036, LLR-052; TC-074): builds the
  release binary, runs `bench --full` over the fixed corpus twice — a
  rotation-off leg (PB-006, the LLR-061 SIMD fast path) then the canonical
  rotation-15 1920x1080 leg (PB-001/002/003/005) — plus a third timed-build
  leg measuring the SR-037 warm-rebuild ratio (PB-004), and compares the
  merged docs/test/perf-metrics.json against performance-budgets.csv and the
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

# Leg 3 — PB-004 warm-rebuild ratio (SR-037 segment cache): a synthetic
# 240-photo album is cold-built with a fresh segment cache, 12 photos (5% —
# the PB-004 row's "+100 per ~2000-photo library" scaled to bench size) are
# added, and the timed warm rebuild re-encodes only the new clips + neighbor.
# PB-004 = warm wall / cold wall * 100 (timed `build` invocations, end-user
# experienced wall incl. scan and concat). Owned here by the wrapper like
# PB-006; the binary never emits PB-004. Short pic/fade times keep the leg
# ~2 min; the ratio, not the absolute time, is the metric. The cache and the
# corpus live under the OS temp dir — the user's real per-user cache is never
# touched (temp_dir pins the store root).
$pb004Pool   = Join-Path ([IO.Path]::GetTempPath()) 'slideshow_bench_pb004_pool'
$pb004Corpus = Join-Path ([IO.Path]::GetTempPath()) 'slideshow_bench_pb004_corpus'
$pb004Cache  = Join-Path ([IO.Path]::GetTempPath()) 'slideshow_bench_pb004_cache'
$pb004Out    = Join-Path ([IO.Path]::GetTempPath()) 'slideshow_bench_pb004_out'
$baseCount = 240; $addCount = 12
if (-not (Test-Path $pb004Pool) -or ((Get-ChildItem $pb004Pool -Filter *.png).Count -lt ($baseCount + $addCount))) {
    Write-Host '==> synthesizing PB-004 corpus pool (one-time, deterministic testsrc frames)' -ForegroundColor Cyan
    New-Item -ItemType Directory -Force $pb004Pool | Out-Null
    ffmpeg -y -v error -f lavfi -i "testsrc=size=960x720:rate=30" -frames:v ($baseCount + $addCount) `
        (Join-Path $pb004Pool 'img_%04d.png')
    if ($LASTEXITCODE) { throw 'PB-004 corpus synthesis failed (ffmpeg on PATH?)' }
}
Remove-Item -Recurse -Force $pb004Corpus, $pb004Cache, $pb004Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $pb004Corpus | Out-Null
$pool = Get-ChildItem $pb004Pool -Filter *.png | Sort-Object Name
$pool | Select-Object -First $baseCount | Copy-Item -Destination $pb004Corpus

$pb004ConfigPath = Join-Path ([IO.Path]::GetTempPath()) 'slideshow_bench_config_pb004.toml'
@"
[input]
media_root = "$($pb004Corpus -replace '\\','/')"
ignore_patterns = []

[output]
base_dir = "$($pb004Out -replace '\\','/')"

[processing]
temp_dir = "$($pb004Cache -replace '\\','/')"

[[outputs]]
name = "pb004"
width = 1920
height = 1080
fps = $Fps
pic_display_time_secs = 1.0
fade_time_secs = 0.25
max_rotation_degrees = 15.0
bulk_video_time_min = 20
quality_crf = 28
enable_audio = false
"@ | Set-Content -Encoding UTF8 $pb004ConfigPath

Write-Host "==> PB-004 cold build ($baseCount photos, fresh cache)" -ForegroundColor Cyan
$cold = Measure-Command { & $exe --config $pb004ConfigPath --non-interactive build | Out-Host }
if ($LASTEXITCODE) { throw 'PB-004 cold build failed' }
$pool | Select-Object -Last $addCount | Copy-Item -Destination $pb004Corpus
Write-Host "==> PB-004 warm rebuild (+$addCount photos)" -ForegroundColor Cyan
$warm = Measure-Command { & $exe --config $pb004ConfigPath --non-interactive build | Out-Host }
if ($LASTEXITCODE) { throw 'PB-004 warm rebuild failed' }
$pb004 = 100.0 * $warm.TotalSeconds / [Math]::Max($cold.TotalSeconds, 0.001)
Remove-Item -Recurse -Force $pb004Corpus, $pb004Cache, $pb004Out -ErrorAction SilentlyContinue

$metrics = Get-Content $metricsPath -Raw | ConvertFrom-Json
$metrics | Add-Member -NotePropertyName 'PB-004' -NotePropertyValue ([Math]::Round($pb004, 2)) -Force
$metrics | ConvertTo-Json | Set-Content -Encoding UTF8 $metricsPath
Write-Host ("PB-004 warm-rebuild ratio: {0:N1}% (cold {1:N1}s, warm +{2} photos {3:N1}s)" -f `
    $pb004, $cold.TotalSeconds, $addCount, $warm.TotalSeconds)

Write-Host '==> check_perf (budgets + baseline comparison)' -ForegroundColor Cyan
python (Join-Path $PSScriptRoot 'check_perf.py') --tier release
if ($LASTEXITCODE) { throw 'check_perf reported a hard-gated breach' }

Write-Host 'Bench complete. Metrics: docs/test/perf-metrics.json; report: docs/test/perf-report.md' -ForegroundColor Green
