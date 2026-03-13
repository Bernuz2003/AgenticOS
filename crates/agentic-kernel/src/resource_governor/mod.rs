use std::path::Path;

use thiserror::Error;

use crate::audit::{self, AuditContext};
use crate::backend::BackendClass;
use crate::config::ResourceGovernorConfig;
use crate::model_catalog::ResolvedModelTarget;
use crate::runtimes::{runtime_key_for_target, RuntimeRegistry, RuntimeReservation};
use crate::session::SessionRegistry;
use crate::storage::{StorageError, StorageService, StoredRuntimeLoadQueueEntry};

#[derive(Debug, Clone)]
pub(crate) struct ResourceGovernorStatus {
    pub(crate) ram_budget_bytes: u64,
    pub(crate) vram_budget_bytes: u64,
    pub(crate) min_ram_headroom_bytes: u64,
    pub(crate) min_vram_headroom_bytes: u64,
    pub(crate) ram_used_bytes: u64,
    pub(crate) vram_used_bytes: u64,
    pub(crate) ram_available_bytes: u64,
    pub(crate) vram_available_bytes: u64,
    pub(crate) pending_queue_depth: usize,
    pub(crate) loader_busy: bool,
    pub(crate) loader_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeLoadQueueEntry {
    pub(crate) queue_id: i64,
    pub(crate) logical_model_id: String,
    pub(crate) display_path: String,
    pub(crate) backend_class: String,
    pub(crate) state: String,
    pub(crate) reservation_ram_bytes: u64,
    pub(crate) reservation_vram_bytes: u64,
    pub(crate) reason: String,
    pub(crate) requested_at_ms: i64,
    pub(crate) updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct AdmissionPlan {
    pub(crate) reservation: RuntimeReservation,
    pub(crate) evict_runtime_ids: Vec<String>,
    pub(crate) requires_loader_lock: bool,
}

#[derive(Debug, Error)]
pub(crate) enum ResourceGovernorError {
    #[error("{0}")]
    Storage(#[from] StorageError),

    #[error("{0}")]
    Busy(String),

    #[error("{0}")]
    Refused(String),
}

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
            if runtime_registry.is_runtime_loaded(&runtime_id) {
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
                if candidate.runtime_id == runtime_id {
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

fn estimate_reservation(
    target: &ResolvedModelTarget,
    config: &ResourceGovernorConfig,
) -> RuntimeReservation {
    if target.driver_resolution().backend_class != BackendClass::ResidentLocal {
        return RuntimeReservation::default();
    }

    let base_bytes = estimate_local_model_bytes(target);
    RuntimeReservation {
        ram_bytes: scaled_reservation_bytes(
            base_bytes,
            config.local_runtime_ram_scale,
            config.local_runtime_ram_overhead_bytes,
        ),
        vram_bytes: scaled_reservation_bytes(
            base_bytes,
            config.local_runtime_vram_scale,
            config.local_runtime_vram_overhead_bytes,
        ),
    }
}

fn estimate_local_model_bytes(target: &ResolvedModelTarget) -> u64 {
    std::fs::metadata(target.display_path())
        .map(|metadata| metadata.len())
        .unwrap_or_else(|_| fallback_model_bytes(target.display_path(), &target.logical_model_id()))
}

fn fallback_model_bytes(path: &Path, logical_model_id: &str) -> u64 {
    let label = format!("{} {}", logical_model_id, path.display()).to_ascii_lowercase();
    let mut digits = String::new();
    for ch in label.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }

    let gib = digits.parse::<u64>().unwrap_or(4).max(1);
    gib.saturating_mul(1024_u64.pow(3))
}

fn scaled_reservation_bytes(base_bytes: u64, scale: f64, overhead_bytes: u64) -> u64 {
    ((base_bytes as f64) * scale).ceil() as u64 + overhead_bytes
}

fn fits_with_headroom(
    config: &ResourceGovernorConfig,
    usage: RuntimeReservation,
    reservation: RuntimeReservation,
) -> bool {
    fits_with_headroom_after_free(config, usage, RuntimeReservation::default(), reservation)
}

fn fits_with_headroom_after_free(
    config: &ResourceGovernorConfig,
    usage: RuntimeReservation,
    freed: RuntimeReservation,
    reservation: RuntimeReservation,
) -> bool {
    let future_ram = usage
        .ram_bytes
        .saturating_sub(freed.ram_bytes)
        .saturating_add(reservation.ram_bytes);
    let future_vram = usage
        .vram_bytes
        .saturating_sub(freed.vram_bytes)
        .saturating_add(reservation.vram_bytes);
    future_ram <= effective_budget(config.ram_budget_bytes, config.min_ram_headroom_bytes)
        && future_vram <= effective_budget(config.vram_budget_bytes, config.min_vram_headroom_bytes)
}

fn effective_budget(budget: u64, headroom: u64) -> u64 {
    if budget == 0 {
        u64::MAX
    } else {
        budget.saturating_sub(headroom)
    }
}

fn budget_available(budget: u64, headroom: u64, used: u64) -> u64 {
    effective_budget(budget, headroom).saturating_sub(used)
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    if bytes == u64::MAX {
        "unbounded".to_string()
    } else {
        format!("{:.2}GiB", bytes as f64 / GIB)
    }
}

#[cfg(test)]
mod tests {
    use super::{ResourceGovernor, ResourceGovernorConfig, ResourceGovernorError};
    use crate::backend::{resolve_driver_for_model, TestExternalEndpointOverrideGuard};
    use crate::model_catalog::ResolvedModelTarget;
    use crate::prompting::PromptFamily;
    use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
    use crate::session::SessionRegistry;
    use crate::storage::StorageService;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::Tokenizer;

    #[test]
    fn admission_fits_immediately_when_budget_allows_it() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
        let dir = make_temp_dir("agenticos-resource-governor-fit");
        let db_path = dir.join("agenticos.db");
        let model_path = write_model_file(&dir, "fit.gguf", 2 * 1024 * 1024);
        let tokenizer_path = write_test_tokenizer(&dir);
        let target = local_target(&model_path, &tokenizer_path);
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let runtime_registry = RuntimeRegistry::load(&mut storage).expect("load registry");
        let session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut governor = ResourceGovernor::load(
            &mut storage,
            ResourceGovernorConfig {
                ram_budget_bytes: 4 * 1024 * 1024 * 1024,
                vram_budget_bytes: 4 * 1024 * 1024 * 1024,
                min_ram_headroom_bytes: 256 * 1024 * 1024,
                min_vram_headroom_bytes: 256 * 1024 * 1024,
                ..ResourceGovernorConfig::default()
            },
        )
        .expect("load governor");

        let plan = governor
            .prepare_activation(&mut storage, &runtime_registry, &session_registry, &target)
            .expect("fit should be admitted");

        assert!(plan.requires_loader_lock);
        assert!(plan.evict_runtime_ids.is_empty());
        assert!(plan.reservation.ram_bytes > 0);
        assert!(plan.reservation.vram_bytes > 0);
    }

    #[test]
    fn admission_can_schedule_lru_eviction_for_idle_runtime() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
        let dir = make_temp_dir("agenticos-resource-governor-evict");
        let db_path = dir.join("agenticos.db");
        let tokenizer_path = write_test_tokenizer(&dir);
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load registry");
        let session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut governor = ResourceGovernor::load(
            &mut storage,
            ResourceGovernorConfig {
                ram_budget_bytes: 3 * 1024 * 1024 * 1024,
                vram_budget_bytes: 3 * 1024 * 1024 * 1024,
                min_ram_headroom_bytes: 256 * 1024 * 1024,
                min_vram_headroom_bytes: 256 * 1024 * 1024,
                local_runtime_ram_scale: 1.0,
                local_runtime_vram_scale: 1.0,
                local_runtime_ram_overhead_bytes: 0,
                local_runtime_vram_overhead_bytes: 0,
                ..ResourceGovernorConfig::default()
            },
        )
        .expect("load governor");

        let left_path = write_model_file(&dir, "left.gguf", 1024 * 1024 * 1024);
        let right_path = write_model_file(&dir, "right.gguf", 2 * 1024 * 1024 * 1024);
        let left_target = local_target(&left_path, &tokenizer_path);
        let right_target = local_target(&right_path, &tokenizer_path);

        let left_reservation = RuntimeReservation {
            ram_bytes: 1024 * 1024 * 1024,
            vram_bytes: 1024 * 1024 * 1024,
        };
        runtime_registry
            .activate_target(&mut storage, &left_target, left_reservation)
            .expect("activate left runtime");

        let plan = governor
            .prepare_activation(
                &mut storage,
                &runtime_registry,
                &session_registry,
                &right_target,
            )
            .expect("eviction plan should be admitted");
        assert_eq!(plan.evict_runtime_ids.len(), 1);
    }

    #[test]
    fn admission_queues_when_only_pinned_runtime_blocks_fit() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
        let dir = make_temp_dir("agenticos-resource-governor-queue");
        let db_path = dir.join("agenticos.db");
        let tokenizer_path = write_test_tokenizer(&dir);
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load registry");
        let session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut governor = ResourceGovernor::load(
            &mut storage,
            ResourceGovernorConfig {
                ram_budget_bytes: 3 * 1024 * 1024 * 1024,
                vram_budget_bytes: 3 * 1024 * 1024 * 1024,
                min_ram_headroom_bytes: 256 * 1024 * 1024,
                min_vram_headroom_bytes: 256 * 1024 * 1024,
                local_runtime_ram_scale: 1.0,
                local_runtime_vram_scale: 1.0,
                local_runtime_ram_overhead_bytes: 0,
                local_runtime_vram_overhead_bytes: 0,
                ..ResourceGovernorConfig::default()
            },
        )
        .expect("load governor");

        let left_path = write_model_file(&dir, "left-pinned.gguf", 1024 * 1024 * 1024);
        let right_path = write_model_file(&dir, "right-queued.gguf", 2 * 1024 * 1024 * 1024);
        let left_target = local_target(&left_path, &tokenizer_path);
        let right_target = local_target(&right_path, &tokenizer_path);

        let left = runtime_registry
            .activate_target(
                &mut storage,
                &left_target,
                RuntimeReservation {
                    ram_bytes: 1024 * 1024 * 1024,
                    vram_bytes: 1024 * 1024 * 1024,
                },
            )
            .expect("activate left runtime");
        runtime_registry
            .set_runtime_pinned(&mut storage, &left.runtime_id, true)
            .expect("pin runtime");

        let result = governor.prepare_activation(
            &mut storage,
            &runtime_registry,
            &session_registry,
            &right_target,
        );
        match result {
            Err(ResourceGovernorError::Busy(message)) => {
                assert!(message.contains("queued"));
            }
            other => panic!("expected queued busy result, got {other:?}"),
        }

        assert_eq!(governor.status(&runtime_registry).pending_queue_depth, 1);
    }

    #[test]
    fn admission_refuses_when_single_runtime_exceeds_budget() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
        let dir = make_temp_dir("agenticos-resource-governor-refuse");
        let db_path = dir.join("agenticos.db");
        let model_path = write_model_file(&dir, "too-big.gguf", 2 * 1024 * 1024 * 1024);
        let tokenizer_path = write_test_tokenizer(&dir);
        let target = local_target(&model_path, &tokenizer_path);
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let runtime_registry = RuntimeRegistry::load(&mut storage).expect("load registry");
        let session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut governor = ResourceGovernor::load(
            &mut storage,
            ResourceGovernorConfig {
                ram_budget_bytes: 1024 * 1024 * 1024,
                vram_budget_bytes: 1024 * 1024 * 1024,
                min_ram_headroom_bytes: 256 * 1024 * 1024,
                min_vram_headroom_bytes: 256 * 1024 * 1024,
                local_runtime_ram_scale: 1.0,
                local_runtime_vram_scale: 1.0,
                local_runtime_ram_overhead_bytes: 0,
                local_runtime_vram_overhead_bytes: 0,
                ..ResourceGovernorConfig::default()
            },
        )
        .expect("load governor");

        let result = governor.prepare_activation(
            &mut storage,
            &runtime_registry,
            &session_registry,
            &target,
        );
        match result {
            Err(ResourceGovernorError::Refused(message)) => {
                assert!(message.contains("refused"));
            }
            other => panic!("expected refusal, got {other:?}"),
        }
    }

