# Rust Implementation Roadmap

**Branch**: `rust-rewrite`  
**Goal**: 5-8x performance improvement (12 hours → 1.5-2.5 hours)  
**Timeline**: 3-5 weeks

---

## Setup Prerequisites

### Install Rust (5 minutes)

```bash
# Windows
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify
rustc --version
cargo --version
```

### Verify Project Setup

```bash
cd c:\Projects\Pictures2VideoSlideShow

# Check structure
cargo build  # Should succeed (with warnings for unimplemented code)
cargo run -- --help  # Should show CLI help
```

---

## Development Workflow

### Daily Commands

```bash
# Build debug (fast)
cargo build

# Build release (optimized, slow)
cargo build --release

# Run with arguments
cargo run -- --config config.toml --input "C:\Photos"

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run -- --config config.toml

# Format code
cargo fmt

# Lint code
cargo clippy

# Benchmark
cargo bench
```

### Git Workflow

```bash
# Stay on rust-rewrite branch
git checkout rust-rewrite

# Regular commits as you complete features
git add .
git commit -m "Implement feature description"

# Before merging back to dev, test thoroughly
cargo test
cargo build --release
```

---

## Phase 1: Configuration System (3-4 days)

**Goal**: Load and validate configuration from TOML/JSON files  
**Status**: ✅ **DONE** (Basic structure in place)

### Tasks

- [x] Create `Config` struct with deserialization
- [x] Implement TOML parsing
- [x] Add validation logic
- [x] Error handling for invalid configs
- [x] Command-line argument parsing (clap)

### Testing

```bash
# Create test config
cat > test_config.toml << 'EOF'
[input]
media_root = "C:\\TestInput"
ignore_patterns = ["DNP"]

[output]
base_dir = "C:\\TestOutput"

[processing]
temp_dir = "R:\\"
use_parallelism = true
verbose = false

[[outputs]]
name = "test"
width = 1440
height = 900
fps = 30
pic_display_time_secs = 6.0
fade_time_secs = 0.5
max_rotation_degrees = 15.0
bulk_video_time_min = 20
quality_crf = 28
enable_audio = false
EOF

cargo run -- --config test_config.toml validate
```

### Files Modified

- `src/config/mod.rs` - ✅ Main config logic
- `src/main.rs` - ✅ CLI integration

### Next Steps

Once working, create a converter script: `PowerShell/Config2TOML.ps1`

---

## Phase 2: Media Loading (4-5 days)

**Goal**: Scan directory, index all media files, filter by config  
**Current Status**: Stub implementation ready

### Tasks

- [ ] Implement parallel directory scanning (walkdir + rayon)
- [ ] Extract image dimensions using `image` crate
- [ ] Extract video duration/properties using ffprobe
- [ ] Apply ignore patterns and exceptions
- [ ] Cache media metadata
- [ ] Handle errors gracefully

### Key Implementation Points

**File**: `src/media/mod.rs`

```rust
// Parallel scanning with rayon
for entry in walkdir::WalkDir::new(&root)
    .into_iter()
    .par_bridge()  // <- Parallel!
    .filter_map(|e| e.ok())
{
    // Process each file in parallel
}

// Extract dimensions with image crate
let img = image::open(path)?;
let (w, h) = img.dimensions();

// Extract video duration with ffprobe
let output = Command::new("ffprobe")
    .arg("-v").arg("error")
    .arg("-show_entries").arg("format=duration")
    .arg("-of").arg("json")
    .arg(path)
    .output()?;
```

### Testing

```bash
# Create test media
mkdir -p C:\TestInput\Photos
# Copy some test images

# Run media scan
cargo run -- --input C:\TestInput stats
```

### Performance Benchmark

**Target**: Scan 2000 images in < 10 seconds

```bash
# Time the scan
time cargo run -- --input "C:\RealPhotos" stats
```

### Files Modified

- `src/media/mod.rs` - Main implementation
- `src/util/file_utils.rs` - Helper functions

---

## Phase 3: Transform Calculations (3-4 days)

**Goal**: Calculate zoom, rotation, pan for each frame  
**Current Status**: Stub in place

### Tasks

- [ ] Implement zoom calculation with deceleration curve
- [ ] Implement rotation calculation with easing function
- [ ] Implement pan/offset calculation
- [ ] Frame count calculation based on timing
- [ ] Unit tests for all math

