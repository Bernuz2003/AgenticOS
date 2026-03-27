use std::collections::{BTreeMap, HashMap};

use rusqlite::params;
use thiserror::Error;

use super::selection::{runtime_id_from_key, runtime_key_for_target, runtime_target_changed};
use crate::backend::BackendClass;
use crate::diagnostics::audit::{self, AuditContext};
use crate::engine::LLMEngine;
use crate::model_catalog::ResolvedModelTarget;
use crate::prompting::PromptFamily;
use crate::storage::{StorageError, StorageService};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeLifecycleState {
    Registered,
    Loading,
    Loaded,
    Active,
    Evicting,
}

impl RuntimeLifecycleState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Loading => "loading",
            Self::Loaded => "loaded",
            Self::Active => "active",
            Self::Evicting => "evicting",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RuntimeReservation {
    pub(crate) ram_bytes: u64,
    pub(crate) vram_bytes: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeDescriptor {
    pub(crate) runtime_id: String,
    pub(crate) runtime_key: String,
    pub(crate) target_kind: String,
    pub(crate) logical_model_id: String,
    pub(crate) display_path: String,
    pub(crate) runtime_reference: String,
    pub(crate) family: PromptFamily,
    pub(crate) backend_id: String,
    pub(crate) backend_class: BackendClass,
    pub(crate) driver_source: String,
    pub(crate) driver_rationale: String,
    pub(crate) provider_id: Option<String>,
    pub(crate) remote_model_id: Option<String>,
    pub(crate) load_mode: String,
    pub(crate) reservation_ram_bytes: u64,
    pub(crate) reservation_vram_bytes: u64,
    pub(crate) pinned: bool,
    pub(crate) transition_state: Option<String>,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) last_used_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredRuntimeRecord {
    pub(crate) runtime_id: String,
    pub(crate) runtime_key: String,
    pub(crate) state: String,
    pub(crate) target_kind: String,
    pub(crate) logical_model_id: String,
    pub(crate) display_path: String,
    pub(crate) runtime_reference: String,
    pub(crate) family: String,
    pub(crate) backend_id: String,
    pub(crate) backend_class: String,
    pub(crate) driver_source: String,
    pub(crate) driver_rationale: String,
    pub(crate) provider_id: Option<String>,
    pub(crate) remote_model_id: Option<String>,
    pub(crate) load_mode: String,
    pub(crate) reservation_ram_bytes: u64,
    pub(crate) reservation_vram_bytes: u64,
    pub(crate) pinned: bool,
    pub(crate) transition_state: Option<String>,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) last_used_at_ms: i64,
}

pub(crate) struct RuntimeHandle {
    pub(crate) descriptor: RuntimeDescriptor,
    pub(crate) state: RuntimeLifecycleState,
    pub(crate) engine: Option<LLMEngine>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeActivation {
    pub(crate) runtime_id: String,
}

#[derive(Debug, Error)]
pub(crate) enum RuntimeRegistryError {
    #[error("{0}")]
    Storage(#[from] StorageError),

    #[error("{0}")]
    LoadFailed(String),

    #[error("runtime '{0}' is already transitioning")]
    TransitionBusy(String),
}

pub(crate) struct RuntimeRegistry {
    pub(super) current_runtime_id: Option<String>,
    pub(super) runtimes: BTreeMap<String, RuntimeHandle>,
    pub(super) runtime_key_to_id: HashMap<String, String>,
    pub(super) pid_to_runtime: HashMap<u64, String>,
    pub(super) next_global_pid: u64,
}

impl RuntimeRegistry {
    pub(crate) fn load(storage: &mut StorageService) -> Result<Self, RuntimeRegistryError> {
        storage.reset_runtime_instances_for_boot()?;
        let stored_runtimes = storage.load_runtime_instances()?;
        let mut runtimes = BTreeMap::new();
        let mut runtime_key_to_id = HashMap::new();

        for runtime in stored_runtimes {
            runtime_key_to_id.insert(runtime.runtime_key.clone(), runtime.runtime_id.clone());
            runtimes.insert(
                runtime.runtime_id.clone(),
                RuntimeHandle {
                    descriptor: descriptor_from_record(&runtime),
                    state: RuntimeLifecycleState::Registered,
                    engine: None,
                },
            );
        }

        Ok(Self {
            current_runtime_id: None,
            runtimes,
            runtime_key_to_id,
            pid_to_runtime: HashMap::new(),
            next_global_pid: 1,
        })
    }

