//! Kernel state persistence — checkpoint / restore.
//!
//! Serialises the *metadata* portion of kernel state (scheduler, process list,
//! config, metrics) to a JSON file.  Model weights and tensor data are NOT
//! included — processes are marked `Orphaned` on restore and require a fresh
//! `LOAD` + `EXEC` cycle.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub state: String,
    pub token_count: usize,
    pub max_tokens: usize,
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

    fs::rename(&tmp_path, path)
        .map_err(|e| {
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

    serde_json::from_str(&data)
        .map_err(|e| format!("corrupt checkpoint {:?}: {}", path, e))
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
pub fn snapshot_scheduler(scheduler: &crate::scheduler::ProcessScheduler) -> SchedulerStateSnapshot {
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
        let processes = engine
            .processes
            .iter()
            .map(|(pid, process)| ProcessSnapshot {
                pid: *pid,
                owner_id: process.owner_id,
                state: format!("{:?}", process.state),
                token_count: process.tokens.len(),
                max_tokens: process.max_tokens,
            })
            .collect();
        let cfg = engine.generation_config();
        let generation = Some(GenerationSnapshot {
            temperature: cfg.temperature,
            top_p: cfg.top_p,
            seed: cfg.seed,
            max_tokens: cfg.max_tokens,
        });
        (processes, generation)
    } else {
        (vec![], None)
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
mod tests {
    use super::*;

    fn make_test_snapshot() -> KernelSnapshot {
        KernelSnapshot {
            timestamp: "epoch_1709600000".to_string(),
            version: "0.5.0".to_string(),
            active_family: "llama".to_string(),
            selected_model: Some("llama3.1-8b".to_string()),
            generation: Some(GenerationSnapshot {
                temperature: 0.7,
                top_p: 0.9,
                seed: 42,
                max_tokens: 256,
            }),
            processes: vec![
                ProcessSnapshot {
                    pid: 1,
                    owner_id: 10,
                    state: "Running".to_string(),
                    token_count: 128,
                    max_tokens: 256,
                },
                ProcessSnapshot {
                    pid: 2,
                    owner_id: 11,
                    state: "Paused".to_string(),
                    token_count: 64,
                    max_tokens: 512,
                },
            ],
            scheduler: SchedulerStateSnapshot {
                entries: vec![SchedulerEntrySnapshot {
                    pid: 1,
                    priority: "high".to_string(),
                    workload: "Code".to_string(),
                    max_tokens: 4096,
                    max_syscalls: 16,
                    tokens_generated: 100,
                    syscalls_used: 3,
                    elapsed_secs: 12.5,
                }],
            },
            metrics: MetricsSnapshot {
                uptime_secs: 3600,
                total_commands: 42,
                total_errors: 1,
                total_exec_started: 10,
                total_signals: 2,
            },
            memory: MemoryCountersSnapshot {
                active: true,
                total_blocks: 256,
                free_blocks: 200,
                allocated_tensors: 5,
                tracked_pids: 2,
                alloc_bytes: 1024000,
                evictions: 3,
                swap_count: 1,
                swap_faults: 0,
                oom_events: 0,
            },
        }
    }

    #[test]
    fn serialization_roundtrip() {
        let snap = make_test_snapshot();
        let json = serde_json::to_string_pretty(&snap).expect("serialize");
        let restored: KernelSnapshot = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.version, "0.5.0");
        assert_eq!(restored.processes.len(), 2);
        assert_eq!(restored.processes[0].pid, 1);
        assert_eq!(restored.scheduler.entries.len(), 1);
        assert_eq!(restored.scheduler.entries[0].priority, "high");
        assert_eq!(restored.metrics.total_commands, 42);
        assert_eq!(restored.memory.total_blocks, 256);
        assert_eq!(
            restored.generation.as_ref().unwrap().temperature,
            0.7
        );
    }

    #[test]
    fn save_and_load_checkpoint_atomic() {
        let snap = make_test_snapshot();
        let dir = PathBuf::from(format!(
            "workspace/test_checkpoint_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("checkpoint.json");

        let msg = save_checkpoint(&snap, &path).expect("save");
        assert!(msg.contains("checkpoint saved"));
        assert!(path.exists());

        // tmp must not linger
        assert!(!path.with_extension("json.tmp").exists());

        let loaded = load_checkpoint(&path).expect("load");
        assert_eq!(loaded.version, snap.version);
        assert_eq!(loaded.processes.len(), snap.processes.len());
        assert_eq!(loaded.scheduler.entries.len(), snap.scheduler.entries.len());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_corrupt_checkpoint_returns_error() {
        let dir = PathBuf::from(format!(
            "workspace/test_checkpoint_corrupt_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("checkpoint.json");
        fs::write(&path, b"{{not valid json").unwrap();

        let err = load_checkpoint(&path).expect_err("should fail on corrupt data");
        assert!(err.contains("corrupt"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_missing_checkpoint_returns_error() {
        let err = load_checkpoint(Path::new("workspace/nonexistent_checkpoint.json"))
            .expect_err("should fail on missing file");
        assert!(err.contains("cannot read"));
    }

    #[test]
    fn now_timestamp_contains_epoch() {
        let ts = now_timestamp();
        assert!(ts.starts_with("epoch_"));
    }
}
