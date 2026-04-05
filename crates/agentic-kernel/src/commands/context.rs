use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use agentic_control_models::KernelEvent;

use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::resource_governor::ResourceGovernor;
use crate::runtime::TurnAssemblyStore;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::job_scheduler::JobScheduler;
use crate::services::status_snapshot::StatusSnapshotDeps;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use super::diagnostics::MetricsState;

/// Shared references for all command handlers, replacing loose parameters.
pub(crate) struct CommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: String,
    pub memory: &'a mut NeuralMemory,
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub resource_governor: &'a mut ResourceGovernor,
    pub model_catalog: &'a mut ModelCatalog,
    pub scheduler: &'a mut ProcessScheduler,
    pub job_scheduler: &'a mut JobScheduler,
    pub orchestrator: &'a mut Orchestrator,
    pub tool_registry: &'a mut ToolRegistry,
    pub session_registry: &'a mut SessionRegistry,
    pub storage: &'a mut StorageService,
    pub turn_assembly: &'a mut TurnAssemblyStore,
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
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub resource_governor: &'a mut ResourceGovernor,
    pub model_catalog: &'a mut ModelCatalog,
    pub session_registry: &'a SessionRegistry,
    pub storage: &'a mut StorageService,
    pub pending_events: &'a mut Vec<KernelEvent>,
}

pub(crate) struct ExecCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub memory: &'a mut NeuralMemory,
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub resource_governor: &'a mut ResourceGovernor,
    pub model_catalog: &'a mut ModelCatalog,
    pub scheduler: &'a mut ProcessScheduler,
    pub in_flight: &'a HashSet<u64>,
    pub client_id: usize,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub metrics: &'a mut MetricsState,
    pub session_registry: &'a mut SessionRegistry,
    pub storage: &'a mut StorageService,
    pub tool_registry: &'a ToolRegistry,
}

pub(crate) struct ProcessCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub resource_governor: &'a mut ResourceGovernor,
    pub model_catalog: &'a mut ModelCatalog,
    pub memory: &'a mut NeuralMemory,
    pub scheduler: &'a mut ProcessScheduler,
    pub in_flight: &'a HashSet<u64>,
    pub pending_kills: &'a mut Vec<u64>,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub metrics: &'a mut MetricsState,
    pub client_id: usize,
    pub session_registry: &'a mut SessionRegistry,
    pub storage: &'a mut StorageService,
    pub turn_assembly: &'a mut TurnAssemblyStore,
    pub tool_registry: &'a ToolRegistry,
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
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub resource_governor: &'a mut ResourceGovernor,
    pub memory: &'a mut NeuralMemory,
    pub model_catalog: &'a mut ModelCatalog,
    pub scheduler: &'a mut ProcessScheduler,
    pub in_flight: &'a HashSet<u64>,
    pub pending_kills: &'a mut Vec<u64>,
    pub job_scheduler: &'a mut JobScheduler,
    pub orchestrator: &'a mut Orchestrator,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub metrics: &'a mut MetricsState,
    pub client_id: usize,
    pub session_registry: &'a mut SessionRegistry,
    pub storage: &'a mut StorageService,
    pub tool_registry: &'a ToolRegistry,
}

pub(crate) struct ToolsCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub tool_registry: &'a mut ToolRegistry,
}

pub(crate) struct MiscCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub shutdown_requested: &'a Arc<AtomicBool>,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub metrics: &'a mut MetricsState,
    pub client_id: usize,
}

pub(crate) struct MemoryCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub memory: &'a mut NeuralMemory,
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub pending_events: &'a mut Vec<KernelEvent>,
}

pub(crate) struct CheckpointCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub model_catalog: &'a mut ModelCatalog,
    pub scheduler: &'a mut ProcessScheduler,
    pub metrics: &'a mut MetricsState,
    pub memory: &'a mut NeuralMemory,
    pub storage: &'a mut StorageService,
    pub in_flight: &'a HashSet<u64>,
    pub pending_events: &'a mut Vec<KernelEvent>,
    pub client_id: usize,
}

pub(crate) struct CoreDumpCommandContext<'a> {
    pub client: &'a mut Client,
    pub request_id: &'a str,
    pub runtime_registry: &'a mut RuntimeRegistry,
    pub scheduler: &'a mut ProcessScheduler,
    pub session_registry: &'a mut SessionRegistry,
    pub storage: &'a mut StorageService,
    pub turn_assembly: &'a mut TurnAssemblyStore,
    pub memory: &'a mut NeuralMemory,
    pub in_flight: &'a HashSet<u64>,
    pub pending_events: &'a mut Vec<KernelEvent>,
}

