mod backend;
mod checkpoint;
mod commands;
mod config;
mod diagnostics;
mod engine;
mod errors;
mod events;
mod kernel;
mod memory;
mod model_catalog;
mod orchestrator;
mod policy;
mod process;
mod prompt;
mod protocol;
mod resource_governor;
mod runtime;
mod runtimes;
mod scheduler;
mod services;
mod session;
mod storage;
mod tool_registry;
mod tools;
mod transport;
mod invocation;
mod workers;
#[cfg(test)]
mod test_support;

#[allow(unused_imports)]
pub(crate) use invocation::text as text_invocation;
#[allow(unused_imports)]
pub(crate) use prompt::agent_prompt as agent_prompt;
#[allow(unused_imports)]
pub(crate) use prompt::capabilities as agent_capabilities;
#[allow(unused_imports)]
pub(crate) use prompt::rendering as prompting;
#[allow(unused_imports)]
pub(crate) use workers::inference as inference_worker;

use kernel::event_loop::Kernel;
use std::io;

pub fn run() -> io::Result<()> {
    diagnostics::tracing::initialize_subscriber();

    let config = config::initialize().map_err(io::Error::other)?;
    tools::cleanup_stale_temp_scripts();
    let mut kernel = Kernel::new(config)?;
    kernel.run()
}