    pub(crate) fn current_runtime_id(&self) -> Option<&str> {
        self.current_runtime_id.as_deref()
    }

    pub(crate) fn current_engine(&self) -> Option<&LLMEngine> {
        self.current_runtime_id
            .as_ref()
            .and_then(|runtime_id| self.runtimes.get(runtime_id))
            .and_then(|handle| handle.engine.as_ref())
    }

    pub(crate) fn current_engine_mut(&mut self) -> Option<&mut LLMEngine> {
        let runtime_id = self.current_runtime_id.clone()?;
        self.engine_mut(&runtime_id)
    }

    pub(crate) fn engine(&self, runtime_id: &str) -> Option<&LLMEngine> {
        self.runtimes
            .get(runtime_id)
            .and_then(|handle| handle.engine.as_ref())
    }

    pub(crate) fn engine_mut(&mut self, runtime_id: &str) -> Option<&mut LLMEngine> {
        self.runtimes
            .get_mut(runtime_id)
            .and_then(|handle| handle.engine.as_mut())
    }

    pub(crate) fn descriptor(&self, runtime_id: &str) -> Option<&RuntimeDescriptor> {
        self.runtimes
            .get(runtime_id)
            .map(|handle| &handle.descriptor)
    }

    pub(crate) fn runtime_id_for_target(&self, target: &ResolvedModelTarget) -> Option<String> {
        let runtime_key = runtime_key_for_target(target);
        self.runtime_key_to_id.get(&runtime_key).cloned()
    }

    pub(crate) fn is_runtime_loaded(&self, runtime_id: &str) -> bool {
        self.runtimes
            .get(runtime_id)
            .is_some_and(|handle| handle.engine.is_some())
    }

    pub(crate) fn runtime_id_for_pid(&self, pid: u64) -> Option<&str> {
        self.pid_to_runtime.get(&pid).map(String::as_str)
    }

    pub(crate) fn engine_for_pid_mut(&mut self, pid: u64) -> Option<&mut LLMEngine> {
        let runtime_id = self.pid_to_runtime.get(&pid)?.clone();
        self.engine_mut(&runtime_id)
    }

    pub(crate) fn live_process_count(&self) -> usize {
        self.pid_to_runtime.len()
    }

    pub(crate) fn any_loaded_runtime_id(&self) -> Option<String> {
        self.runtimes
            .values()
            .find(|handle| handle.engine.is_some())
            .map(|handle| handle.descriptor.runtime_id.clone())
    }

    pub(crate) fn loaded_runtime_ids(&self) -> Vec<String> {
        self.runtimes
            .values()
            .filter(|handle| handle.engine.is_some())
            .map(|handle| handle.descriptor.runtime_id.clone())
            .collect()
    }

    pub(crate) fn loaded_runtime_id_for_backend_class(
        &self,
        backend_class: BackendClass,
    ) -> Option<String> {
        self.runtimes
            .values()
            .find(|handle| {
                handle.engine.is_some() && handle.descriptor.backend_class == backend_class
            })
            .map(|handle| handle.descriptor.runtime_id.clone())
    }

    pub(crate) fn runtime_count(&self) -> usize {
        self.runtimes.len()
    }

    pub(crate) fn next_pid_floor(&self) -> u64 {
        self.next_global_pid
    }

    pub(crate) fn all_active_pids(&self) -> Vec<u64> {
        self.pid_to_runtime.keys().copied().collect()
    }

