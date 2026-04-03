use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use mio::Waker;

use crate::backend::{InferenceFinishReason, InferenceStepRequest};
use crate::process::{AgentProcess, ProcessState};
use crate::services::accounting::BackendAccountingEvent;

/// Command sent from the main thread to the inference worker.
pub enum InferenceCmd {
    /// Perform one inference step for the given process.
    Step {
        pid: u64,
        process: Box<AgentProcess>,
        rendered_prompt: String,
        resident_prompt_suffix: String,
        eos_token_id: u32,
        eot_token_id: u32,
    },
    /// Shut down the worker thread.
    Shutdown,
}

/// Result returned from the inference worker to the main thread.
pub enum InferenceResult {
    /// Streaming chunk emitted while an inference step is still in-flight.
    StreamChunk {
        pid: u64,
        text: String,
        first_chunk: bool,
    },
    /// Successful inference step.
    Token {
        pid: u64,
        process: Box<AgentProcess>,
        text_output: String,
        reasoning_output: String,
        generated_tokens: usize,
        finished: bool,
        finish_reason: Option<InferenceFinishReason>,
        accounting_event: Option<BackendAccountingEvent>,
    },
    /// Inference failed — the process has been dropped (model weights freed).
    Error {
        pid: u64,
        error: String,
        accounting_event: Option<BackendAccountingEvent>,
    },
}

/// Spawn the inference worker thread.
///
/// The worker receives `InferenceCmd`s from `cmd_rx`, runs the forward pass,
/// and sends `InferenceResult`s back via `result_tx`.
pub fn spawn_worker(
    result_tx: mpsc::Sender<InferenceResult>,
    cmd_rx: mpsc::Receiver<InferenceCmd>,
    wake_loop: Option<Arc<Waker>>,
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
                        rendered_prompt,
                        resident_prompt_suffix,
                        eos_token_id,
                        eot_token_id,
                    } => {
                        let mut streamed_text = String::new();
                        let mut sent_any_chunk = false;
                        let mut on_chunk = |chunk: &str| {
                            if chunk.is_empty() {
                                return;
                            }
                            streamed_text.push_str(chunk);
                            let first_chunk = !sent_any_chunk;
                            sent_any_chunk = true;
                            if result_tx
                                .send(InferenceResult::StreamChunk {
                                    pid,
                                    text: chunk.to_string(),
                                    first_chunk,
                                })
                                .is_ok()
                            {
                                if let Some(waker) = wake_loop.as_ref() {
                                    let _ = waker.wake();
                                }
                            }
                        };

                        let mut process = *process;
                        let remaining_generation_budget = process
                            .max_tokens
                            .saturating_sub(process.generated_tokens_in_current_turn());

                        if remaining_generation_budget == 0 {
                            process.state = if process.lifecycle_policy.is_interactive() {
                                ProcessState::WaitingForInput
                            } else {
                                ProcessState::Finished
                            };
                            if result_tx
                                .send(InferenceResult::Token {
                                    pid,
                                    process: Box::new(process),
                                    text_output: String::new(),
                                    reasoning_output: String::new(),
                                    generated_tokens: 0,
                                    finished: true,
                                    finish_reason: Some(InferenceFinishReason::TurnBudgetExhausted),
                                    accounting_event: None,
                                })
                                .is_err()
                            {
                                break;
                            }
                            if let Some(waker) = wake_loop.as_ref() {
                                let _ = waker.wake();
                            }
                            continue;
                        }

                        process.state = ProcessState::Running;

                        let step = match process.model.generate_step(InferenceStepRequest {
                            context_slot_id: process.context_slot_id,
                            tokens: &process.tokens,
                            rendered_prompt: &rendered_prompt,
                            resident_prompt_suffix: &resident_prompt_suffix,
                            index_pos: process.index_pos,
                            remaining_generation_budget,
                            tokenizer: &process.tokenizer,
                            generation: process.generation,
                            stream_observer: Some(&mut on_chunk),
                            eos_token_id,
                            eot_token_id,
                        }) {
                            Ok(step) => step,
                            Err(err) => {
                                let accounting_event = process.model.take_last_accounting_event();
                                if result_tx
                                    .send(InferenceResult::Error {
                                        pid,
                                        error: err.to_string(),
                                        accounting_event,
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                                if let Some(waker) = wake_loop.as_ref() {
                                    let _ = waker.wake();
                                }
                                continue;
                            }
                        };

                        process.index_pos = step.next_index_pos;
                        let generated_tokens = step.appended_tokens.len();
                        process.tokens.extend(step.appended_tokens);
                        let accounting_event = process.model.take_last_accounting_event();

                        let mut finished = step.finished;
                        let mut finish_reason = step.finish_reason;
                        if process.generated_tokens_in_current_turn() >= process.max_tokens {
                            finished = true;
                            finish_reason.get_or_insert(InferenceFinishReason::TurnBudgetExhausted);
                        }
                        if finished {
                            process.state = if process.lifecycle_policy.is_interactive()
                                && finish_reason == Some(InferenceFinishReason::TurnBudgetExhausted)
                            {
                                ProcessState::AwaitingTurnDecision
                            } else if process.lifecycle_policy.is_interactive() {
                                ProcessState::WaitingForInput
                            } else {
                                ProcessState::Finished
                            };
                        }

                        let text_output = if streamed_text.is_empty() {
                            step.emitted_text
                        } else {
                            step.emitted_text
                                .strip_prefix(&streamed_text)
                                .map(|suffix| suffix.to_string())
                                .unwrap_or_default()
                        };
                        let reasoning_output = step.emitted_reasoning_text;

                        if result_tx
                            .send(InferenceResult::Token {
                                pid,
                                process: Box::new(process),
                                text_output,
                                reasoning_output,
                                generated_tokens,
                                finished,
                                finish_reason,
                                accounting_event,
                            })
                            .is_err()
                        {
                            tracing::info!("INFERENCE_WORKER: result channel closed, exiting");
                            break;
                        }
                        if let Some(waker) = wake_loop.as_ref() {
                            let _ = waker.wake();
                        }
                    }
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
