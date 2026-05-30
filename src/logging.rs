use log::LevelFilter;

pub fn init_logging(verbose: bool) -> std::io::Result<()> {
    let level = if verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    env_logger::Builder::from_default_env()
        .filter_level(level)
        .format_timestamp_millis()
        .try_init()
        .ok();

    Ok(())
}