    pub(crate) fn finishable_pids(&self) -> Vec<u64> {
        let mut finished = Vec::new();
        for handle in self.runtimes.values() {
            if let Some(engine) = handle.engine.as_ref() {
                finished.extend(engine.list_finished_pids());
            }
        }
        finished
    }

    pub(crate) fn activate_target(
        &mut self,
        storage: &mut StorageService,
        target: &ResolvedModelTarget,
        reservation: RuntimeReservation,
    ) -> Result<RuntimeActivation, RuntimeRegistryError> {
        let runtime_key = runtime_key_for_target(target);
        let runtime_id = self
            .runtime_key_to_id
            .get(&runtime_key)
            .cloned()
            .unwrap_or_else(|| runtime_id_from_key(&runtime_key));
        let now = current_timestamp_ms();

        self.ensure_runtime_registered(storage, &runtime_id, &runtime_key, target, reservation)?;

        let already_loaded = self
            .runtimes
            .get(&runtime_id)
            .is_some_and(|handle| handle.engine.is_some());
        let target_changed = self
            .runtimes
            .get(&runtime_id)
            .is_some_and(|handle| runtime_target_changed(handle, target));

        if already_loaded && !target_changed {
            let handle = self
                .runtimes
                .get_mut(&runtime_id)
                .expect("loaded runtime handle should exist");
            handle.state = runtime_state_for_handle(handle);
            handle.descriptor.reservation_ram_bytes = reservation.ram_bytes;
            handle.descriptor.reservation_vram_bytes = reservation.vram_bytes;
            handle.descriptor.updated_at_ms = now;
            handle.descriptor.last_used_at_ms = now;
            persist_runtime_handle(storage, handle)?;
            audit::record(
                storage,
                audit::RUNTIME_REUSED,
                format!(
                    "runtime={} model={} backend={}",
                    runtime_id, handle.descriptor.logical_model_id, handle.descriptor.backend_id
                ),
                AuditContext::for_runtime(&runtime_id),
            );
            self.set_current_runtime(Some(runtime_id.clone()))?;
            return Ok(RuntimeActivation { runtime_id });
        }

        if already_loaded && target_changed {
            let has_active_pids = self
                .pid_to_runtime
                .values()
                .any(|bound_runtime_id| bound_runtime_id == &runtime_id);
            if has_active_pids {
                return Err(RuntimeRegistryError::LoadFailed(format!(
                    "Local family runtime '{}' is busy with active processes and cannot switch to '{}'.",
                    runtime_id,
                    target.display_path().display()
                )));
            }

            let _ = self.evict_runtime(storage, &runtime_id)?;
        }

        self.begin_transition(storage, &runtime_id, RuntimeLifecycleState::Loading)?;
        audit::record(
            storage,
            audit::RUNTIME_LOAD_STARTED,
            format!(
                "runtime={} model={} backend={} path={}",
                runtime_id,
                target.logical_model_id(),
                target.driver_resolution().resolved_backend_id,
                target.display_path().display()
            ),
            AuditContext::for_runtime(&runtime_id),
        );
        let engine = LLMEngine::load_target(target)
            .map_err(|err| RuntimeRegistryError::LoadFailed(err.to_string()));
        match engine {
            Ok(engine) => {
                let descriptor = descriptor_from_target(
                    &runtime_id,
                    &runtime_key,
                    target,
                    &engine,
                    now,
                    reservation,
                );
                let handle = RuntimeHandle {
                    descriptor,
                    state: RuntimeLifecycleState::Loaded,
                    engine: Some(engine),
                };
                self.runtime_key_to_id
                    .insert(runtime_key, runtime_id.clone());
                self.runtimes.insert(runtime_id.clone(), handle);
                self.end_transition(storage, &runtime_id)?;
                let descriptor = self
                    .descriptor(&runtime_id)
                    .expect("activated runtime descriptor should exist");
                audit::record(
                    storage,
                    audit::RUNTIME_LOAD_READY,
                    format!(
                        "runtime={} model={} backend={} load_mode={}",
                        runtime_id,
                        descriptor.logical_model_id,
                        descriptor.backend_id,
                        descriptor.load_mode
                    ),
                    AuditContext::for_runtime(&runtime_id),
                );
                self.set_current_runtime(Some(runtime_id.clone()))?;
                Ok(RuntimeActivation { runtime_id })
            }
            Err(err) => {
                self.fail_transition(storage, &runtime_id)?;
                audit::record(
                    storage,
                    audit::RUNTIME_LOAD_FAILED,
                    format!(
                        "runtime={} model={} error={}",
                        runtime_id,
                        target.logical_model_id(),
                        err
                    ),
                    AuditContext::for_runtime(&runtime_id),
                );
                Err(err)
            }
        }
    }

