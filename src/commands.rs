use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::{infer_workload_class, parse_workload_hint, ModelCatalog};
use crate::prompting::{GenerationConfig, PromptFamily};
use crate::protocol::{self, OpCode};
use crate::transport::Client;

#[derive(Default)]
struct MetricsState {
    total_commands: u64,
    total_errors: u64,
    total_exec_started: u64,
    total_signals: u64,
}

fn metrics_state() -> &'static Mutex<MetricsState> {
    static METRICS: std::sync::OnceLock<Mutex<MetricsState>> = std::sync::OnceLock::new();
    METRICS.get_or_init(|| Mutex::new(MetricsState::default()))
}

fn metrics_start() -> &'static Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now)
}

fn record_command(success: bool) {
    let mut lock = metrics_state().lock().unwrap();
    lock.total_commands += 1;
    if !success {
        lock.total_errors += 1;
    }
}

fn inc_exec_started() {
    let mut lock = metrics_state().lock().unwrap();
    lock.total_exec_started += 1;
}

fn inc_signal_count() {
    let mut lock = metrics_state().lock().unwrap();
    lock.total_signals += 1;
}

fn snapshot_metrics() -> (u64, u64, u64, u64, u64) {
    let lock = metrics_state().lock().unwrap();
    (
        metrics_start().elapsed().as_secs(),
        lock.total_commands,
        lock.total_errors,
        lock.total_exec_started,
        lock.total_signals,
    )
}

fn log_event(event: &str, client_id: usize, pid: Option<u64>, detail: &str) {
    let pid_text = pid
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".to_string());
    eprintln!(
        "event={} client_id={} pid={} detail=\"{}\"",
        event,
        client_id,
        pid_text,
        detail.replace('"', "'")
    );
}

pub fn execute_command(
    client: &mut Client,
    header: crate::protocol::CommandHeader,
    payload: Vec<u8>,
    memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    model_catalog: &mut ModelCatalog,
    active_family: &mut PromptFamily,
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
                        Err(e) => {
                            protocol::response_err_code("LOAD_FAILED", &format!("{}", e))
                        }
                    }
                }
                Err(e) => protocol::response_err_code("MODEL_SELECTOR", &e),
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
                    Err(e) => protocol::response_err_code("MODEL_NOT_FOUND", &e),
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
                    Err(e) => protocol::response_err_code("MODEL_INFO", &e),
                }
            }
        }
        OpCode::Exec => {
            let prompt_raw = String::from_utf8_lossy(&payload).to_string();
            let (hinted_workload, prompt) = parse_workload_hint(&prompt_raw);
            let workload = hinted_workload.unwrap_or_else(|| infer_workload_class(&prompt));

            let _ = model_catalog.refresh();
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

            let mut lock = engine_state.lock().unwrap();
            if let Some(engine) = lock.as_mut() {
                match engine.spawn_process(&prompt, 0, client_id) {
                    Ok(pid) => {
                        if let Some(token_slots) = engine.process_max_tokens(pid) {
                            if let Err(e) = memory.borrow_mut().register_process(pid, token_slots) {
                                engine.kill_process(pid);
                                return client.output_buffer.extend(protocol::response_err_code(
                                    "MEMORY_ADMISSION",
                                    &e,
                                ));
                            }
                        }

                        inc_exec_started();
                        log_event(
                            "process_spawn",
                            client_id,
                            Some(pid),
                            &format!("exec_started workload={:?}", workload),
                        );
                        protocol::response_ok(&format!("Process Started PID: {}", pid))
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
                    protocol::response_ok_code(
                        "STATUS",
                        &format!(
                            "uptime_s={} total_commands={} total_errors={} total_exec_started={} total_signals={} active_processes={} waiting_processes={} active_pids={:?} waiting_pids={:?} generation=temperature:{} top_p:{} seed:{} max_tokens:{} mem_active={} mem_total_blocks={} mem_free_blocks={} mem_tracked_pids={} mem_allocated_tensors={} mem_alloc_bytes={} mem_evictions={} mem_swap_count={} mem_swap_faults={} mem_swap_failures={} mem_pending_swaps={} mem_waiting_pids={} mem_oom_events={}",
                            uptime_s,
                            total_cmd,
                            total_err,
                            total_exec,
                            total_signals,
                            active.len(),
                            waiting.len(),
                            active,
                            waiting,
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
                            mem.oom_events
                        ),
                    )
                } else if let Ok(pid) = requested.parse::<u64>() {
                    if let Some(line) = engine.process_status_line(pid) {
                        protocol::response_ok_code("STATUS", &line)
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
                        "uptime_s={} total_commands={} total_errors={} total_exec_started={} total_signals={} active_processes=0 active_pids=[] no_model_loaded=true",
                        uptime_s, total_cmd, total_err, total_exec, total_signals
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
                    Err(e) => protocol::response_err_code("MEMW_FAILED", &e),
                }
            }
            Err(e) => protocol::response_err_code("MEMW_INVALID", &e),
        },
    };

    if response.starts_with(b"+OK") {
        record_command(true);
    } else {
        record_command(false);
    }

    client.output_buffer.extend(response);
}

