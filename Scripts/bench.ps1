<#
.SYNOPSIS
  Canonical performance-bench runner (SR-036, LLR-052; TC-074/TC-105): builds
  the release binary, runs `bench --full` over the fixed corpus three times —
  a rotation-off leg (PB-006, the LLR-061 SIMD fast path), a render_backend=gpu
  rotation-15 leg (PB-007, the SR-039 GPU renderer), then the canonical
  rotation-15 1920x1080 leg (PB-001/002/003/005) — plus a fourth timed-build
  leg measuring the SR-037 warm-rebuild ratio (PB-004), and compares the
  merged docs/test/perf-metrics.json against performance-budgets.csv and the
  committed baseline via Scripts/check_perf.py.

.DESCRIPTION
  This wrapper — not an ad-hoc `bench --full` with a user config — is the
  documented producer of PB-comparable numbers: it pins the output definition
  (resolution/fps/crf/fade) so runs differ only by code and hardware. Record
  the resulting numbers plus the host hardware in docs/status.md per plan §2.

  One variable per leg pair: the rotation-off leg differs from the canonical
  leg ONLY in `max_rotation_degrees` (0.0 vs the canonical 15.0) — it exists
  because rotation 15 never exercises the LLR-061 axis-aligned SIMD resize
  path, so PB-006 is that path's regression guard (Round-4b MINOR finding).
  The gpu leg differs from the canonical leg ONLY in `render_backend`
  ("gpu" vs the pinned "cpu"), so PB-007 vs PB-001 isolates exactly the
  SR-039 render-backend change. The canonical leg keeps PB-001 comparable
  with every prior baseline.

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
    [switch]$SkipBuild,
    # Skip the quiet-host gate and measure immediately regardless of load.
    [switch]$SkipQuietWait,
    # Max minutes to wait for the host to go quiet before measuring anyway.
    [int]$QuietTimeoutMin = 30,
    # Total-CPU percentage below which the host counts as quiet.
    [int]$QuietLoadPct = 20
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
# across runs; only the Ken Burns rotation and the render backend differ
# between legs. media_root is a placeholder — bench --full resolves the
# corpus itself (explicit --corpus, else TestInput/, else synthesized).
# render_backend defaults to "cpu" HERE (not the config-file default "auto"):
# since Round 8 `auto` silently selects the GPU renderer on a GPU host, which
# would poison PB-001/002/005/006 comparability with every pre-GPU baseline —
# the gpu leg (PB-007) is the one deliberate exception. The encoder is left at
# its config default ("software", libx264) on every leg, so the gpu leg stays
# single-variable vs the canonical leg.
function New-BenchConfig([string]$Name, [double]$RotationDegrees, [string]$RenderBackend = 'cpu') {
    $path = Join-Path ([IO.Path]::GetTempPath()) "slideshow_bench_config_$Name.toml"
    @"
[input]
media_root = "TestInput"
ignore_patterns = ["DNP"]

[output]
base_dir = "TestOut/BenchOut"

[processing]
render_backend = "$RenderBackend"

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

# Quiet-host gate: the fps rows (PB-001/PB-006) are only comparable when no
# other process holds the CPU — a Defender scan or game skews them 10%+
# (Round 6/6b evidence). Two consecutive quiet samples 30 s apart are required
# before the measured legs run; on timeout the bench proceeds with a loud
# warning so an unattended run still completes (treat those fps rows as
# load-suspect). Runs AFTER the release build so our own compile load — and
# any scan it triggers — has finished before sampling starts.
function Wait-QuietHost {
    if ($SkipQuietWait) {
        Write-Host '==> quiet-host gate skipped (-SkipQuietWait)' -ForegroundColor Yellow
        return
    }
    Write-Host "==> quiet-host gate (total CPU < $QuietLoadPct% twice, 30s apart; timeout $QuietTimeoutMin min)" -ForegroundColor Cyan
    $consecutive = 0
    $deadline = (Get-Date).AddMinutes($QuietTimeoutMin)
    while ($true) {
        $load = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
        if ($load -lt $QuietLoadPct) {
            $consecutive++
            Write-Host ("    load {0:N0}% - quiet ({1}/2)" -f $load, $consecutive)
            if ($consecutive -ge 2) { return }
            $sleep = 30
        } else {
            $consecutive = 0
            # Name the busiest process so the operator knows what to wait out.
            $busy = ''
            try {
                $top = (Get-Counter '\Process(*)\% Processor Time' -ErrorAction Stop).CounterSamples |
                    Where-Object { $_.InstanceName -notin '_total', 'idle' } |
                    Sort-Object CookedValue -Descending | Select-Object -First 1
                if ($top) { $busy = " (busiest: $($top.InstanceName) ~$([Math]::Round($top.CookedValue,0))% of one core)" }
            } catch {}
            Write-Host ("    load {0:N0}% - waiting{1}" -f $load, $busy)
            $sleep = 60
        }
        if ((Get-Date) -gt $deadline) {
            Write-Host "    WARNING: host never went quiet within $QuietTimeoutMin min - benching anyway; fps rows are load-suspect" -ForegroundColor Yellow
            return
        }
        Start-Sleep -Seconds $sleep
    }
}
Wait-QuietHost

function Invoke-BenchLeg([string]$ConfigPath, [string]$Label, [string]$ExpectPattern) {
    Write-Host "==> bench --full ($Label)" -ForegroundColor Cyan
    $benchArgs = @('--config', $ConfigPath, '--non-interactive', 'bench', '--full')
    if ($Corpus) { $benchArgs += @('--corpus', $Corpus) }
    if ($ExpectPattern) {
        # Capture (while still displaying) so the leg can verify the backend
        # actually in use — a silent fallback would mislabel the PB row.
        & $exe @benchArgs 2>&1 | Tee-Object -Variable legOut | Out-Host
        if ($LASTEXITCODE) { throw "bench --full ($Label) failed (exit $LASTEXITCODE)" }
        if (-not ($legOut -match $ExpectPattern)) {
            Write-Host "    WARNING: '$Label' never logged '$ExpectPattern' - its PB row measured the fallback path; do not accept it as a baseline" -ForegroundColor Yellow
        }
    } else {
        & $exe @benchArgs
        if ($LASTEXITCODE) { throw "bench --full ($Label) failed (exit $LASTEXITCODE)" }
    }
}

$metricsPath = Join-Path $repo 'docs\test\perf-metrics.json'

# Leg 1 — rotation off: the only leg where the LLR-061 SIMD fast path fires.
# Runs first; its end-to-end fps is captured as PB-006 before the canonical
# leg overwrites perf-metrics.json (the binary only emits the canonical keys —
# the two-leg protocol and the PB-006 key are owned here, by the wrapper).
Invoke-BenchLeg (New-BenchConfig 'bench-1920x1080-rot0' 0.0) 'rotation-off leg, PB-006'
$pb006 = (Get-Content $metricsPath -Raw | ConvertFrom-Json).'PB-001'

# Leg 2 — gpu rotation-15 leg (PB-007, SR-039/TC-105): identical to the
# canonical leg except render_backend="gpu" — the encoder stays the config
# default (software libx264), so PB-007 vs PB-001 isolates exactly the render
# change. Its end-to-end fps is captured as PB-007 before the canonical leg
# overwrites perf-metrics.json (wrapper-owned key, like PB-006). On an
# adapter-less host the build falls back to CPU (SR-039 never-fail clause);
# the ExpectPattern check warns loudly so a fallback run is never mistaken
# for a GPU measurement.
Invoke-BenchLeg (New-BenchConfig 'bench-1920x1080-gpu' 15.0 'gpu') 'gpu rotation-15 leg, PB-007' 'rendering with gpu'
$pb007 = (Get-Content $metricsPath -Raw | ConvertFrom-Json).'PB-001'

# Leg 3 — canonical rotation-15 leg: the PB-001/002/003/005 producer,
# comparable with every prior baseline. Runs last of the fps legs so its
# numbers are the ones left authoritative in perf-metrics.json.
Invoke-BenchLeg (New-BenchConfig 'bench-1920x1080' 15.0) 'canonical leg'

# Fold the rotation-off and gpu throughputs back in as PB-006/PB-007.
$metrics = Get-Content $metricsPath -Raw | ConvertFrom-Json
$metrics | Add-Member -NotePropertyName 'PB-006' -NotePropertyValue $pb006 -Force
$metrics | Add-Member -NotePropertyName 'PB-007' -NotePropertyValue $pb007 -Force
$metrics | ConvertTo-Json | Set-Content -Encoding UTF8 $metricsPath
Write-Host ("PB-006 rotation-off end-to-end frames/s: {0:N1}" -f $pb006)
Write-Host ("PB-007 gpu-render end-to-end frames/s: {0:N1}" -f $pb007)

# Leg 4 — PB-004 warm-rebuild ratio (SR-037 segment cache): a synthetic
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
# Pinned cpu for baseline comparability (see New-BenchConfig note).
render_backend = "cpu"

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
