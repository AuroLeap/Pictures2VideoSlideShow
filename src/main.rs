use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod config;
mod error;
mod ffmpeg;
mod image;
mod logging;
mod media;
mod pipeline;
mod transform;
mod util;
mod video;

use config::Config;
use logging::init_logging;

#[derive(Parser, Debug)]
#[command(name = "slideshow")]
#[command(about = "Convert photos/videos to slideshow for digital frames", long_about = None)]
struct Args {
    /// Configuration file path (TOML/JSON)
    #[arg(short, long)]
    config: PathBuf,

    /// Input media directory
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Output video directory
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Dry run (plan without executing)
    #[arg(long)]
    dry_run: bool,

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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(args.verbose)?;

    log::info!("Slideshow Engine v0.1.0 starting...");
    log::debug!("Arguments: {:?}", args);

    // Load configuration
    let config = Config::from_file(&args.config)?;
    log::info!("Configuration loaded from: {}", args.config.display());

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
            run_build(&config).await?;
        }
        Some(Commands::Validate) => {
            log::info!("Validating configuration...");
            config.validate()?;
            println!("✓ Configuration is valid");
        }
        Some(Commands::Stats) => {
            log::info!("Collecting project statistics...");
            run_stats(&config).await?;
        }
        Some(Commands::Bench { images }) => {
            log::info!("Running benchmark with {} images...", images);
            run_bench(&config, images).await?;
        }
    }

    log::info!("Slideshow generation complete!");
    Ok(())
}

async fn run_build(config: &Config) -> Result<()> {
    config.validate()?;

    // 1. Load media
    log::info!("Loading media files...");
    let media_loader = media::MediaLoader::new(config.input.clone());
    let album = media_loader.scan_and_index().await?;
    log::info!(
        "Found {} media files, total size: {} MB",
        album.media_files.len(),
        album.total_size / (1024 * 1024)
    );

    // 2. Process all output definitions in one pipeline.
    let pipeline = pipeline::FrameGenerationPipeline::new(
        album,
        config.outputs.clone(),
        config.processing.clone(),
        config.output.base_dir.clone(),
    );
    pipeline.execute().await?;

    log::info!("All outputs processed successfully");
    Ok(())
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
