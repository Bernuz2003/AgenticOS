use super::admission::*;
/// Core resource governor implementation handling activation queues.
use super::state::*;

use crate::backend::BackendClass;
use crate::config::ResourceGovernorConfig;
use crate::diagnostics::audit::{self, AuditContext};
use crate::model_catalog::ResolvedModelTarget;
use crate::runtimes::StoredRuntimeLoadQueueEntry;
use crate::runtimes::{runtime_key_for_target, RuntimeRegistry, RuntimeReservation};
use crate::session::SessionRegistry;
use crate::storage::StorageService;

pub(crate) struct ResourceGovernor {
    config: ResourceGovernorConfig,
    queue_entries: Vec<StoredRuntimeLoadQueueEntry>,
    loader_reason: Option<String>,
}

impl ResourceGovernor {
    pub(crate) fn load(
        storage: &mut StorageService,
        config: ResourceGovernorConfig,
    ) -> Result<Self, ResourceGovernorError> {
        let queue_entries =
            storage.load_runtime_load_queue_entries(config.max_queue_entries.max(16) * 4)?;
        Ok(Self {
            config,
            queue_entries,
            loader_reason: None,
        })
    }

    pub(crate) fn prepare_activation(
        &mut self,
        storage: &mut StorageService,
        runtime_registry: &RuntimeRegistry,
        session_registry: &SessionRegistry,
        target: &ResolvedModelTarget,
    ) -> Result<AdmissionPlan, ResourceGovernorError> {
        let runtime_key = runtime_key_for_target(target);
        let backend_class = target.driver_resolution().backend_class;
        let reservation = estimate_reservation(target, &self.config);

        if backend_class != BackendClass::ResidentLocal {
            return Ok(AdmissionPlan {
                reservation,
                evict_runtime_ids: Vec::new(),
                requires_loader_lock: false,
            });
        }

        if let Some(runtime_id) = runtime_registry.runtime_id_for_target(target) {
            let same_runtime_target =
                runtime_registry
                    .descriptor(&runtime_id)
                    .is_some_and(|descriptor| {
                        descriptor.runtime_reference == target.runtime_reference()
                    });
            if runtime_registry.is_runtime_loaded(&runtime_id) && same_runtime_target {
                return Ok(AdmissionPlan {
                    reservation,
                    evict_runtime_ids: Vec::new(),
                    requires_loader_lock: false,
                });
            }
        }

        if let Some(loader_reason) = self.loader_reason.as_ref() {
            let reason = format!(
                "loader busy: {}; requested reservation ram={} vram={}",
                loader_reason,
                format_bytes(reservation.ram_bytes),
                format_bytes(reservation.vram_bytes)
            );
            self.enqueue_pending(storage, &runtime_key, target, reservation, &reason)?;
            audit::record(
                storage,
                audit::ADMISSION_LOADER_BUSY,
                format!("model={} {}", target.logical_model_id(), reason),
                AuditContext::default(),
            );
            audit::record(
                storage,
                audit::ADMISSION_QUEUED,
                format!("model={} {}", target.logical_model_id(), reason),
                AuditContext::default(),
            );
            return Err(ResourceGovernorError::Busy(format!(
                "Local runtime load queued: {}",
                reason
            )));
        }

        if let Some(reason) = self.reject_if_single_runtime_exceeds_budget(reservation) {
            self.record_refusal(storage, &runtime_key, target, reservation, &reason)?;
            audit::record(
                storage,
                audit::ADMISSION_DENIED,
                format!("model={} {}", target.logical_model_id(), reason),
                AuditContext::default(),
            );
            return Err(ResourceGovernorError::Refused(format!(
                "Local runtime load refused: {}",
                reason
            )));
        }

        let usage = self.current_usage(runtime_registry);
        if fits_with_headroom(&self.config, usage, reservation) {
            return Ok(AdmissionPlan {
                reservation,
                evict_runtime_ids: Vec::new(),
                requires_loader_lock: true,
            });
        }

        let mut freed = RuntimeReservation::default();
        let mut evict_runtime_ids = Vec::new();
        let mut candidates = runtime_registry
            .runtime_views()
            .into_iter()
            .filter(|runtime| {
                runtime.backend_class == BackendClass::ResidentLocal.as_str()
                    && runtime.active_pid_count == 0
                    && !runtime.pinned
                    && runtime.transition_state.is_none()
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            let left_sessions = session_registry.session_count_for_runtime(&left.runtime_id);
            let right_sessions = session_registry.session_count_for_runtime(&right.runtime_id);
            left_sessions
                .cmp(&right_sessions)
                .then_with(|| left.active_pids.len().cmp(&right.active_pids.len()))
                .then_with(|| left.runtime_id.cmp(&right.runtime_id))
        });
        candidates.sort_by_key(|runtime| {
            runtime_registry
                .descriptor(&runtime.runtime_id)
                .map(|descriptor| descriptor.last_used_at_ms)
                .unwrap_or(i64::MAX)
        });

        for candidate in candidates {
            if let Some(runtime_id) = runtime_registry.runtime_id_for_target(target) {
                let same_runtime_target =
                    runtime_registry
                        .descriptor(&runtime_id)
                        .is_some_and(|descriptor| {
                            descriptor.runtime_reference == target.runtime_reference()
                        });
                if candidate.runtime_id == runtime_id && same_runtime_target {
                    continue;
                }
            }
            evict_runtime_ids.push(candidate.runtime_id);
            freed.ram_bytes = freed
                .ram_bytes
                .saturating_add(candidate.reservation_ram_bytes);
            freed.vram_bytes = freed
                .vram_bytes
                .saturating_add(candidate.reservation_vram_bytes);
            if fits_with_headroom_after_free(&self.config, usage, freed, reservation) {
                return Ok(AdmissionPlan {
                    reservation,
                    evict_runtime_ids,
                    requires_loader_lock: true,
                });
            }
        }

        let reason = format!(
            "insufficient local budget: used ram={} vram={}, requested ram={} vram={}, budget ram={} vram={}, headroom ram={} vram={}",
            format_bytes(usage.ram_bytes),
            format_bytes(usage.vram_bytes),
            format_bytes(reservation.ram_bytes),
            format_bytes(reservation.vram_bytes),
            format_bytes(self.config.ram_budget_bytes),
            format_bytes(self.config.vram_budget_bytes),
            format_bytes(self.config.min_ram_headroom_bytes),
            format_bytes(self.config.min_vram_headroom_bytes)
        );
        if self.pending_queue_depth() < self.config.max_queue_entries {
            self.enqueue_pending(storage, &runtime_key, target, reservation, &reason)?;
            audit::record(
                storage,
                audit::ADMISSION_QUEUED,
                format!("model={} {}", target.logical_model_id(), reason),
                AuditContext::default(),
            );
            return Err(ResourceGovernorError::Busy(format!(
                "Local runtime load queued: {}",
                reason
            )));
        }

        let refusal = format!("{reason}; pending queue is full");
        self.record_refusal(storage, &runtime_key, target, reservation, &refusal)?;
        audit::record(
            storage,
            audit::ADMISSION_DENIED,
            format!("model={} {}", target.logical_model_id(), refusal),
            AuditContext::default(),
        );
        Err(ResourceGovernorError::Refused(format!(
            "Local runtime load refused: {}",
            refusal
        )))
    }

