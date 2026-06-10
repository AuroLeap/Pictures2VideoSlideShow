# Rust Slideshow Engine - Technical Specification

> **Design doc.** Describes the intended architecture/data model. For the authoritative **commands, config fields, and recommended settings**, use the **[Quick Reference](../docs/quick-reference.md)** (single source of truth); `src/config/mod.rs` is the config schema of record. Code/config samples below are design sketches and may differ from shipped code (e.g. the engine defaults to **H.264 / yuv420p / +faststart**, not the H.265 shown in some samples here).

## Project Overview

**Goal**: Rewrite Pictures2VideoSlideShow in Rust for **5-8x performance improvement** while maintaining feature parity with PowerShell version.

**Expected Performance**:
- Current (PowerShell): 12 hours for 2000 images
- Target (Rust): 1.5-2.5 hours for 2000 images
- Per-image improvement: ~5-8x

**Implementation Timeline**: 3-5 weeks

---

## Architecture Overview

### High-Level Flow

```
INPUT: Photo/Video Collection
    ↓
┌─────────────────────────────────────────────┐
│         Slideshow Engine (Rust)             │
├─────────────────────────────────────────────┤
│                                             │
│  1. Media Loader                            │
│     ├─ Scan directories (parallel)          │
│     ├─ Filter by config                     │
│     └─ Index all media files                │
│                                             │
│  2. Frame Generator                         │
│     ├─ Load image                           │
│     ├─ Calculate transforms (zoom/rot/pan)  │
│     ├─ Render frames (rayon parallel)       │
│     └─ Stream to encoder                    │
│                                             │
│  3. FFmpeg Coordinator                      │
│     ├─ Spawn encoder process                │
│     ├─ Stream frames via pipe               │
│     ├─ Handle audio (if available)          │
│     └─ Manage output streams                │
│                                             │
│  4. Video Concatenator                      │
│     ├─ Create transition frames             │
│     ├─ Coordinate FFmpeg concat             │
│     └─ Output final videos                  │
│                                             │
└─────────────────────────────────────────────┘
    ↓
OUTPUT: Slideshow Videos (1+ MP4 files)
```

### Key Improvements Over PowerShell

| Aspect | PowerShell | Rust |
|---|---|---|
| **Frame Generation** | ImageMagick CLI (overhead) | Direct image-rs (native) |
| **Parallelism** | Sequential loops | rayon (automatic thread pool) |
| **Memory Usage** | High (CLI processes) | Low (direct memory) |
| **I/O Handling** | Blocking only | Async (tokio) + streaming |
| **Error Handling** | Limited | Comprehensive (Result types) |
| **Performance** | 1x baseline | 5-8x faster |

---

## Project Structure

```
slideshow-core/
├── Cargo.toml                           # Rust dependencies & config
├── src/
│   ├── main.rs                         # CLI entry point
│   ├── lib.rs                          # Library exports
│   ├── config/
│   │   ├── mod.rs                      # Config module
│   │   ├── parser.rs                   # Parse TOML/JSON config
│   │   └── models.rs                   # Config data structures
│   ├── media/
│   │   ├── mod.rs                      # Media module
│   │   ├── loader.rs                   # Scan & index files
│   │   ├── filter.rs                   # Apply ignore patterns
│   │   └── models.rs                   # MediaFile, Album structures
│   ├── transform/
│   │   ├── mod.rs                      # Transform module
│   │   ├── geometry.rs                 # Zoom, rotation, pan calculations
│   │   └── models.rs                   # Transform parameter structures
│   ├── image/
│   │   ├── mod.rs                      # Image processing module
│   │   ├── loader.rs                   # Load image files
│   │   ├── renderer.rs                 # Generate frames
│   │   ├── compositor.rs               # Composite onto canvas
│   │   └── effects.rs                  # Distortions, effects
│   ├── video/
│   │   ├── mod.rs                      # Video module
│   │   ├── encoder.rs                  # FFmpeg encoder wrapper
│   │   ├── transition.rs               # Create fade transitions
│   │   ├── concatenator.rs             # Concat with audio
│   │   └── models.rs                   # Video configuration
│   ├── pipeline/
│   │   ├── mod.rs                      # Pipeline orchestration
│   │   ├── frame_pipeline.rs           # Frame generation pipeline
│   │   ├── encoding_pipeline.rs        # Encoding pipeline
│   │   └── job.rs                      # Job management
│   ├── ffmpeg/
│   │   ├── mod.rs                      # FFmpeg wrapper
│   │   ├── process.rs                  # Process spawning & control
│   │   ├── pipe.rs                     # Stream piping
│   │   └── models.rs                   # FFmpeg command builders
│   ├── error.rs                        # Error types
│   ├── logging.rs                      # Logging setup
│   └── util/
│       ├── mod.rs
│       ├── file_utils.rs               # File operations
│       └── progress.rs                 # Progress reporting
├── tests/
│   ├── integration_tests.rs            # End-to-end tests
│   ├── fixtures/                       # Test data
│   └── golden/                         # Expected outputs
├── benches/
│   └── benchmarks.rs                   # Performance benchmarks
├── .gitignore
├── README.md                           # Rust-specific docs
└── IMPLEMENTATION_LOG.md               # Development progress

powershell/
├── Config2TOML.ps1                     # Convert old config to TOML
└── Slideshow.ps1                       # PowerShell wrapper (thin)
```

