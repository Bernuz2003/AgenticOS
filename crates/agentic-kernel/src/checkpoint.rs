//! Kernel state persistence — checkpoint / restore.
//!
//! Serialises the *metadata* portion of kernel state (scheduler, process list,
//! config, metrics) to a JSON file.  Model weights and tensor data are NOT
//! included — processes are marked `Orphaned` on restore and require a fresh
//! `LOAD` + `EXEC` cycle.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::process::{ContextPolicy, ContextState, HumanInputRequest};
use crate::tools::invocation::{ProcessPermissionPolicy, ProcessTrustScope, ToolCaller};

fn current_family_snapshot(
    engine_state: Option<&crate::engine::LLMEngine>,
    model_catalog: &crate::model_catalog::ModelCatalog,
) -> String {
    engine_state
        .map(|engine| format!("{:?}", engine.loaded_family()))
        .or_else(|| {
            model_catalog
                .selected_entry()
                .map(|entry| format!("{:?}", entry.family))
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

// ── Snapshot types ──────────────────────────────────────────────────────

/// Top-level checkpoint payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelSnapshot {
    /// ISO-8601-ish UTC timestamp of when the checkpoint was taken.
    pub timestamp: String,
    /// Kernel version at time of snapshot.
    pub version: String,
    /// Active prompt family (e.g. "llama", "qwen").
    pub active_family: String,
    /// Selected model id in catalog (if any).
    pub selected_model: Option<String>,
    /// Generation config at kernel level.
    pub generation: Option<GenerationSnapshot>,
    /// Per-process metadata.
    pub processes: Vec<ProcessSnapshot>,
    /// Scheduler metadata.
    pub scheduler: SchedulerStateSnapshot,
    /// Aggregate metrics.
    pub metrics: MetricsSnapshot,
    /// Memory subsystem counters.
    pub memory: MemoryCountersSnapshot,
}

/// Serialised view of a single process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub pid: u64,
    pub owner_id: usize,
    #[serde(default = "default_tool_caller")]
    pub tool_caller: ToolCaller,
    #[serde(default = "default_permission_policy")]
    pub permission_policy: ProcessPermissionPolicy,
    pub state: String,
    pub token_count: usize,
    pub max_tokens: usize,
    #[serde(default = "default_context_policy")]
    pub context_policy: ContextPolicy,
    #[serde(default)]
    pub context_state: ContextState,
    #[serde(default)]
    pub pending_human_request: Option<HumanInputRequest>,
}

fn default_context_policy() -> ContextPolicy {
    ContextPolicy::from_kernel_defaults()
}

fn default_tool_caller() -> ToolCaller {
    ToolCaller::AgentText
}

fn default_permission_policy() -> ProcessPermissionPolicy {
    ProcessPermissionPolicy {
        trust_scope: ProcessTrustScope::InteractiveChat,
        actions_allowed: false,
        allowed_tools: Vec::new(),
        path_scopes: vec![".".to_string()],
    }
}

/// Scheduler state for all registered PIDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStateSnapshot {
    pub entries: Vec<SchedulerEntrySnapshot>,
}

/// One PID's scheduler metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerEntrySnapshot {
    pub pid: u64,
    pub priority: String,
    pub workload: String,
    pub max_tokens: usize,
    pub max_syscalls: usize,
    pub tokens_generated: usize,
    pub syscalls_used: usize,
    pub elapsed_secs: f64,
}

/// Generation sampling parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationSnapshot {
    pub temperature: f64,
    pub top_p: f64,
    pub seed: u64,
    pub max_tokens: usize,
}

/// Aggregate kernel metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub uptime_secs: u64,
    pub total_commands: u64,
    pub total_errors: u64,
    pub total_exec_started: u64,
    pub total_signals: u64,
}

/// Memory subsystem counters (not actual data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCountersSnapshot {
    pub active: bool,
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub allocated_tensors: usize,
    pub tracked_pids: usize,
    pub alloc_bytes: usize,
    pub evictions: u64,
    pub swap_count: u64,
    pub swap_faults: u64,
    pub oom_events: u64,
}

// ── Persistence helpers ─────────────────────────────────────────────────

/// Default checkpoint path inside `workspace/`.
pub fn default_checkpoint_path() -> PathBuf {
    crate::config::kernel_config().paths.checkpoint_path.clone()
}

/// Atomically write a checkpoint to disk (temp + rename).
pub fn save_checkpoint(snapshot: &KernelSnapshot, path: &Path) -> Result<String, String> {
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("serialization error: {}", e))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create checkpoint dir {:?}: {}", parent, e))?;
    }

    let tmp_path = path.with_extension("json.tmp");

    fs::write(&tmp_path, json.as_bytes())
        .map_err(|e| format!("write tmp checkpoint failed: {}", e))?;

    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("atomic rename failed: {}", e)
    })?;

    Ok(format!(
        "checkpoint saved: {} ({} bytes)",
        path.display(),
        json.len()
    ))
}

/// Load a checkpoint from disk.
pub fn load_checkpoint(path: &Path) -> Result<KernelSnapshot, String> {
    let data = fs::read_to_string(path)
        .map_err(|e| format!("cannot read checkpoint {:?}: {}", path, e))?;

    serde_json::from_str(&data).map_err(|e| format!("corrupt checkpoint {:?}: {}", path, e))
}