    fn local_target(model_path: &PathBuf, tokenizer_path: &PathBuf) -> ResolvedModelTarget {
        let driver =
            resolve_driver_for_model(PromptFamily::Mistral, None, Some("external-llamacpp"))
                .expect("resolve driver");
        ResolvedModelTarget::local(
            Some(
                model_path
                    .file_stem()
                    .expect("file stem")
                    .to_string_lossy()
                    .to_string(),
            ),
            model_path.clone(),
            PromptFamily::Mistral,
            Some(tokenizer_path.clone()),
            None,
            driver,
        )
    }

    fn write_model_file(dir: &Path, name: &str, size_bytes: u64) -> PathBuf {
        let path = dir.join(name);
        let file = std::fs::File::create(&path).expect("create model file");
        file.set_len(size_bytes).expect("size model file");
        path
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time monotonic")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_test_tokenizer(dir: &Path) -> PathBuf {
        let path = dir.join("tokenizer.json");
        let vocab = [
            ("<unk>".to_string(), 0),
            ("hello".to_string(), 1),
            ("</s>".to_string(), 2),
        ]
        .into_iter()
        .collect();
        let model = WordLevel::builder()
            .vocab(vocab)
            .unk_token("<unk>".to_string())
            .build()
            .expect("build tokenizer");
        Tokenizer::new(model)
            .save(&path, false)
            .expect("save tokenizer");
        path
    }
}
