mod backend;
mod checkpoint;
mod config;
mod commands;
mod engine;
mod errors;
mod inference_worker;
mod kernel;
mod memory;
mod model_catalog;
mod orchestrator;
mod policy;
mod process;
mod prompting;
mod protocol;
mod runtime;
mod scheduler;
mod services;
mod tools;
mod transport;

use std::io;
use kernel::server::Kernel;

fn main() -> io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = config::initialize().map_err(io::Error::other)?;
    tools::cleanup_stale_temp_scripts();
    let mut kernel = Kernel::new(config)?;
    kernel.run()
}
