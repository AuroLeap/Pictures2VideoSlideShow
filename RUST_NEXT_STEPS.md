# Rust Rewrite: Next Steps

> **Historical planning doc (2026-05-30).** Written when the Rust engine was a set of stubs. The engine is now **working end-to-end** — see the [README Project Status](README.md#project-status). For the authoritative **commands, config fields, and recommended settings**, use the **[Quick Reference](docs/quick-reference.md)** (single source of truth); any command/config snippets below are illustrative and may be out of date.

**Branch**: `rust-rewrite`  
**Timeline (original estimate)**: 3-5 weeks to 5-8x improvement

---

## What's Been Done ✅

### Documentation (Complete)
- ✅ **RUST_IMPLEMENTATION_SPEC.md** - Complete technical architecture
- ✅ **RUST_IMPLEMENTATION_ROADMAP.md** - Week-by-week schedule  
- ✅ **README.md** - Project overview consolidated
- ✅ **OPTIMIZATION_PATHS.md** - Analysis of all improvement options

### Project Structure (Complete)
- ✅ **Cargo.toml** - All dependencies configured (rayon, tokio, image, etc.)
- ✅ **src/main.rs** - CLI interface with subcommands
- ✅ **src/lib.rs** - Library exports
- ✅ **Error handling** - Type-safe error system
- ✅ **Logging** - Debug/info logging setup
- ✅ **Module stubs** - All core modules created and ready

### Module Structure (All Stubbed)
```
src/
├── config/     ✅ Configuration loading & validation
├── media/      ✅ Media scanning and indexing
├── image/      ⏳ Frame generation (needs implementation)
├── video/      ⏳ FFmpeg coordination (needs implementation)
├── transform/  ⏳ Transform calculations (needs implementation)
├── ffmpeg/     ⏳ FFmpeg wrapper (needs implementation)
├── pipeline/   ⏳ Pipeline orchestration (needs implementation)
└── util/       ✅ Helper utilities
```

### Git Setup (Complete)
- ✅ Branch `rust-rewrite` created
- ✅ All code committed
- ✅ Isolated from dev/main branches

---

## What Needs to Be Done ⏳

### Phase 1: Configuration (Days 1-2) - HIGH PRIORITY
**Status**: Mostly complete, needs testing

```bash
# Test configuration loading
cargo run -- --config test_config.toml validate

# Create test config file and verify it loads
```

**Files to complete**:
- `src/config/mod.rs` - Add PowerShell config converter

---

### Phase 2: Media Loading (Days 3-5) - HIGH PRIORITY
**Status**: Stub ready, needs full implementation

Key tasks:
- [ ] Implement parallel directory scanning with rayon
- [ ] Extract image dimensions (use `image` crate)
- [ ] Extract video metadata (use ffprobe)
- [ ] Apply ignore patterns
- [ ] Test with real media

**Code location**: `src/media/mod.rs`

**Expected improvement**: Enable next phases, ~20% of total speedup

---

### Phase 3: Transform Calculations (Days 6-7)
**Status**: Stub ready, needs math implementation

Key tasks:
- [ ] Zoom calculation (deceleration curve)
- [ ] Rotation calculation (cosine easing)
- [ ] Pan calculation (linear movement)
- [ ] Frame count calculation
- [ ] Comprehensive unit tests

**Code location**: `src/transform/mod.rs`

**Expected improvement**: Foundation for phase 4

---

### Phase 4: Image Frame Generation (Days 8-12) - CRITICAL
**Status**: Stub ready, high-impact implementation

Key tasks:
- [ ] Load image with `image` crate
- [ ] Create frame renderer with rayon parallelism
- [ ] Apply transformations to frames
- [ ] Composite onto canvas
- [ ] Encode to JPEG
- [ ] Stream (don't buffer)

**Code location**: `src/image/mod.rs`, `src/image/renderer.rs`

**Expected improvement**: 60% of total speedup (biggest bottleneck!)

**Critical Optimization**: Use `Arc<DynamicImage>` to share loaded image

---

### Phase 5: FFmpeg Integration (Days 13-15)
**Status**: Stub ready, needs implementation

Key tasks:
- [ ] Spawn FFmpeg process
- [ ] Stream frames via stdin pipe
- [ ] Handle output files
- [ ] Error handling
- [ ] Audio support (optional)

**Code location**: `src/ffmpeg/process.rs`, `src/ffmpeg/encoder.rs`

**Expected improvement**: 20% of total speedup

---

### Phase 6: Pipeline Orchestration (Days 16-18)
**Status**: Stub ready, needs connection logic

Key tasks:
- [ ] Connect all components
- [ ] Process multiple images
- [ ] Process multiple outputs
- [ ] Progress reporting
- [ ] Error recovery

**Code location**: `src/pipeline/frame_pipeline.rs`

**Expected improvement**: Enable end-to-end processing

---

### Phase 7: Testing & Optimization (Days 19-22)
**Status**: Minimal tests in place

Key tasks:
- [ ] Unit tests (target: >80% coverage)
- [ ] Integration tests
- [ ] Performance benchmarks
- [ ] Memory profiling
- [ ] Optimization tweaks

**Expected improvement**: Reliability and final performance tuning

---

## Getting Started Right Now

### Step 1: Install Rust (5 minutes)

```bash
# Download and install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version  # Should show: rustc 1.73+ 
cargo --version  # Should show: cargo 1.73+

# If on Windows, restart terminal after install
```

### Step 2: Verify Project Setup (5 minutes)

```bash
cd c:\Projects\Pictures2VideoSlideShow

# Switch to rust-rewrite branch
git checkout rust-rewrite

# Try to build (should succeed with warnings)
cargo build

# Run help command
cargo run -- --help

# Expected output:
# Usage: slideshow --config <CONFIG> [OPTIONS] [COMMAND]
# Commands:
#   build     Run full slideshow generation pipeline
#   validate  Validate configuration file
#   stats     Show project statistics
#   bench     Benchmark performance
```

### Step 3: Create Test Configuration (10 minutes)

```bash
# Create test directory
mkdir -p C:\TestPhotos
mkdir -p C:\TestOutput

# Create test config file: test_config.toml
[input]
media_root = "C:\\TestPhotos"
ignore_patterns = ["DNP"]

[output]
base_dir = "C:\\TestOutput"

[processing]
temp_dir = ""
max_workers = 8
use_parallelism = true
verbose = true

[[outputs]]
name = "test-display"
width = 1440
height = 900
fps = 30
pic_display_time_secs = 6.0
fade_time_secs = 0.5
max_rotation_degrees = 15.0
bulk_video_time_min = 20
quality_crf = 28
enable_audio = false

# Test loading
cargo run -- --config test_config.toml validate
```

### Step 4: Start Phase 1 Implementation

Follow detailed instructions in **RUST_IMPLEMENTATION_ROADMAP.md**

---

## Daily Development Workflow

### Build & Run

```bash
# Fast debug build
cargo build
cargo run -- --config test_config.toml stats

# Optimized release build (slow compile, fast run)
cargo build --release
cargo run --release -- --config test_config.toml build

# With debugging output
RUST_LOG=debug cargo run -- --config test_config.toml stats

# With timing
time cargo run --release -- --config test_config.toml build
```

### Testing & Quality

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_zoom_calculation

# Code formatting
cargo fmt

# Linting (warnings)
cargo clippy

# Check everything
cargo check
```

### Benchmarking

```bash
# Run benchmarks
cargo bench

# Run specific benchmark
cargo bench frame_generation
```

### Git Workflow

```bash
# Check current branch
git branch

# Make changes
# ... edit files ...

# Commit changes
git add .
git commit -m "Phase X: Description of changes"

# View history
git log --oneline -10

# When complete, merge back to dev
git checkout dev
git merge rust-rewrite
```

---

## Expected Performance Progression

### Baseline (PowerShell)
- 2000 images: 12 hours
- 100 images: 36 minutes
- Memory: 2 GB

### After Phase 2-3 (Configuration + Media Loading)
- No performance change yet
- Foundation for phases 4-6

### After Phase 4 (Frame Generation - CRITICAL)
- 2000 images: 6-8 hours (50% improvement)
- 100 images: 18-24 minutes
- Memory: 1.5 GB
- *This is where most speedup comes from*

### After Phase 5 (FFmpeg Integration)
- 2000 images: 5-6 hours (1.5-2x improvement)
- 100 images: 15-18 minutes
- Memory: 800 MB

### After Phase 6 (Complete Pipeline)
- 2000 images: 1.5-2.5 hours (5-8x total improvement)
- 100 images: 4-8 minutes
- Memory: 500 MB
- **TARGET ACHIEVED** ✅

---

## Performance Validation

### Create Benchmarking Script

```powershell
# File: benchmark.ps1
param(
    [int]$NumImages = 50,
    [string]$ConfigName = "Test"
)

Write-Host "Benchmarking: $ConfigName ($NumImages images)" -ForegroundColor Cyan

$sw = [System.Diagnostics.Stopwatch]::StartNew()

cargo run --release -- --config test_config.toml build

$sw.Stop()

Write-Host "Completed in $($sw.Elapsed.ToString('hh\:mm\:ss'))" -ForegroundColor Green
```

### Benchmark Each Phase

```bash
# Phase 1-2: Just media scanning
cargo run --release -- --config test_config.toml stats
# Should be instant (< 1 second)

# Phase 4: Frame generation (core bottleneck)
# After implementation, should show significant improvement

# Phase 6: End-to-end
# Should show 5-8x improvement over PowerShell
```

---

## Common Issues & Solutions

### "cargo: command not found"
**Solution**: Rust not installed or not in PATH. Reinstall Rust:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### "ffmpeg: command not found"
**Solution**: FFmpeg not in PATH. Install FFmpeg and add to PATH:
```bash
# Windows: Download from https://ffmpeg.org/download.html
# Or use: choco install ffmpeg
```

### "Build errors with dependencies"
**Solution**: Update and rebuild:
```bash
rustup update
cargo clean
cargo build
```

### "Code doesn't compile"
**Check lint warnings**:
```bash
cargo clippy
cargo fmt
```

### Performance not improving
**Check**:
1. Are you using release build? `cargo build --release`
2. Are you using parallelism? Check rayon is enabled
3. Profile with: `cargo run --release -- ... 2>&1 | time`

---

## Helpful Resources

### Rust Basics
- **Rust Book**: https://doc.rust-lang.org/book/
- **Rust by Example**: https://doc.rust-lang.org/rust-by-example/

### Key Libraries
- **rayon** (parallelism): https://docs.rs/rayon/
- **tokio** (async): https://tokio.rs/
- **image** (image processing): https://docs.rs/image/
- **imageproc**: https://docs.rs/imageproc/

### Performance
- **Rust Performance Guide**: https://nnethercote.github.io/perf-book/
- **Flamegraph**: `cargo install flamegraph`

---

## Decision Points

### Which Phase Should I Start With?

**Answer**: Start with **Phase 1**, it's already partially complete!

The phases are sequential and each builds on the previous:
1. Phase 1: Config ✅ (mostly done)
2. Phase 2: Media Loading (start here)
3. Phase 3: Transforms
4. Phase 4: **CRITICAL** Frame Generation (biggest bottleneck)
5. Phase 5: FFmpeg
6. Phase 6: Pipeline
7. Phase 7: Polish

---

## Success Criteria

**MUST HAVE** (to declare success):
- ✅ Compiles with no errors
- ✅ All unit tests pass
- ✅ 5-8x performance improvement confirmed
- ✅ Output video quality matches PowerShell version

**SHOULD HAVE**:
- ✅ >80% test coverage
- ✅ Comprehensive error handling
- ✅ Good performance benchmarks
- ✅ Documentation complete

---

## Ready to Begin?

### Quick Start (15 minutes)

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Navigate to project
cd c:\Projects\Pictures2VideoSlideShow

# 3. Switch to rust-rewrite branch
git checkout rust-rewrite

# 4. Build project
cargo build

# 5. Create test config (see above)
# 6. Test it
cargo run -- --config test_config.toml validate

# 7. Read Phase 1 in RUST_IMPLEMENTATION_ROADMAP.md
# 8. Start coding!
```

---

## Next Checkpoint

**After completing Phase 2 (Media Loading)**:
- [ ] Can scan directory with 2000+ images in < 10 seconds
- [ ] Can display media statistics (file count, total size)
- [ ] Test passes: `cargo test media::`
- [ ] Ready for Phase 3

---

## Questions?

Reference these files in order:
1. **RUST_IMPLEMENTATION_SPEC.md** - Technical architecture details
2. **RUST_IMPLEMENTATION_ROADMAP.md** - Step-by-step implementation guide
3. Code comments in `src/` directory
4. Rust documentation: `cargo doc --open`

---

**Status**: ✅ Ready to implement  
**Current Branch**: `rust-rewrite`  
**Next Step**: Phase 1 (configuration) + Phase 2 (media loading)  
**Timeline**: 3-5 weeks to 5-8x improvement  

Good luck! 🚀
