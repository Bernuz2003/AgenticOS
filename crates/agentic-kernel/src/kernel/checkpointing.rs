use std::time::Instant;

use crate::checkpoint;
use crate::commands::MetricsState;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::scheduler::ProcessScheduler;

pub(crate) fn maybe_run_auto_checkpoint(
    checkpoint_interval_secs: u64,
    last_checkpoint: &mut Instant,
    engine_state: Option<&LLMEngine>,
    model_catalog: &ModelCatalog,
    scheduler: &ProcessScheduler,
    metrics: &MetricsState,
    memory: &NeuralMemory,
) {
    if checkpoint_interval_secs == 0
        || last_checkpoint.elapsed().as_secs() < checkpoint_interval_secs
    {
        return;
    }

    *last_checkpoint = Instant::now();
    run_auto_checkpoint(engine_state, model_catalog, scheduler, metrics, memory);
}

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