---

## Core Components

### 1. Configuration System

**File**: `src/config/models.rs`

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub input: InputConfig,
    pub output: OutputConfig,
    pub processing: ProcessingConfig,
    pub outputs: Vec<OutputDef>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct InputConfig {
    pub media_root: PathBuf,
    pub ignore_patterns: Vec<String>,
    pub exception_pattern: Option<String>,
    pub exception_threshold: Option<i32>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OutputDef {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pic_display_time_secs: f32,
    pub fade_time_secs: f32,
    pub max_rotation_degrees: f32,
    pub bulk_video_time_min: u32,
    pub quality_crf: u32,
    pub enable_audio: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProcessingConfig {
    pub temp_dir: Option<PathBuf>,
    pub max_workers: Option<usize>,
    pub use_parallelism: bool,
    pub dry_run: bool,
    pub verbose: bool,
}
```

**Supported Config Formats**:
- TOML (primary)
- JSON (compatible)
- YAML (optional)
- Can convert from old PowerShell format

---

### 2. Media Loading System

**File**: `src/media/loader.rs`

```rust
pub struct MediaLoader {
    config: InputConfig,
}

impl MediaLoader {
    pub async fn scan_and_index(&self) -> Result<Album> {
        // Parallel directory traversal using walkdir + rayon
        let files = walkdir::WalkDir::new(&self.config.media_root)
            .into_iter()
            .par_bridge()  // rayon parallel bridge
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if entry.file_type().is_file() {
                    Some(self.process_file(entry.path()))
                } else {
                    None
                }
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Album {
            media_files: files,
            total_size: calculate_total_size(&files),
            created_at: SystemTime::now(),
        })
    }

    fn should_include(&self, path: &Path) -> bool {
        // Apply ignore patterns, exceptions
        // Returns: true if file should be processed
    }

    pub fn get_file_properties(&self, path: &Path) -> Result<MediaFile> {
        // Determine if image or video
        // Extract metadata
        // Get dimensions, duration, etc.
    }
}

pub struct Album {
    pub media_files: Vec<MediaFile>,
    pub total_size: u64,
    pub created_at: SystemTime,
}

#[derive(Debug, Clone)]
pub struct MediaFile {
    pub path: PathBuf,
    pub file_type: MediaType,
    pub dimensions: (u32, u32),
    pub duration_secs: Option<f32>,
    pub size_bytes: u64,
    pub created_date: SystemTime,
}

#[derive(Debug, Clone, Copy)]
pub enum MediaType {
    Image(ImageFormat),
    Video(VideoFormat),
}
```

---

### 3. Image Frame Generation

**File**: `src/image/renderer.rs`

**Key Concept**: Generate all frames for one image in parallel using rayon.

```rust
pub struct FrameRenderer {
    image: DynamicImage,
    params: RenderParams,
}

#[derive(Debug, Clone)]
pub struct RenderParams {
    pub output_width: u32,
    pub output_height: u32,
    pub frame_count: u32,
    pub fps: u32,
    pub display_time_secs: f32,
    pub fade_time_secs: f32,
    pub max_rotation: f32,
    pub zoom_start: f32,
    pub zoom_rate: f32,
    pub quality: u32,
}

impl FrameRenderer {
    pub fn render_frames(&self) -> Result<FrameSequence> {
        let transforms = self.calculate_all_transforms();

        // Parallel frame generation using rayon
        let frames: Vec<Frame> = (0..self.params.frame_count)
            .into_par_iter()  // rayon parallel iterator
            .map(|frame_idx| {
                self.render_single_frame(
                    frame_idx,
                    &transforms[frame_idx as usize],
                )
            })
            .collect::<Result<_>>()?;

        Ok(FrameSequence {
            frames,
            fps: self.params.fps,
            duration_secs: (frames.len() as f32) / (self.params.fps as f32),
        })
    }

    fn calculate_all_transforms(&self) -> Vec<FrameTransform> {
        let fade_frames = (self.params.fade_time_secs * self.params.fps as f32) as u32;
        let display_frames = (self.params.display_time_secs * self.params.fps as f32) as u32;
        let total_frames = (fade_frames * 2) + display_frames;

        (0..self.params.frame_count)
            .map(|i| {
                let zoom = self.calculate_zoom(i);
                let rotation = self.calculate_rotation(i, total_frames);
                let pan = self.calculate_pan(i, total_frames);

                FrameTransform { zoom, rotation, pan }
            })
            .collect()
    }

    fn render_single_frame(
        &self,
        frame_idx: u32,
        transform: &FrameTransform,
    ) -> Result<Frame> {
        let mut frame = self.image.clone();

        // Apply transformations using imageproc
        frame = self.apply_transforms(&frame, transform)?;

        // Composite onto canvas with background
        let output = self.composite_on_canvas(&frame)?;

        // Encode to JPEG with quality setting
        let encoded = self.encode_jpeg(&output)?;

        Ok(Frame {
            index: frame_idx,
            data: encoded,
            format: ImageFormat::Jpeg,
        })
    }

    fn apply_transforms(
        &self,
        image: &DynamicImage,
        transform: &FrameTransform,
    ) -> Result<DynamicImage> {
        // Apply zoom using resize
        let zoomed = imageproc::geometric_transformations::rotate_around_center(
            image,
            transform.rotation,
            imageproc::utils::Interpolation::Bilinear,
            image::Rgba([0, 0, 0, 255]),
        );

        // Apply pan (offset)
        // Apply zoom (scale)
        Ok(zoomed)
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub index: u32,
    pub data: Vec<u8>,
    pub format: ImageFormat,
}

#[derive(Debug, Clone)]
pub struct FrameSequence {
    pub frames: Vec<Frame>,
    pub fps: u32,
    pub duration_secs: f32,
}
```

---

### 4. FFmpeg Coordinator

**File**: `src/ffmpeg/process.rs`

```rust
pub struct FFmpegEncoder {
    input_stream: FrameStreamReader,
    config: VideoConfig,
}

#[derive(Debug, Clone)]
pub struct VideoConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: String,      // "5M", "10M", etc.
    pub preset: Preset,       // slow, medium, fast
    pub crf: u32,            // 18-28 for quality
    pub codec: VideoCodec,    // h264, h265, vp9
}

#[derive(Debug, Clone, Copy)]
pub enum Preset {
    Slow,
    Medium,
    Fast,
}

pub enum VideoCodec {
    H264,
    H265,
    VP9,
}

impl FFmpegEncoder {
    pub async fn encode(
        &mut self,
        frame_source: FrameSource,
    ) -> Result<EncodingProgress> {
        // Spawn ffmpeg process
        let mut child = Command::new("ffmpeg")
            .arg("-f").arg("rawvideo")
            .arg("-video_size").arg(format!("{}x{}", self.config.width, self.config.height))
            .arg("-pixel_format").arg("rgb24")
            .arg("-framerate").arg(self.config.fps.to_string())
            .arg("-i").arg("pipe:")
            .args(self.build_output_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut stdin = child.stdin.take().ok_or(anyhow!("Failed to open stdin"))?;

        // Stream frames to ffmpeg
        tokio::spawn(async move {
            while let Some(frame) = frame_source.next().await {
                stdin.write_all(&frame.data).await?;
            }
            Ok::<(), anyhow::Error>(())
        });

        // Wait for encoding
        let output = child.wait_with_output().await?;

        Ok(EncodingProgress {
            success: output.status.success(),
            file_size: get_output_file_size()?,
        })
    }

    fn build_output_args(&self) -> Vec<String> {
        vec![
            "-c:v".to_string(),
            match self.config.codec {
                VideoCodec::H264 => "libx264".to_string(),
                VideoCodec::H265 => "libx265".to_string(),
                VideoCodec::VP9 => "libvpx-vp9".to_string(),
            },
            "-crf".to_string(),
            self.config.crf.to_string(),
            "-preset".to_string(),
            match self.config.preset {
                Preset::Slow => "slow".to_string(),
                Preset::Medium => "medium".to_string(),
                Preset::Fast => "fast".to_string(),
            },
        ]
    }
}

pub struct FrameSource {
    frames: Vec<Frame>,
    current_index: usize,
}

impl FrameSource {
    pub async fn next(&mut self) -> Option<Frame> {
        if self.current_index < self.frames.len() {
            let frame = self.frames[self.current_index].clone();
            self.current_index += 1;
            Some(frame)
        } else {
            None
        }
    }
}
```

---

### 5. Pipeline Orchestration

**File**: `src/pipeline/frame_pipeline.rs`

```rust
pub struct FrameGenerationPipeline {
    album: Album,
    config: Vec<OutputDef>,
}

impl FrameGenerationPipeline {
    pub async fn execute(self) -> Result<()> {
        // For each image in album
        for media_file in self.album.media_files {
            if media_file.file_type == MediaType::Image {
                // For each output definition
                for output_def in &self.config {
                    self.process_image_for_output(&media_file, output_def).await?;
                }
            }
        }

        Ok(())
    }

    async fn process_image_for_output(
        &self,
        media: &MediaFile,
        output_def: &OutputDef,
    ) -> Result<()> {
        // Load image
        let image = image::open(&media.path)?;

        // Create renderer
        let renderer = FrameRenderer {
            image,
            params: RenderParams::from(output_def),
        };

        // Generate frames (parallel, automatically uses all cores)
        let frame_sequence = renderer.render_frames()?;

        // Stream to encoder
        self.stream_to_encoder(frame_sequence, output_def).await?;

        Ok(())
    }

    async fn stream_to_encoder(
        &self,
        frames: FrameSequence,
        output_def: &OutputDef,
    ) -> Result<()> {
        let config = VideoConfig {
            width: output_def.width,
            height: output_def.height,
            fps: output_def.fps,
            crf: output_def.quality_crf,
            preset: Preset::Medium,
            codec: VideoCodec::H265,
        };

        let encoder = FFmpegEncoder {
            input_stream: FrameStreamReader::new(frames),
            config,
        };

        encoder.encode().await?;

        Ok(())
    }
}
```

---

## Performance Optimizations

### 1. Parallelism (rayon)

**Where**: Frame generation, directory scanning, transform calculation

```rust
// Single image, all frames in parallel
(0..frame_count)
    .into_par_iter()
    .map(|i| render_frame(i))
    .collect()
```

**Expected Speedup**: 4-8x on 8-16 core systems

### 2. Streaming I/O (tokio)

**Where**: FFmpeg coordination, writing output files

```rust
// Async frame streaming to encoder
while let Some(frame) = frame_source.next().await {
    encoder_stdin.write_all(&frame.data).await?;
}
```

**Expected Speedup**: 1.5-2x (eliminates buffering)

### 3. Memory Efficiency

**Techniques**:
- Image loaded once, reused for all frames (not per-frame)
- Frames streamed to encoder (not buffered in memory)
- Iterator-based processing (lazy evaluation)

**Expected Benefit**: Lower memory footprint, better cache locality

### 4. No CLI Overhead

**Current (PowerShell)**: `magick` CLI invoked 50-200 times per image = process overhead
**New (Rust)**: Direct library calls, single process

**Expected Speedup**: 2-3x from process overhead reduction

---

## Implementation Phases

### Phase 1: Core Architecture (Week 1)
- [ ] Rust project setup
- [ ] Config system (TOML parsing)
- [ ] Media loader (parallel directory scanning)
- [ ] Error handling framework
- [ ] Logging setup

**Deliverable**: Can load media files and enumerate them

---

### Phase 2: Image Processing (Week 2)
- [ ] Transform calculation (geometry math)
- [ ] Frame rendering (image-rs + rayon)
- [ ] Basic compositor (canvas + composite)
- [ ] Unit tests for transforms

**Deliverable**: Can generate frame sequences from images

---

### Phase 3: Video Encoding (Week 2-3)
- [ ] FFmpeg process spawning
- [ ] Streaming frames to encoder
- [ ] Output file management
- [ ] Error handling for encoding

**Deliverable**: Can generate complete videos from frames

---

### Phase 4: Pipeline & Orchestration (Week 3)
- [ ] Frame generation pipeline
- [ ] Encoding pipeline
- [ ] Job management
- [ ] Progress reporting

**Deliverable**: Complete end-to-end processing for single image set

---

### Phase 5: Advanced Features (Week 4-5)
- [ ] Transitions (xfade filters)
- [ ] Audio handling
- [ ] Multiple output definitions
- [ ] Concatenation

**Deliverable**: Feature-complete (matches PowerShell version)

---

### Phase 6: Optimization & Polish (Week 5)
- [ ] Performance benchmarking
- [ ] Parallelism tuning
- [ ] Error recovery
- [ ] Documentation
- [ ] Integration tests

**Deliverable**: Production-ready, 5-8x faster

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zoom_calculation() {
        let zoom = calculate_zoom(50, 100, 1.1, 0.01);
        assert!((zoom - expected).abs() < 0.01);
    }

    #[test]
    fn test_rotation_deceleration() {
        let rot = calculate_rotation(0, 100, 30.0);
        assert!(rot > 0.0 && rot <= 30.0);
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_full_pipeline() {
    let config = load_test_config();
    let album = load_test_album();
    let result = run_pipeline(album, config).await;
    assert!(result.is_ok());
    assert!(output_video_exists());
}
```

### Benchmarks

```rust
#[bench]
fn bench_frame_generation(b: &mut Bencher) {
    let image = load_test_image();
    let renderer = FrameRenderer::new(image, test_params());

    b.iter(|| renderer.render_frames())
}
```

---

## Build & Deployment

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version
cargo --version
```

### Building

```bash
# Debug build (fast compile, slow runtime)
cargo build

# Release build (slow compile, fast runtime, 10-20x faster)
cargo build --release

# With optimizations (LTO, single codegen unit)
# Configured in Cargo.toml [profile.release]
```

### Output

- **Binary**: `target/release/slideshow.exe` (Windows)
- **Size**: ~30-50 MB (includes all dependencies)
- **Dependencies**: FFmpeg, ImageMagick (optional, falls back to pure Rust)

### Deployment

```powershell
# Option 1: Replace PowerShell scripts with executable
.\slideshow.exe --config config.toml --input "C:\Photos" --output "C:\Videos"

# Option 2: Keep PowerShell wrapper (thin layer)
# Slideshow.ps1 → calls slideshow.exe → returns results
```

---

## Configuration File Format

### Example `slideshow.toml`

```toml
[input]
media_root = "\\nas\Photos"
ignore_patterns = ["DNP", "PrivateVideos", "*_noprocess"]
exception_pattern = "^(19|20)\\d{2}"  # Include files with year > 999
exception_threshold = 1000

[processing]
temp_dir = "R:"  # RAM disk path (optional)
max_workers = 16  # Thread count (auto-detect if not set)
use_parallelism = true
verbose = false
dry_run = false

[[outputs]]
name = "LivingRoom"
width = 1440
height = 900
fps = 30
pic_display_time_secs = 6.0
fade_time_secs = 0.5
max_rotation_degrees = 15.0
bulk_video_time_min = 20
quality_crf = 28
enable_audio = false

[[outputs]]
name = "Bedroom"
width = 1920
height = 1080
fps = 24
pic_display_time_secs = 8.0
fade_time_secs = 0.7
max_rotation_degrees = 20.0
bulk_video_time_min = 30
quality_crf = 26
enable_audio = false
```

---

## Error Handling Strategy

**Use Rust's Result type for all fallible operations:**

```rust
type Result<T> = std::result::Result<T, SlideshowError>;

#[derive(Debug, thiserror::Error)]
pub enum SlideshowError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("Config error: {0}")]
    Config(String),

    #[error("FFmpeg error: {0}")]
    Ffmpeg(String),

    #[error("Transform error: {0}")]
    Transform(String),
}
```

**Comprehensive error context:**
- Where did it fail (file, function)
- Why did it fail (specific error)
- What should user do (suggestion)

---

## Performance Targets

| Metric | Current (PS) | Target (Rust) | Improvement |
|---|---|---|---|
| **20-image album** | 3 hours | 25 minutes | 7.2x |
| **2000-image album** | 12 hours | 2 hours | 6x |
| **5000-image album** | 30 hours | 5 hours | 6x |
| **Memory usage** | ~2 GB | ~500 MB | 4x |
| **CPU efficiency** | 40% | 85% | Better |
| **Frame generation** | 60% of time | 35% of time | Improved bottleneck |

---

## Known Limitations & Future Work

### Current Release (1.0)
- ✅ Single output definition
- ✅ Image processing (JPEG, PNG)
- ✅ H.264/H.265 encoding
- ⚠️ Audio (basic support)
- ❌ GPU acceleration (Phase 2)
- ❌ Distributed processing (Phase 2)

### Future Enhancements
- **GPU Frame Generation** (CUDA/Vulkan) - additional 5-15x
- **Streaming Transitions** - better I/O
- **Real-time Progress UI** - web dashboard
- **Distributed Encoding** - multi-machine support
- **Cloud Integration** - AWS/GCP support

---

## Success Criteria

✅ **Performance**:
- [ ] 5-8x improvement over PowerShell version
- [ ] 2000-image album in under 3 hours
- [ ] Sub-1-hour processing for 500-image album

✅ **Correctness**:
- [ ] Output videos match PowerShell version quality
- [ ] All images/videos process without errors
- [ ] Audio sync (when enabled)

✅ **Quality**:
- [ ] >80% test coverage
- [ ] Zero unsafe code (except FFmpeg interop)
- [ ] Comprehensive error handling
- [ ] Good error messages

✅ **Usability**:
- [ ] Config file format is clear
- [ ] CLI arguments are intuitive
- [ ] PowerShell wrapper available for legacy users
- [ ] Documentation complete

---

## References

### Rust Resources
- **Rust Book**: https://doc.rust-lang.org/book/
- **Tokio Guide**: https://tokio.rs/tokio/tutorial
- **rayon**: https://docs.rs/rayon/

### Image Processing
- **image crate**: https://docs.rs/image/
- **imageproc**: https://docs.rs/imageproc/

### FFmpeg Integration
- **FFmpeg docs**: https://ffmpeg.org/ffmpeg.html
- **ffmpeg-next crate**: https://docs.rs/ffmpeg-next/

---

## Glossary

| Term | Meaning |
|---|---|
| **rayon** | Data parallelism library (automatic thread pool) |
| **tokio** | Async runtime (for async/await) |
| **FFmpeg** | External tool for video encoding |
| **CRF** | Constant Rate Factor (quality setting) |
| **Preset** | Speed/quality tradeoff (slow/medium/fast) |
| **Codec** | Video compression algorithm (H.264, H.265, VP9) |
| **xfade** | FFmpeg transition filter |

