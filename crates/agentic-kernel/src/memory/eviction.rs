use super::core::NeuralMemory;
/// LRU eviction logic for NeuralMemory.
///
/// Extracted from `core.rs` to keep the main allocator file focused on
/// block allocation and process-level bookkeeping.
use super::types::ContextSlotId;

impl NeuralMemory {
    /// Remove all pages of a context slot and return their blocks to the free list.
    /// Optionally counts the operation as an eviction in metrics.
    pub(super) fn clear_slot_pages(
        &mut self,
        slot_id: ContextSlotId,
        count_as_eviction: bool,
    ) -> usize {
        let Some(slot) = self.slot_table.get_mut(&slot_id) else {
            return 0;
        };

        if slot.pages.is_empty() {
            return 0;
        }

        let released_blocks = slot.pages.len();
        let elements_per_block = self.config.block_size * self.config.hidden_dim;

        for block_idx in slot.pages.drain(..) {
            self.free_blocks.push_back(block_idx);
        }

        let released_bytes = released_blocks * elements_per_block * 4;
        self.counters.alloc_bytes = self.counters.alloc_bytes.saturating_sub(released_bytes);

        if count_as_eviction {
            self.counters.evictions += 1;
        }

        released_blocks
    }

    /// Move a context slot to the most-recently-used end of the LRU queue.
    pub(super) fn touch_slot_lru(&mut self, slot_id: ContextSlotId) {
        self.lru_order.retain(|&current| current != slot_id);
        self.lru_order.push_back(slot_id);
    }

    /// Pick the next LRU victim that has allocated pages,
    /// skipping `protected` (the slot we're currently writing to).
    pub(super) fn next_lru_victim(
        &mut self,
        protected: Option<ContextSlotId>,
    ) -> Option<ContextSlotId> {
        let attempts = self.lru_order.len();
        for _ in 0..attempts {
            let candidate = self.lru_order.pop_front()?;
            self.lru_order.push_back(candidate);

            if Some(candidate) == protected {
                continue;
            }

            let has_pages = self
                .slot_table
                .get(&candidate)
                .map(|slot| !slot.pages.is_empty())
                .unwrap_or(false);
            if has_pages {
                return Some(candidate);
            }
        }

        None
    }

    /// Evict LRU context slots until at least `required_blocks` are free.
    /// Returns `true` if the space was reclaimed successfully.
    pub(super) fn evict_lru_until_fit(
        &mut self,
        required_blocks: usize,
        protected: Option<ContextSlotId>,
    ) -> bool {
        let mut guard = 0usize;
        let guard_limit = self.slot_table.len().saturating_add(1);

        while self.free_blocks.len() < required_blocks {
            if guard >= guard_limit {
                return false;
            }

            let Some(victim) = self.next_lru_victim(protected) else {
                return false;
            };

            let freed = self.clear_slot_pages(victim, true);
            if freed == 0 {
                guard += 1;
            } else {
                guard = 0;
            }
        }

        true
    }
}
