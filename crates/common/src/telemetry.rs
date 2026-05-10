use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init(service_name: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,driftbase=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true).with_thread_ids(false))
        .init();

    tracing::info!(service = service_name, "telemetry initialized");
}