    pub(crate) fn register_pid(
        &mut self,
        storage: &mut StorageService,
        runtime_id: &str,
        pid: u64,
    ) -> Result<(), RuntimeRegistryError> {
        self.pid_to_runtime.insert(pid, runtime_id.to_string());
        self.next_global_pid = self.next_global_pid.max(pid.saturating_add(1));
        self.touch_runtime_state(storage, runtime_id)
    }

    pub(crate) fn release_pid(
        &mut self,
        storage: &mut StorageService,
        pid: u64,
    ) -> Result<Option<String>, RuntimeRegistryError> {
        let Some(runtime_id) = self.pid_to_runtime.remove(&pid) else {
            return Ok(None);
        };
        self.touch_runtime_state(storage, &runtime_id)?;
        Ok(Some(runtime_id))
    }

    pub(crate) fn clear_loaded_runtimes(
        &mut self,
        storage: &mut StorageService,
    ) -> Result<(), RuntimeRegistryError> {
        let runtime_ids: Vec<String> = self.runtimes.keys().cloned().collect();
        self.pid_to_runtime.clear();
        self.set_current_runtime(None)?;

        for runtime_id in runtime_ids {
            let Some(handle) = self.runtimes.get_mut(&runtime_id) else {
                continue;
            };
            handle.engine = None;
            handle.state = RuntimeLifecycleState::Registered;
            handle.descriptor.transition_state = None;
            let now = current_timestamp_ms();
            handle.descriptor.updated_at_ms = now;
            handle.descriptor.last_used_at_ms = now;
            persist_runtime_handle(storage, handle)?;
        }

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_runtime_pinned(
        &mut self,
        storage: &mut StorageService,
        runtime_id: &str,
        pinned: bool,
    ) -> Result<(), RuntimeRegistryError> {
        let Some(handle) = self.runtimes.get_mut(runtime_id) else {
            return Ok(());
        };
        handle.descriptor.pinned = pinned;
        handle.descriptor.updated_at_ms = current_timestamp_ms();
        persist_runtime_handle(storage, handle)?;
        Ok(())
    }

    pub(crate) fn evict_runtime(
        &mut self,
        storage: &mut StorageService,
        runtime_id: &str,
    ) -> Result<bool, RuntimeRegistryError> {
        let has_active_pids = self
            .pid_to_runtime
            .values()
            .any(|bound_runtime_id| bound_runtime_id == runtime_id);
        if has_active_pids {
            return Ok(false);
        }

        let is_loaded = self
            .runtimes
            .get(runtime_id)
            .is_some_and(|handle| handle.engine.is_some());
        if !is_loaded {
            return Ok(false);
        }

        let descriptor = self.descriptor(runtime_id).cloned();
        if let Some(descriptor) = descriptor.as_ref() {
            audit::record(
                storage,
                audit::RUNTIME_EVICT_STARTED,
                format!(
                    "runtime={} model={} backend={}",
                    runtime_id, descriptor.logical_model_id, descriptor.backend_id
                ),
                AuditContext::for_runtime(runtime_id),
            );
        }
        self.begin_transition(storage, runtime_id, RuntimeLifecycleState::Evicting)?;
        if let Some(handle) = self.runtimes.get_mut(runtime_id) {
            handle.engine = None;
        }
        self.end_transition(storage, runtime_id)?;
        if let Some(descriptor) = descriptor {
            audit::record(
                storage,
                audit::RUNTIME_EVICT_COMPLETE,
                format!(
                    "runtime={} model={} backend={}",
                    runtime_id, descriptor.logical_model_id, descriptor.backend_id
                ),
                AuditContext::for_runtime(runtime_id),
            );
        }
        if self.current_runtime_id.as_deref() == Some(runtime_id) {
            self.set_current_runtime(None)?;
        }
        Ok(true)
    }

    fn touch_runtime_state(
        &mut self,
        storage: &mut StorageService,
        runtime_id: &str,
    ) -> Result<(), RuntimeRegistryError> {
        let is_active = self
            .pid_to_runtime
            .values()
            .any(|bound_runtime_id| bound_runtime_id == runtime_id);
        let Some(handle) = self.runtimes.get_mut(runtime_id) else {
            return Ok(());
        };
        let now = current_timestamp_ms();
        handle.state = if handle.descriptor.transition_state.is_some() {
            handle.state
        } else if handle.engine.is_none() {
            RuntimeLifecycleState::Registered
        } else if is_active {
            RuntimeLifecycleState::Active
        } else {
            RuntimeLifecycleState::Loaded
        };
        handle.descriptor.updated_at_ms = now;
        handle.descriptor.last_used_at_ms = now;
        persist_runtime_handle(storage, handle)?;
        Ok(())
    }

    fn ensure_runtime_registered(
        &mut self,
        storage: &mut StorageService,
        runtime_id: &str,
        runtime_key: &str,
        target: &ResolvedModelTarget,
        reservation: RuntimeReservation,
    ) -> Result<(), RuntimeRegistryError> {
        if self.runtimes.contains_key(runtime_id) {
            if let Some(handle) = self.runtimes.get_mut(runtime_id) {
                handle.descriptor.reservation_ram_bytes = reservation.ram_bytes;
                handle.descriptor.reservation_vram_bytes = reservation.vram_bytes;
                handle.descriptor.updated_at_ms = current_timestamp_ms();
                persist_runtime_handle(storage, handle)?;
            }
            return Ok(());
        }

        let now = current_timestamp_ms();
        let handle = RuntimeHandle {
            descriptor: provisional_descriptor_from_target(
                runtime_id,
                runtime_key,
                target,
                now,
                reservation,
            ),
            state: RuntimeLifecycleState::Registered,
            engine: None,
        };
        persist_runtime_handle(storage, &handle)?;
        self.runtime_key_to_id
            .insert(runtime_key.to_string(), runtime_id.to_string());
        self.runtimes.insert(runtime_id.to_string(), handle);
        Ok(())
    }

    fn begin_transition(
        &mut self,
        storage: &mut StorageService,
        runtime_id: &str,
        state: RuntimeLifecycleState,
    ) -> Result<(), RuntimeRegistryError> {
        let Some(handle) = self.runtimes.get_mut(runtime_id) else {
            return Ok(());
        };
        if handle.descriptor.transition_state.is_some() {
            return Err(RuntimeRegistryError::TransitionBusy(runtime_id.to_string()));
        }
        handle.state = state;
        handle.descriptor.transition_state = Some(state.as_str().to_string());
        handle.descriptor.updated_at_ms = current_timestamp_ms();
        persist_runtime_handle(storage, handle)?;
        Ok(())
    }

    fn fail_transition(
        &mut self,
        storage: &mut StorageService,
        runtime_id: &str,
    ) -> Result<(), RuntimeRegistryError> {
        let Some(handle) = self.runtimes.get_mut(runtime_id) else {
            return Ok(());
        };
        handle.state = RuntimeLifecycleState::Registered;
        handle.descriptor.transition_state = None;
        handle.descriptor.updated_at_ms = current_timestamp_ms();
        persist_runtime_handle(storage, handle)?;
        Ok(())
    }

    fn end_transition(
        &mut self,
        storage: &mut StorageService,
        runtime_id: &str,
    ) -> Result<(), RuntimeRegistryError> {
        let is_active = self
            .pid_to_runtime
            .values()
            .any(|bound_runtime_id| bound_runtime_id == runtime_id);
        let Some(handle) = self.runtimes.get_mut(runtime_id) else {
            return Ok(());
        };
        handle.descriptor.transition_state = None;
        handle.state = if handle.engine.is_none() {
            RuntimeLifecycleState::Registered
        } else if is_active {
            RuntimeLifecycleState::Active
        } else {
            RuntimeLifecycleState::Loaded
        };
        let now = current_timestamp_ms();
        handle.descriptor.updated_at_ms = now;
        handle.descriptor.last_used_at_ms = now;
        persist_runtime_handle(storage, handle)?;
        Ok(())
    }

    fn set_current_runtime(
        &mut self,
        runtime_id: Option<String>,
    ) -> Result<(), RuntimeRegistryError> {
        self.current_runtime_id = runtime_id.clone();
        Ok(())
    }
}

impl StorageService {
    pub(crate) fn reset_runtime_instances_for_boot(&mut self) -> Result<usize, StorageError> {
        Ok(self.connection.execute(
            r#"
            UPDATE runtime_instances
            SET
                state = 'registered',
                transition_state = NULL,
                updated_at_ms = ?1
            WHERE state != 'registered'
            "#,
            params![current_timestamp_ms()],
        )?)
    }

