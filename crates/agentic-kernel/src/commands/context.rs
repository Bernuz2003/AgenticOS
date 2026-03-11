use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use agentic_control_models::KernelEvent;

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::scheduler::ProcessScheduler;
use crate::services::status_snapshot::StatusSnapshotDeps;
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
    pub pending_events: &'a mut Vec<KernelEvent>,
    // ── Metrics (C6 — no global statics) ────────────────────────
    pub metrics: &'a mut MetricsState,
}

pub(crate) struct StatusCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub snapshot: StatusSnapshotDeps<'a>,
}

pub(crate) struct ModelCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub engine_state: &'a mut Option<LLMEngine>,
    pub model_catalog: &'a mut ModelCatalog,
    pub in_flight: &'a HashSet<u64>,
    pub pending_events: &'a mut Vec<KernelEvent>,
}

pub(crate) struct ExecCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub memory: &'a mut NeuralMemory,
    pub engine_state: &'a mut Option<LLMEngine>,
    pub model_catalog: &'a mut ModelCatalog,
    pub scheduler: &'a mut ProcessScheduler,
    pub in_flight: &'a HashSet<u64>,
    pub client_id: usize,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub metrics: &'a mut MetricsState,
}

pub(crate) struct ProcessCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub engine_state: &'a mut Option<LLMEngine>,
    pub memory: &'a mut NeuralMemory,
    pub scheduler: &'a mut ProcessScheduler,
    pub in_flight: &'a HashSet<u64>,
    pub pending_kills: &'a mut Vec<u64>,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub metrics: &'a mut MetricsState,
    pub client_id: usize,
}

pub(crate) struct SchedulerCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub scheduler: &'a mut ProcessScheduler,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub client_id: usize,
}

pub(crate) struct OrchestrationCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub engine_state: &'a mut Option<LLMEngine>,
    pub memory: &'a mut NeuralMemory,
    pub scheduler: &'a mut ProcessScheduler,
    pub orchestrator: &'a mut Orchestrator,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub metrics: &'a mut MetricsState,
    pub client_id: usize,
}

pub(crate) struct ToolsCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub tool_registry: &'a mut ToolRegistry,
}

pub(crate) struct MiscCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub engine_state: &'a mut Option<LLMEngine>,
    pub shutdown_requested: &'a Arc<AtomicBool>,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub metrics: &'a mut MetricsState,
    pub client_id: usize,
}

pub(crate) struct MemoryCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub memory: &'a mut NeuralMemory,
    pub engine_state: &'a mut Option<LLMEngine>,
    pub pending_events: &'a mut Vec<KernelEvent>,
}

pub(crate) struct CheckpointCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub engine_state: &'a mut Option<LLMEngine>,
    pub model_catalog: &'a mut ModelCatalog,
    pub scheduler: &'a mut ProcessScheduler,
    pub metrics: &'a mut MetricsState,
    pub memory: &'a mut NeuralMemory,
    pub in_flight: &'a HashSet<u64>,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub client_id: usize,
}

impl<'a> CommandContext<'a> {
    pub fn status_view(&mut self) -> StatusCommandContext<'_> {
        StatusCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            snapshot: StatusSnapshotDeps {
                memory: &*self.memory,
                engine_state: &*self.engine_state,
                model_catalog: &*self.model_catalog,
                scheduler: &*self.scheduler,
                orchestrator: &*self.orchestrator,
                in_flight: self.in_flight,
                metrics: &*self.metrics,
            },
        }
    }

    pub fn model_view(&mut self) -> ModelCommandContext<'_> {
        ModelCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            engine_state: &mut *self.engine_state,
            model_catalog: &mut *self.model_catalog,
            in_flight: self.in_flight,
            pending_events: &mut *self.pending_events,
        }
    }

    pub fn exec_view(&mut self) -> ExecCommandContext<'_> {
        ExecCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            memory: &mut *self.memory,
            engine_state: &mut *self.engine_state,
            model_catalog: &mut *self.model_catalog,
            scheduler: &mut *self.scheduler,
            in_flight: self.in_flight,
            client_id: self.client_id,
            pending_events: &mut *self.pending_events,
            metrics: &mut *self.metrics,
        }
    }

    pub fn process_view(&mut self) -> ProcessCommandContext<'_> {
        ProcessCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            engine_state: &mut *self.engine_state,
            memory: &mut *self.memory,
            scheduler: &mut *self.scheduler,
            in_flight: self.in_flight,
            pending_kills: &mut *self.pending_kills,
            pending_events: &mut *self.pending_events,
            metrics: &mut *self.metrics,
            client_id: self.client_id,
        }
    }

    pub fn scheduler_view(&mut self) -> SchedulerCommandContext<'_> {
        SchedulerCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            scheduler: &mut *self.scheduler,
            pending_events: &mut *self.pending_events,
            client_id: self.client_id,
        }
    }

    pub fn orchestration_view(&mut self) -> OrchestrationCommandContext<'_> {
        OrchestrationCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            engine_state: &mut *self.engine_state,
            memory: &mut *self.memory,
            scheduler: &mut *self.scheduler,
            orchestrator: &mut *self.orchestrator,
            pending_events: &mut *self.pending_events,
            metrics: &mut *self.metrics,
            client_id: self.client_id,
        }
    }

    pub fn tools_view(&mut self) -> ToolsCommandContext<'_> {
        ToolsCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            tool_registry: &mut *self.tool_registry,
        }
    }

    pub fn misc_view(&mut self) -> MiscCommandContext<'_> {
        MiscCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            engine_state: &mut *self.engine_state,
            shutdown_requested: self.shutdown_requested,
            pending_events: &mut *self.pending_events,
            metrics: &mut *self.metrics,
            client_id: self.client_id,
        }
    }

    pub fn memory_view(&mut self) -> MemoryCommandContext<'_> {
        MemoryCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            memory: &mut *self.memory,
            engine_state: &mut *self.engine_state,
            pending_events: &mut *self.pending_events,
        }
    }

    pub fn checkpoint_view(&mut self) -> CheckpointCommandContext<'_> {
        CheckpointCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            engine_state: &mut *self.engine_state,
            model_catalog: &mut *self.model_catalog,
            scheduler: &mut *self.scheduler,
            metrics: &mut *self.metrics,
            memory: &mut *self.memory,
            in_flight: self.in_flight,
            pending_events: &mut *self.pending_events,
            client_id: self.client_id,
        }
    }
}
