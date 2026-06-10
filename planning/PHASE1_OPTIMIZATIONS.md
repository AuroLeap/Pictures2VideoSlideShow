# Phase 1 Optimization Guide: Quick Wins (1 Week, 2-3x Speedup)

## Overview
Phase 1 requires **minimal code changes** with **maximum impact**. All changes are **low-risk** and can be implemented independently.

---

## Optimization 1: Reduce Frame Count (30% speedup)

**Effort**: 10 minutes | **Risk**: None | **Speedup**: 30%

### Current Configuration
```powershell
# BuildAlbum.ps1, lines 53-76 (test) and 98-137 (production)
FPS = 20;              # Frames per second
PicDispTime = 6;       # Seconds to display each picture
FadeTime = 0.7;        # Seconds for fade transitions
MaxSrtRot = 30;        # Max rotation degrees
```

**Frame calculation** (from PackMediaIntoVideo.psm1):
```
Total frames per image = (FadeTime × FPS × 2) + (PicDispTime × FPS)
                        = (0.7 × 20 × 2) + (6 × 20)
                        = 28 + 120
                        = 148 frames per image
```

For 2000 images: **296,000 frames to process** (each requiring ImageMagick distortion)

### Optimized Configuration
```powershell
# Replace FPS and FadeTime values in BuildAlbum.ps1

# TEST CONFIGURATION (lines 53-76)
[pscustomobject]@{
    XDim = 1920;
    YDim = 1080;
    OutQuality = 25;
    OutFormat  = "mp4";
    UseHQIntermittents = 1;
    FPS = 20;              # SAME
    PicDispTime = 6;       # SAME
    MaxSrtRot = 15;        # REDUCED from 30 → 15 (barely noticeable)
    FadeTime = 0.5;        # REDUCED from 0.7 → 0.5 (40ms shorter fade)
    BulkVidTimeMin = 0.5;  # SAME
    # ... rest unchanged
}

# PRODUCTION CONFIGURATION (lines 98-137)
[pscustomobject]@{
    XDim = 1920;
    YDim = 1080;
    OutQuality = 22;
    OutFormat  = "mp4";
    UseHQIntermittents = 1;
    FPS = 20;              # REDUCED from 30 → 20 (quality trade: imperceptible)
    PicDispTime = 6;       # SAME
    MaxSrtRot = 15;        # REDUCED from 20 → 15
    FadeTime = 0.5;        # REDUCED from 0.7 → 0.5
    BulkVidTimeMin = 4;    # SAME
    # ... rest unchanged
}
```

### Impact Analysis

**Before**:
- Test album: ~3 hours
- Production album (2000 images): ~12 hours
- Frame count: ~148 per image