### Key Formulas

**Zoom**: Start at `zoom_start`, decrease by `zoom_rate` per frame
```rust
fn calculate_zoom(frame: u32, total: u32, start: f32, rate: f32) -> f32 {
    let zoom = start - (rate * frame as f32);
    zoom.max(1.0)  // Never go below 1.0
}
```

**Rotation**: Deceleration curve (cosine easing)
```rust
fn calculate_rotation(frame: u32, total: u32, max_rot: f32) -> f32 {
    let t = frame as f32 / total as f32;
    let easing = (std::f32::consts::PI * t).cos();
    max_rot * easing
}
```

**Pan**: Linear movement across frame
```rust
fn calculate_pan(frame: u32, total: u32, width: u32, height: u32) -> (f32, f32) {
    let t = frame as f32 / total as f32;
    let x = (t * width as f32) - (width as f32 / 2.0);
    let y = (t * height as f32) - (height as f32 / 2.0);
    (x, y)
}
```

### File Structure

**File**: `src/transform/mod.rs`

```rust
pub struct TransformCalculator;

impl TransformCalculator {
    pub fn calculate_all(&self, config: &OutputDef) -> Vec<FrameTransform> {
        let fade_frames = (config.fade_time_secs * config.fps as f32) as u32;
        let display_frames = (config.pic_display_time_secs * config.fps as f32) as u32;
        let total_frames = (fade_frames * 2) + display_frames;

        (0..total_frames)
            .map(|i| {
                FrameTransform {
                    zoom: self.calculate_zoom(i, total_frames),
                    rotation: self.calculate_rotation(i, total_frames),
                    pan: self.calculate_pan(i, total_frames),
                }
            })
            .collect()
    }
}
```

### Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zoom_deceleration() {
        let calc = TransformCalculator;
        let z0 = calc.calculate_zoom(0, 100, 1.2, 0.002);
        let z50 = calc.calculate_zoom(50, 100, 1.2, 0.002);
        assert!(z0 > z50);  // Zoom decreases
        assert!(z0 <= 1.2);  // Never exceeds start
    }
}

cargo test
```

### Files Modified

- `src/transform/mod.rs` - All math
- Create `src/transform/tests.rs` - Comprehensive tests

---

## Phase 4: Image Frame Generation (5-7 days)

**Goal**: Generate all frames for one image using parallel processing  
**Current Status**: Stub in place  
**This is the primary performance bottleneck - critical!**

### Tasks

- [ ] Load image from disk with `image` crate
- [ ] Create frame renderer with rayon parallelism
- [ ] Apply zoom/rotation/pan transformations
- [ ] Composite onto output canvas (black background)
- [ ] Encode to JPEG with quality setting
- [ ] Memory-efficient streaming (not buffering all frames)

### Architecture

```
Image File
    ↓
Load image (once)
    ↓
For each frame (0..frame_count):
    ├─ Calculate transform (zoom, rot, pan)
    ├─ Load image (use Arc<DynamicImage> to share)
    ├─ Apply transformations
    ├─ Composite on canvas
    ├─ Encode to JPEG
    └─ Return encoded bytes
    ↓
Stream frames to encoder (no buffering!)
```

### Key Code Structure

**File**: `src/image/mod.rs`

```rust
pub struct FrameRenderer {
    image: Arc<DynamicImage>,
    params: RenderParams,
}

impl FrameRenderer {
    pub fn render_frames(&self) -> Result<Vec<Vec<u8>>> {
        let transforms = self.calculate_transforms();

        // Parallel frame generation
        let frames: Vec<Vec<u8>> = (0..self.params.frame_count)
            .into_par_iter()  // rayon parallel iterator
            .map(|frame_idx| {
                self.render_single_frame(frame_idx, &transforms[frame_idx as usize])
                    .expect("Frame render failed")
            })
            .collect();

        Ok(frames)
    }