    pub(crate) fn load_runtime_instances(&self) -> Result<Vec<StoredRuntimeRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                runtime_id,
                runtime_key,
                state,
                target_kind,
                logical_model_id,
                display_path,
                runtime_reference,
                family,
                backend_id,
                backend_class,
                driver_source,
                driver_rationale,
                provider_id,
                remote_model_id,
                load_mode,
                reservation_ram_bytes,
                reservation_vram_bytes,
                pinned,
                transition_state,
                created_at_ms,
                updated_at_ms,
                last_used_at_ms
            FROM runtime_instances
            ORDER BY created_at_ms ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredRuntimeRecord {
                runtime_id: row.get(0)?,
                runtime_key: row.get(1)?,
                state: row.get(2)?,
                target_kind: row.get(3)?,
                logical_model_id: row.get(4)?,
                display_path: row.get(5)?,
                runtime_reference: row.get(6)?,
                family: row.get(7)?,
                backend_id: row.get(8)?,
                backend_class: row.get(9)?,
                driver_source: row.get(10)?,
                driver_rationale: row.get(11)?,
                provider_id: row.get(12)?,
                remote_model_id: row.get(13)?,
                load_mode: row.get(14)?,
                reservation_ram_bytes: row.get(15)?,
                reservation_vram_bytes: row.get(16)?,
                pinned: row.get::<_, i64>(17)? != 0,
                transition_state: row.get(18)?,
                created_at_ms: row.get(19)?,
                updated_at_ms: row.get(20)?,
                last_used_at_ms: row.get(21)?,
            })
        })?;

