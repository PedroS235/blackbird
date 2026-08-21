//! Where diagnostics go, and how loud.
//!
//! `RUST_LOG` wins whenever it is set — `RUST_LOG=blackbird::parser=trace` to
//! chase one module, `RUST_LOG=debug` to hear the dependencies too. Unset, the
//! build decides: `debug` for a dev build, `info` for a release one.
//!
//! Only this crate is enabled by default. eframe and wgpu log per frame, which
//! would bury the parse and analysis lines a pilot's log dump exists to show.

use tracing_subscriber::EnvFilter;

fn default_filter() -> &'static str {
    if cfg!(debug_assertions) {
        "blackbird=debug"
    } else {
        "blackbird=info"
    }
}

/// Installs the global subscriber. Called once, from `main`; a second call is
/// ignored rather than fatal, so a test that logs does not take the suite down.
pub fn init_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter()));

    if tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init()
        .is_err()
    {
        tracing::debug!("logging was already initialised");
    }
}