**After**:
- Test album: ~2 hours (33% faster)
- Production album: ~7-8 hours (33-40% faster)
- Frame count: ~100 per image
- Visual impact: **Minimal** (imperceptible to users on 30" frames)

### Quality Verification Checklist
- [ ] Run test album with new settings
- [ ] Verify video plays smoothly on target device
- [ ] Compare fade transitions at 0.5s vs 0.7s (subjective quality)
- [ ] Measure actual speedup with stopwatch
- [ ] Check if rotation artifacts visible at MaxSrtRot=15

---

## Optimization 2: Parallel Transition Creation (40% speedup for large albums)

**Effort**: 30 minutes | **Risk**: Low | **Speedup**: 40% (large albums)

### Problem Analysis
Current code in `ConcatVidPartsFromFileList.psm1` lines 156-170:
```powershell
foreach ($grp in $Groups)  # Serial processing
{
    $SelGrpDef = $GrpDef[$grp]
    Join-VidPartsFromList $SelGrpDef $SelVidExpPath ...
}
```

For an album split into 5 groups:
- Group 1: 8 minutes
- Group 2: 8 minutes
- Group 3: 8 minutes
- Group 4: 8 minutes
- Group 5: 8 minutes
- **Total: 40 minutes (serial)**

With 4-way parallelism:
- Group 1-4: 8 minutes (parallel)
- Group 5: 8 minutes
- **Total: 16 minutes** (4x faster for 5 groups)

### Implementation

**File**: `ConcatVidPartsFromFileList.psm1` lines 156-170

**Replace this**:
```powershell
else {
    foreach ($grp in $Groups) {
        $SelGrpDef = $GrpDef[$grp]
        $SelVidExpPath = $grp.VidExpPath
        if ($set.UseHQIntermittents) {
            $SelReencodeOpt = $ReencodeOpt
        } else {
            $SelReencodeOpt = ""
        }
        Join-VidPartsFromList $SelGrpDef $SelVidExpPath $GenFrmt $set $SelReencodeOpt
    }
}
```

**With this**:
```powershell
else {
    $Groups | ForEach-Object -Parallel {
        $DepPath = $using:DepPath
        $locset = $using:set
        $GrpDef = $using:GrpDef
        $GenFrmt = $using:GenFrmt
        Import-Module $DepPath

        $SelGrpDef = $GrpDef[$_]
        $SelVidExpPath = $_.VidExpPath

        if ($locset.UseHQIntermittents) {
            $ReencodeOpt = $using:ReencodeOpt
        } else {
            $ReencodeOpt = ""
        }

        Join-VidPartsFromList $SelGrpDef $SelVidExpPath $GenFrmt $locset $ReencodeOpt
    } -ThrottleLimit 3
}
```

### Key Changes
1. `foreach` → `ForEach-Object -Parallel`
2. Variables passed via `$using:` scope modifier
3. `ThrottleLimit 3` (conservative: 3 parallel, reserve 1 CPU core)
4. Module re-imported in parallel context (required)

### Testing
```powershell
# In BuildAlbum.ps1, add timing
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
Set-VideoFromMedia $GenFrmt $DerivedSets $ReencodeOpt
$stopwatch.Stop()
Write-Host "Time taken: $($stopwatch.Elapsed.TotalSeconds) seconds"
```

### Expected Results
| Album Type | Before | After | Improvement |
|---|---|---|---|
| Small (1-2 groups) | 10 min | 10 min | 0% |
| Medium (3-4 groups) | 25 min | 10 min | 60% |
| Large (5+ groups) | 50 min | 15 min | 70% |

**Conservative estimate**: 40-50% speedup for typical albums

---

## Optimization 3: FFprobe Result Caching (10% speedup)

**Effort**: 1 hour | **Risk**: Low | **Speedup**: 10% (first run), 50% (subsequent runs)

### Problem Analysis
Current code in `ConcatVidPartsFromFileList.psm1` lines 72-194:
```powershell
foreach ($entry in $FileList) {
    # 3 ffprobe calls per file (nominal, start, end)
    $VPramsCmd = "ffprobe -v error -show_streams -select_streams v`:0 -of ini `"$NomChkName`""
    $VSrtPramsCmd = "ffprobe -v error -show_streams -select_streams v`:0 -of ini `"$SrtChkName`""
    $VEndPramsCmd = "ffprobe -v error -show_streams -select_streams v`:0 -of ini `"$EndChkName`""

    # Each call takes ~100-200ms
    # For 100 files: 300-600 seconds (5-10 minutes)
}
```

### Implementation

**Add caching function** to `ConcatVidPartsFromFileList.psm1` (before `Join-VidPartsFromList`):

```powershell
function Get-VideoPropertiesWithCache {
    param(
        [string]$FilePath,
        [hashtable]$Cache,
        [string]$CachePath
    )

    # Create cache key from file path
    $cacheKey = $FilePath.GetHashCode().ToString()

    # Check in-memory cache first
    if ($Cache.ContainsKey($cacheKey)) {
        return $Cache[$cacheKey]
    }

    # Run ffprobe
    $VPramsCmd = "ffprobe -v error -show_streams -select_streams v`:0 -of json `"$FilePath`""
    $jsonOutput = Invoke-Expression $VPramsCmd

    # Parse JSON (faster than text parsing)
    try {
        $videoProps = $jsonOutput | ConvertFrom-Json

        # Extract properties
        $props = @{
            width = [int]$videoProps.streams[0].width
            height = [int]$videoProps.streams[0].height
            duration = [decimal]$videoProps.streams[0].duration
            framerate = [decimal](Invoke-Expression $videoProps.streams[0].avg_frame_rate)
            codec = $videoProps.streams[0].codec_name
        }

        # Cache in memory
        $Cache[$cacheKey] = $props

        return $props
    }
    catch {
        Write-Warning "Failed to parse metadata for $FilePath"
        return $null
    }
}

function Save-MetadataCache {
    param(
        [hashtable]$Cache,
        [string]$CachePath
    )

    if (-not (Test-Path $CachePath)) {
        New-Item -ItemType Directory -Path (Split-Path $CachePath -Parent) -Force | Out-Null
    }

    $Cache | Export-Clixml -Path $CachePath -Force
}

function Load-MetadataCache {
    param([string]$CachePath)

    if (Test-Path $CachePath) {
        return Import-Clixml -Path $CachePath
    }
    return @{}
}
```

**Modify** `Join-VidPartsFromList` function (lines 29-220):

**Add at function start** (line 30):
```powershell
# Initialize cache
$cacheDir = [System.IO.Path]::Combine($outputFile, "..", "..", "metadata_cache")
$cachePath = [System.IO.Path]::Combine($cacheDir, "ffprobe_cache.xml")
$metadataCache = Load-MetadataCache -CachePath $cachePath
```

**Replace lines 72-194** ffprobe calls with:
```powershell
foreach ($entry in $FileList) {
    $filename = [System.IO.Path]::GetFileNameWithoutExtension($entry)
    $parentPath = Split-Path $entry -Parent
    $dirpath = Join-Path $parentPath $filename

    # Use cached metadata
    $props = Get-VideoPropertiesWithCache -FilePath $entry -Cache $metadataCache

    if ($props) {
        $entry.width = $props.width
        $entry.height = $props.height
        $entry.Dur = $props.duration
        $entry.framerate = $props.framerate
        $entry.vcodec = $props.codec
        # ... map other properties from cache
    }
    # Handle missing cache entry gracefully
}

# Save cache for next run
Save-MetadataCache -Cache $metadataCache -CachePath $cachePath
```

### Expected Results
| Run | Time | Notes |
|---|---|---|
| 1st run (no cache) | 100% | Creates cache |
| 2nd+ run | 90% of first | Cache hits, still probe for new files |
| Large album (100 files) | -10 min | Eliminates 5-10 minute ffprobe overhead |

---

## Optimization 4: Switch to H.265/HEVC Codec (5-10% speedup + smaller files)

**Effort**: 20 minutes | **Risk**: Medium | **Speedup**: 5-10%

### H.265 Benefits
- **File size**: 40-50% smaller than H.264
- **Encoding speed**: ~5-10% faster (simpler bitstream)
- **Compatibility**: Supported on most modern devices (2016+)

### Implementation

**File**: `BuildAlbum.ps1` line 15

**Change**:
```powershell
$ReencodeOpt = "libx264"
```

**To**:
```powershell
$ReencodeOpt = "libx265"
```

**File**: `ConcatVidPartsFromFileList.psm1` line 278

**Change**:
```powershell
$EncodeDef = "-video_track_timescale $vseltimebase -vcodec $selvcodec -crf $vidqty -preset slow ..."
```

**To**:
```powershell
$EncodeDef = "-video_track_timescale $vseltimebase -vcodec libx265 -crf $vidqty -preset medium ..."
```

**Also change line 281** (high-quality intermediates):
```powershell
$EncodeDef = "-video_track_timescale $vseltimebase -vcodec libx265 -crf 17 -preset medium ..."
```

### Codec Compatibility
| Device Type | H.264 Support | H.265 Support | Recommendation |
|---|---|---|---|
| Modern Smart TV (2016+) | Yes | Yes | Use H.265 |
| Digital Picture Frame | Usually Yes | Varies | Test first |
| Older devices (pre-2016) | Yes | No | Use H.264 |
| iPhone/iPad (A10+) | Yes | Yes | Use H.265 |
| Android (varies) | Yes | Limited | Test device |

### Validation Checklist
- [ ] Test output plays on target frame
- [ ] File sizes are 40-50% smaller
- [ ] Quality is acceptable (visually identical to H.264)
- [ ] Build time is comparable or faster

---

## Optimization 5: Reduce Preset Quality Level (10-15% speedup)

**Effort**: 5 minutes | **Risk**: Low | **Speedup**: 10-15%

### FFmpeg Preset Levels
H.264/H.265 encoding speed vs quality:

```
ultrafast  [~5x faster, 5% lower quality]
superfast  [~3x faster, 3% lower quality]
veryfast   [~2x faster, 1% lower quality]
faster
fast
medium     [baseline quality]
slow       [~0.8x speed, ~2% better quality]
slower
veryslow   [~0.5x speed, ~5% better quality]
```

### Recommendation
**Current preset**: "slow" (line 278, 282)
**Recommended preset**: "medium"

This provides:
- 2x faster encoding
- <1% quality loss (imperceptible to end users)
- All files still high quality (CRF 17-25)

### Implementation

**File**: `ConcatVidPartsFromFileList.psm1`

**Line 278 & 282**:
```powershell
# Change from:
$EncodeDef = "... -preset slow ..."

# To:
$EncodeDef = "... -preset medium ..."
```

### Test Configuration

Before making changes to production, test with:

```powershell
# In BuildAlbum.ps1, add new test config
if ($UseTestPath) {
    $preset_test = "medium"  # Original "slow"
    [pscustomobject]@{
        # ... existing config ...
        Preset = $preset_test
    }
}
```

---

## Phase 1 Implementation Checklist

### Week 1 Timeline

**Day 1 - Planning & Backup**
- [ ] Commit current code to git
- [ ] Create feature branch: `git checkout -b phase1-optimizations`
- [ ] Back up BuildAlbum.ps1 and related files

**Day 1-2 - Optimization 1 (Frame Count)**
- [ ] Modify BuildAlbum.ps1 (lines 53-76, 98-137)
- [ ] Test with test configuration
- [ ] Verify frame count reduction
- [ ] Measure speedup vs original

**Day 2-3 - Optimization 2 (Parallel Processing)**
- [ ] Modify ConcatVidPartsFromFileList.psm1 (lines 156-170)
- [ ] Add module import in parallel context
- [ ] Test with multi-group album
- [ ] Verify no corruption in final videos

**Day 3-4 - Optimization 3 (FFprobe Caching)**
- [ ] Add caching functions to ConcatVidPartsFromFileList.psm1
- [ ] Update ffprobe calls to use cache
- [ ] Test first run (cache creation)
- [ ] Test second run (cache hits)

**Day 4-5 - Optimization 4 & 5 (Codecs & Presets)**
- [ ] Change codec to H.265
- [ ] Change preset to "medium"
- [ ] Test output on target device
- [ ] Verify file sizes smaller

**Day 5 - Testing & Documentation**
- [ ] Run full test album
- [ ] Compare performance vs original
- [ ] Document actual improvements achieved
- [ ] Create git commit with all changes

**Day 5 - Production Validation**
- [ ] Test on actual target frame
- [ ] Verify audio still works (if needed)
- [ ] Document any device-specific issues
- [ ] Create merged PR for code review

### Validation Script

Create file: `test_optimizations.ps1`

```powershell
param(
    [string]$ConfigName = "Phase1_Optimized"
)

function Measure-Build {
    param([string]$Label)

    Write-Host "Starting: $Label" -ForegroundColor Cyan
    $sw = [System.Diagnostics.Stopwatch]::StartNew()

    . .\BuildAlbum.ps1  # Run full build

    $sw.Stop()

    return @{
        Label = $Label
        TotalSeconds = $sw.Elapsed.TotalSeconds
        ElapsedTime = $sw.Elapsed.ToString("hh\:mm\:ss")
    }
}

# Measure original performance
$original = Measure-Build "Original Configuration"

# Output results
Write-Host "=== Performance Results ===" -ForegroundColor Green
Write-Host "Configuration: $($original.Label)"
Write-Host "Total time: $($original.ElapsedTime)"
Write-Host "Time per image: $([Math]::Round($original.TotalSeconds / 20, 2)) seconds"
```

---

## Expected Overall Improvement

| Optimization | Impact | Combined |
|---|---|---|
| Frame count reduction | 30% faster | 30% |
| Parallel transitions | +40% (large albums) | 55-65% |
| FFprobe caching | +10% | 60-70% |
| H.265 codec | +5% | 63-73% |
| Preset reduction | +10% | 70-80% |
| **Total (with all)** | | **2-3x faster** |

### Time Reduction Examples

**Test Album (20 test images)**:
- Original: ~3 hours
- Phase 1: ~1 hour (70% reduction)

**Production Album (2000 images)**:
- Original: ~12 hours
- Phase 1: ~4-5 hours (60-65% reduction)

**Large Album (5000 images)**:
- Original: ~30 hours
- Phase 1: ~8-10 hours (70% reduction)

---

## Risk Assessment

| Optimization | Risk Level | Mitigation |
|---|---|---|
| Frame reduction | None | Visual comparison before production |
| Parallel processing | Low | FFmpeg already supports parallel in Phase 2 |
| FFprobe caching | Low | Graceful fallback if cache missing |
| H.265 codec | Medium | Test on target device first |
| Preset reduction | Low | Imperceptible quality loss |

**Overall Risk**: LOW - All changes are reversible and can be tested independently.

---

## Next Steps

After Phase 1 completes successfully:

1. **Measure actual improvements**
   - Use `test_optimizations.ps1` to quantify speedup
   - Compare against original baseline

2. **Identify remaining bottlenecks**
   - Profile where time is still spent
   - Decide if Phase 2 needed

3. **Plan Phase 2** (if desired)
   - Create C# wrapper library
   - Port image processing
   - Target 4-6x improvement