    fn render_single_frame(
        &self,
        frame_idx: u32,
        transform: &FrameTransform,
    ) -> Result<Vec<u8>> {
        // 1. Clone image (cheap with Arc)
        let mut frame = self.image.to_rgb8();

        // 2. Apply transformations using imageproc
        // - Rotate
        // - Scale (zoom)
        // - Translate (pan)

        // 3. Composite onto canvas
        let output = DynamicImage::ImageRgb8(frame);

        // 4. Encode to JPEG
        let mut jpeg_data = Vec::new();
        output.write_to(
            &mut jpeg_data,
            image::ImageFormat::Jpeg,
        )?;

        Ok(jpeg_data)
    }
}
```

### Performance Considerations

**Critical Optimization**: Use `Arc<DynamicImage>` to share loaded image across threads
- Without Arc: Load image N times (N = frame count) - SLOW
- With Arc: Load image once, share across all threads - FAST

```rust
let image = Arc::new(image::open(&path)?);

(0..frame_count).into_par_iter().map(|i| {
    let img = image.clone();  // Cheap clone (Arc)
    // Use img
})
```

### Testing

```bash
# Create simple test image
# Run frame generation
cargo run -- ... (with test image)

# Benchmark frame generation
cargo bench frame_generation
```

### Performance Targets

| Metric | Target | Notes |
|---|---|---|
| Per-image frames | < 2 seconds | 50 frames from 1920x1080 |
| Memory per image | < 500 MB | All frames in memory briefly |
| Parallelism | 8-16x | Use all CPU cores |

### Files Modified

- `src/image/mod.rs` - Frame renderer
- Create `src/image/renderer.rs` - Detailed implementation
- Create `src/image/compositor.rs` - Canvas compositing

---

## Phase 5: FFmpeg Coordination (4-5 days)

**Goal**: Spawn FFmpeg and stream frames to it via pipe  
**Current Status**: Stub in place

### Tasks

- [ ] Spawn FFmpeg process with correct arguments
- [ ] Open stdin pipe for frame data
- [ ] Stream encoded frames to FFmpeg
- [ ] Handle audio (if file has audio)
- [ ] Capture output and errors
- [ ] Handle FFmpeg progress reporting

### Architecture

```
Frame Renderer (produces frames)
    ↓ (pipe)
FFmpeg Process
    ├─ Input: RGB frames via stdin
    ├─ Processing: Encode to H.265
    └─ Output: MP4 file
```

### Implementation

**File**: `src/ffmpeg/process.rs`

```rust
pub struct FFmpegEncoder {
    width: u32,
    height: u32,
    fps: u32,
    quality: u32,
}

impl FFmpegEncoder {
    pub async fn encode(
        &self,
        mut frame_receiver: FrameReceiver,  // async channel
        output_path: &Path,
    ) -> Result<()> {
        let mut child = Command::new("ffmpeg")
            .arg("-f").arg("rawvideo")
            .arg("-pixel_format").arg("rgb24")
            .arg("-video_size").arg(format!("{}x{}", self.width, self.height))
            .arg("-framerate").arg(self.fps.to_string())
            .arg("-i").arg("pipe:")
            // Output args
            .arg("-c:v").arg("libx265")
            .arg("-crf").arg(self.quality.to_string())
            .arg("-preset").arg("medium")
            .arg(output_path)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut stdin = child.stdin.take().unwrap();

        // Stream frames to FFmpeg
        while let Some(frame_data) = frame_receiver.recv().await {
            stdin.write_all(&frame_data).await?;
        }
        drop(stdin);  // Signal EOF

        // Wait for encoding to complete
        let output = child.wait_with_output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SlideshowError::Ffmpeg(stderr.to_string()));
        }

        Ok(())
    }
}
```

### Testing

```bash
# Create test frames
# Pipe to FFmpeg
# Verify output video

cargo test ffmpeg
```

### Performance Targets

| Metric | Target | Notes |
|---|---|---|
| Encoding time | 1 second per 30 frames | Depends on preset/quality |
| Streaming lag | < 100ms | Frames available faster than encoder consumes |

### Files Modified

- `src/ffmpeg/process.rs` - Process management
- Create `src/ffmpeg/encoder.rs` - FFmpeg command building

---

## Phase 6: Pipeline Orchestration (3-4 days)

**Goal**: Connect all components into complete pipeline  
**Current Status**: Stub in place

### Tasks

- [ ] Implement main pipeline orchestration
- [ ] Connect media loader → frame generator → FFmpeg
- [ ] Handle multiple images in sequence
- [ ] Handle multiple output definitions
- [ ] Progress reporting
- [ ] Error recovery

### Architecture

```
For each media file:
    ├─ Load image
    └─ For each output definition:
        ├─ Calculate transforms
        ├─ Generate frames (parallel)
        └─ Stream to FFmpeg encoder
            └─ Output video file