        let mut runtimes = Vec::new();
        for row in rows {
            runtimes.push(row?);
        }

        Ok(runtimes)
    }

    pub(crate) fn upsert_runtime_instance(
        &mut self,
        record: &StoredRuntimeRecord,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO runtime_instances (
                runtime_id,
                runtime_key,
                state,
                target_kind,
                logical_model_id,
                display_path,
                runtime_reference,
                family,
                backend_id,
                backend_class,
                driver_source,
                driver_rationale,
                provider_id,
                remote_model_id,
                load_mode,
                reservation_ram_bytes,
                reservation_vram_bytes,
                pinned,
                transition_state,
                created_at_ms,
                updated_at_ms,
                last_used_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
            ON CONFLICT(runtime_id) DO UPDATE SET
                runtime_key = excluded.runtime_key,
                state = excluded.state,
                target_kind = excluded.target_kind,
                logical_model_id = excluded.logical_model_id,
                display_path = excluded.display_path,
                runtime_reference = excluded.runtime_reference,
                family = excluded.family,
                backend_id = excluded.backend_id,
                backend_class = excluded.backend_class,
                driver_source = excluded.driver_source,
                driver_rationale = excluded.driver_rationale,
                provider_id = excluded.provider_id,
                remote_model_id = excluded.remote_model_id,
                load_mode = excluded.load_mode,
                reservation_ram_bytes = excluded.reservation_ram_bytes,
                reservation_vram_bytes = excluded.reservation_vram_bytes,
                pinned = excluded.pinned,
                transition_state = excluded.transition_state,
                updated_at_ms = excluded.updated_at_ms,
                last_used_at_ms = excluded.last_used_at_ms
            "#,
            params![
                record.runtime_id,
                record.runtime_key,
                record.state,
                record.target_kind,
                record.logical_model_id,
                record.display_path,
                record.runtime_reference,
                record.family,
                record.backend_id,
                record.backend_class,
                record.driver_source,
                record.driver_rationale,
                record.provider_id,
                record.remote_model_id,
                record.load_mode,
                record.reservation_ram_bytes,
                record.reservation_vram_bytes,
                if record.pinned { 1 } else { 0 },
                record.transition_state,
                record.created_at_ms,
                record.updated_at_ms,
                record.last_used_at_ms,
            ],
        )?;

        Ok(())
    }
}

