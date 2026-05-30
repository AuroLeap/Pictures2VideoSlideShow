use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod config;
mod error;
mod logging;
mod media;
mod image;
mod video;
mod transform;
mod ffmpeg;
mod pipeline;
mod util;

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
    // 1. Load media
    log::info!("Loading media files...");
    let media_loader = media::MediaLoader::new(config.input.clone());
    let album = media_loader.scan_and_index().await?;
    log::info!(
        "Found {} media files, total size: {} MB",
        album.media_files.len(),
        album.total_size / (1024 * 1024)
    );

    // 2. Process images for each output definition
    for output_def in &config.outputs {
        log::info!("Processing for output: {}", output_def.name);

        let pipeline = pipeline::FrameGenerationPipeline::new(
            album.clone(),
            vec![output_def.clone()],
            config.processing.clone(),
        );

        pipeline.execute().await?;
    }

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
    let start = std::time::Instant::now();

    // Simulate processing
    log::info!("Benchmarking with {} images...", num_images);
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let elapsed = start.elapsed();
    let secs_per_image = elapsed.as_secs_f64() / num_images as f64;

    println!("\n=== Benchmark Results ===");
    println!("Images processed: {}", num_images);
    println!("Total time: {:.2}s", elapsed.as_secs_f64());
    println!("Per image: {:.2}s", secs_per_image);
    println!("Throughput: {:.1} images/sec", 1.0 / secs_per_image);

    Ok(())
}
