//! Binary entry point (`make_video_slideshow`): CLI parsing (build /
//! validate / setup), the first-run wizard trigger, and the thin
//! orchestration that wires config → pipeline.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod cache;
mod config;
mod error;
mod ffmpeg;
mod image;
mod logging;
mod media;
mod pipeline;
mod preflight;
mod roi;
mod setup;
mod transform;
mod util;
mod video;

use config::Config;
use logging::init_logging;
use std::io::IsTerminal;
use util::estimate::human_bytes;

#[derive(Parser, Debug)]
#[command(name = "slideshow")]
#[command(about = "Convert photos/videos to slideshow for digital frames", long_about = None)]
struct Args {
    /// Configuration file path (TOML/JSON). When omitted, defaults to the
    /// location beside the exe (or %APPDATA%); a first run with no config there
    /// opens the setup wizard. (SR-030)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Input media directory
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Output video directory
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Dry run (plan without executing)
    #[arg(long)]
    dry_run: bool,

    /// Never prompt or open the setup GUI; fail fast instead of waiting for
    /// input. For automation, CI, and scheduled runs. (SR-028)
    #[arg(long)]
    non_interactive: bool,

    /// Bypass the SR-037 segment cache entirely: the build neither reads nor
    /// writes cached segments (the pre-cache streaming pipeline).
    // Implements: LLR-054, SR-037
    #[arg(long)]
    no_cache: bool,