Final step:
    └─ Concatenate all videos with transitions
```

### Implementation

**File**: `src/pipeline/frame_pipeline.rs`

```rust
pub struct FrameGenerationPipeline {
    album: Album,
    outputs: Vec<OutputDef>,
    processing: ProcessingConfig,
}

impl FrameGenerationPipeline {
    pub async fn execute(&self) -> Result<()> {
        let total_tasks = self.album.media_files.len() * self.outputs.len();
        let mut completed = 0;

        for media_file in &self.album.media_files {
            // Skip videos for now (handle in future phase)
            if media_file.file_type == MediaType::Video {
                continue;
            }

            for output_def in &self.outputs {
                // 1. Load image
                let image = image::open(&media_file.path)?;

                // 2. Create renderer
                let renderer = FrameRenderer::new(image, output_def);

                // 3. Generate frames (automatically parallel)
                let frames = renderer.render_frames()?;

                // 4. Create FFmpeg encoder
                let encoder = FFmpegEncoder::new(output_def);

                // 5. Stream to encoder
                encoder.encode(frames.into_iter(), output_path).await?;

                completed += 1;
                log::info!(
                    "Progress: {}/{} ({:.1}%)",
                    completed,
                    total_tasks,
                    (completed as f64 / total_tasks as f64) * 100.0
                );
            }
        }

        Ok(())
    }
}
```

### Files Modified

- `src/pipeline/frame_pipeline.rs` - Orchestration
- Create `src/pipeline/encoding_pipeline.rs` - Encoding coordination
- Create `src/pipeline/job.rs` - Job tracking

---

## Phase 7: Testing & Optimization (4-5 days)

**Goal**: Comprehensive testing, performance benchmarking, optimization  
**Current Status**: Minimal tests in place

### Tasks

- [ ] Unit tests for all modules (target: >80% coverage)
- [ ] Integration tests (end-to-end)
- [ ] Performance benchmarks
- [ ] Memory profiling
- [ ] Error recovery testing
- [ ] Documentation

### Testing Checklist

```bash
# Unit tests
cargo test --lib

# Integration tests
cargo test --test '*'

# Benchmarks
cargo bench

# Code coverage (requires llvm-cov)
cargo llvm-cov

# Memory profiling
cargo test --release 2>&1 | less

# Check for warnings
cargo clippy
cargo fmt --check
```

### Benchmark Template

**File**: `benches/frame_gen.rs`

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use slideshow_core::image::FrameRenderer;

fn bench_frame_generation(c: &mut Criterion) {
    let image = image::open("test.jpg").unwrap();
    let params = RenderParams::default();
    let renderer = FrameRenderer::new(image, params);

    c.bench_function("50 frames", |b| {
        b.iter(|| renderer.render_frames())
    });
}

criterion_group!(benches, bench_frame_generation);
criterion_main!(benches);
```

### Performance Targets

| Operation | Target | Current (PS) |
|---|---|---|
| 50 frames from image | 2 seconds | 45 seconds |
| 2000-image album | 1.5-2.5 hours | 12 hours |
| Memory usage | 500 MB | 2 GB |

---

## Phase 8: Advanced Features (2-3 days) - Optional

**Goal**: Add features beyond basic MVP  
**If performance targets met, do these**

### Optional Tasks

- [ ] Video processing (extract frames from videos)
- [ ] Audio handling (if not working)
- [ ] Transition effects (xfade, dissolve)
- [ ] Multiple output grouping
- [ ] Final concatenation

### Files

- `src/video/codec.rs` - Video handling
- `src/video/transition.rs` - Transition effects
- `src/pipeline/concat_pipeline.rs` - Final concatenation

---

## Phase 9: Release & Documentation (2 days)

**Goal**: Polish, document, prepare for production  
**Status**: Not started

### Tasks

