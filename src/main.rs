//! Binary entry point (`make_video_slideshow`): CLI parsing (build /
//! validate / setup), the first-run wizard trigger, and the thin
//! orchestration that wires config → pipeline.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

// The binary is a thin shell over the library crate — modules are compiled
// once (in the lib) and imported here, never re-declared.
use slideshow_core::{config, error, media, pipeline, preflight, roi, setup};

use config::Config;
use slideshow_core::logging::init_logging;
use slideshow_core::util::estimate::human_bytes;
use std::io::IsTerminal;

#[derive(Parser, Debug)]
#[command(name = "make_video_slideshow", version)]
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
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(args.verbose)?;

    log::info!(
        "Slideshow Engine v{} starting...",
        env!("CARGO_PKG_VERSION")
    );
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
        None => {
            log::info!("Starting slideshow generation pipeline...");
            run_build(&config, non_interactive, None)?;
        }
        Some(Commands::Build { output }) => {
            log::info!("Starting slideshow generation pipeline...");
            run_build(&config, non_interactive, output.as_deref())?;
        }
        Some(Commands::Validate) => {
            log::info!("Validating runtime prerequisites...");
            run_validate(&config);
        }
        Some(Commands::Stats) => {
            log::info!("Collecting project statistics...");
            run_stats(&config)?;
        }
        Some(Commands::Bench { images }) => {
            log::info!("Running benchmark with {} images...", images);
            run_bench(&config, images)?;
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

/// Run `build`. `only_output` (the `build --output <name>` filter) restricts
/// the run to the one named output definition; an unknown name is a hard error
/// listing the available definitions, never a silent full build.
// Implements: SR-017, LLR-045
fn run_build(config: &Config, non_interactive: bool, only_output: Option<&str>) -> Result<()> {
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

    // Resolve the `build --output <name>` filter up front so a typo'd name
    // fails fast, before any scanning or encoding starts.
    // Implements: SR-017, LLR-045
    let outputs: Vec<config::OutputDef> = match only_output {
        Some(name) => {
            let selected: Vec<_> = config
                .outputs
                .iter()
                .filter(|o| o.name == name)
                .cloned()
                .collect();
            if selected.is_empty() {
                let available: Vec<_> = config.outputs.iter().map(|o| o.name.as_str()).collect();
                return Err(error::SlideshowError::InvalidConfig(format!(
                    "no output definition named '{}'; available outputs: {}",
                    name,
                    available.join(", ")
                ))
                .into());
            }
            selected
        }
        None => config.outputs.clone(),
    };

    // 1. Load media
    log::info!("Loading media files...");
    let media_loader = media::MediaLoader::new(config.input.clone());
    let album = media_loader.scan_and_index()?;
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

    // 3. Process all output definitions in one pipeline.
    let pipeline = pipeline::FrameGenerationPipeline::new(
        album,
        outputs,
        config.processing.clone(),
        config.output.base_dir.clone(),
        config.input.media_root.clone(),
        roi,
    );
    let summary = pipeline.execute()?;

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
            println!("  {} ({})", o.path.display(), human_bytes(o.size_bytes));
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

fn run_stats(config: &Config) -> Result<()> {
    let media_loader = media::MediaLoader::new(config.input.clone());
    let album = media_loader.scan_and_index()?;

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

fn run_bench(config: &Config, num_images: usize) -> Result<()> {
    let media_loader = media::MediaLoader::new(config.input.clone());
    let album = media_loader.scan_and_index()?;

    let first_image = album
        .media_files
        .iter()
        .find(|m| m.file_type == media::MediaType::Image)
        .ok_or_else(|| error::SlideshowError::Media("No images found to benchmark".into()))?;

    let output_def = config
        .outputs
        .first()
        .ok_or_else(|| error::SlideshowError::InvalidConfig("No output defined".into()))?;

    log::info!(
        "Benchmarking frame generation on {} ({} simulated images)...",
        first_image.path.display(),
        num_images
    );

    let renderer = slideshow_core::image::FrameRenderer::load(&first_image.path, output_def)?;
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
