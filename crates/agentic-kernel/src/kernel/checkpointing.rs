use crate::checkpoint;
use crate::commands::MetricsState;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::scheduler::ProcessScheduler;

pub(crate) fn run_auto_checkpoint(
    engine_state: Option<&LLMEngine>,
    model_catalog: &ModelCatalog,
    scheduler: &ProcessScheduler,
    metrics: &MetricsState,
    memory: &NeuralMemory,
) {
    let path = checkpoint::default_checkpoint_path();
    let snap =
        checkpoint::build_kernel_snapshot(engine_state, model_catalog, scheduler, metrics, memory);

    match checkpoint::save_checkpoint(&snap, &path) {
        Ok(msg) => tracing::debug!(msg, "auto-checkpoint"),
        Err(e) => tracing::warn!(%e, "auto-checkpoint failed"),
    }
}
