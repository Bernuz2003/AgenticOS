use candle_core::{DType, Device, Tensor};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

pub type TensorId = u64;
type BlockIndex = usize;

#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub active: bool,
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub allocated_tensors: usize,
    pub tracked_pids: usize,
    pub alloc_bytes: usize,
    pub evictions: u64,
    pub swap_count: u64,
    pub swap_faults: u64,
    pub swap_failures: u64,
    pub pending_swaps: usize,
    pub waiting_pids: usize,
    pub oom_events: u64,
}

#[derive(Default)]
struct MemoryCounters {
    alloc_bytes: usize,
    evictions: u64,
    swap_count: u64,
    swap_faults: u64,
    swap_failures: u64,
    oom_events: u64,
}

#[derive(Debug, Clone)]
pub struct SwapEvent {
    pub pid: u64,
    pub success: bool,
    pub detail: String,
}

struct SwapJob {
    pid: u64,
    payload: Vec<u8>,
}

struct SwapResult {
    pid: u64,
    success: bool,
    detail: String,
}

#[derive(Clone)]
pub struct MemoryConfig {
    pub block_size: usize,
    pub hidden_dim: usize,
    pub total_memory_mb: usize,
}

struct PhysicalBlock {
    tensor: Tensor,
}

impl PhysicalBlock {
    fn new(size_elements: usize, device: &Device) -> Result<Self, String> {
        // Creiamo un tensore vuoto (zeri) sul dispositivo (CPU/GPU)
        let t = Tensor::zeros((size_elements,), DType::F32, device)
            .map_err(|e| format!("Candle alloc error: {}", e))?;

        Ok(PhysicalBlock { tensor: t })
    }
}

pub struct NeuralMemory {
    config: MemoryConfig,
    device: Device, // CPU, Cuda, o Metal

    physical_blocks: Vec<PhysicalBlock>,
    free_blocks: VecDeque<BlockIndex>,
    page_table: HashMap<TensorId, Vec<BlockIndex>>,

    pid_to_tensor: HashMap<u64, TensorId>,
    tensor_to_pid: HashMap<TensorId, u64>,
    pid_token_slots: HashMap<u64, usize>,
    token_slot_quota_per_pid: usize,
    counters: MemoryCounters,
    active: bool,

    swap_enabled: bool,
    swap_dir: PathBuf,
    swap_tx: Option<Sender<SwapJob>>,
    swap_rx: Option<Receiver<SwapResult>>,
    waiting_for_memory: HashSet<u64>,
    lru_order: VecDeque<TensorId>,

    next_tensor_id: TensorId,
}

impl NeuralMemory {
    pub fn new(config: MemoryConfig) -> Result<Self, String> {
        // Seleziona Hardware.
        // In futuro qui logic per Device::Cuda(0) o Device::Metal
        let device = Device::Cpu;

        let elements_per_block = config.block_size * config.hidden_dim;
        let bytes_per_element = 4; // f32
        let total_bytes = config.total_memory_mb * 1024 * 1024;
        let bytes_per_block = elements_per_block * bytes_per_element;
        let num_blocks = total_bytes / bytes_per_block;

        println!(
            "INIT GPU MEMORY (Candle): {} MB. Blocks: {} ({} params each)",
            config.total_memory_mb, num_blocks, elements_per_block
        );

        let mut physical_blocks = Vec::with_capacity(num_blocks);
        let mut free_blocks = VecDeque::with_capacity(num_blocks);

        for i in 0..num_blocks {
            physical_blocks.push(PhysicalBlock::new(elements_per_block, &device)?);
            free_blocks.push_back(i);
        }

        Ok(NeuralMemory {
            config,
            device,
            physical_blocks,
            free_blocks,
            page_table: HashMap::new(),
            pid_to_tensor: HashMap::new(),
            tensor_to_pid: HashMap::new(),
            pid_token_slots: HashMap::new(),
            token_slot_quota_per_pid: 4096,
            counters: MemoryCounters::default(),
            active: true,
            swap_enabled: false,
            swap_dir: PathBuf::from("workspace/swap"),
            swap_tx: None,
            swap_rx: None,
            waiting_for_memory: HashSet::new(),
            lru_order: VecDeque::new(),
            next_tensor_id: 1,
        })
    }

