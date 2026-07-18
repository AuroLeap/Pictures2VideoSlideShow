//! Logging setup: env_logger with millisecond timestamps; `--verbose`
//! raises the level to Debug. `RUST_LOG`, when set, wins over the default so
//! the documented `RUST_LOG=debug` path works without `--verbose`.

pub fn init_logging(verbose: bool) -> std::io::Result<()> {
    let default = if verbose { "debug" } else { "info" };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default))
        .format_timestamp_millis()
        .try_init()
        .ok();

    Ok(())
}
