# Pictures2VideoSlideShow

**Convert your photo and video collection into beautiful slideshow videos for digital picture frames with professional transitions and effects.**

Does your wife, yourself, or your significant other have thousands of pictures and videos stored away but not being enjoyed on a daily basis? This tool puts recorded media and memories into a set of videos resized and smoothly sequenced for a specific display, framerate, and quality.

**[Watch Demo Video](https://youtu.be/e9tY5a5I5o4)**

---

## Table of Contents

1. [Project Status](#project-status)
2. [Architecture Overview](#architecture-overview)
3. [Quick Start](#quick-start)
4. [Rust Engine (High-Performance Rewrite)](#rust-engine-high-performance-rewrite)
5. [Output Constraints & Limitations (Digital Picture Frames)](#output-constraints--limitations-digital-picture-frames)
6. [Performance Optimization](#performance-optimization)
7. [Configuration Guide](#configuration-guide)
8. [Troubleshooting](#troubleshooting)
9. [Contributing](#contributing)

> **Just want the commands and safe settings?** See the **[Quick Reference](docs/quick-reference.md)** — the one-page cheat-sheet for the Rust engine (commands, recommended frame settings, every config field, and current gaps). It is the single source of truth for those facts.

---

## Project Status

There are **two implementations** in this repository:

| Implementation | Branch | Status | Use it for |
|---|---|---|---|
| **PowerShell** (original) | `main` / `dev` | Feature-complete reference | Audio, multi-part splitting, battle-tested output |
| **Rust engine** (rewrite) | `rust-rewrite` | Active development, end-to-end working | **Speed** (~10-15x faster); see [Rust Engine](#rust-engine-high-performance-rewrite) |

### PowerShell Performance (baseline)
- **Test album** (20 images): ~3 hours
- **Production album** (2000 images): ~12 hours
- **Large album** (5000+ images): ~30+ hours

**Bottlenecks**: ImageMagick frame generation (60%), FFmpeg re-encoding (20-25%), file I/O (10-15%), ffprobe (5%).

### Rust Engine Performance (rust-rewrite, measured 24-core/release)
- **Frame generation**: ~230 frames/s (≈0.78 s per 1440×900 image at 180 frames)
- **Mixed 10-item album** (6 images + 4 videos): ~2,000 frames encoded in ~25 s
- **Projected 2000-image album**: ~25-30 min of frame generation vs ~12 h in PowerShell

### Recent Changes (rust-rewrite)
- Full media → frame-generation → FFmpeg pipeline implemented end-to-end
- Ken Burns pan/zoom + subtle rotation (rendered with a margin so rotation never shows black corners)
- **Cross-fade (dissolve) transitions** between every clip via a streaming mixer
- Inline video playback (videos decoded, cover-fit, fps-resampled, and dissolved like images)
- Parallel media scan, real image/video metadata, `cargo test`/`clippy`/`fmt` clean

---

## Architecture Overview

### Current Workflow

```
Input Media (NAS/Local)
    ↓
Copy to Local/Prep (CopyMediaFromNetwork2Local.psm1)
    ↓
Convert Images to Video (PrepareMediaForDisplay.psm1)
    ├─ ImageMagick: Generate frames with SRT transforms
    ├─ ImageMagick: Apply zoom/rotation/perspective
    └─ FFmpeg: Stitch frames → video
    ↓
Create Transitions (ConcatVidPartsFromFileList.psm1)
    ├─ FFmpeg xfade: Create fade effects
    ├─ FFmpeg: Concatenate with audio
    └─ FFmpeg: Optional re-encoding
    ↓
Output Videos (multiple resolutions/quality levels)
```

### Key Modules

| Module | Purpose | Lines |
|---|---|---|
| **BuildAlbum.ps1** | Main orchestration & user configuration | 275 |
| **PrepareMediaForDisplay.psm1** | Image to video frame generation | 1400 |
| **ConcatVidPartsFromFileList.psm1** | FFmpeg coordination & transitions | 800 |
| **PackMediaIntoVideo.psm1** | Video grouping & orchestration | 180 |
| **CopyMediaFromNetwork2Local.psm1** | Input media preparation | Variable |

### Technology Stack

- **Language**: PowerShell 7.1+
- **Image Processing**: ImageMagick CLI (`magick`)
- **Video Processing**: FFmpeg/FFprobe CLI
- **Execution Model**: Sequential CLI invocations
- **Dependencies**: RAM disk support (optional), network paths

---

## Quick Start

### Windows Setup

#### 1. Install Dependencies

**Option 1 - Automated:**
```powershell
.\InstallDependencies.ps1
```

**Option 2 - Manual:**
- [ImageMagick](https://imagemagick.org/script/download.php) - Command-line photo manipulation
- [FFmpeg](https://www.ffmpeg.org/download.html) - Command-line media manipulation
- Ensure installation paths are in system PATH

#### 2. Optional - Create RAM Disk (Recommended)
A RAM disk significantly improves frame generation speed:
- Download [AIM Toolkit](https://sourceforge.net/projects/aim-toolkit/)
- Create a RAM disk (e.g., 10-20 GB)
- Configure `$SetTmpPath` in BuildAlbum.ps1

#### 3. Configure for Your Media

Edit **BuildAlbum.ps1**:

```powershell
# (1) Set temporary path (optional - for RAM disk)
$SetTmpPath = "R:\"  # Or empty string "" for default temp

# (2) Define media to ignore
$Names2Ig = @{
    Ignore = @("DNP", "PrivateVideos")
    ExceptionParentFldrGreaterThan = 1000  # Include if parent folder # > 1000
}

# (3) Configure input/output paths
$InputFileRootPath = "\\nas\Photos"           # Source media
$PrepFileRootPath = "C:\PreparedMedia"        # Working copy
$ConvFileRootPath = ""                        # For image corrections (optional)
$OutputFilePrepend = "AlbumOut"               # Output folder prefix

# (4) Define output configurations (for each display resolution)
$OutputDefs = @(
    @{
        Name = "LivingRoom"
        XDim = 1440
        YDim = 900
        FPS = 30
        PicDispTime = 6               # Seconds per image
        MaxSrtRot = 15                # Max rotation degrees
        FadeTime = 0.5                # Fade duration in seconds
        BulkVidTimeMin = 20           # Target length per video
        NameMethod = "FldrLvl2"       # Naming scheme
        ImgVidFldr = "ImgInVid"       # Subfolder for image videos
        Quality = 30                  # CRF quality (lower = better, 18-28 typical)
        ExpAud = 0                    # Audio export (0 = disabled)
        CleanBuild = $false           # Wipe previous output
    }
)

# (5) Set to 0 to run your configuration (default is 1 for test)
$UseTestPath = 1
```

#### 4. Run

```powershell
# Run as administrator
.\BuildAlbum.ps1

# Or use the batch wrapper
.\BuildAlbum.bat
```

Output will be in: `AlbumOut1440x900q30/` (or your configured folder)

---

## Rust Engine (High-Performance Rewrite)

> **Status:** Active development on the `rust-rewrite` branch. It produces complete slideshow videos end-to-end and is roughly **10-15x faster** than the PowerShell pipeline. The PowerShell scripts remain the feature-complete reference (audio, multi-part splitting); the Rust engine is the recommended path when speed matters.

A native Rust reimplementation that renders frames directly in-process (no per-frame ImageMagick spawn) and streams them to FFmpeg, using all CPU cores via [rayon](https://docs.rs/rayon/).

### What it does today
- **Parallel media scan** — image dimensions via the `image` crate, video dimensions/duration via `ffprobe`, case-insensitive ignore patterns
- **Ken Burns pan/zoom** — smoothstep easing, direction/pan randomized but deterministic per file (reproducible output)
- **Subtle rotation** — rendered with a computed margin so a tilt never exposes black corners
- **Cross-fade (dissolve) transitions** — the tail of each clip dissolves into the head of the next over `fade_time_secs`; the first clip fades in from black and the last fades out
- **Inline video playback** — videos are decoded, cover-fit to the target resolution, resampled to the target fps, and dissolved exactly like images
- **Streaming H.264 encode** — `yuv420p`, `+faststart`; one `<name>.mp4` per output definition

### Run it (end user) — no Rust required

End users do **not** install Rust or compile anything. The product is a prebuilt `slideshow.exe` whose only runtime dependency is FFmpeg; a setup script installs/locates FFmpeg and places the binary for you. The end-user steps (run setup script → edit one config → one `build`), the commands, the example TOML config, and **every config field with its unit and default** live in one place: the **[Quick Reference §1](docs/quick-reference.md#1-end-user-setup-in-three-steps-no-rust-no-compiler)** (single source of truth; UN-001/UN-019/UN-020 → SR-001/SR-023/SR-024). `test_config.toml` in the repo root is a ready-to-edit starting point.

> The prebuilt release and setup script are **planned / TARGET state** (SR-023/SR-024) and not published yet. Until they ship, see the [Quick Reference §1 interim note](docs/quick-reference.md#1-end-user-setup-in-three-steps-no-rust-no-compiler) — build the binary once via the Developer path below, or use the PowerShell pipeline on `main`.

### Build from source (developer) — Rust

This is the **developer** path (audience split UN-021 / SR-025); end users skip it. Prerequisites: [Rust](https://rustup.rs/) (cargo) and FFmpeg on `PATH`. Full steps are in the **[Quick Reference §1a](docs/quick-reference.md#1a-developer-setup--build-from-source-rust)**. In short:

```bash
cargo build --release        # produces target/release/slideshow.exe
```

Then run the built binary (or `cargo run --release -- ...`) per [Quick Reference §2](docs/quick-reference.md#2-commands) — e.g. `slideshow.exe --config test_config.toml validate` then `build`.

### Not yet implemented in the Rust engine

Audio and `bulk_video_time_min` part-splitting are **not implemented** in the Rust engine, plus some performance items. The authoritative gap list and the PowerShell workaround for each are in the [Quick Reference §5](docs/quick-reference.md#5-rust-engine--what-is-not-implemented-yet-and-the-workaround). See the constraints below for why the splitting gap matters.

---

## Output Constraints & Limitations (Digital Picture Frames)

The whole point of this tool is to produce videos that **loop on a digital picture frame**. Those are typically inexpensive devices with real hardware and filesystem limits, so plan the output around them rather than around what your PC can play.

### 1. File size — the 4 GB FAT32 wall (most important)

Most picture frames read media from an SD card or USB stick, and many only support **FAT32**, which **cannot store a single file larger than 4 GB**. A slideshow that crosses that boundary will fail to copy or silently truncate.

- **Keep each output file under ~3.5 GB** for headroom.
- This is exactly why `bulk_video_time_min` exists: the PowerShell pipeline splits output into ~20-minute parts. **The Rust engine does not split yet** — it writes one continuous MP4 per output. The workarounds for the Rust engine (keep the album small, raise CRF, use exFAT/NTFS, or use the PowerShell pipeline) are listed in the [Quick Reference §5](docs/quick-reference.md#5-rust-engine--what-is-not-implemented-yet-and-the-workaround).

### 2. Estimating size and length

File size is driven by **bitrate × duration**, and bitrate depends on CRF, resolution, and how much motion/detail the content has. As a rule of thumb at 1080p with H.264:

| CRF | Approx. bitrate | Minutes to reach 4 GB |
|---|---|---|
| 22 (high quality) | ~12-15 Mbps | ~35-45 min |
| 26 (balanced) | ~6-9 Mbps | ~60-90 min |
| 28 (recommended) | ~4-6 Mbps | ~90-130 min |
| 30 (smaller) | ~3-4 Mbps | ~130-180 min |

Quick formula: **max minutes ≈ 32768 ÷ (bitrate in Mbps × 60)** (32768 Mb = 4 GB).

**Estimating slideshow duration** (cross-fades shorten each boundary by one `fade_time_secs`):

```
duration ≈ Σ(image clips) × (pic_display_time_secs)
         + Σ(video durations)
         − (number_of_transitions × fade_time_secs)
```

### 3. Resolution & dimensions

- **Match the frame's native panel** (common: 1024×768, 1280×800, 1440×900, 1920×1080). Encoding larger than the panel just wastes bitrate and file size.
- **Width and height must be even** — required by `yuv420p` H.264 (the engine and most frames need this).

### 4. Codec / container compatibility

- Output is **H.264 in an MP4** with `yuv420p` and `+faststart` — the most broadly compatible combination for cheap players.
- **Avoid H.265/HEVC** unless you have verified your specific frame decodes it; many do not.
- If a frame refuses a file, try a lower resolution, lower bitrate (higher CRF), and confirm 30 fps.

### 5. Frame rate

- Many panels cap at **30 fps** (some at 24/25). **60 fps is often unsupported** and roughly doubles file size for little visible benefit on a frame. Stick to **24-30 fps**.

### 6. Playback smoothness (bitrate)

- Budget frames have weak decoders; very high bitrate (very low CRF) can **stutter or drop frames**. **CRF 26-30** is a good quality/smoothness balance for frames.

### 7. Duration / responsiveness

- Some frames have a maximum per-file duration or **seek slowly** on long files. Shorter files (the ~20-minute part target) both dodge the 4 GB cap and feel more responsive.

### 8. Audio

- Many frames ignore or cannot play audio. The **Rust engine output is silent by design** for now; the PowerShell pipeline has partial audio support.

### Recommended starting point for a typical frame

The recommended values (resolution, codec, fps, CRF, size/duration caps, card format) live in the **[Quick Reference §3](docs/quick-reference.md#3-recommended-starting-point-for-a-typical-frame)** — the single source for those settings. The sections above explain *why* each value is chosen; use the Quick Reference for *what to set*.

---

## Performance Optimization

### Overview

We've identified 5 independent optimizations that can be applied incrementally. Choose your path:

| Your Goal | Effort | Improvement | Risk | Start Here |
|---|---|---|---|---|
| **I want immediate improvement** | 6-8 hours | 2-3x faster | Very Low | [Phase 1](#phase-1-quick-wins) |
| **I need maximum performance** | 80-120 hours | 4-6x faster | Medium | [Phase 2](#phase-2-c-wrapper) |
| **I want the absolute best** | 150+ hours | 8-10x faster | High | [Phase 3](#phase-3-advanced) |

### Phase 1: Quick Wins (1 Week, 2-3x Faster, Very Low Risk)

**5 Independent Optimizations** - Apply any or all of them:

#### 1. Reduce Frame Count (10 min → 30% faster)

**Current**: ~60-200 frames per image depending on display time
**Optimization**: Reduce frames based on fade time

**Implementation** (BuildAlbum.ps1, lines 53-76):
```powershell
# Reduce redundant frames for fade transitions
# Only include frames that visually change

# If fade time < 0.5s, skip every other frame
if ($FadeTime -lt 0.5) {
    $FrameCount = [Math]::Max($FrameCount - 30, 30)
}
```

**Impact**: 30% faster frame generation (primary bottleneck)

---

#### 2. Parallel Transitions (30 min → 40% faster)

**Current**: Transitions created sequentially (one at a time)
**Optimization**: Generate multiple transitions in parallel

**Implementation** (ConcatVidPartsFromFileList.psm1, lines 135-152):
```powershell
# Before: Sequential loop
foreach ($item in $VideoArray) {
    Create-TransitionVideo -Source $item
}

# After: Parallel jobs
$jobs = @()
foreach ($item in $VideoArray) {
    $jobs += Start-Job -ScriptBlock {
        Create-TransitionVideo -Source $using:item
    }
}

Wait-Job $jobs | Out-Null
```

**Impact**: 40% faster for large albums (many transitions)

---

#### 3. FFprobe Caching (1 hour → 10% faster)

**Current**: FFprobe runs for every video file independently
**Optimization**: Cache metadata in JSON file

**Create New File** (`MetadataCache.psm1`):
```powershell
function Get-CachedVideoProperties {
    param([string]$FilePath, [string]$CacheDir)

    $cacheFile = Join-Path $CacheDir "metadata.json"
    $cache = @{}
    
    if (Test-Path $cacheFile) {
        $cache = Get-Content $cacheFile | ConvertFrom-Json
    }
    
    $fileHash = (Get-FileHash $FilePath -Algorithm SHA256).Hash
    
    if ($cache.$fileHash) {
        return $cache.$fileHash
    }
    
    # Run ffprobe if not cached
    $props = & ffprobe -v error -show_streams -of json $FilePath | ConvertFrom-Json
    
    # Cache result
    $cache[$fileHash] = $props
    $cache | ConvertTo-Json | Set-Content $cacheFile
    
    return $props
}
```

**Impact**: 10% faster overall (eliminates redundant ffprobe calls)

---

#### 4. H.265 Codec (20 min → 5-10% faster + 40% smaller files)

**Current**: H.264 codec (CRF 30, slow preset)
**Optimization**: Use H.265 (HEVC) codec

**Implementation** (ConcatVidPartsFromFileList.psm1, line 278):
```powershell
# Before
-c:v libx264 -crf 30 -preset slow

# After
-c:v libx265 -crf 28 -preset slow
```

**Benefits**:
- 5-10% faster encoding
- 40% smaller output files
- Better compression quality

**Trade-off**: H.265 support varies by device (check your frame before deploying). **For digital picture frames the default guidance is to stay on H.264** — many cheap frames cannot decode H.265 (see [Output Constraints §4](#4-codec--container-compatibility)). Only switch to H.265 if you have verified your specific frame plays it.

---

#### 5. Lower Encoding Preset (5 min → 10-15% faster)

**Current**: Preset="slow" (maximum quality, maximum time)
**Optimization**: Use preset="medium" (good balance)

**Implementation** (ConcatVidPartsFromFileList.psm1, lines 278, 282):
```powershell
# Before
-preset slow

# After
-preset medium   # Instead of slow (faster) or fast (lower quality)
```

**Quality Trade-off**: Minimal visual difference on most displays
- slow → medium: 10-15% faster, imperceptible quality loss
- medium → fast: 25% faster, visible quality loss (avoid)

---

### Implementation Roadmap for Phase 1

**Week 1**: 6-8 hours of actual coding

```
Day 1: Setup & Testing
├─ git checkout -b phase1-optimizations
├─ Create benchmark baseline
└─ Set up test environment

Day 2-3: Optimizations 1 & 2
├─ Reduce frame count (30 min)
├─ Parallel transitions (30 min)
└─ Test with medium album (100 images)

Day 4: Optimizations 3, 4, 5
├─ FFprobe caching (1 hour)
├─ H.265 codec (20 min)
└─ Lower preset (5 min)

Day 5: Validation & Merge
├─ Run full test album
├─ Compare performance vs baseline
├─ Create PR with results
└─ Merge to main
```

**Expected Result**: 2-3x speed improvement with zero breaking changes

---

### Benchmarking Script

Create `benchmark.ps1`:

```powershell
param(
    [int]$NumImages = 20,
    [string]$ConfigName = "Baseline"
)

Write-Host "Starting benchmark: $ConfigName ($NumImages images)" -ForegroundColor Cyan
Write-Host "Time: $(Get-Date)" -ForegroundColor Gray

$sw = [System.Diagnostics.Stopwatch]::StartNew()

# Run full build
.\BuildAlbum.ps1

$sw.Stop()

$results = @{
    Config = $ConfigName
    NumImages = $NumImages
    TotalSeconds = $sw.Elapsed.TotalSeconds
    TotalTime = $sw.Elapsed.ToString("hh\:mm\:ss")
    SecsPerImage = [Math]::Round($sw.Elapsed.TotalSeconds / $NumImages, 2)
}

Write-Host ""
Write-Host "=== Benchmark Results ===" -ForegroundColor Green
Write-Host "Configuration: $($results.Config)"
Write-Host "Images: $($results.NumImages)"
Write-Host "Total time: $($results.TotalTime)"
Write-Host "Per image: $($results.SecsPerImage) seconds"
Write-Host ""

$results | Export-Csv -Path "benchmarks_$ConfigName.csv" -Append -NoTypeInformation
```

**Usage**:
```powershell
# Baseline
.\benchmark.ps1 -ConfigName "Baseline_Original" -NumImages 50

# After Phase 1
.\benchmark.ps1 -ConfigName "Phase1_Optimized" -NumImages 50

# Compare
$baseline = Import-Csv "benchmarks_Baseline_Original.csv" | Select-Object -Last 1
$optimized = Import-Csv "benchmarks_Phase1_Optimized.csv" | Select-Object -Last 1

$improvement = [Math]::Round([decimal]$baseline.TotalSeconds / [decimal]$optimized.TotalSeconds, 2)
Write-Host "Speed improvement: ${improvement}x"
```

---

### Phase 2: C# Wrapper Library (2-3 Weeks, 4-6x Total Faster, Medium Risk)

**Only pursue Phase 2 if Phase 1 doesn't meet performance targets.**

#### Architecture
```
PowerShell (orchestration)
    ↓
C# Class Library (SlideShowEngine.dll)
    ├─ ImageProcessor (MagickDotNet)     → 2-3x faster
    ├─ MetadataReader (JSON parsing)     → 5x faster
    ├─ TransitionEngine (FFmpeg wrapper)
    └─ VideoEncoder (FFmpeg coordination)
    ↓
CLI Tools (magick, ffmpeg)
```

#### Expected Improvements
- Frame generation: 3-4x faster than PowerShell CLI
- Metadata reading: 5x faster than text parsing
- Overall: 4-6x faster (combined with Phase 1)

#### Key Components Needed
1. **SlideShow.Core** - C# class library with MagickDotNet integration
2. **ImageProcessor** - Native frame generation
3. **MetadataReader** - JSON-based property extraction
4. **PowerShell Wrapper** - Integration layer

*See PHASE2_IMPLEMENTATION.md for complete C# implementation guide*

---

### Phase 3: Advanced Optimization (4-6 Weeks, 8-10x Faster, High Risk)

**Only pursue Phase 3 if Phase 2 still insufficient.**

#### Options
- **Rust rewrite** - Maximum performance, GPU acceleration possible
- **GPU acceleration** - CUDA/OpenCL for frame generation
- **Distributed processing** - Multi-machine parallel encoding

---

## Configuration Guide

### BuildAlbum.ps1 Configuration Options

#### Media Filtering
```powershell
$Names2Ig = @{
    # Folders/patterns to ignore
    Ignore = @("DNP", "PrivateVideos", "RawPhotos")
    
    # If parent folder name contains number > this, include anyway
    ExceptionParentFldrGreaterThan = 1000
}
```

#### Path Configuration
```powershell
# Source (can be NAS path)
$InputFileRootPath = "\\nas\Photos"

# Working directory (must have space for copy)
$PrepFileRootPath = "C:\PreparedMedia"

# Image corrections (optional, leave empty)
$ConvFileRootPath = ""

# Output folder prefix
$OutputFilePrepend = "AlbumOut"
```

#### Output Definitions
Each definition controls resolution, quality, and timing:

```powershell
@{
    Name = "LivingRoom"
    XDim = 1440                    # Width
    YDim = 900                     # Height
    FPS = 30                        # Frame rate
    PicDispTime = 6                 # Seconds per image
    MaxSrtRot = 15                  # Max rotation (degrees)
    FadeTime = 0.5                  # Fade duration (seconds)
    BulkVidTimeMin = 20             # Target video length (minutes)
    NameMethod = "FldrLvl2"         # Folder-level naming
    ImgVidFldr = "ImgInVid"         # Subfolder for videos
    Quality = 30                    # CRF (18=best, 51=worst)
    ExpAud = 0                      # Audio export (currently broken)
    CleanBuild = $false             # Rebuild everything
}
```

### Understanding Quality Settings

**CRF (Constant Rate Factor)** — general quality scale (lower = better, larger):
- `18-20`: Visually lossless (large files)
- `22-26`: Good quality, balanced
- `28-32`: Acceptable quality, smaller files
- `40+`: Poor quality

**Recommended by Device**:
- High-end displays: CRF 22-24
- Standard frames: CRF 26-28
- Budget frames: CRF 30-32

> For the **default frame-friendly starting value**, use **CRF 28** as given in the [Quick Reference §3](docs/quick-reference.md#3-recommended-starting-point-for-a-typical-frame) (the single source for recommended settings). The table above is general guidance for tuning around that default.

---

## Troubleshooting

### Common Issues

#### "ImageMagick not found"
```powershell
# Verify installation
magick -version

# Add to PATH if needed
$env:PATH += ";C:\Program Files\ImageMagick-7.x.x"
```

#### "FFmpeg not found"
```powershell
# Verify installation
ffmpeg -version

# Add to PATH
$env:PATH += ";C:\Program Files\ffmpeg\bin"
```

#### "Output videos have wrong aspect ratio"
- Adjust `MaxSrtRot` (rotation range)
- Check `XDim` and `YDim` match your display
- Verify source images aren't extreme aspect ratios

#### "Processing very slow"
1. Check if you're using a RAM disk (recommend: 15-20 GB)
2. Ensure SSD storage for temporary files
3. Run Phase 1 optimizations (should yield 2-3x improvement)
4. Monitor CPU/disk usage with Task Manager

#### "Audio export not working"
- Currently known issue (ExpAud flag doesn't work)
- Recommended: Leave as `ExpAud = 0`
- Will be fixed in Phase 2 C# implementation

#### "Videos don't play on frame"
- Check frame supports H.264 or H.265
- If using H.265, verify device support or switch to H.264
- Test with a short 1-minute video first

### Performance Tuning

#### For Faster Processing
1. Enable Phase 1 optimizations (2-3x faster)
2. Use RAM disk for temp files
3. Set `FadeTime` to 0.3-0.4s (shorter = fewer frames)
4. Set `Quality` to 28-30 (acceptable quality, faster)
5. Set `FPS` to 24 instead of 30 (imperceptible difference)

#### For Better Quality
1. Set `Quality` to 20-22
2. Set `FPS` to 60 (if device supports)
3. Use `preset slow` in FFmpeg (slower, highest quality)
4. Increase `PicDispTime` (longer image display)

---

## Documentation Index

| Document | Purpose | Read When |
|---|---|---|
| **[docs/quick-reference.md](docs/quick-reference.md)** | One-page cheat-sheet: Rust commands, recommended frame settings, every config field, current gaps | You just want the command + safe settings |
| **[docs/](docs/README.md)** | Engineering docs index: requirements, traceability, architecture, tests | Contributing or tracing a requirement |
| **[docs/process.md](docs/process.md)** | Multi-agent SDLC: roles, gates, ID scheme, anti-duplication, verdict protocol | Running/understanding the `/grind` workflow |
| **RUST_IMPLEMENTATION_ROADMAP.md** | Rust rewrite phases + live implementation status | Working on the Rust engine |
| **RUST_IMPLEMENTATION_SPEC.md** | Rust technical architecture & data model | Understanding the Rust design |
| **PERFORMANCE_REVIEW.md** | Deep technical bottleneck analysis | Planning Phase 2+, understanding architecture |
| **PHASE1_OPTIMIZATIONS.md** | Step-by-step Phase 1 guide | Starting Phase 1 implementation |
| **PHASE2_IMPLEMENTATION.md** | C# wrapper implementation guide | Planning/implementing Phase 2 |
| **REFACTORING_SUMMARY.md** | Quick reference & decision matrix | Deciding which phase to implement |
| **README.md** (this file) | Project overview & quick start | Getting started, configuration help |

---

## Project Statistics

- **Total Code**: ~2,700 lines of PowerShell
- **Module Files**: 5 main modules + helpers
- **Test Framework**: Manual testing via BuildAlbum.ps1
- **Git History**: 20+ commits on dev branch
- **Performance Potential**: 5-10x improvement possible

---

## Development Notes

### Current Work (Dev Branch)
- Audio extraction improvements (partially working)
- Index definition updates
- Parallel video creation refinements

### Known Limitations
- Audio export not fully implemented
- No unit test framework
- Limited error handling in some areas
- No CI/CD pipeline

### Future Improvements
- [ ] Complete audio extraction (Phase 2)
- [ ] Add comprehensive error handling
- [ ] Implement unit test framework
- [ ] Create CI/CD pipeline
- [ ] GPU acceleration (Phase 3)

---

## Contributing

Found a bug or want to contribute?

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/your-improvement`
3. Make your changes and test thoroughly
4. Submit a pull request with:
   - Clear description of what changed
   - Before/after performance metrics (if applicable)
   - Testing results on your setup

### Optimization Contribution Ideas
- Reduce ImageMagick frame generation time further
- Improve FFmpeg transition encoding
- Implement audio export properly
- Add GPU acceleration support

---

## License

See LICENSE file for details.

---

## Support

For questions or issues:
- Check **Troubleshooting** section above
- Review relevant documentation file
- Check git history for context on similar issues
- Review BuildAlbum.ps1 comments for configuration details

---

**Last Updated**: 2026-06-02  
**Status**: PowerShell pipeline stable on `main`; Rust engine working end-to-end on `rust-rewrite` (Ken Burns + rotation + cross-fade + inline video)  
**Next Step (Rust)**: `bulk_video_time_min` splitting into part files, then audio
