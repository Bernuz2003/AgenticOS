use std::path::{Path, PathBuf};

use super::residency::LogicalResidencyManager;
use super::types::{ContextSlotId, MemorySnapshot, SlotPersistenceKind, SwapEvent};
use crate::errors::MemoryError;

#[derive(Default)]
pub(super) struct MemoryCounters {
    pub(super) swap_count: u64,
    pub(super) swap_faults: u64,
    pub(super) swap_failures: u64,
    pub(super) oom_events: u64,
}

pub struct NeuralMemory {
    active: bool,
    residency: LogicalResidencyManager,
    pub(super) counters: MemoryCounters,
}

impl NeuralMemory {
    pub fn new() -> Result<Self, MemoryError> {
        tracing::info!("NeuralMemory: init (logical residency + resident slot parking)");

        Ok(Self {
            active: true,
            residency: LogicalResidencyManager::new(
                crate::config::kernel_config()
                    .memory
                    .token_slot_quota_per_pid,
            ),
            counters: MemoryCounters::default(),
        })
    }

    pub fn configure_async_swap(
        &mut self,
        enabled: bool,
        swap_dir: Option<PathBuf>,
    ) -> Result<(), MemoryError> {
        self.residency.configure_async_swap(enabled, swap_dir)
    }

    #[cfg(test)]
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_token_slot_quota_per_pid(&mut self, quota: usize) {
        self.residency.set_token_slot_quota_per_pid(quota);
    }

    pub fn register_process(
        &mut self,
        pid: u64,
        token_slots: usize,
    ) -> Result<ContextSlotId, MemoryError> {
        match self.residency.register_process(pid, token_slots) {
            Ok(slot_id) => Ok(slot_id),
            Err(err @ (MemoryError::ZeroTokenSlots | MemoryError::QuotaExceeded { .. })) => {
                self.counters.oom_events += 1;
                Err(err)
            }
            Err(err) => Err(err),
        }
    }

    pub fn release_process(&mut self, pid: u64) -> Result<String, MemoryError> {
        let Some(slot_id) = self.residency.release_process(pid) else {
            return Ok(format!("NeuralMemory: PID {} had no allocation", pid));
        };

        Ok(format!("Released logical slot {} (pid={})", slot_id, pid))
    }

    #[allow(dead_code)]
    pub fn write_for_pid_bytes(
        &mut self,
        pid: u64,
        raw_data: &[u8],
    ) -> Result<String, MemoryError> {
        self.write_for_pid_bytes_with_backend(pid, raw_data, None)
    }

    pub fn write_for_pid_bytes_with_backend(
        &mut self,
        pid: u64,
        raw_data: &[u8],
        backend_id: Option<&str>,
    ) -> Result<String, MemoryError> {
        let Some(slot_id) = self.residency.slot_for_pid(pid) else {
            if !self.active {
                return Ok(format!(
                    "NeuralMemory inactive: MEMW skipped for PID {} ({} bytes)",
                    pid,
                    raw_data.len()
                ));
            }
            return Err(MemoryError::PidNotRegistered(pid));
        };

        if !self.active {
            return Ok(format!(
                "NeuralMemory inactive: MEMW skipped for PID {} ({} bytes)",
                pid,
                raw_data.len()
            ));
        }

        if !self.residency.swap_enabled() {
            self.residency.clear_parked(pid);
            return Ok(format!(
                "Resident memory pressure noted for PID {} slot {} ({} bytes); async parking disabled",
                pid,
                slot_id,
                raw_data.len()
            ));
        }

        let backend_id = backend_id.ok_or_else(|| {
            MemoryError::Swap(
                "Cannot enqueue resident-slot park without an active backend id".to_string(),
            )
        })?;

        self.counters.swap_faults += 1;
        self.residency.enqueue_swap(pid, backend_id, raw_data.len())
    }

    pub fn snapshot(&self) -> MemorySnapshot {
        let residency = self.residency.snapshot();

        MemorySnapshot {
            active: self.active,
            total_blocks: 0,
            free_blocks: 0,
            allocated_tensors: residency.logical_slots,
            tracked_pids: residency.tracked_pids,
            alloc_bytes: 0,
            evictions: 0,
            swap_count: self.counters.swap_count,
            swap_faults: self.counters.swap_faults,
            swap_failures: self.counters.swap_failures,
            pending_swaps: residency.pending_swaps,
            parked_pids: residency.parked_pids,
            oom_events: self.counters.oom_events,
            swap_worker_crashes: residency.swap_worker_crashes,
        }
    }

    pub fn is_pid_parked(&self, pid: u64) -> bool {
        self.residency.is_pid_parked(pid)
    }

    pub fn slot_for_pid(&self, pid: u64) -> Option<ContextSlotId> {
        self.residency.slot_for_pid(pid)
    }