- [ ] Complete README.md (Rust-specific)
- [ ] API documentation (cargo doc)
- [ ] Installation guide
- [ ] PowerShell wrapper script
- [ ] Config conversion tool
- [ ] Example configs
- [ ] Performance comparison
- [ ] Release build & optimization

### Release Build

```bash
# Build optimized release
cargo build --release

# Output: target/release/slideshow.exe (~40 MB)

# Create installer (optional)
# Or distribute as standalone exe
```

### Documentation

```bash
# Generate docs
cargo doc --open

# Should show well-documented API
```

---

## Week-by-Week Schedule

### Week 1: Foundation
- **Days 1-2**: Setup & Configuration (Phase 1)
- **Days 3-4**: Media Loading (Phase 2)
- **Days 5**: Transform Math (Phase 3)
- **Deliverable**: Can load media and calculate transforms

### Week 2: Image Processing
- **Days 1-2**: Frame Generation (Phase 4) - Part A
- **Days 3-4**: Frame Generation (Phase 4) - Part B
- **Days 5**: Optimization & Testing
- **Deliverable**: Can generate frames in parallel

### Week 3: Video Output
- **Days 1-2**: FFmpeg Integration (Phase 5)
- **Days 3-4**: Pipeline Orchestration (Phase 6)
- **Days 5**: Testing & Debugging
- **Deliverable**: End-to-end processing works

### Week 4: Optimization
- **Days 1-3**: Performance Optimization
- **Days 4-5**: Benchmarking & Validation
- **Deliverable**: 5-8x improvement confirmed

### Week 5: Polish (Optional)
- **Days 1-3**: Advanced features or optimization
- **Days 4-5**: Documentation & Release
- **Deliverable**: Production-ready release

---

## Key Decision Points

### If Phase 4 (Frame Generation) Is Still Slow

**Options**:
1. Increase rayon parallelism (already using all cores)
2. Optimize image encoding (switch to fast JPEG encoder)
3. Pre-allocate buffers (avoid allocations in hot loop)
4. Consider GPU acceleration (Phase 2 future work)

### If FFmpeg Encoding Is Bottleneck

**Options**:
1. Lower quality (CRF 30 instead of 28)
2. Change preset (medium instead of slow)
3. Use H.264 instead of H.265 (faster, larger file)
4. GPU-accelerated encoding (if GPU available)

### If Memory Usage Is High

**Options**:
1. Stream frames directly (don't buffer all in memory)
2. Process images in batches
3. Use smaller frame dimensions for intermediate processing

---

## Git Workflow

### Branch Management

```bash
# Current: rust-rewrite branch
git branch
# Output: * rust-rewrite
#           dev
#           main

# Make commits on rust-rewrite
git add .
git commit -m "Phase X: ..."

# When ready to merge
git checkout dev
git merge rust-rewrite --ff-only

# If conflicts, resolve them and commit
```

### Commit Message Format

```
Phase X: Brief description

- Implement feature A
- Add tests for feature B
- Fix issue C

Performance impact:
- Frame generation: ~X ms per frame
- Memory: ~X MB
```

---

## Troubleshooting

### Build Errors

```bash
# Check Rust version
rustc --version

# Update Rust
rustup update

# Clear build cache
cargo clean
cargo build
```

### FFmpeg Issues

```bash
# Check ffmpeg is available
ffmpeg -version
ffprobe -version

# If not found, add to PATH
```

### Performance Issues

```bash
# Profile with release build
cargo build --release
time cargo run --release -- ...

# Check CPU usage
# Windows: Task Manager
# Linux: top
```

---

## Success Metrics

### Must Have
- ✅ Configuration loading works
- ✅ Media scanning works
- ✅ Frame generation works
- ✅ FFmpeg encoding works
- ✅ End-to-end pipeline works
- ✅ 5-8x improvement achieved

### Should Have
- ✅ >80% test coverage
- ✅ Comprehensive error handling
- ✅ Good performance benchmarks
- ✅ Documentation complete

### Nice to Have
- ✅ Advanced features (transitions, audio)
- ✅ GPU acceleration path
- ✅ Distributed processing support

---

## Contact & Questions

If stuck on a phase:
1. Check `RUST_IMPLEMENTATION_SPEC.md` for detailed design
2. Review code comments
3. Search Rust documentation
4. Check crate documentation (cargo doc)
5. Run tests to see expected behavior