impl<'a> CommandContext<'a> {
    pub fn status_view(&mut self) -> StatusCommandContext<'_> {
        StatusCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            snapshot: StatusSnapshotDeps {
                memory: &*self.memory,
                runtime_registry: &*self.runtime_registry,
                resource_governor: &*self.resource_governor,
                model_catalog: &*self.model_catalog,
                scheduler: &*self.scheduler,
                job_scheduler: &*self.job_scheduler,
                orchestrator: &*self.orchestrator,
                in_flight: self.in_flight,
                metrics: &*self.metrics,
                session_registry: &*self.session_registry,
                storage: &*self.storage,
            },
        }
    }

    pub fn model_view(&mut self) -> ModelCommandContext<'_> {
        ModelCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            runtime_registry: &mut *self.runtime_registry,
            resource_governor: &mut *self.resource_governor,
            model_catalog: &mut *self.model_catalog,
            session_registry: &*self.session_registry,
            storage: &mut *self.storage,
            pending_events: &mut *self.pending_events,
        }
    }

    pub fn exec_view(&mut self) -> ExecCommandContext<'_> {
        ExecCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            memory: &mut *self.memory,
            runtime_registry: &mut *self.runtime_registry,
            resource_governor: &mut *self.resource_governor,
            model_catalog: &mut *self.model_catalog,
            scheduler: &mut *self.scheduler,
            in_flight: self.in_flight,
            client_id: self.client_id,
            pending_events: &mut *self.pending_events,
            metrics: &mut *self.metrics,
            session_registry: &mut *self.session_registry,
            storage: &mut *self.storage,
            tool_registry: &*self.tool_registry,
        }
    }

    pub fn process_view(&mut self) -> ProcessCommandContext<'_> {
        ProcessCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            runtime_registry: &mut *self.runtime_registry,
            resource_governor: &mut *self.resource_governor,
            model_catalog: &mut *self.model_catalog,
            memory: &mut *self.memory,
            scheduler: &mut *self.scheduler,
            in_flight: self.in_flight,
            pending_kills: &mut *self.pending_kills,
            pending_events: &mut *self.pending_events,
            metrics: &mut *self.metrics,
            client_id: self.client_id,
            session_registry: &mut *self.session_registry,
            storage: &mut *self.storage,
            turn_assembly: &mut *self.turn_assembly,
            tool_registry: &*self.tool_registry,
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
            runtime_registry: &mut *self.runtime_registry,
            resource_governor: &mut *self.resource_governor,
            memory: &mut *self.memory,
            model_catalog: &mut *self.model_catalog,
            scheduler: &mut *self.scheduler,
            in_flight: self.in_flight,
            pending_kills: &mut *self.pending_kills,
            job_scheduler: &mut *self.job_scheduler,
            orchestrator: &mut *self.orchestrator,
            pending_events: &mut *self.pending_events,
            metrics: &mut *self.metrics,
            client_id: self.client_id,
            session_registry: &mut *self.session_registry,
            storage: &mut *self.storage,
            tool_registry: &*self.tool_registry,
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
            runtime_registry: &mut *self.runtime_registry,
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
            runtime_registry: &mut *self.runtime_registry,
            pending_events: &mut *self.pending_events,
        }
    }

    pub fn checkpoint_view(&mut self) -> CheckpointCommandContext<'_> {
        CheckpointCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            runtime_registry: &mut *self.runtime_registry,
            model_catalog: &mut *self.model_catalog,
            scheduler: &mut *self.scheduler,
            metrics: &mut *self.metrics,
            memory: &mut *self.memory,
            storage: &mut *self.storage,
            in_flight: self.in_flight,
            pending_events: &mut *self.pending_events,
            client_id: self.client_id,
        }
    }

    pub fn core_dump_view(&mut self) -> CoreDumpCommandContext<'_> {
        CoreDumpCommandContext {
            client: &mut *self.client,
            request_id: self.request_id.as_str(),
            runtime_registry: &mut *self.runtime_registry,
            scheduler: &mut *self.scheduler,
            session_registry: &mut *self.session_registry,
            storage: &mut *self.storage,
            turn_assembly: &mut *self.turn_assembly,
            memory: &mut *self.memory,
            in_flight: self.in_flight,
            pending_events: &mut *self.pending_events,
        }
    }
}