    pub fn poll_swap_events(&mut self) -> Vec<SwapEvent> {
        let (events, deltas) = self.residency.poll_swap_events();
        self.counters.swap_count += deltas.swap_count_inc;
        self.counters.swap_failures += deltas.swap_failures_inc;
        self.counters.swap_faults += deltas.swap_faults_inc;
        events
    }

    pub fn restore_swapped_pid(
        &mut self,
        pid: u64,
        slot_id: ContextSlotId,
        persistence_kind: SlotPersistenceKind,
        swap_path: Option<&Path>,
    ) -> Result<String, MemoryError> {
        match persistence_kind {
            SlotPersistenceKind::BackendSlotSnapshot => {
                let Some(path) = swap_path else {
                    return Err(MemoryError::Swap(format!(
                        "resident backend slot restore for PID {} had no snapshot path",
                        pid
                    )));
                };

                Ok(format!(
                    "resident backend slot snapshot ready pid={} slot={} snapshot={}",
                    pid,
                    slot_id,
                    path.display()
                ))
            }
            SlotPersistenceKind::Unknown => Err(MemoryError::Swap(format!(
                "swap completion for PID {} used unknown persistence kind",
                pid
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::TestExternalEndpointOverrideGuard;
    use crate::memory::{NeuralMemory, SlotPersistenceKind};
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn spawn_mock_slot_save_server(
        expected_requests: usize,
    ) -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock swap server");
        let address = listener.local_addr().expect("mock swap addr");
        let paths = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let paths_for_thread = std::sync::Arc::clone(&paths);

        let handle = thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("accept mock swap request");
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).expect("read mock swap request");
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                paths_for_thread.lock().expect("lock paths").push(path);

                let body = r#"{"ok":true}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write mock swap response");
            }
        });

        (format!("http://{}", address), paths, handle)
    }

    #[test]
    fn register_write_release_pid_flow_uses_residency_only_metrics() {
        let mut mem = NeuralMemory::new().expect("memory init");

        mem.set_token_slot_quota_per_pid(32);
        let slot_id = mem.register_process(42, 16).expect("register pid");

        let write_msg = mem
            .write_for_pid_bytes(42, b"resident pressure")
            .expect("record resident pressure");
        assert!(write_msg.contains("Resident memory pressure noted"));
        assert!(write_msg.contains(&format!("slot {}", slot_id)));

        let snapshot_before_release = mem.snapshot();
        assert_eq!(snapshot_before_release.tracked_pids, 1);
        assert_eq!(snapshot_before_release.allocated_tensors, 1);
        assert_eq!(snapshot_before_release.alloc_bytes, 0);
        assert_eq!(snapshot_before_release.evictions, 0);

        let rel = mem.release_process(42).expect("release pid");
        assert!(rel.contains("Released logical slot"));

        let snapshot_after_release = mem.snapshot();
        assert_eq!(snapshot_after_release.tracked_pids, 0);
    }

    #[test]
    fn write_for_pid_bytes_rejects_unregistered_pid() {
        let mut mem = NeuralMemory::new().expect("memory init");

        let err = mem
            .write_for_pid_bytes(9, b"12345")
            .expect_err("missing pid should fail");
        assert!(err.to_string().contains("not registered"));
    }

    #[test]
    fn quota_enforcement_increments_oom_counter() {
        let mut mem = NeuralMemory::new().expect("memory init");

        mem.set_token_slot_quota_per_pid(8);
        let err = mem
            .register_process(7, 64)
            .expect_err("quota should reject large token slots");
        assert!(err.to_string().contains("quota"));
        assert!(mem.snapshot().oom_events >= 1);
    }

    #[test]
    fn logical_residency_survives_memory_disable() {
        let mut mem = NeuralMemory::new().expect("memory init");

        let slot_id = mem.register_process(100, 12).expect("register pid");
        mem.set_active(false);
        assert!(!mem.is_active());
        assert_eq!(mem.slot_for_pid(100), Some(slot_id));
        assert_eq!(mem.snapshot().tracked_pids, 1);
        assert!(!mem.snapshot().active);

        let wr = mem
            .write_for_pid_bytes(100, b"hello")
            .expect("write skip should be ok");
        assert!(wr.contains("NeuralMemory inactive"));

        let rel = mem.release_process(100).expect("release logical slot");
        assert!(rel.contains("Released logical slot"));
        assert_eq!(mem.snapshot().tracked_pids, 0);
    }

    #[test]
    fn resident_pressure_without_swap_does_not_park_process() {
        let mut mem = NeuralMemory::new().expect("memory init");

        mem.set_token_slot_quota_per_pid(32);
        mem.register_process(5, 16).expect("register pid");

        let detail = mem
            .write_for_pid_bytes(5, b"simulated pressure")
            .expect("resident pressure should be recorded");

        assert!(detail.contains("async parking disabled"));
        assert!(!mem.is_pid_parked(5));
        assert_eq!(mem.snapshot().swap_faults, 0);
    }

