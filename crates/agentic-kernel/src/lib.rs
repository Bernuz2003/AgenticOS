mod accounting;
mod audit;
mod backend;
mod checkpoint;
mod commands;
mod config;
mod engine;
mod errors;
mod events;
mod inference_worker;
mod kernel;
mod memory;
mod model_catalog;
mod orchestrator;
mod policy;
mod process;
mod prompting;
mod protocol;
mod resource_governor;
mod runtime;
mod runtimes;
mod scheduler;
mod services;
mod session;
mod storage;
mod text_invocation;
mod tool_registry;
mod tools;
mod transport;

use kernel::server::Kernel;
use std::io;

pub fn run() -> io::Result<()> {
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
