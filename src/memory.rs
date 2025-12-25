use candle_core::{DType, Device, Tensor};
use std::collections::{HashMap, VecDeque};

pub type TensorId = u64;
type BlockIndex = usize;

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
            next_tensor_id: 1,
        })
    }

    pub fn alloc(&mut self) -> TensorId {
        let id = self.next_tensor_id;
        self.next_tensor_id += 1;
        self.page_table.insert(id, Vec::new());
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
        if !self.page_table.contains_key(&id) {
            return Err("Tensor ID not found".to_string());
        }

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

        Ok(format!(
            "Written {} floats into {} blocks",
            f32_data.len(),
            blocks_needed
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
        format!("Free Blocks: {}", self.free_blocks.len())
    }
}
