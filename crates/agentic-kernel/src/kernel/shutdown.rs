use std::sync::mpsc;
use std::thread::JoinHandle;

use crate::inference_worker::InferenceCmd;
use crate::runtime::syscalls::SyscallCmd;

pub(crate) fn shutdown_workers(
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    syscall_cmd_tx: &mpsc::Sender<SyscallCmd>,
    worker_handle: &mut Option<JoinHandle<()>>,
    syscall_worker_handle: &mut Option<JoinHandle<()>>,
) {
    let _ = cmd_tx.send(InferenceCmd::Shutdown);
    let _ = syscall_cmd_tx.send(SyscallCmd::Shutdown);
    if let Some(handle) = worker_handle.take() {
        let _ = handle.join();
    }
    if let Some(handle) = syscall_worker_handle.take() {
        let _ = handle.join();
    }
}