    #[test]
    fn resident_pressure_with_swap_requires_backend_id() {
        let mut mem = NeuralMemory::new().expect("memory init");

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let swap_dir = PathBuf::from(format!("workspace/test_swap_requires_backend_{}", now_ns));
        mem.configure_async_swap(true, Some(swap_dir.clone()))
            .expect("enable async swap");
        mem.register_process(9, 16).expect("register pid");

        let err = mem
            .write_for_pid_bytes(9, b"pressure")
            .expect_err("swap-enabled pressure requires backend id");
        assert!(err.to_string().contains("active backend id"));
        assert!(!mem.is_pid_parked(9));

        let _ = fs::remove_dir_all(swap_dir);
    }

    #[test]
    fn async_swap_queue_marks_parked_and_completes() {
        let (endpoint, paths, server_handle) = spawn_mock_slot_save_server(1);
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);
        let mut mem = NeuralMemory::new().expect("memory init");

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let swap_dir = PathBuf::from(format!("workspace/test_swap_{}", now_ns));
        mem.configure_async_swap(true, Some(swap_dir.clone()))
            .expect("enable async swap");

        mem.set_token_slot_quota_per_pid(4096);
        mem.register_process(1, 512).expect("register pid");

        let payload = vec![1_u8; 300_000];

        let msg = mem
            .write_for_pid_bytes_with_backend(1, &payload, Some("external-llamacpp"))
            .expect("should enqueue resident slot park");
        assert!(msg.contains("queued for async parking"));
        assert!(msg.contains("slot"));
        assert!(mem.is_pid_parked(1));
        assert!(mem.snapshot().swap_faults >= 1);

        let mut completed = false;
        for _ in 0..50 {
            let events = mem.poll_swap_events();
            if events
                .iter()
                .any(|ev| ev.pid == 1 && ev.slot_id != 0 && ev.success)
            {
                completed = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert!(completed, "swap event did not complete in time");
        assert!(!mem.is_pid_parked(1));
        server_handle.join().expect("join mock swap server");

        let snap = mem.snapshot();
        assert!(snap.swap_count >= 1);
        assert_eq!(snap.parked_pids, 0);
        assert_eq!(
            paths.lock().expect("lock paths").as_slice(),
            &["/slots/1?action=save"]
        );

        let _ = std::fs::remove_dir_all(swap_dir);
    }

    #[test]
    fn configure_async_swap_rejects_outside_workspace() {
        let mut mem = NeuralMemory::new().expect("memory init");

        let outside = std::env::temp_dir().join("agenticos_swap_outside");
        let err = mem
            .configure_async_swap(true, Some(outside))
            .expect_err("outside workspace path must be rejected");
        assert!(err.to_string().contains("inside workspace root"));
    }

    #[test]
    fn configure_async_swap_rejects_relative_traversal() {
        let mut mem = NeuralMemory::new().expect("memory init");

        let err = mem
            .configure_async_swap(true, Some(PathBuf::from("../swap_escape")))
            .expect_err("relative traversal must be rejected");
        assert!(err.to_string().contains("traversal"));
    }

    #[test]
    fn persist_swap_payload_uses_backend_slot_snapshot_for_resident_runtime() {
        use crate::memory::swap::SwapManager;

        let (endpoint, paths, server_handle) = spawn_mock_slot_save_server(1);
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = PathBuf::from(format!("workspace/test_swap_io_{}", now_ns));
        fs::create_dir_all(&base).expect("create base dir");

        let (final_path, persistence_kind) =
            SwapManager::persist_payload(&base, 7, 11, "external-llamacpp")
                .expect("persist payload");
        server_handle.join().expect("join mock swap server");
        assert_eq!(persistence_kind, SlotPersistenceKind::BackendSlotSnapshot);
        assert!(!final_path.exists());

        let tmp_path = final_path.with_extension("tmp");
        assert!(!tmp_path.exists());
        assert_eq!(
            paths.lock().expect("lock paths").as_slice(),
            &["/slots/11?action=save"]
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn restore_backend_slot_snapshot_does_not_require_local_file() {
        let mut mem = NeuralMemory::new().expect("memory init");

        mem.set_token_slot_quota_per_pid(32);
        let slot_id = mem.register_process(5, 16).expect("register pid");

        let detail = mem
            .restore_swapped_pid(
                5,
                slot_id,
                SlotPersistenceKind::BackendSlotSnapshot,
                Some(Path::new("workspace/swap/pid_5_slot_1.swap")),
            )
            .expect("backend slot snapshot restore should not read local file");

        assert!(detail.contains("resident backend slot snapshot ready"));
        assert_eq!(mem.slot_for_pid(5), Some(slot_id));
    }
}