fn parse_memw_payload(payload: &[u8]) -> Result<(u64, Vec<u8>), String> {
    if payload.is_empty() {
        return Err("MEMW payload empty. Use '<pid>\\n<raw-bytes>' or '<pid>|<text>'".to_string());
    }

    if let Some(pos) = payload.iter().position(|b| *b == b'\n') {
        let pid_str = String::from_utf8_lossy(&payload[..pos]).trim().to_string();
        let pid: u64 = pid_str
            .parse()
            .map_err(|_| format!("Invalid PID '{}'.", pid_str))?;
        let raw = payload[pos + 1..].to_vec();
        if raw.is_empty() {
            return Err("MEMW raw bytes are empty after PID header".to_string());
        }
        return Ok((pid, raw));
    }

    let text = String::from_utf8(payload.to_vec())
        .map_err(|_| "MEMW payload must be valid UTF-8 when using pipe format".to_string())?;
    let mut parts = text.splitn(2, '|');
    let pid_str = parts.next().unwrap_or("").trim();
    let body = parts
        .next()
        .ok_or_else(|| "MEMW pipe format requires '<pid>|<text>'".to_string())?;

    let pid: u64 = pid_str
        .parse()
        .map_err(|_| format!("Invalid PID '{}'.", pid_str))?;

    Ok((pid, body.as_bytes().to_vec()))
}

fn parse_generation_payload(payload: &str, base: GenerationConfig) -> Result<GenerationConfig, String> {
    if payload.is_empty() {
        return Err("SET_GEN payload is empty. Use key=value pairs.".to_string());
    }

    let mut cfg = base;

    for pair in payload.split([',', ';']) {
        let item = pair.trim();
        if item.is_empty() {
            continue;
        }

        let mut it = item.splitn(2, '=');
        let key = it.next().unwrap_or("").trim().to_lowercase();
        let value = it
            .next()
            .ok_or_else(|| format!("Invalid item '{}'. Expected key=value", item))?
            .trim();

        match key.as_str() {
            "temperature" | "temp" => {
                let parsed: f64 = value
                    .parse()
                    .map_err(|_| format!("Invalid temperature '{}'.", value))?;
                if !(0.0..=2.0).contains(&parsed) {
                    return Err("temperature must be in [0.0, 2.0]".to_string());
                }
                cfg.temperature = parsed;
            }
            "top_p" | "topp" => {
                let parsed: f64 = value
                    .parse()
                    .map_err(|_| format!("Invalid top_p '{}'.", value))?;
                if !(0.0..=1.0).contains(&parsed) {
                    return Err("top_p must be in [0.0, 1.0]".to_string());
                }
                cfg.top_p = parsed;
            }
            "seed" => {
                cfg.seed = value
                    .parse()
                    .map_err(|_| format!("Invalid seed '{}'.", value))?;
            }
            "max_tokens" | "max_new_tokens" => {
                let parsed: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid max_tokens '{}'.", value))?;
                if parsed == 0 {
                    return Err("max_tokens must be > 0".to_string());
                }
                cfg.max_tokens = parsed;
            }
            _ => return Err(format!("Unknown SET_GEN key '{}'.", key)),
        }
    }

    Ok(cfg)
}