fn persist_runtime_handle(
    storage: &mut StorageService,
    handle: &RuntimeHandle,
) -> Result<(), RuntimeRegistryError> {
    storage.upsert_runtime_instance(&StoredRuntimeRecord {
        runtime_id: handle.descriptor.runtime_id.clone(),
        runtime_key: handle.descriptor.runtime_key.clone(),
        state: handle.state.as_str().to_string(),
        target_kind: handle.descriptor.target_kind.clone(),
        logical_model_id: handle.descriptor.logical_model_id.clone(),
        display_path: handle.descriptor.display_path.clone(),
        runtime_reference: handle.descriptor.runtime_reference.clone(),
        family: format!("{:?}", handle.descriptor.family),
        backend_id: handle.descriptor.backend_id.clone(),
        backend_class: handle.descriptor.backend_class.as_str().to_string(),
        driver_source: handle.descriptor.driver_source.clone(),
        driver_rationale: handle.descriptor.driver_rationale.clone(),
        provider_id: handle.descriptor.provider_id.clone(),
        remote_model_id: handle.descriptor.remote_model_id.clone(),
        load_mode: handle.descriptor.load_mode.clone(),
        reservation_ram_bytes: handle.descriptor.reservation_ram_bytes,
        reservation_vram_bytes: handle.descriptor.reservation_vram_bytes,
        pinned: handle.descriptor.pinned,
        transition_state: handle.descriptor.transition_state.clone(),
        created_at_ms: handle.descriptor.created_at_ms,
        updated_at_ms: handle.descriptor.updated_at_ms,
        last_used_at_ms: handle.descriptor.last_used_at_ms,
    })?;
    Ok(())
}

fn descriptor_from_record(record: &StoredRuntimeRecord) -> RuntimeDescriptor {
    RuntimeDescriptor {
        runtime_id: record.runtime_id.clone(),
        runtime_key: record.runtime_key.clone(),
        target_kind: record.target_kind.clone(),
        logical_model_id: record.logical_model_id.clone(),
        display_path: record.display_path.clone(),
        runtime_reference: record.runtime_reference.clone(),
        family: parse_prompt_family(&record.family),
        backend_id: record.backend_id.clone(),
        backend_class: parse_backend_class(&record.backend_class),
        driver_source: record.driver_source.clone(),
        driver_rationale: record.driver_rationale.clone(),
        provider_id: record.provider_id.clone(),
        remote_model_id: record.remote_model_id.clone(),
        load_mode: record.load_mode.clone(),
        reservation_ram_bytes: record.reservation_ram_bytes,
        reservation_vram_bytes: record.reservation_vram_bytes,
        pinned: record.pinned,
        transition_state: record.transition_state.clone(),
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.updated_at_ms,
        last_used_at_ms: record.last_used_at_ms,
    }
}

