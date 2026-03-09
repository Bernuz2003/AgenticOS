use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::scheduler::ProcessScheduler;
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use super::metrics::MetricsState;

/// Shared references for all command handlers, replacing loose parameters.
pub(crate) struct CommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: String,
    pub memory: &'a mut NeuralMemory,
    pub engine_state: &'a mut Option<LLMEngine>,
    pub model_catalog: &'a mut ModelCatalog,
    pub scheduler: &'a mut ProcessScheduler,
    pub orchestrator: &'a mut Orchestrator,
    pub tool_registry: &'a mut ToolRegistry,
    pub client_id: usize,
    pub shutdown_requested: &'a Arc<AtomicBool>,
    // ── Inference worker (checkout/checkin) ──────────────────────
    pub in_flight: &'a HashSet<u64>,
    pub pending_kills: &'a mut Vec<u64>,
    // ── Metrics (C6 — no global statics) ────────────────────────
    pub metrics: &'a mut MetricsState,
}
