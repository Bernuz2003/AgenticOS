use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use mio::Waker;

use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use crate::tools::{handle_syscall, SysCallOutcome, SyscallRateMap};

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum SyscallCmd {
    Execute {
        pid: u64,
        tool_call_id: String,
        content: String,
        caller: ToolCaller,
        permissions: ProcessPermissionPolicy,
        registry: ToolRegistry,
    },
    ReplayCompletion {
        completion: SyscallCompletion,
    },
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct SyscallCompletion {
    pub pid: u64,
    pub tool_call_id: String,
    pub command: String,
    pub caller: ToolCaller,
    pub outcome: SysCallOutcome,
}

pub(crate) fn spawn_syscall_worker(
    rate_map: Arc<Mutex<SyscallRateMap>>,
    result_tx: mpsc::Sender<SyscallCompletion>,
    cmd_rx: mpsc::Receiver<SyscallCmd>,
    wake_loop: Option<Arc<Waker>>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("syscall-worker".into())
        .spawn(move || {
            while let Ok(command) = cmd_rx.recv() {
                match command {
                    SyscallCmd::Execute {
                        pid,
                        tool_call_id,
                        content,
                        caller,
                        permissions,
                        registry,
                    } => {
                        let command = content.clone();
                        let outcome = match rate_map.lock() {
                            Ok(mut guard) => handle_syscall(
                                &content,
                                pid,
                                caller.clone(),
                                permissions,
                                Some(tool_call_id.clone()),
                                &mut guard,
                                &registry,
                            ),
                            Err(_) => SysCallOutcome {
                                output: "SysCall Error: worker rate-limit state is unavailable."
                                    .to_string(),
                                success: false,
                                duration_ms: 0,
                                should_kill_process: true,
                                output_json: None,
                                warnings: Vec::new(),
                                error_kind: Some("worker_unavailable".to_string()),
                                effects: Vec::new(),
                            },
                        };
                        if result_tx
                            .send(SyscallCompletion {
                                pid,
                                tool_call_id,
                                command,
                                caller,
                                outcome,
                            })
                            .is_err()
                        {
                            break;
                        }
                        if let Some(waker) = wake_loop.as_ref() {
                            let _ = waker.wake();
                        }
                    }
                    SyscallCmd::ReplayCompletion { completion } => {
                        if result_tx.send(completion).is_err() {
                            break;
                        }
                        if let Some(waker) = wake_loop.as_ref() {
                            let _ = waker.wake();
                        }
                    }
                    SyscallCmd::Shutdown => break,
                }
            }
        })
        .expect("failed to spawn syscall worker")
}