fn descriptor_from_target(
    runtime_id: &str,
    runtime_key: &str,
    target: &ResolvedModelTarget,
    engine: &LLMEngine,
    now: i64,
    reservation: RuntimeReservation,
) -> RuntimeDescriptor {
    RuntimeDescriptor {
        runtime_id: runtime_id.to_string(),
        runtime_key: runtime_key.to_string(),
        target_kind: target.target_kind().to_string(),
        logical_model_id: target.logical_model_id(),
        display_path: target.display_path().display().to_string(),
        runtime_reference: target.runtime_reference().to_string(),
        family: engine.loaded_family(),
        backend_id: engine.loaded_backend_id().to_string(),
        backend_class: engine.loaded_backend_class(),
        driver_source: engine.driver_resolution_source().to_string(),
        driver_rationale: engine.driver_resolution_rationale().to_string(),
        provider_id: target.provider_id().map(ToString::to_string),
        remote_model_id: target.remote_model_id().map(ToString::to_string),
        load_mode: match engine.loaded_backend_class() {
            BackendClass::ResidentLocal => "resident_local_adapter".to_string(),
            BackendClass::RemoteStateless => "remote_stateless".to_string(),
        },
        reservation_ram_bytes: reservation.ram_bytes,
        reservation_vram_bytes: reservation.vram_bytes,
        pinned: false,
        transition_state: None,
        created_at_ms: now,
        updated_at_ms: now,
        last_used_at_ms: now,
    }
}

fn provisional_descriptor_from_target(
    runtime_id: &str,
    runtime_key: &str,
    target: &ResolvedModelTarget,
    now: i64,
    reservation: RuntimeReservation,
) -> RuntimeDescriptor {
    RuntimeDescriptor {
        runtime_id: runtime_id.to_string(),
        runtime_key: runtime_key.to_string(),
        target_kind: target.target_kind().to_string(),
        logical_model_id: target.logical_model_id(),
        display_path: target.display_path().display().to_string(),
        runtime_reference: target.runtime_reference().to_string(),
        family: target.family(),
        backend_id: target.driver_resolution().resolved_backend_id.clone(),
        backend_class: target.driver_resolution().backend_class,
        driver_source: target.driver_resolution().resolution_source.to_string(),
        driver_rationale: target.driver_resolution().resolution_rationale.clone(),
        provider_id: target.provider_id().map(ToString::to_string),
        remote_model_id: target.remote_model_id().map(ToString::to_string),
        load_mode: match target.driver_resolution().backend_class {
            BackendClass::ResidentLocal => "resident_local_adapter".to_string(),
            BackendClass::RemoteStateless => "remote_stateless".to_string(),
        },
        reservation_ram_bytes: reservation.ram_bytes,
        reservation_vram_bytes: reservation.vram_bytes,
        pinned: false,
        transition_state: None,
        created_at_ms: now,
        updated_at_ms: now,
        last_used_at_ms: now,
    }
}

pub(super) fn runtime_state_for_handle(handle: &RuntimeHandle) -> RuntimeLifecycleState {
    if handle.engine.is_none() {
        RuntimeLifecycleState::Registered
    } else if handle
        .engine
        .as_ref()
        .is_some_and(|engine| !engine.processes.is_empty())
    {
        RuntimeLifecycleState::Active
    } else {
        RuntimeLifecycleState::Loaded
    }
}

fn parse_prompt_family(raw: &str) -> PromptFamily {
    match raw {
        "Llama" => PromptFamily::Llama,
        "Qwen" => PromptFamily::Qwen,
        "Mistral" => PromptFamily::Mistral,
        _ => PromptFamily::Unknown,
    }
}

fn parse_backend_class(raw: &str) -> BackendClass {
    match raw {
        "remote_stateless" => BackendClass::RemoteStateless,
        _ => BackendClass::ResidentLocal,
    }
}

fn current_timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
