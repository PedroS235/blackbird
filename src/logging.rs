use tracing_subscriber::EnvFilter;

pub fn init_logging() {
    let default_level = if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| default_level.into()))
        .with_env_filter(EnvFilter::new("blackbird=debug"))
        .init();
}