    pub fn configure_async_swap(
        &mut self,
        enabled: bool,
        swap_dir: Option<PathBuf>,
    ) -> Result<(), String> {
        if !enabled {
            self.swap_enabled = false;
            self.swap_tx = None;
            self.swap_rx = None;
            self.waiting_for_memory.clear();
            return Ok(());
        }

        let validated_swap_dir = Self::resolve_valid_swap_dir(swap_dir)?;
        self.swap_dir = validated_swap_dir.clone();

        let (tx_job, rx_job) = mpsc::channel::<SwapJob>();
        let (tx_result, rx_result) = mpsc::channel::<SwapResult>();
        let worker_dir = validated_swap_dir;

        thread::Builder::new()
            .name("agentic_swap_worker".to_string())
            .spawn(move || {
                while let Ok(job) = rx_job.recv() {
                    let now_ns = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    let file_stem = format!("pid_{}_{}", job.pid, now_ns);

                    let result = Self::persist_swap_payload(&worker_dir, &file_stem, &job.payload)
                        .map(|final_path| {
                            format!(
                                "swap persisted pid={} bytes={} file={}",
                                job.pid,
                                job.payload.len(),
                                final_path.display()
                            )
                        });

                    let event = match result {
                        Ok(msg) => SwapResult {
                            pid: job.pid,
                            success: true,
                            detail: msg,
                        },
                        Err(err) => {
                            SwapResult {
                                pid: job.pid,
                                success: false,
                                detail: format!("swap failed pid={}: {}", job.pid, err),
                            }
                        }
                    };

                    if tx_result.send(event).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn swap worker: {}", e))?;

        self.swap_enabled = true;
        self.swap_tx = Some(tx_job);
        self.swap_rx = Some(rx_result);
        Ok(())
    }

    fn resolve_valid_swap_dir(requested: Option<PathBuf>) -> Result<PathBuf, String> {
        let cwd = std::env::current_dir().map_err(|e| format!("Cannot read current dir: {}", e))?;
        let workspace_root = cwd.join("workspace");
        fs::create_dir_all(&workspace_root)
            .map_err(|e| format!("Cannot create workspace dir {:?}: {}", workspace_root, e))?;

        let candidate = requested.unwrap_or_else(|| workspace_root.join("swap"));

        if !candidate.is_absolute() {
            for comp in candidate.components() {
                if matches!(comp, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
                    return Err(format!(
                        "Invalid swap path {:?}: traversal or absolute components are not allowed for relative paths",
                        candidate
                    ));
                }
            }
        }

        let candidate_abs = if candidate.is_absolute() {
            candidate
        } else {
            cwd.join(candidate)
        };

        fs::create_dir_all(&candidate_abs)
            .map_err(|e| format!("Failed to create swap dir {:?}: {}", candidate_abs, e))?;

        let workspace_canon = fs::canonicalize(&workspace_root)
            .map_err(|e| format!("Failed to canonicalize workspace dir {:?}: {}", workspace_root, e))?;
        let candidate_canon = fs::canonicalize(&candidate_abs)
            .map_err(|e| format!("Failed to canonicalize swap dir {:?}: {}", candidate_abs, e))?;

        if !candidate_canon.starts_with(&workspace_canon) {
            return Err(format!(
                "Swap directory must be inside workspace root (workspace={:?}, requested={:?})",
                workspace_canon, candidate_canon
            ));
        }

        Ok(candidate_canon)
    }

    fn persist_swap_payload(base_dir: &Path, file_stem: &str, payload: &[u8]) -> Result<PathBuf, String> {
        let tmp_path = base_dir.join(format!("{}.tmp", file_stem));
        let final_path = base_dir.join(format!("{}.swap", file_stem));

        if tmp_path.parent() != Some(base_dir) || final_path.parent() != Some(base_dir) {
            return Err("Swap path safety violation: computed file path escaped base dir".to_string());
        }

        let mut tmp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|e| format!("tmp open failed: {}", e))?;

        if let Err(e) = tmp_file.write_all(payload) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("tmp write failed: {}", e));
        }

        if let Err(e) = tmp_file.sync_all() {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("tmp fsync failed: {}", e));
        }

