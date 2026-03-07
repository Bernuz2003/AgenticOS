use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use candle_core::{DType, Device, Tensor};

use crate::process::{AgentProcess, ProcessState};

/// Command sent from the main thread to the inference worker.
pub enum InferenceCmd {
    /// Perform one inference step for the given process.
    Step {
        pid: u64,
        process: AgentProcess,
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
        process: AgentProcess,
        token_id: u32,
        finished: bool,
    },
    /// Inference failed — the process has been dropped (model weights freed).
    Error {
        pid: u64,
        error: String,
    },
}

/// Run one inference step on a checked-out process.
///
/// Replicates the forward+sample logic from `LLMEngine::step_process()`
/// but operates on an owned `AgentProcess` without needing access to the engine.
fn run_step(
    mut process: AgentProcess,
    eos_token_id: u32,
    eot_token_id: u32,
) -> Result<(AgentProcess, u32, bool)> {
    let device = Device::Cpu;

    process.state = ProcessState::Running;

    let mut next_token: u32 = 0;

    while process.index_pos < process.tokens.len() {
        let input_token = process.tokens[process.index_pos];
        let input_tensor = Tensor::new(&[input_token], &device)?.unsqueeze(0)?;
        let logits = process.model.forward(&input_tensor, process.index_pos)?;
        process.index_pos += 1;

        if process.index_pos == process.tokens.len() {
            let logits = logits.squeeze(0)?.squeeze(0)?.to_dtype(DType::F32)?;
            next_token = process.logits_processor.sample(&logits)?;
        }
    }

    process.tokens.push(next_token);

    let mut finished = false;
    if next_token == eos_token_id || next_token == eot_token_id || next_token == 2 {
        finished = true;
    }
    if process.tokens.len() >= process.max_tokens {
        finished = true;
    }
    if finished {
        process.state = ProcessState::Finished;
    }

    Ok((process, next_token, finished))
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
                    } => match run_step(process, eos_token_id, eot_token_id) {
                        Ok((process, token_id, finished)) => {
                            if result_tx
                                .send(InferenceResult::Token {
                                    pid,
                                    process,
                                    token_id,
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
