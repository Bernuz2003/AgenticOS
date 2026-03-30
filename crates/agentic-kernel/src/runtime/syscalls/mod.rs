mod completion;
mod dispatch;
mod human;
mod ids;
mod ipc;
pub(crate) mod parser;
mod worker;

pub(crate) use completion::drain_syscall_results;
pub(crate) use dispatch::{dispatch_process_syscall, scan_syscall_buffer, SyscallDispatchOutcome};
pub(crate) use worker::{spawn_syscall_worker, SyscallCmd, SyscallCompletion};