    pub(crate) fn try_acquire_loader_lock(
        &mut self,
        reason: &str,
    ) -> Result<(), ResourceGovernorError> {
        if let Some(loader_reason) = self.loader_reason.as_ref() {
            return Err(ResourceGovernorError::Busy(format!(
                "Local runtime loader busy: {}",
                loader_reason
            )));
        }
        self.loader_reason = Some(reason.to_string());
        Ok(())
    }

    pub(crate) fn release_loader_lock(&mut self) {
        self.loader_reason = None;
    }

    pub(crate) fn mark_runtime_admitted(
        &mut self,
        storage: &mut StorageService,
        runtime_key: &str,
        reason: &str,
    ) -> Result<(), ResourceGovernorError> {
        let updated = storage.mark_runtime_load_queue_entries_for_runtime(
            runtime_key,
            "pending",
            "admitted",
            reason,
        )?;
        if updated > 0 {
            self.reload_queue_entries(storage)?;
        }
        Ok(())
    }

    pub(crate) fn status(&self, runtime_registry: &RuntimeRegistry) -> ResourceGovernorStatus {
        let usage = self.current_usage(runtime_registry);
        ResourceGovernorStatus {
            ram_budget_bytes: self.config.ram_budget_bytes,
            vram_budget_bytes: self.config.vram_budget_bytes,
            min_ram_headroom_bytes: self.config.min_ram_headroom_bytes,
            min_vram_headroom_bytes: self.config.min_vram_headroom_bytes,
            ram_used_bytes: usage.ram_bytes,
            vram_used_bytes: usage.vram_bytes,
            ram_available_bytes: budget_available(
                self.config.ram_budget_bytes,
                self.config.min_ram_headroom_bytes,
                usage.ram_bytes,
            ),
            vram_available_bytes: budget_available(
                self.config.vram_budget_bytes,
                self.config.min_vram_headroom_bytes,
                usage.vram_bytes,
            ),
            pending_queue_depth: self.pending_queue_depth(),
            loader_busy: self.loader_reason.is_some(),
            loader_reason: self.loader_reason.clone(),
        }
    }

