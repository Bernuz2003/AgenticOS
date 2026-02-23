use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::prompting::{GenerationConfig, PromptFamily};
use crate::protocol::{self, OpCode};
use crate::transport::Client;

pub fn execute_command(
    client: &mut Client,
    header: crate::protocol::CommandHeader,
    payload: Vec<u8>,
    _memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    model_catalog: &mut ModelCatalog,
    active_family: &mut PromptFamily,
    client_id: usize,
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
            let prompt = String::from_utf8_lossy(&payload).to_string();
            let mut lock = engine_state.lock().unwrap();
            if let Some(engine) = lock.as_mut() {
                match engine.spawn_process(&prompt, 0, client_id) {
                    Ok(pid) => protocol::response_ok(&format!("Process Started PID: {}", pid)),
                    Err(e) => protocol::response_err_code("SPAWN_FAILED", &format!("{}", e)),
                }
            } else {
                protocol::response_err_code("NO_MODEL", "No Model Loaded")
            }
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
        _ => protocol::response_err_code("NOT_IMPLEMENTED", "Not Implemented"),
    };

    client.output_buffer.extend(response);
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