/// Generate an ISO-8601-ish UTC timestamp string.
pub fn now_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Simple epoch-based timestamp; no chrono dependency needed.
    format!("epoch_{}", secs)
}

// ── Snapshot builders (called from commands) ────────────────────────────

/// Build a `SchedulerStateSnapshot` from the live `ProcessScheduler`.
pub fn snapshot_scheduler(
    scheduler: &crate::scheduler::ProcessScheduler,
) -> SchedulerStateSnapshot {
    let entries = scheduler
        .registered_pids()
        .into_iter()
        .filter_map(|pid| {
            scheduler.snapshot(pid).map(|s| SchedulerEntrySnapshot {
                pid,
                priority: s.priority.to_string(),
                workload: format!("{:?}", s.workload),
                max_tokens: s.quota.max_tokens,
                max_syscalls: s.quota.max_syscalls,
                tokens_generated: s.tokens_generated,
                syscalls_used: s.syscalls_used,
                elapsed_secs: s.elapsed_secs,
            })
        })
        .collect();
    SchedulerStateSnapshot { entries }
}

/// Build a `MemoryCountersSnapshot` from the live `NeuralMemory`.
pub fn snapshot_memory(memory: &crate::memory::NeuralMemory) -> MemoryCountersSnapshot {
    let s = memory.snapshot();
    MemoryCountersSnapshot {
        active: s.active,
        total_blocks: s.total_blocks,
        free_blocks: s.free_blocks,
        allocated_tensors: s.allocated_tensors,
        tracked_pids: s.tracked_pids,
        alloc_bytes: s.alloc_bytes,
        evictions: s.evictions,
        swap_count: s.swap_count,
        swap_faults: s.swap_faults,
        oom_events: s.oom_events,
    }
}

pub fn build_kernel_snapshot(
    engine_state: Option<&crate::engine::LLMEngine>,
    model_catalog: &crate::model_catalog::ModelCatalog,
    scheduler: &crate::scheduler::ProcessScheduler,
    metrics: &crate::commands::MetricsState,
    memory: &crate::memory::NeuralMemory,
) -> KernelSnapshot {
    let (uptime_s, total_cmd, total_err, total_exec, total_signals) = metrics.snapshot();

    let (processes, generation) = if let Some(engine) = engine_state {
        let mut live_pids = HashSet::new();
        let mut processes: Vec<ProcessSnapshot> = engine
            .processes
            .iter()
            .map(|(pid, process)| {
                live_pids.insert(*pid);
                ProcessSnapshot {
                    pid: *pid,
                    owner_id: process.owner_id,
                    tool_caller: process.tool_caller.clone(),
                    permission_policy: process.permission_policy.clone(),
                    state: format!("{:?}", process.state),
                    token_count: process.tokens.len(),
                    max_tokens: process.max_tokens,
                    context_policy: process.context_policy.clone(),
                    context_state: process.context_state.clone(),
                    pending_human_request: process.pending_human_request.clone(),
                }
            })
            .collect();
        processes.extend(
            scheduler
                .restored_pids()
                .into_iter()
                .filter(|pid| !live_pids.contains(pid))
                .filter_map(|pid| {
                    scheduler
                        .restored_process(pid)
                        .map(|metadata| ProcessSnapshot {
                            pid,
                            owner_id: metadata.owner_id,
                            tool_caller: metadata.tool_caller.clone(),
                            permission_policy: metadata.permission_policy.clone(),
                            state: metadata.state.clone(),
                            token_count: metadata.token_count,
                            max_tokens: metadata.max_tokens,
                            context_policy: metadata.context_policy.clone(),
                            context_state: metadata.context_state.clone(),
                            pending_human_request: metadata.pending_human_request.clone(),
                        })
                }),
        );
        let cfg = engine.generation_config();
        let generation = Some(GenerationSnapshot {
            temperature: cfg.temperature,
            top_p: cfg.top_p,
            seed: cfg.seed,
            max_tokens: cfg.max_tokens,
        });
        (processes, generation)
    } else {
        (
            scheduler
                .restored_pids()
                .into_iter()
                .filter_map(|pid| {
                    scheduler
                        .restored_process(pid)
                        .map(|metadata| ProcessSnapshot {
                            pid,
                            owner_id: metadata.owner_id,
                            tool_caller: metadata.tool_caller.clone(),
                            permission_policy: metadata.permission_policy.clone(),
                            state: metadata.state.clone(),
                            token_count: metadata.token_count,
                            max_tokens: metadata.max_tokens,
                            context_policy: metadata.context_policy.clone(),
                            context_state: metadata.context_state.clone(),
                            pending_human_request: metadata.pending_human_request.clone(),
                        })
                })
                .collect(),
            None,
        )
    };

    KernelSnapshot {
        timestamp: now_timestamp(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        active_family: current_family_snapshot(engine_state, model_catalog),
        selected_model: model_catalog.selected_id.clone(),
        generation,
        processes,
        scheduler: snapshot_scheduler(scheduler),
        metrics: MetricsSnapshot {
            uptime_secs: uptime_s,
            total_commands: total_cmd,
            total_errors: total_err,
            total_exec_started: total_exec,
            total_signals,
        },
        memory: snapshot_memory(memory),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
