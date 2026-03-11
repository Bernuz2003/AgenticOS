use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use candle_core::Device;

use crate::backend::InferenceStepRequest;
use crate::process::{AgentProcess, ProcessState};

/// Command sent from the main thread to the inference worker.
pub enum InferenceCmd {
    /// Perform one inference step for the given process.
    Step {
        pid: u64,
        process: Box<AgentProcess>,
        eos_token_id: u32,
        eot_token_id: u32,
    },
    /// Shut down the worker thread.
    Shutdown,
}

/// Result returned from the inference worker to the main thread.
pub enum InferenceResult {
    /// Successful inference step.
    Token {
        pid: u64,
        process: Box<AgentProcess>,
        text_output: String,
        generated_tokens: usize,
        finished: bool,
    },
    /// Inference failed — the process has been dropped (model weights freed).
    Error { pid: u64, error: String },
}

/// Run one inference step on a checked-out process.
///
/// Replicates the forward+sample logic from `LLMEngine::step_process()`
/// but operates on an owned `AgentProcess` without needing access to the engine.
fn run_step(
    mut process: AgentProcess,
    eos_token_id: u32,
    eot_token_id: u32,
) -> Result<(AgentProcess, String, usize, bool)> {
    let device = Device::Cpu;

    process.state = ProcessState::Running;

    let step = process.model.generate_step(InferenceStepRequest {
        context_slot_id: process.context_slot_id,
        tokens: &process.tokens,
        index_pos: process.index_pos,
        logits_processor: &mut process.logits_processor,
        tokenizer: &process.tokenizer,
        generation: process.generation,
        device: &device,
        eos_token_id,
        eot_token_id,
    })?;

    process.index_pos = step.next_index_pos;
    let generated_tokens = step.appended_tokens.len();
    process.tokens.extend(step.appended_tokens);

    let mut finished = step.finished;
    if process.tokens.len() >= process.max_tokens {
        finished = true;
    }
    if finished {
        process.state = ProcessState::Finished;
    }

    Ok((process, step.emitted_text, generated_tokens, finished))
}

/// Spawn the inference worker thread.
///
/// The worker receives `InferenceCmd`s from `cmd_rx`, runs the forward pass,
/// and sends `InferenceResult`s back via `result_tx`.
pub fn spawn_worker(
    result_tx: mpsc::Sender<InferenceResult>,
    cmd_rx: mpsc::Receiver<InferenceCmd>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("inference-worker".into())
        .spawn(move || {
            tracing::info!("INFERENCE_WORKER: started");
            loop {
                let cmd = match cmd_rx.recv() {
                    Ok(cmd) => cmd,
                    Err(_) => {
                        tracing::info!("INFERENCE_WORKER: channel closed, exiting");
                        break;
                    }
                };
                match cmd {
                    InferenceCmd::Step {
                        pid,
                        process,
                        eos_token_id,
                        eot_token_id,
                    } => match run_step(*process, eos_token_id, eot_token_id) {
                        Ok((process, text_output, generated_tokens, finished)) => {
                            if result_tx
                                .send(InferenceResult::Token {
                                    pid,
                                    process: Box::new(process),
                                    text_output,
                                    generated_tokens,
                                    finished,
                                })
                                .is_err()
                            {
                                tracing::info!("INFERENCE_WORKER: result channel closed, exiting");
                                break;
                            }
                        }
                        Err(e) => {
                            if result_tx
                                .send(InferenceResult::Error {
                                    pid,
                                    error: e.to_string(),
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    },
                    InferenceCmd::Shutdown => {
                        tracing::info!("INFERENCE_WORKER: shutdown command received");
                        break;
                    }
                }
            }
            tracing::info!("INFERENCE_WORKER: exited");
        })
        .expect("failed to spawn inference worker thread")
}