        drop(tmp_file);

        if let Err(e) = fs::rename(&tmp_path, &final_path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("atomic rename failed: {}", e));
        }

        Ok(final_path)
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_token_slot_quota_per_pid(&mut self, quota: usize) {
        self.token_slot_quota_per_pid = quota.max(1);
    }

    pub fn register_process(&mut self, pid: u64, token_slots: usize) -> Result<TensorId, String> {
        if !self.active {
            return Ok(0);
        }

        if token_slots == 0 {
            self.counters.oom_events += 1;
            return Err("NeuralMemory: token_slots must be > 0".to_string());
        }
        if token_slots > self.token_slot_quota_per_pid {
            self.counters.oom_events += 1;
            return Err(format!(
                "NeuralMemory: PID {} requested {} token slots > quota {}",
                pid, token_slots, self.token_slot_quota_per_pid
            ));
        }

        if let Some(existing) = self.pid_to_tensor.get(&pid).copied() {
            self.pid_token_slots.insert(pid, token_slots);
            self.touch_tensor_lru(existing);
            return Ok(existing);
        }

        let tensor_id = self.alloc();
        self.pid_to_tensor.insert(pid, tensor_id);
        self.tensor_to_pid.insert(tensor_id, pid);
        self.pid_token_slots.insert(pid, token_slots);
        Ok(tensor_id)
    }

    pub fn release_process(&mut self, pid: u64) -> Result<String, String> {
        if !self.active {
            return Ok(format!("NeuralMemory disabled: release skipped for PID {}", pid));
        }

        self.waiting_for_memory.remove(&pid);

        let Some(tensor_id) = self.pid_to_tensor.remove(&pid) else {
            return Ok(format!("NeuralMemory: PID {} had no allocation", pid));
        };

        self.pid_token_slots.remove(&pid);
        self.tensor_to_pid.remove(&tensor_id);
        self.release_tensor(tensor_id)
    }

    pub fn write_for_pid_bytes(&mut self, pid: u64, raw_data: &[u8]) -> Result<String, String> {
        if !self.active {
            return Ok(format!(
                "NeuralMemory disabled: MEMW skipped for PID {} ({} bytes)",
                pid,
                raw_data.len()
            ));
        }

        let tensor_id = self
            .pid_to_tensor
            .get(&pid)
            .copied()
            .ok_or_else(|| format!("NeuralMemory: PID {} is not registered", pid))?;
        match self.write_from_bytes(tensor_id, raw_data) {
            Ok(msg) => {
                self.waiting_for_memory.remove(&pid);
                Ok(msg)
            }
            Err(e) => {
                if e.starts_with("OOM:") && self.swap_enabled {
                    self.counters.swap_faults += 1;
                    let queued = self.enqueue_swap(pid, raw_data.to_vec())?;
                    return Ok(queued);
                }
                Err(e)
            }
        }
    }

    pub fn snapshot(&self) -> MemorySnapshot {
        MemorySnapshot {
            active: self.active,
            total_blocks: self.physical_blocks.len(),
            free_blocks: self.free_blocks.len(),
            allocated_tensors: self.page_table.len(),
            tracked_pids: self.pid_to_tensor.len(),
            alloc_bytes: self.counters.alloc_bytes,
            evictions: self.counters.evictions,
            swap_count: self.counters.swap_count,
            swap_faults: self.counters.swap_faults,
            swap_failures: self.counters.swap_failures,
            pending_swaps: self.waiting_for_memory.len(),
            waiting_pids: self.waiting_for_memory.len(),
            oom_events: self.counters.oom_events,
        }
    }

    pub fn is_pid_waiting_for_memory(&self, pid: u64) -> bool {
        self.waiting_for_memory.contains(&pid)
    }

    pub fn waiting_pids(&self) -> Vec<u64> {
        let mut out = self.waiting_for_memory.iter().copied().collect::<Vec<_>>();
        out.sort_unstable();
        out
    }

    pub fn poll_swap_events(&mut self) -> Vec<SwapEvent> {
        let mut events = Vec::new();
        let Some(rx) = &self.swap_rx else {
            return events;
        };

        loop {
            match rx.try_recv() {
                Ok(result) => {
                    self.waiting_for_memory.remove(&result.pid);
                    if result.success {
                        self.counters.swap_count += 1;
                    } else {
                        self.counters.swap_failures += 1;
                    }
                    events.push(SwapEvent {
                        pid: result.pid,
                        success: result.success,
                        detail: result.detail,
                    });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.swap_enabled = false;
                    self.swap_tx = None;
                    self.swap_rx = None;
                    break;
                }
            }
        }

        events
    }

    fn enqueue_swap(&mut self, pid: u64, payload: Vec<u8>) -> Result<String, String> {
        let Some(tx) = &self.swap_tx else {
            return Err("Swap queue is not available".to_string());
        };

        self.waiting_for_memory.insert(pid);

        tx.send(SwapJob {
            pid,
            payload: payload.clone(),
        })
        .map_err(|e| {
            self.waiting_for_memory.remove(&pid);
            format!("Failed to enqueue swap job for PID {}: {}", pid, e)
        })?;

        Ok(format!(
            "OOM: PID {} queued for async swap ({} bytes)",
            pid,
            payload.len()
        ))
    }

    pub fn alloc(&mut self) -> TensorId {
        let id = self.next_tensor_id;
        self.next_tensor_id += 1;
        self.page_table.insert(id, Vec::new());
        self.touch_tensor_lru(id);
        id
    }

    /// Legge un tensore ricostruendo i dati dai blocchi fisici sparsi.
    /// Operazione lenta (Device -> Host), usata solo per debug o salvataggio.
    pub fn read(&self, id: TensorId) -> Result<Vec<f32>, String> {
        // 1. Recupera la lista delle pagine
        let pages = self
            .page_table
            .get(&id)
            .ok_or("Tensor ID not found".to_string())?;

        let mut output = Vec::new();

        // Itera su ogni blocco fisico
        for &block_idx in pages {
            let block = &self.physical_blocks[block_idx];

            // Converte il Tensore Candle in Vec<f32> standard
            // to_vec1() scarica i dati dalla GPU/Tensor alla CPU se necessario
            let chunk: Vec<f32> = block
                .tensor
                .to_vec1()
                .map_err(|e| format!("Candle read error: {}", e))?;

            output.extend(chunk);
        }

        Ok(output)
    }

    /// Scrittura reale: Prende byte grezzi, li converte in Tensor e li salva nei blocchi
    pub fn write_from_bytes(&mut self, id: TensorId, raw_data: &[u8]) -> Result<String, String> {
        if !self.active {
            return Ok(format!(
                "NeuralMemory disabled: write skipped for tensor {} ({} bytes)",
                id,
                raw_data.len()
            ));
        }

        if !self.page_table.contains_key(&id) {
            return Err("Tensor ID not found".to_string());
        }

        self.clear_tensor_pages(id, true);

        // Converti bytes -> f32 (assumiamo Little Endian per ora)
        // Nota: In produzione questo è unsafe cast per velocità, qui safe copy
        let f32_data: Vec<f32> = raw_data
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        if f32_data.is_empty() {
            return Ok("No data".to_string());
        }

        let elements_per_block = self.config.block_size * self.config.hidden_dim;
        let blocks_needed = (f32_data.len() + elements_per_block - 1) / elements_per_block;

        if self.free_blocks.len() < blocks_needed {
            let recovered = self.evict_lru_until_fit(blocks_needed, Some(id));
            if !recovered {
                self.counters.oom_events += 1;
                return Err("OOM: Not enough GPU blocks".to_string());
            }
        }

        if self.free_blocks.len() < blocks_needed {
            self.counters.oom_events += 1;
            return Err("OOM: Not enough GPU blocks".to_string());
        }

        let mut data_offset = 0;

        for _ in 0..blocks_needed {
            let block_idx = self.free_blocks.pop_front().unwrap();

            // Logica di slice dei dati
            let end = std::cmp::min(data_offset + elements_per_block, f32_data.len());
            let chunk_data = &f32_data[data_offset..end];

            // Creiamo un tensore temporaneo dai dati
            let temp_tensor = Tensor::from_slice(chunk_data, (chunk_data.len(),), &self.device)
                .map_err(|e| e.to_string())?;

            // Scriviamo nel blocco fisico (Sostituzione parziale o totale)
            // Nota: Candle non ha "copy_into" mutabile facile sui tensori base.
            // Sostituiamo direttamente il tensore nel blocco per semplicità o usiamo slice_assign in futuro.
            // Qui facciamo una semplificazione: se il blocco è pieno, lo sovrascriviamo.
            // Se è parziale, dovremmo fare padding.

            // Per ora: Padding con zeri se il chunk è più piccolo del blocco
            let final_tensor = if chunk_data.len() < elements_per_block {
                let pad_size = elements_per_block - chunk_data.len();
                let zeros = Tensor::zeros((pad_size,), DType::F32, &self.device)
                    .map_err(|e| e.to_string())?;
                Tensor::cat(&[&temp_tensor, &zeros], 0).map_err(|e| e.to_string())?
            } else {
                temp_tensor
            };

            self.physical_blocks[block_idx].tensor = final_tensor;

            // Update Page Table
            if let Some(pages) = self.page_table.get_mut(&id) {
                pages.push(block_idx);
            }

            data_offset = end;
        }

        self.counters.alloc_bytes += blocks_needed * elements_per_block * 4;
        self.touch_tensor_lru(id);

        Ok(format!(
            "Written {} floats into {} blocks",
            f32_data.len(),
            blocks_needed
        ))
    }

    pub fn release_tensor(&mut self, id: TensorId) -> Result<String, String> {
        if !self.active {
            return Ok(format!("NeuralMemory disabled: release skipped for tensor {}", id));
        }

        let pages = self
            .page_table
            .remove(&id)
            .ok_or_else(|| "Tensor ID not found".to_string())?;

        let elements_per_block = self.config.block_size * self.config.hidden_dim;
        let released_blocks = pages.len();
        for block in pages {
            self.free_blocks.push_back(block);
        }

        let released_bytes = released_blocks * elements_per_block * 4;
        self.counters.alloc_bytes = self.counters.alloc_bytes.saturating_sub(released_bytes);

        if let Some(pid) = self.tensor_to_pid.remove(&id) {
            self.pid_to_tensor.remove(&pid);
            self.pid_token_slots.remove(&pid);
        }

        self.lru_order.retain(|&t| t != id);

        Ok(format!(
            "Released tensor {} ({} blocks)",
            id, released_blocks
        ))
    }

    pub fn compute_test(&self, id: TensorId, multiplier: f32) -> Result<String, String> {
        let pages = self.page_table.get(&id).ok_or("ID not found")?;

        let mut report = String::new();

        for (i, &block_idx) in pages.iter().enumerate() {
            let block_tensor = &self.physical_blocks[block_idx].tensor;

            // Calcolo (Inference)
            let res = (block_tensor * (multiplier as f64)).map_err(|e| e.to_string())?;

            let max_val: f32 = res
                .max_all()
                .map_err(|e| e.to_string())?
                .to_scalar()
                .map_err(|e| e.to_string())?;

            let mean_val: f32 = res
                .mean_all()
                .map_err(|e| e.to_string())?
                .to_scalar()
                .map_err(|e| e.to_string())?;

            // Peek (Anteprima): Estraiamo i primi 5 valori per vederli con i nostri occhi
            // Appiattiamo il tensore e prendiamo i primi valori
            let vec: Vec<f32> = res
                .flatten_all()
                .map_err(|e| e.to_string())?
                .to_vec1()
                .map_err(|e| e.to_string())?;
            let snippet = &vec[..std::cmp::min(vec.len(), 5)];

            report.push_str(&format!(
                "\n  > Block {}: Max={:.2}, Mean={:.5}, Data={:?}",
                i, max_val, mean_val, snippet
            ));
        }

        Ok(report)
    }

    pub fn stats(&self) -> String {
        let s = self.snapshot();
        format!(
            "active={} free_blocks={} total_blocks={} tracked_pids={} allocated_tensors={} alloc_bytes={} evictions={} swap_count={} swap_faults={} swap_failures={} pending_swaps={} oom_events={}",
            s.active,
            s.free_blocks,
            s.total_blocks,
            s.tracked_pids,
            s.allocated_tensors,
            s.alloc_bytes,
            s.evictions,
            s.swap_count,
            s.swap_faults,
            s.swap_failures,
            s.pending_swaps,
            s.oom_events
        )
    }

    fn clear_tensor_pages(&mut self, id: TensorId, count_as_eviction: bool) -> usize {
        let Some(pages) = self.page_table.get_mut(&id) else {
            return 0;
        };

        if pages.is_empty() {
            return 0;
        }

        let released_blocks = pages.len();
        let elements_per_block = self.config.block_size * self.config.hidden_dim;

        for block_idx in pages.drain(..) {
            self.free_blocks.push_back(block_idx);
        }

        let released_bytes = released_blocks * elements_per_block * 4;
        self.counters.alloc_bytes = self.counters.alloc_bytes.saturating_sub(released_bytes);

        if count_as_eviction {
            self.counters.evictions += 1;
        }

        released_blocks
    }

    fn touch_tensor_lru(&mut self, id: TensorId) {
        self.lru_order.retain(|&current| current != id);
        self.lru_order.push_back(id);
    }

    fn next_lru_victim(&mut self, protected: Option<TensorId>) -> Option<TensorId> {
        let attempts = self.lru_order.len();
        for _ in 0..attempts {
            let candidate = self.lru_order.pop_front()?;
            self.lru_order.push_back(candidate);

            if Some(candidate) == protected {
                continue;
            }

            let has_pages = self
                .page_table
                .get(&candidate)
                .map(|pages| !pages.is_empty())
                .unwrap_or(false);
            if has_pages {
                return Some(candidate);
            }
        }

        None
    }

    fn evict_lru_until_fit(&mut self, required_blocks: usize, protected: Option<TensorId>) -> bool {
        let mut guard = 0usize;
        let guard_limit = self.page_table.len().saturating_add(1);

        while self.free_blocks.len() < required_blocks {
            if guard >= guard_limit {
                return false;
            }

            let Some(victim) = self.next_lru_victim(protected) else {
                return false;
            };

            let freed = self.clear_tensor_pages(victim, true);
            if freed == 0 {
                guard += 1;
            } else {
                guard = 0;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryConfig, NeuralMemory};
    use std::fs;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn register_write_release_pid_flow() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_token_slot_quota_per_pid(32);
        let _tensor = mem.register_process(42, 16).expect("register pid");

        let payload = [1.0f32, 2.0f32, 3.0f32, 4.0f32]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        let write_msg = mem
            .write_for_pid_bytes(42, &payload)
            .expect("write bytes for pid");
        assert!(write_msg.contains("Written"));

        let snapshot_before_release = mem.snapshot();
        assert_eq!(snapshot_before_release.tracked_pids, 1);
        assert!(snapshot_before_release.alloc_bytes > 0);

        let rel = mem.release_process(42).expect("release pid");
        assert!(rel.contains("Released tensor") || rel.contains("had no allocation"));

        let snapshot_after_release = mem.snapshot();
        assert_eq!(snapshot_after_release.tracked_pids, 0);
    }

    #[test]
    fn quota_enforcement_increments_oom_counter() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_token_slot_quota_per_pid(8);
        let err = mem
            .register_process(7, 64)
            .expect_err("quota should reject large token slots");
        assert!(err.contains("quota"));
        assert!(mem.snapshot().oom_events >= 1);
    }

    #[test]
    fn fallback_mode_is_noop_and_safe() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_active(false);
        assert!(!mem.is_active());

        let reg = mem.register_process(100, 12).expect("register noop");
        assert_eq!(reg, 0);

        let wr = mem
            .write_for_pid_bytes(100, b"hello")
            .expect("write noop should be ok");
        assert!(wr.contains("disabled"));

        let rel = mem.release_process(100).expect("release noop");
        assert!(rel.contains("disabled"));
    }

    #[test]
    fn pressure_with_multiple_pids_triggers_oom_events() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 1024,
            hidden_dim: 1024,
            total_memory_mb: 1,
        })
        .expect("memory init");
        mem.set_token_slot_quota_per_pid(4096);

        let float_count = 300_000usize;
        let payload = vec![0f32; float_count]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();

        for pid in 1..=3u64 {
            let _ = mem.register_process(pid, 512).expect("register pid");
            let _ = mem.write_for_pid_bytes(pid, &payload);
        }

        let snap = mem.snapshot();
        assert!(snap.oom_events >= 1);
    }

    #[test]
    fn lru_eviction_frees_other_tensor_before_oom() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 512,
            hidden_dim: 256,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_token_slot_quota_per_pid(4096);
        mem.register_process(1, 512).expect("register pid1");
        mem.register_process(2, 512).expect("register pid2");

        let one_block_floats = 512 * 256;
        let one_block_payload = vec![1.0f32; one_block_floats]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();

        mem.write_for_pid_bytes(1, &one_block_payload)
            .expect("write pid1");
        mem.write_for_pid_bytes(2, &one_block_payload)
            .expect("write pid2");

        let two_block_payload = vec![2.0f32; one_block_floats * 2]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        mem.write_for_pid_bytes(1, &two_block_payload)
            .expect("lru eviction should avoid oom");

        let tensor2 = mem.pid_to_tensor.get(&2).copied().expect("tensor pid2");
        let pages2 = mem.page_table.get(&tensor2).cloned().unwrap_or_default();
        assert!(pages2.is_empty(), "pid2 tensor should be evicted by LRU");
        assert!(mem.snapshot().evictions >= 2);
    }

    #[test]
    fn async_swap_queue_marks_waiting_and_completes() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 1024,
            hidden_dim: 1024,
            total_memory_mb: 1,
        })
        .expect("memory init");

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let swap_dir = PathBuf::from(format!("workspace/test_swap_{}", now_ns));
        mem.configure_async_swap(true, Some(swap_dir.clone()))
            .expect("enable async swap");

        mem.set_token_slot_quota_per_pid(4096);
        mem.register_process(1, 512).expect("register pid");

        let float_count = 300_000usize;
        let payload = vec![1.0f32; float_count]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();

        let msg = mem
            .write_for_pid_bytes(1, &payload)
            .expect("should enqueue on oom");
        assert!(msg.contains("queued for async swap"));
        assert!(mem.is_pid_waiting_for_memory(1));
        assert!(mem.snapshot().swap_faults >= 1);

        let mut completed = false;
        for _ in 0..50 {
            let events = mem.poll_swap_events();
            if events.iter().any(|ev| ev.pid == 1 && ev.success) {
                completed = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert!(completed, "swap event did not complete in time");
        assert!(!mem.is_pid_waiting_for_memory(1));

        let snap = mem.snapshot();
        assert!(snap.swap_count >= 1);

        let _ = std::fs::remove_dir_all(swap_dir);
    }

    #[test]
    fn configure_async_swap_rejects_outside_workspace() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        let outside = std::env::temp_dir().join("agenticos_swap_outside");
        let err = mem
            .configure_async_swap(true, Some(outside))
            .expect_err("outside workspace path must be rejected");
        assert!(err.contains("inside workspace root"));
    }

    #[test]
    fn configure_async_swap_rejects_relative_traversal() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        let err = mem
            .configure_async_swap(true, Some(PathBuf::from("../swap_escape")))
            .expect_err("relative traversal must be rejected");
        assert!(err.contains("traversal"));
    }

    #[test]
    fn persist_swap_payload_is_atomic_and_cleans_tmp() {
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = PathBuf::from(format!("workspace/test_swap_io_{}", now_ns));
        fs::create_dir_all(&base).expect("create base dir");

        let final_path = NeuralMemory::persist_swap_payload(&base, "pid_7_test", b"abc123")
            .expect("persist payload");
        assert!(final_path.exists());

        let tmp_path = base.join("pid_7_test.tmp");
        assert!(!tmp_path.exists());

        let body = fs::read(&final_path).expect("read final swap file");
        assert_eq!(body, b"abc123");

        let _ = fs::remove_dir_all(base);
    }
}
