use super::*;
use crate::checkpoint::{
    GenerationSnapshot, KernelSnapshot, MemoryCountersSnapshot, MetricsSnapshot, ProcessSnapshot,
    SchedulerEntrySnapshot, SchedulerStateSnapshot,
};
use crate::model_catalog::ModelCatalog;
use crate::process::{ContextPolicy, ContextState, ContextStrategy};
use crate::scheduler::ProcessScheduler;
use crate::tools::invocation::{ProcessPermissionPolicy, ProcessTrustScope, ToolCaller};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn apply_restore_snapshot_clears_existing_scheduler_state() {
    let base = mk_temp_dir("agenticos_restore_apply");
    let models = base.join("models");
    let llama_dir = models.join("llama3.1-8b");
    fs::create_dir_all(&llama_dir).expect("create llama dir");
    fs::write(llama_dir.join("meta-llama-3.1-8b.gguf"), b"stub").expect("write model");

    let mut catalog = ModelCatalog::discover(&models).expect("discover models");
    let model_id = catalog.entries[0].id.clone();
    let mut scheduler = ProcessScheduler::new();
    scheduler.register(
        1,
        crate::model_catalog::WorkloadClass::Fast,
        ProcessPriority::High,
    );
    scheduler.register(
        2,
        crate::model_catalog::WorkloadClass::General,
        ProcessPriority::Low,
    );

    let snapshot = KernelSnapshot {
        timestamp: "epoch_1".to_string(),
        version: "0.5.0".to_string(),
        active_family: "Qwen".to_string(),
        selected_model: Some(model_id.clone()),
        generation: Some(GenerationSnapshot {
            temperature: 0.7,
            top_p: 0.9,
            seed: 42,
            max_tokens: 256,
        }),
        processes: vec![ProcessSnapshot {
            pid: 7,
            owner_id: 1,
            tool_caller: ToolCaller::AgentText,
            permission_policy: test_permissions(),
            state: "Orphaned".to_string(),
            token_count: 0,
            max_tokens: 256,
            context_policy: ContextPolicy::new(ContextStrategy::Summarize, 256, 224, 192, 3),
            context_state: ContextState {
                tokens_used: 12,
                context_compressions: 1,
                context_retrieval_hits: 0,
                last_compaction_reason: Some(
                    "summarize_compacted_segments=2 replaced_tokens=42".to_string(),
                ),
                last_summary_ts: Some("epoch_123".to_string()),
                segments: Vec::new(),
                episodic_segments: Vec::new(),
            },
        }],
        scheduler: SchedulerStateSnapshot {
            entries: vec![SchedulerEntrySnapshot {
                pid: 7,
                priority: "critical".to_string(),
                workload: "code".to_string(),
                max_tokens: 1024,
                max_syscalls: 8,
                tokens_generated: 0,
                syscalls_used: 0,
                elapsed_secs: 0.0,
            }],
        },
        metrics: MetricsSnapshot {
            uptime_secs: 1,
            total_commands: 1,
            total_errors: 0,
            total_exec_started: 0,
            total_signals: 0,
        },
        memory: MemoryCountersSnapshot {
            active: false,
            total_blocks: 0,
            free_blocks: 0,
            allocated_tensors: 0,
            tracked_pids: 0,
            alloc_bytes: 0,
            evictions: 0,
            swap_count: 0,
            swap_faults: 0,
            oom_events: 0,
        },
    };

    let cleared = apply_restore_snapshot(&snapshot, &mut scheduler, &mut catalog);
    assert_eq!(cleared, 2);
    assert_eq!(scheduler.registered_pids(), vec![7]);
    assert_eq!(catalog.selected_id.as_deref(), Some(model_id.as_str()));
    let restored = scheduler
        .restored_process(7)
        .expect("restored process exists");
    assert_eq!(restored.context_policy.strategy, ContextStrategy::Summarize);
    assert_eq!(restored.context_state.tokens_used, 12);

    let _ = fs::remove_dir_all(base);
}

fn test_permissions() -> ProcessPermissionPolicy {
    ProcessPermissionPolicy {
        trust_scope: ProcessTrustScope::InteractiveChat,
        actions_allowed: false,
        allowed_tools: Vec::new(),
        path_scopes: vec![".".to_string()],
    }
}

fn mk_temp_dir(prefix: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time ok")
        .as_nanos();
    std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), ts))
}
