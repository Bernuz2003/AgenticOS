/// LRU eviction logic for NeuralMemory.
///
/// Extracted from `core.rs` to keep the main allocator file focused on
/// block allocation and process-level bookkeeping.
use super::types::TensorId;
use super::core::NeuralMemory;

impl NeuralMemory {
    /// Remove all pages of a tensor and return their blocks to the free list.
    /// Optionally counts the operation as an eviction in metrics.
    pub(super) fn clear_tensor_pages(&mut self, id: TensorId, count_as_eviction: bool) -> usize {
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

    /// Move a tensor to the most-recently-used end of the LRU queue.
    pub(super) fn touch_tensor_lru(&mut self, id: TensorId) {
        self.lru_order.retain(|&current| current != id);
        self.lru_order.push_back(id);
    }

    /// Pick the next LRU victim that has allocated pages,
    /// skipping `protected` (the tensor we're currently writing to).
    pub(super) fn next_lru_victim(&mut self, protected: Option<TensorId>) -> Option<TensorId> {
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

    /// Evict LRU tensors until at least `required_blocks` are free.
    /// Returns `true` if the space was reclaimed successfully.
    pub(super) fn evict_lru_until_fit(&mut self, required_blocks: usize, protected: Option<TensorId>) -> bool {
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