    /// Empty the segment cache before building.
    // Implements: LLR-054, SR-037
    #[arg(long)]
    clear_cache: bool,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run full slideshow generation pipeline
    Build {
        /// Only process specific output definition
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Validate configuration file
    Validate,
    /// Show project statistics
    Stats,
    /// Benchmark performance
    Bench {
        /// Number of images to benchmark with
        #[arg(short, long, default_value = "50")]
        images: usize,

        /// Full end-to-end bench (SR-036): build the corpus for real and emit
        /// `docs/test/perf-metrics.json` (PB-ID -> number) for check_perf.py.
        /// `Scripts/bench.ps1` is the canonical wrapper for comparable numbers.
        #[arg(long)]
        full: bool,

        /// Corpus directory for --full. Default: `TestInput/` when present,
        /// else PNGs synthesized into a temp dir.
        #[arg(long)]
        corpus: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(args.verbose)?;

    log::info!("Slideshow Engine v0.1.0 starting...");
    log::debug!("Arguments: {:?}", args);

    let non_interactive = args.non_interactive;

    // Resolve the config path: explicit --config, else the location beside the
    // exe (or %APPDATA%). (SR-030)
    let config_path = args
        .config
        .clone()
        .unwrap_or_else(config::location::resolve_config_path);

    // First-run gating (SR-028): only launch the setup wizard when there is no
    // config AND this is a genuine interactive session. Automation/CI never
    // blocks here (it gets a clear "config not found" error from the load below).
    if setup::interaction::should_prompt(
        config_path.exists(),
        non_interactive,
        std::io::stdin().is_terminal(),
    ) {
        log::info!(
            "No config at {} — launching first-run setup...",
            config_path.display()
        );
        setup::run_first_run_setup(&config_path)?;
    }

    // Load configuration
    let config = Config::from_file(&config_path)?;
    log::info!("Configuration loaded from: {}", config_path.display());

    // Override with command-line args if provided
    let mut config = config;
    if let Some(input) = args.input {
        config.input.media_root = input;
    }
    if let Some(output) = args.output {
        config.output.base_dir = output;
    }
    if args.dry_run {
        config.processing.dry_run = true;
    }

    // Execute command
    match args.command {
        None | Some(Commands::Build { .. }) => {
            log::info!("Starting slideshow generation pipeline...");
            run_build(&config, non_interactive, args.no_cache, args.clear_cache).await?;
        }
        Some(Commands::Validate) => {
            log::info!("Validating runtime prerequisites...");
            run_validate(&config);
        }
        Some(Commands::Stats) => {
            log::info!("Collecting project statistics...");
            run_stats(&config).await?;
        }
        Some(Commands::Bench {
            images,
            full,
            corpus,
        }) => {
            if full {
                log::info!("Running full end-to-end bench...");
                run_bench_full(&config, corpus, non_interactive).await?;
            } else {
                log::info!("Running benchmark with {} images...", images);
                run_bench(&config, images).await?;
            }
        }
    }

    log::info!("Slideshow generation complete!");
    Ok(())
}

/// Run the `validate` command: probe each runtime prerequisite, print one
/// plain pass/fail line each, and exit non-zero if any essential check fails.
// Implements: LLR-004, SR-002
fn run_validate(config: &Config) {
    let results = preflight::run_checks(config);
    print_check_results(&results);
    // Report where the config is/will be stored (SR-030) so the user knows
    // which file the wizard writes and subsequent runs read.
    println!(
        "Config location: {}",
        config::location::resolve_config_path().display()
    );
    let code = preflight::exit_code(&results);
    if code == 0 {
        println!("All prerequisites passed.");
    } else {
        println!("One or more essential prerequisites failed; build cannot run.");
    }
    std::process::exit(code);
}

/// Print one labeled `[PASS]`/`[FAIL]` line per prerequisite.
// Implements: LLR-004, SR-002
fn print_check_results(results: &[preflight::CheckResult]) {
    for r in results {
        let tag = if r.passed { "PASS" } else { "FAIL" };
        println!("[{}] {}: {}", tag, r.name, r.detail);
    }
}

async fn run_build(
    config: &Config,
    non_interactive: bool,
    no_cache: bool,
    clear_cache: bool,
) -> Result<()> {
    // Resolve FFmpeg (configured path -> PATH -> per-user cache), with the
    // offline/use-existing fallback and a gated auto-fetch. Fails fast with an
    // actionable message in automation rather than blocking.
    // Implements: SR-027, SR-028
    let ffmpeg = setup::ensure_ffmpeg(config, non_interactive)?;
    log::info!("Using FFmpeg: {}", ffmpeg.display());

    // Gate the build on the same runtime-prerequisite set as `validate`; refuse
    // to start (non-zero, no output) when any essential check fails.
    // Implements: LLR-005, SR-002, SR-012
    let checks = preflight::run_checks(config);
    if !preflight::all_essential_passed(&checks) {
        eprintln!("Cannot start build — prerequisite check failed:");
        for r in checks.iter().filter(|r| r.essential && !r.passed) {
            eprintln!("  [FAIL] {}: {}", r.name, r.detail);
        }
        std::process::exit(1);
    }

    // 1. Load media
    log::info!("Loading media files...");
    let media_loader = media::MediaLoader::new(config.input.clone());
    let album = media_loader.scan_and_index().await?;
    log::info!(
        "Found {} media files, total size: {} MB",
        album.media_files.len(),
        album.total_size / (1024 * 1024)
    );

    // 2. Load the optional region-of-interest database (per-image Ken Burns
    //    focus). A missing/invalid file is a hard error so a typo'd path is
    //    not silently ignored.
    let roi = match &config.input.roi_db {
        Some(path) => {
            let db = roi::RoiDb::load(path)?;
            log::info!(
                "Loaded {} ROI focus entries from {}",
                db.len(),
                path.display()
            );
            Some(db)
        }
        None => None,
    };

    // 3. Process all output definitions in one pipeline. The SR-037 segment
    //    cache is the default path; `--no-cache` neither reads nor writes it
    //    (the streaming pipeline); `--clear-cache` empties it first.
    // Implements: SR-037, LLR-054
    let pipeline = pipeline::FrameGenerationPipeline::new(
        album,
        config.outputs.clone(),
        config.processing.clone(),
        config.output.base_dir.clone(),
        config.input.media_root.clone(),
        roi,
    );
    let cache_root = cache::store::SegmentStore::root_for(config.processing.temp_dir.as_deref());
    if clear_cache {
        let mut store = cache::store::SegmentStore::open(&cache_root)?;
        store.clear();
        log::info!("Segment cache cleared: {}", cache_root.display());
    }
    let summary = if no_cache {
        pipeline.execute().await?
    } else {
        let mut store = cache::store::SegmentStore::open(&cache_root)?;
        pipeline.execute_cached(&mut store).await?
    };

    print_completion_summary(&summary);

    // Exit semantics: zero when >=1 output produced, non-zero otherwise.
    // Implements: LLR-017, SR-014
    if summary.written.is_empty() {
        eprintln!(
            "no outputs produced — every input was skipped or no output could be written ({} skipped)",
            summary.skipped.len()
        );
        std::process::exit(1);
    }

    log::info!("All outputs processed successfully");
    Ok(())
}

/// Print the end-of-build summary: each written output + size, the skipped
/// count, and any final file over the ~3.5 GB threshold.
// Implements: LLR-023, LLR-024, LLR-013, SR-004, SR-009
fn print_completion_summary(summary: &pipeline::BuildSummary) {
    println!("\n=== Build Summary ===");
    if summary.written.is_empty() {
        println!("Outputs written: 0");
    } else {
        println!("Outputs written: {}", summary.written.len());
        for o in &summary.written {
            // Cache evidence (SR-037): how much of the output was reused vs
            // re-encoded this build. Absent on the --no-cache path.
            // Implements: LLR-055, SR-037
            match o.cache {
                Some((reused, re_encoded)) => println!(
                    "  {} ({}) — segments: {} reused, {} re-encoded",
                    o.path.display(),
                    human_bytes(o.size_bytes),
                    reused,
                    re_encoded
                ),
                None => println!("  {} ({})", o.path.display(), human_bytes(o.size_bytes)),
            }
        }
    }
    println!("Inputs skipped: {}", summary.skipped.len());
    for s in &summary.skipped {
        println!("  skipped {}: {}", s.path.display(), s.reason);
    }

    let oversize = summary.oversize_outputs();
    if !oversize.is_empty() {
        println!("WARNING: outputs exceeding ~3.5 GB FAT32 limit:");
        for o in oversize {
            println!("  {} ({})", o.path.display(), human_bytes(o.size_bytes));
        }
    }
}

async fn run_stats(config: &Config) -> Result<()> {
    let media_loader = media::MediaLoader::new(config.input.clone());
    let album = media_loader.scan_and_index().await?;

    println!("\n=== Project Statistics ===");
    println!("Media files: {}", album.media_files.len());
    println!("Total size: {} MB", album.total_size / (1024 * 1024));
    println!("Output definitions: {}", config.outputs.len());

    for output_def in &config.outputs {
        println!(
            "  - {}: {}x{} @ {}fps, quality: {}",
            output_def.name,
            output_def.width,
            output_def.height,
            output_def.fps,
            output_def.quality_crf
        );
    }

    Ok(())
}

async fn run_bench(config: &Config, num_images: usize) -> Result<()> {
    let media_loader = media::MediaLoader::new(config.input.clone());
    let album = media_loader.scan_and_index().await?;

    let first_image = album
        .media_files
        .iter()
        .find(|m| m.file_type == media::MediaType::Image)
        .ok_or_else(|| {
            crate::error::SlideshowError::Media("No images found to benchmark".into())
        })?;

    let output_def = config
        .outputs
        .first()
        .ok_or_else(|| crate::error::SlideshowError::InvalidConfig("No output defined".into()))?;

    log::info!(
        "Benchmarking frame generation on {} ({} simulated images)...",
        first_image.path.display(),
        num_images
    );

    let renderer = image::FrameRenderer::load(&first_image.path, output_def)?;
    let frames_per_image = renderer.total_frames();

    let start = std::time::Instant::now();
    // Render the full clip once (this is the real per-image cost).
    let mut total_frames = 0u64;
    let mut next = 0;
    while next < frames_per_image {
        let end = (next + 32).min(frames_per_image);
        total_frames += renderer.render_range(next, end).len() as u64;
        next = end;
    }
    let elapsed = start.elapsed();

    let per_image = elapsed.as_secs_f64();
    let fps = total_frames as f64 / per_image.max(0.001);

    println!("\n=== Benchmark Results ===");
    println!("Image: {}", first_image.path.display());
    println!(
        "Output: {}x{} @ {}fps, {} frames/image",
        output_def.width, output_def.height, output_def.fps, frames_per_image
    );
    println!("Per image: {:.2}s", per_image);
    println!("Frame throughput: {:.1} frames/sec", fps);
    println!(
        "Projected for {} images: {:.1} min",
        num_images,
        per_image * num_images as f64 / 60.0
    );

    Ok(())
}

/// Removes a synthesized bench corpus when the bench is done (best effort).
struct BenchCorpusGuard(PathBuf);

impl Drop for BenchCorpusGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The corpus for `bench --full`: an explicit `--corpus` dir, else `TestInput/`
/// when present, else deterministic PNGs synthesized into a temp dir (removed
/// when the guard drops). Synthesized frames are gradients, not photos — the
/// numbers are plumbing-valid but only `Scripts/bench.ps1` over a real corpus
/// is PB-comparable.
// Implements: LLR-052, SR-036
fn resolve_bench_corpus(explicit: Option<PathBuf>) -> Result<(PathBuf, Option<BenchCorpusGuard>)> {
    if let Some(dir) = explicit {
        if !dir.is_dir() {
            return Err(crate::error::SlideshowError::Media(format!(
                "bench corpus directory not found: {}",
                dir.display()
            ))
            .into());
        }
        return Ok((dir, None));
    }
    let default = PathBuf::from("TestInput");
    if default.is_dir() {
        return Ok((default, None));
    }
    let dir = std::env::temp_dir().join(format!("slideshow_bench_corpus_{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(crate::error::SlideshowError::Io)?;
    for i in 0u32..10 {
        let img = ::image::RgbImage::from_fn(1280, 720, |x, y| {
            ::image::Rgb([(x % 256) as u8, (y % 256) as u8, (i * 25) as u8])
        });
        img.save(dir.join(format!("synth_{:02}.png", i)))
            .map_err(crate::error::SlideshowError::Image)?;
    }
    log::info!(
        "No corpus given and no TestInput/; synthesized 10 PNGs in {}",
        dir.display()
    );
    Ok((dir.clone(), Some(BenchCorpusGuard(dir))))
}

/// `bench --full`: a real end-to-end build of the bench corpus that measures
/// the PB rows and writes `docs/test/perf-metrics.json` (cwd-relative) for
/// `Scripts/check_perf.py`. Measured: PB-001 end-to-end frames/s, PB-002 mean
/// clip-boundary stall ms/photo, PB-003 scan s per 1k files (warm scan served
/// by the SR-038 probe cache, pinned to a fresh temp file per bench run),
/// PB-005 peak working set MB (omitted where unmeasurable). PB-004 (warm
/// rebuild, SR-037) is measured by the `Scripts/bench.ps1` wrapper via timed
/// `build` invocations and merged in there, like PB-006.
// Implements: LLR-052, LLR-051, SR-036
async fn run_bench_full(
    config: &Config,
    corpus: Option<PathBuf>,
    non_interactive: bool,
) -> Result<()> {
    let ffmpeg = setup::ensure_ffmpeg(config, non_interactive)?;
    log::info!("Using FFmpeg: {}", ffmpeg.display());

    let output_def =
        config.outputs.first().cloned().ok_or_else(|| {
            crate::error::SlideshowError::InvalidConfig("No output defined".into())
        })?;

    let (corpus_dir, _synth_guard) = resolve_bench_corpus(corpus)?;

    // Scan twice: cold, then warm. The probe cache (SR-038) is pinned to a
    // fresh temp file so the cold leg is genuinely cold and the warm leg is
    // the honest cache-served number (PB-003) — and the developer's real
    // per-user cache is never touched by a bench.
    let input = config::InputConfig {
        media_root: corpus_dir.clone(),
        ignore_patterns: config.input.ignore_patterns.clone(),
        exception_pattern: None,
        exception_threshold: None,
        roi_db: None,
    };
    let bench_cache = std::env::temp_dir().join(format!(
        "slideshow_bench_probe_cache_{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&bench_cache);
    let loader = media::MediaLoader::new(input).with_probe_cache_path(bench_cache.clone());
    let t = std::time::Instant::now();
    let _cold = loader.scan_and_index().await?;
    let cold_scan = t.elapsed();
    let t = std::time::Instant::now();
    let album = loader.scan_and_index().await?;
    let warm_scan = t.elapsed();
    let _ = std::fs::remove_file(&bench_cache);
    let warm_stats = album.probe_stats;
    let files = album.media_files.len();
    if files == 0 {
        return Err(crate::error::SlideshowError::Media(format!(
            "bench corpus is empty: {}",
            corpus_dir.display()
        ))
        .into());
    }

    // Build the first output definition for real, into a temp dir.
    let out_dir = std::env::temp_dir().join(format!("slideshow_bench_out_{}", std::process::id()));
    let pipeline = pipeline::FrameGenerationPipeline::new(
        album,
        vec![output_def.clone()],
        config.processing.clone(),
        out_dir.clone(),
        corpus_dir.clone(),
        None,
    );
    let t = std::time::Instant::now();
    let summary = pipeline.execute().await?;
    let build_wall = t.elapsed();
    let _ = std::fs::remove_dir_all(&out_dir);
    let ot = summary.timings.first().ok_or_else(|| {
        crate::error::SlideshowError::Media("bench build produced no output".into())
    })?;
    let s = &ot.stages;

    let pb001 = ot.frames as f64 / build_wall.as_secs_f64().max(0.001);
    let pb002 = s.boundary_stall_ms_per_photo();
    let pb003 = warm_scan.as_secs_f64() * 1000.0 / files as f64;
    let pb005 = util::timing::peak_working_set_mb();

    println!("\n=== Bench (full) Results ===");
    println!("Corpus: {} ({} files)", corpus_dir.display(), files);
    println!(
        "Output: {} {}x{} @ {}fps, crf {} ({} frames)",
        ot.name,
        output_def.width,
        output_def.height,
        output_def.fps,
        output_def.quality_crf,
        ot.frames
    );
    println!(
        "Scan: cold {:.2}s, warm {:.2}s ({} cached / {} probed on warm; SR-038 probe cache)",
        cold_scan.as_secs_f64(),
        warm_scan.as_secs_f64(),
        warm_stats.cache_hits,
        warm_stats.probed,
    );
    // Per-stage share of the build wall (plan §2 exit criterion).
    let wall_ms = (build_wall.as_secs_f64() * 1000.0).max(0.001);
    for (name, ms) in [
        ("decode", s.decode_ms),
        ("prescale", s.prescale_ms),
        ("prefetch-stall", s.stall_ms),
        ("render", s.render_ms),
        ("blend", s.blend_ms),
        ("encode-write-stall", s.encode_write_stall_ms),
        ("pipe-write", s.pipe_write_ms),
        ("ffmpeg-wall", s.ffmpeg_wall_ms),
    ] {
        println!(
            "  stage {}: {:.1} ms ({:.1}% of wall)",
            name,
            ms,
            100.0 * ms / wall_ms
        );
    }
    println!("PB-001 end-to-end frames/s: {:.1}", pb001);
    println!("PB-002 boundary stall ms/photo: {:.1}", pb002);
    println!("PB-003 scan s per 1k files: {:.3}", pb003);
    match pb005 {
        Some(mb) => println!("PB-005 peak working set MB: {:.1}", mb),
        None => println!("PB-005 peak working set: unmeasured on this platform (omitted)"),
    }
    println!(
        "PB-004 warm-rebuild ratio: measured by Scripts/bench.ps1 (timed build legs), not here"
    );

    let mut pairs: Vec<(&str, f64)> = vec![("PB-001", pb001), ("PB-002", pb002), ("PB-003", pb003)];
    if let Some(mb) = pb005 {
        pairs.push(("PB-005", mb));
    }
    let metrics_path = PathBuf::from("docs/test/perf-metrics.json");
    util::timing::write_perf_metrics(&metrics_path, &pairs)?;
    println!("Metrics -> {}", metrics_path.display());

    Ok(())
}
