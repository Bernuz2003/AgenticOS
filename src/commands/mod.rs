mod metrics;
mod parsing;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::checkpoint;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::{infer_workload_class, parse_workload_hint, ModelCatalog};
use crate::orchestrator::Orchestrator;
use crate::prompting::PromptFamily;
use crate::protocol::{self, OpCode};
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::transport::Client;

use self::metrics::{inc_exec_started, inc_signal_count, log_event, record_command, snapshot_metrics};
use self::parsing::{parse_generation_payload, parse_memw_payload};

// Re-export for auto-checkpoint in main.rs.
pub(crate) use self::metrics::snapshot_metrics as snapshot_metrics_fn;

use crate::config::env_bool;

#[allow(clippy::too_many_arguments)]
pub fn execute_command(
    client: &mut Client,
    header: crate::protocol::CommandHeader,
    payload: Vec<u8>,
    memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    model_catalog: &mut ModelCatalog,
    active_family: &mut PromptFamily,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
) {
    let response = match header.opcode {
        OpCode::Ping => protocol::response_ok_code("PING", "PONG"),
        OpCode::Load => {
            let _ = model_catalog.refresh();
            let selector = String::from_utf8_lossy(&payload).trim().to_string();
            match model_catalog.resolve_load_target(&selector) {
                Ok((resolved_path, family)) => {
                    let tokenizer_hint = model_catalog
                        .entries
                        .iter()
                        .find(|m| m.path == resolved_path)
                        .and_then(|m| m.tokenizer_path.clone());

                    match LLMEngine::load(
                        resolved_path.to_string_lossy().as_ref(),
                        family,
                        tokenizer_hint,
                    ) {
                        Ok(new_engine) => {
                            let mut lock = engine_state.lock().unwrap();
                            *lock = Some(new_engine);
                            *active_family = family;

                            if let Some(entry) =
                                model_catalog.entries.iter().find(|m| m.path == resolved_path)
                            {
                                model_catalog.selected_id = Some(entry.id.clone());
                            }

                            protocol::response_ok(&format!(
                                "Master Model Loaded. family={:?} path={}",
                                family,
                                resolved_path.display()
                            ))
                        }
                        Err(e) => protocol::response_err_code("LOAD_FAILED", &format!("{}", e)),
                    }
                }
                Err(e) => protocol::response_err_code("MODEL_SELECTOR", &e.to_string()),
            }
        }
        OpCode::ListModels => {
            let _ = model_catalog.refresh();
            protocol::response_ok(&model_catalog.format_list())
        }
        OpCode::SelectModel => {
            let _ = model_catalog.refresh();
            let model_id = String::from_utf8_lossy(&payload).trim().to_string();
            if model_id.is_empty() {
                protocol::response_err_code("MISSING_MODEL_ID", "SELECT_MODEL requires a model id")
            } else {
                match model_catalog.set_selected(&model_id) {
                    Ok(_) => {
                        if let Some(entry) = model_catalog.find_by_id(&model_id) {
                            *active_family = entry.family;
                        }
                        protocol::response_ok(&format!("Selected model '{}'.", model_id))
                    }
                    Err(e) => protocol::response_err_code("MODEL_NOT_FOUND", &e.to_string()),
                }
            }
        }
        OpCode::ModelInfo => {
            let _ = model_catalog.refresh();
            let requested = String::from_utf8_lossy(&payload).trim().to_string();
            let model_id = if requested.is_empty() {
                if let Some(selected) = &model_catalog.selected_id {
                    selected.clone()
                } else {
                    String::new()
                }
            } else {
                requested
            };

            if model_id.is_empty() {
                protocol::response_err("MODEL_INFO requires a model id or an active selected model")
            } else {
                match model_catalog.format_info(&model_id) {
                    Ok(info) => protocol::response_ok(&info),
                    Err(e) => protocol::response_err_code("MODEL_INFO", &e.to_string()),
                }
            }
        }
        OpCode::Exec => {
            let prompt_raw = String::from_utf8_lossy(&payload).to_string();
            let (hinted_workload, prompt) = parse_workload_hint(&prompt_raw);
            let workload = hinted_workload.unwrap_or_else(|| infer_workload_class(&prompt));
            let auto_switch = env_bool("AGENTIC_EXEC_AUTO_SWITCH", false);

            let _ = model_catalog.refresh();
            let can_scheduler_switch = auto_switch || hinted_workload.is_some();
            if can_scheduler_switch {
                if let Some(selected) = model_catalog.select_for_workload(workload).cloned() {
                    let should_reload = *active_family != selected.family;
                    if should_reload {
                        let tokenizer_hint = selected.tokenizer_path.clone();
                        match LLMEngine::load(
                            selected.path.to_string_lossy().as_ref(),
                            selected.family,
                            tokenizer_hint,
                        ) {
                            Ok(new_engine) => {
                                let mut lock = engine_state.lock().unwrap();
                                *lock = Some(new_engine);
                                *active_family = selected.family;
                                model_catalog.selected_id = Some(selected.id.clone());
                                log_event(
                                    "scheduler_model_switch",
                                    client_id,
                                    None,
                                    &format!(
                                        "workload={:?} model_id={} family={:?}",
                                        workload, selected.id, selected.family
                                    ),
                                );
                            }
                            Err(e) => {
                                return client.output_buffer.extend(protocol::response_err_code(
                                    "SCHEDULER_LOAD_FAILED",
                                    &format!("{}", e),
                                ));
                            }
                        }
                    }
                }
            }

            let mut lock = engine_state.lock().unwrap();
            if let Some(engine) = lock.as_mut() {
                match engine.spawn_process(&prompt, 0, client_id) {
                    Ok(pid) => {
                        if let Some(token_slots) = engine.process_max_tokens(pid) {
                            if let Err(e) = memory.borrow_mut().register_process(pid, token_slots) {
                                engine.kill_process(pid);
                                return client.output_buffer.extend(protocol::response_err_code(
                                    "MEMORY_ADMISSION",
                                    &e.to_string(),
                                ));
                            }
                        }

                        scheduler.register(pid, workload, ProcessPriority::Normal);

                        inc_exec_started();
                        log_event(
                            "process_spawn",
                            client_id,
                            Some(pid),
                            &format!("exec_started workload={:?} priority=normal", workload),
                        );
                        protocol::response_ok(&format!("Process Started PID: {} workload={:?} priority=normal", pid, workload))
                    }
                    Err(e) => protocol::response_err_code("SPAWN_FAILED", &format!("{}", e)),
                }
            } else {
                protocol::response_err_code("NO_MODEL", "No Model Loaded")
            }
        }
        OpCode::Status => {
            let requested = String::from_utf8_lossy(&payload).trim().to_string();
            let lock = engine_state.lock().unwrap();
            let (uptime_s, total_cmd, total_err, total_exec, total_signals) = snapshot_metrics();

            if let Some(engine) = lock.as_ref() {
                if requested.is_empty() {
                    let active = engine.list_active_pids();
                    let waiting = engine.list_waiting_pids();
                    let cfg = engine.generation_config();
                    let mem = memory.borrow().snapshot();
                    let loaded_path = engine.loaded_model_path().to_string();
                    let loaded_family = engine.loaded_family();
                    let loaded_model_id = model_catalog
                        .entries
                        .iter()
                        .find(|entry| entry.path.to_string_lossy() == loaded_path)
                        .map(|entry| entry.id.clone())
                        .unwrap_or_else(|| "<unknown>".to_string());
                    let selected_model_id = model_catalog
                        .selected_id
                        .clone()
                        .unwrap_or_else(|| "<none>".to_string());
                    let sched_summary = scheduler.summary();
                    protocol::response_ok_code(
                        "STATUS",
                        &format!(
                            "uptime_s={} total_commands={} total_errors={} total_exec_started={} total_signals={} active_processes={} waiting_processes={} active_pids={:?} waiting_pids={:?} selected_model_id={} loaded_model_id={} loaded_family={:?} loaded_model_path={} generation=temperature:{} top_p:{} seed:{} max_tokens:{} mem_active={} mem_total_blocks={} mem_free_blocks={} mem_tracked_pids={} mem_allocated_tensors={} mem_alloc_bytes={} mem_evictions={} mem_swap_count={} mem_swap_faults={} mem_swap_failures={} mem_pending_swaps={} mem_waiting_pids={} mem_oom_events={} {}",
                            uptime_s,
                            total_cmd,
                            total_err,
                            total_exec,
                            total_signals,
                            active.len(),
                            waiting.len(),
                            active,
                            waiting,
                            selected_model_id,
                            loaded_model_id,
                            loaded_family,
                            loaded_path,
                            cfg.temperature,
                            cfg.top_p,
                            cfg.seed,
                            cfg.max_tokens,
                            mem.active,
                            mem.total_blocks,
                            mem.free_blocks,
                            mem.tracked_pids,
                            mem.allocated_tensors,
                            mem.alloc_bytes,
                            mem.evictions,
                            mem.swap_count,
                            mem.swap_faults,
                            mem.swap_failures,
                            mem.pending_swaps,
                            mem.waiting_pids,
                            mem.oom_events,
                            sched_summary
                        ),
                    )
                } else if let Some(orch_id_str) = requested.strip_prefix("orch:") {
                    if let Ok(orch_id) = orch_id_str.parse::<u64>() {
                        if let Some(status_text) = orchestrator.format_status(orch_id) {
                            protocol::response_ok_code("STATUS", &status_text)
                        } else {
                            protocol::response_err_code(
                                "ORCH_NOT_FOUND",
                                &format!("Orchestration {} not found", orch_id),
                            )
                        }
                    } else {
                        protocol::response_err_code(
                            "STATUS_INVALID",
                            "Orchestration ID must be numeric (orch:<N>)",
                        )
                    }
                } else if let Ok(pid) = requested.parse::<u64>() {
                    if let Some(line) = engine.process_status_line(pid) {
                        // Enrich per-PID status with scheduler info.
                        let sched_info = scheduler.snapshot(pid).map(|s| {
                            format!(
                                " priority={} workload={:?} quota_tokens={} quota_syscalls={} tokens_generated={} syscalls_used={} elapsed_secs={:.2}",
                                s.priority, s.workload, s.quota.max_tokens, s.quota.max_syscalls,
                                s.tokens_generated, s.syscalls_used, s.elapsed_secs
                            )
                        }).unwrap_or_default();
                        protocol::response_ok_code("STATUS", &format!("{}{}", line, sched_info))
                    } else {
                        protocol::response_err_code(
                            "PID_NOT_FOUND",
                            &format!("PID {} not found", pid),
                        )
                    }
                } else {
                    protocol::response_err_code(
                        "STATUS_INVALID",
                        "STATUS payload must be empty or numeric PID",
                    )
                }
            } else {
                protocol::response_ok_code(
                    "STATUS",
                    &format!(
                        "uptime_s={} total_commands={} total_errors={} total_exec_started={} total_signals={} active_processes=0 active_pids=[] selected_model_id={} loaded_model_id=<none> loaded_family=Unknown loaded_model_path=<none> no_model_loaded=true",
                        uptime_s, total_cmd, total_err, total_exec, total_signals
                        ,model_catalog.selected_id.clone().unwrap_or_else(|| "<none>".to_string())
                    ),
                )
            }
        }
        OpCode::Term => {
            let payload_text = String::from_utf8_lossy(&payload).trim().to_string();
            if payload_text.is_empty() {
                protocol::response_err_code("MISSING_PID", "TERM requires PID payload")
            } else if let Ok(pid) = payload_text.parse::<u64>() {
                let mut lock = engine_state.lock().unwrap();
                if let Some(engine) = lock.as_mut() {
                    if engine.terminate_process(pid) {
                        let _ = memory.borrow_mut().release_process(pid);
                        scheduler.unregister(pid);
                        inc_signal_count();
                        log_event("process_term", client_id, Some(pid), "graceful_termination_requested");
                        protocol::response_ok_code("TERM", &format!("Termination requested for PID {}", pid))
                    } else {
                        protocol::response_err_code("PID_NOT_FOUND", &format!("PID {} not found", pid))
                    }
                } else {
                    protocol::response_err_code("NO_MODEL", "No Model Loaded")
                }
            } else {
                protocol::response_err_code("INVALID_PID", "TERM payload must be numeric PID")
            }
        }
        OpCode::Kill => {
            let payload_text = String::from_utf8_lossy(&payload).trim().to_string();
            if payload_text.is_empty() {
                protocol::response_err_code("MISSING_PID", "KILL requires PID payload")
            } else if let Ok(pid) = payload_text.parse::<u64>() {
                let mut lock = engine_state.lock().unwrap();
                if let Some(engine) = lock.as_mut() {
                    engine.kill_process(pid);
                    let _ = memory.borrow_mut().release_process(pid);
                    scheduler.unregister(pid);
                    inc_signal_count();
                    log_event("process_kill", client_id, Some(pid), "killed_immediately");
                    protocol::response_ok_code("KILL", &format!("Killed PID {}", pid))
                } else {
                    protocol::response_err_code("NO_MODEL", "No Model Loaded")
                }
            } else {
                protocol::response_err_code("INVALID_PID", "KILL payload must be numeric PID")
            }
        }
        OpCode::Shutdown => {
            shutdown_requested.store(true, Ordering::SeqCst);
            inc_signal_count();
            log_event("kernel_shutdown", client_id, None, "shutdown_requested=true");
            protocol::response_ok_code("SHUTDOWN", "Kernel shutdown requested")
        }
        OpCode::SetGen => {
            let payload_text = String::from_utf8_lossy(&payload).trim().to_string();
            let mut lock = engine_state.lock().unwrap();
            if let Some(engine) = lock.as_mut() {
                match parse_generation_payload(&payload_text, engine.generation_config()) {
                    Ok(cfg) => {
                        engine.set_generation_config(cfg);
                        protocol::response_ok_code(
                            "SET_GEN",
                            &format!(
                                "temperature={} top_p={} seed={} max_tokens={}",
                                cfg.temperature, cfg.top_p, cfg.seed, cfg.max_tokens
                            ),
                        )
                    }
                    Err(e) => protocol::response_err_code("SET_GEN_INVALID", &e),
                }
            } else {
                protocol::response_err_code("NO_MODEL", "No Model Loaded")
            }
        }
        OpCode::GetGen => {
            let lock = engine_state.lock().unwrap();
            if let Some(engine) = lock.as_ref() {
                let cfg = engine.generation_config();
                protocol::response_ok_code(
                    "GET_GEN",
                    &format!(
                        "temperature={} top_p={} seed={} max_tokens={}",
                        cfg.temperature, cfg.top_p, cfg.seed, cfg.max_tokens
                    ),
                )
            } else {
                protocol::response_err_code("NO_MODEL", "No Model Loaded")
            }
        }
        OpCode::MemoryWrite => match parse_memw_payload(&payload) {
            Ok((pid, raw)) => {
                let mut mem = memory.borrow_mut();
                match mem.write_for_pid_bytes(pid, &raw) {
                    Ok(msg) => {
                        let is_waiting = mem.is_pid_waiting_for_memory(pid);
                        drop(mem);

                        if is_waiting {
                            let mut lock = engine_state.lock().unwrap();
                            if let Some(engine) = lock.as_mut() {
                                let _ = engine.set_process_waiting_for_memory(pid);
                            }
                            protocol::response_ok_code("MEMW_QUEUED", &msg)
                        } else {
                            protocol::response_ok_code("MEMW", &msg)
                        }
                    }
                    Err(e) => protocol::response_err_code("MEMW_FAILED", &e.to_string()),
                }
            }
            Err(e) => protocol::response_err_code("MEMW_INVALID", &e),
        },
        OpCode::SetPriority => {
            let payload_text = String::from_utf8_lossy(&payload).trim().to_string();
            let parts: Vec<&str> = payload_text.splitn(2, char::is_whitespace).collect();
            if parts.len() != 2 {
                protocol::response_err_code(
                    "SET_PRIORITY_INVALID",
                    "SET_PRIORITY requires: <PID> <low|normal|high|critical>",
                )
            } else if let Ok(pid) = parts[0].parse::<u64>() {
                if let Some(level) = ProcessPriority::from_str_loose(parts[1].trim()) {
                    if scheduler.set_priority(pid, level) {
                        log_event("set_priority", client_id, Some(pid), &format!("priority={}", level));
                        protocol::response_ok_code(
                            "SET_PRIORITY",
                            &format!("PID {} priority set to {}", pid, level),
                        )
                    } else {
                        protocol::response_err_code(
                            "PID_NOT_FOUND",
                            &format!("PID {} not tracked by scheduler", pid),
                        )
                    }
                } else {
                    protocol::response_err_code(
                        "SET_PRIORITY_INVALID",
                        &format!("Unknown priority level '{}'. Use: low, normal, high, critical", parts[1]),
                    )
                }
            } else {
                protocol::response_err_code("SET_PRIORITY_INVALID", "PID must be numeric")
            }
        }
        OpCode::GetQuota => {
            let payload_text = String::from_utf8_lossy(&payload).trim().to_string();
            if let Ok(pid) = payload_text.parse::<u64>() {
                if let Some(snap) = scheduler.snapshot(pid) {
                    protocol::response_ok_code(
                        "GET_QUOTA",
                        &format!(
                            "pid={} priority={} workload={:?} max_tokens={} max_syscalls={} tokens_generated={} syscalls_used={} elapsed_secs={:.2}",
                            pid, snap.priority, snap.workload, snap.quota.max_tokens, snap.quota.max_syscalls,
                            snap.tokens_generated, snap.syscalls_used, snap.elapsed_secs
                        ),
                    )
                } else {
                    protocol::response_err_code(
                        "PID_NOT_FOUND",
                        &format!("PID {} not tracked by scheduler", pid),
                    )
                }
            } else {
                protocol::response_err_code("GET_QUOTA_INVALID", "GET_QUOTA requires numeric PID")
            }
        }
        OpCode::SetQuota => {
            let payload_text = String::from_utf8_lossy(&payload).trim().to_string();
            let parts: Vec<&str> = payload_text.splitn(2, char::is_whitespace).collect();
            if parts.len() != 2 {
                protocol::response_err_code(
                    "SET_QUOTA_INVALID",
                    "SET_QUOTA requires: <PID> <max_tokens=N,max_syscalls=N>",
                )
            } else if let Ok(pid) = parts[0].parse::<u64>() {
                if let Some(current) = scheduler.quota(pid).copied() {
                    let mut new_quota = current;
                    let mut parse_ok = true;
                    for kv in parts[1].split(',') {
                        let kv = kv.trim();
                        if let Some((k, v)) = kv.split_once('=') {
                            match k.trim() {
                                "max_tokens" => {
                                    if let Ok(val) = v.trim().parse::<usize>() {
                                        new_quota.max_tokens = val;
                                    } else {
                                        parse_ok = false;
                                    }
                                }
                                "max_syscalls" => {
                                    if let Ok(val) = v.trim().parse::<usize>() {
                                        new_quota.max_syscalls = val;
                                    } else {
                                        parse_ok = false;
                                    }
                                }
                                _ => { parse_ok = false; }
                            }
                        } else {
                            parse_ok = false;
                        }
                    }
                    if parse_ok {
                        scheduler.set_quota(pid, new_quota);
                        log_event("set_quota", client_id, Some(pid),
                            &format!("max_tokens={} max_syscalls={}", new_quota.max_tokens, new_quota.max_syscalls));
                        protocol::response_ok_code(
                            "SET_QUOTA",
                            &format!("PID {} quota updated: max_tokens={} max_syscalls={}",
                                pid, new_quota.max_tokens, new_quota.max_syscalls),
                        )
                    } else {
                        protocol::response_err_code(
                            "SET_QUOTA_INVALID",
                            "Invalid quota format. Use: max_tokens=N,max_syscalls=N",
                        )
                    }
                } else {
                    protocol::response_err_code(
                        "PID_NOT_FOUND",
                        &format!("PID {} not tracked by scheduler", pid),
                    )
                }
            } else {
                protocol::response_err_code("SET_QUOTA_INVALID", "PID must be numeric")
            }
        }
        OpCode::Checkpoint => {
            // Build a KernelSnapshot from live kernel state.
            let payload_text = String::from_utf8_lossy(&payload).trim().to_string();
            let path = if payload_text.is_empty() {
                checkpoint::default_checkpoint_path()
            } else {
                std::path::PathBuf::from(&payload_text)
            };

            let (uptime_s, total_cmd, total_err, total_exec, total_signals) = snapshot_metrics();

            // Collect process list from engine.
            let (processes, generation, active_fam, sel_model) = {
                let lock = engine_state.lock().unwrap();
                if let Some(engine) = lock.as_ref() {
                    let procs: Vec<checkpoint::ProcessSnapshot> = engine
                        .processes
                        .iter()
                        .map(|(pid, p)| checkpoint::ProcessSnapshot {
                            pid: *pid,
                            owner_id: p.owner_id,
                            state: format!("{:?}", p.state),
                            token_count: p.tokens.len(),
                            max_tokens: p.max_tokens,
                        })
                        .collect();
                    let cfg = engine.generation_config();
                    let gen = Some(checkpoint::GenerationSnapshot {
                        temperature: cfg.temperature,
                        top_p: cfg.top_p,
                        seed: cfg.seed,
                        max_tokens: cfg.max_tokens,
                    });
                    (procs, gen, format!("{:?}", *active_family), model_catalog.selected_id.clone())
                } else {
                    (vec![], None, format!("{:?}", *active_family), model_catalog.selected_id.clone())
                }
            };

            let sched_snap = checkpoint::snapshot_scheduler(scheduler);
            let mem_snap = checkpoint::snapshot_memory(&memory.borrow());

            let snapshot = checkpoint::KernelSnapshot {
                timestamp: checkpoint::now_timestamp(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                active_family: active_fam,
                selected_model: sel_model,
                generation,
                processes,
                scheduler: sched_snap,
                metrics: checkpoint::MetricsSnapshot {
                    uptime_secs: uptime_s,
                    total_commands: total_cmd,
                    total_errors: total_err,
                    total_exec_started: total_exec,
                    total_signals,
                },
                memory: mem_snap,
            };

            match checkpoint::save_checkpoint(&snapshot, &path) {
                Ok(msg) => {
                    log_event("checkpoint_save", client_id, None, &msg);
                    protocol::response_ok_code("CHECKPOINT", &msg)
                }
                Err(e) => protocol::response_err_code("CHECKPOINT_FAILED", &e),
            }
        }
        OpCode::Restore => {
            let payload_text = String::from_utf8_lossy(&payload).trim().to_string();
            let path = if payload_text.is_empty() {
                checkpoint::default_checkpoint_path()
            } else {
                std::path::PathBuf::from(&payload_text)
            };

            match checkpoint::load_checkpoint(&path) {
                Ok(snap) => {
                    // Restore scheduler entries: re-register PIDs with saved
                    // priorities and quotas.  Processes themselves are NOT
                    // re-spawned (model weights are not in the checkpoint).
                    for entry in &snap.scheduler.entries {
                        let priority = crate::scheduler::ProcessPriority::from_str_loose(&entry.priority)
                            .unwrap_or(ProcessPriority::Normal);
                        let workload = match entry.workload.to_lowercase().as_str() {
                            "fast" => crate::model_catalog::WorkloadClass::Fast,
                            "code" => crate::model_catalog::WorkloadClass::Code,
                            "reasoning" => crate::model_catalog::WorkloadClass::Reasoning,
                            _ => crate::model_catalog::WorkloadClass::General,
                        };
                        scheduler.register(entry.pid, workload, priority);
                        let quota = crate::scheduler::ProcessQuota {
                            max_tokens: entry.max_tokens,
                            max_syscalls: entry.max_syscalls,
                        };
                        scheduler.set_quota(entry.pid, quota);
                    }

                    // Restore selected model in catalog (won't load weights).
                    if let Some(ref model_id) = snap.selected_model {
                        let _ = model_catalog.set_selected(model_id);
                    }

                    log_event(
                        "checkpoint_restore",
                        client_id,
                        None,
                        &format!(
                            "version={} procs={} sched_entries={} from={:?}",
                            snap.version,
                            snap.processes.len(),
                            snap.scheduler.entries.len(),
                            path
                        ),
                    );
                    protocol::response_ok_code(
                        "RESTORE",
                        &format!(
                            "restored checkpoint version={} timestamp={} scheduler_entries={} processes_metadata={}",
                            snap.version, snap.timestamp, snap.scheduler.entries.len(), snap.processes.len()
                        ),
                    )
                }
                Err(e) => protocol::response_err_code("RESTORE_FAILED", &e),
            }
        }
        OpCode::Orchestrate => {
            // Require a loaded engine before registering the graph.
            {
                let lock = engine_state.lock().unwrap();
                if lock.is_none() {
                    return client.output_buffer.extend(
                        protocol::response_err_code("NO_MODEL", "No Model Loaded — ORCHESTRATE requires a loaded engine"),
                    );
                }
            }

            let payload_text = String::from_utf8_lossy(&payload);
            match serde_json::from_str::<crate::orchestrator::TaskGraphDef>(payload_text.trim()) {
                Ok(graph) => {
                    let total_tasks = graph.tasks.len();
                    match orchestrator.register(graph, client_id) {
                        Ok((orch_id, spawn_requests)) => {
                            let mut spawned = 0usize;
                            let mut lock = engine_state.lock().unwrap();
                            let engine = lock.as_mut().unwrap(); // safe: checked above

                            for req in spawn_requests {
                                match engine.spawn_process(&req.prompt, 0, req.owner_id) {
                                    Ok(pid) => {
                                        if let Some(token_slots) = engine.process_max_tokens(pid) {
                                            if let Err(e) = memory.borrow_mut().register_process(pid, token_slots) {
                                                engine.kill_process(pid);
                                                orchestrator.mark_spawn_failed(orch_id, &req.task_id, &e.to_string());
                                                continue;
                                            }
                                        }
                                        scheduler.register(pid, req.workload, ProcessPriority::Normal);
                                        orchestrator.register_pid(pid, orch_id, &req.task_id);
                                        inc_exec_started();
                                        spawned += 1;
                                    }
                                    Err(e) => {
                                        orchestrator.mark_spawn_failed(orch_id, &req.task_id, &e.to_string());
                                    }
                                }
                            }

                            log_event("orchestrate", client_id, None,
                                &format!("orch_id={} total={} spawned={}", orch_id, total_tasks, spawned));
                            protocol::response_ok_code("ORCHESTRATE",
                                &format!("orchestration_id={} total_tasks={} spawned={}", orch_id, total_tasks, spawned))
                        }
                        Err(e) => protocol::response_err_code("ORCHESTRATE_INVALID", &e),
                    }
                }
                Err(e) => protocol::response_err_code("ORCHESTRATE_JSON", &format!("Invalid task graph JSON: {}", e)),
            }
        }
    };

    if response.starts_with(b"+OK") {
        record_command(true);
    } else {
        record_command(false);
    }

    client.output_buffer.extend(response);
}