    pub(crate) fn queue_views(&self) -> Vec<RuntimeLoadQueueEntry> {
        self.queue_entries
            .iter()
            .map(|entry| RuntimeLoadQueueEntry {
                queue_id: entry.queue_id,
                logical_model_id: entry.logical_model_id.clone(),
                display_path: entry.display_path.clone(),
                backend_class: entry.backend_class.clone(),
                state: entry.state.clone(),
                reservation_ram_bytes: entry.reservation_ram_bytes,
                reservation_vram_bytes: entry.reservation_vram_bytes,
                reason: entry.reason.clone(),
                requested_at_ms: entry.requested_at_ms,
                updated_at_ms: entry.updated_at_ms,
            })
            .collect()
    }

    fn enqueue_pending(
        &mut self,
        storage: &mut StorageService,
        runtime_key: &str,
        target: &ResolvedModelTarget,
        reservation: RuntimeReservation,
        reason: &str,
    ) -> Result<(), ResourceGovernorError> {
        if let Some(existing) = storage.find_pending_runtime_load_queue_entry(runtime_key)? {
            storage.update_runtime_load_queue_entry(
                existing.queue_id,
                "pending",
                reservation.ram_bytes,
                reservation.vram_bytes,
                reason,
            )?;
        } else {
            storage.insert_runtime_load_queue_entry(
                runtime_key,
                &target.logical_model_id(),
                &target.display_path().display().to_string(),
                target.driver_resolution().backend_class.as_str(),
                "pending",
                reservation.ram_bytes,
                reservation.vram_bytes,
                reason,
            )?;
        }
        self.reload_queue_entries(storage)?;
        Ok(())
    }

    fn record_refusal(
        &mut self,
        storage: &mut StorageService,
        runtime_key: &str,
        target: &ResolvedModelTarget,
        reservation: RuntimeReservation,
        reason: &str,
    ) -> Result<(), ResourceGovernorError> {
        storage.mark_runtime_load_queue_entries_for_runtime(
            runtime_key,
            "pending",
            "refused",
            reason,
        )?;
        storage.insert_runtime_load_queue_entry(
            runtime_key,
            &target.logical_model_id(),
            &target.display_path().display().to_string(),
            target.driver_resolution().backend_class.as_str(),
            "refused",
            reservation.ram_bytes,
            reservation.vram_bytes,
            reason,
        )?;
        self.reload_queue_entries(storage)?;
        Ok(())
    }

    fn reject_if_single_runtime_exceeds_budget(
        &self,
        reservation: RuntimeReservation,
    ) -> Option<String> {
        let ram_limit = effective_budget(
            self.config.ram_budget_bytes,
            self.config.min_ram_headroom_bytes,
        );
        if ram_limit != u64::MAX && reservation.ram_bytes > ram_limit {
            return Some(format!(
                "requested RAM reservation {} exceeds budget {} after headroom {}",
                format_bytes(reservation.ram_bytes),
                format_bytes(self.config.ram_budget_bytes),
                format_bytes(self.config.min_ram_headroom_bytes)
            ));
        }

        let vram_limit = effective_budget(
            self.config.vram_budget_bytes,
            self.config.min_vram_headroom_bytes,
        );
        if vram_limit != u64::MAX && reservation.vram_bytes > vram_limit {
            return Some(format!(
                "requested VRAM reservation {} exceeds budget {} after headroom {}",
                format_bytes(reservation.vram_bytes),
                format_bytes(self.config.vram_budget_bytes),
                format_bytes(self.config.min_vram_headroom_bytes)
            ));
        }

        None
    }

    fn current_usage(&self, runtime_registry: &RuntimeRegistry) -> RuntimeReservation {
        runtime_registry
            .runtime_views()
            .into_iter()
            .filter(|runtime| {
                runtime.backend_class == BackendClass::ResidentLocal.as_str()
                    && matches!(
                        runtime.state.as_str(),
                        "loaded" | "active" | "loading" | "evicting"
                    )
            })
            .fold(RuntimeReservation::default(), |mut usage, runtime| {
                usage.ram_bytes = usage
                    .ram_bytes
                    .saturating_add(runtime.reservation_ram_bytes);
                usage.vram_bytes = usage
                    .vram_bytes
                    .saturating_add(runtime.reservation_vram_bytes);
                usage
            })
    }

    fn pending_queue_depth(&self) -> usize {
        self.queue_entries
            .iter()
            .filter(|entry| entry.state == "pending")
            .count()
    }

    fn reload_queue_entries(
        &mut self,
        storage: &mut StorageService,
    ) -> Result<(), ResourceGovernorError> {
        self.queue_entries =
            storage.load_runtime_load_queue_entries(self.config.max_queue_entries.max(16) * 4)?;
        Ok(())
    }
}
