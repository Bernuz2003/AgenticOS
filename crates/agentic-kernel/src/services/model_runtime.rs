use crate::audit::{self, AuditContext};
use crate::backend::{BackendCapabilities, BackendClass};
use crate::model_catalog::{ModelCatalog, ResolvedModelTarget};
use crate::resource_governor::{ResourceGovernor, ResourceGovernorError};
use crate::runtimes::RuntimeRegistry;
use crate::session::SessionRegistry;
use crate::storage::StorageService;

pub struct LoadedModelSummary {
    pub runtime_id: String,
    pub family: crate::prompting::PromptFamily,
    pub loaded_model_id: String,
    pub loaded_target_kind: String,
    pub loaded_provider_id: Option<String>,
    pub loaded_remote_model_id: Option<String>,
    pub backend_id: String,
    pub backend_class: BackendClass,
    pub backend_capabilities: BackendCapabilities,
    pub driver_source: String,
    pub driver_rationale: String,
    pub path: std::path::PathBuf,
    pub architecture: Option<String>,
    pub load_mode: String,
    pub remote_model: Option<agentic_control_models::RemoteModelRuntimeView>,
}

#[derive(Debug)]
pub enum ModelActivationError {
    Busy(String),
    Failed(String),
}

impl ModelActivationError {
    pub fn message(&self) -> &str {
        match self {
            Self::Busy(message) | Self::Failed(message) => message,
        }
    }
}

pub fn activate_model_target(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
    model_catalog: &mut ModelCatalog,
    target: &ResolvedModelTarget,
) -> Result<LoadedModelSummary, ModelActivationError> {
    let admission =
        resource_governor.prepare_activation(storage, runtime_registry, session_registry, target);
    let admission = match admission {
        Ok(admission) => admission,
        Err(ResourceGovernorError::Busy(message)) => {
            return Err(ModelActivationError::Busy(message))
        }
        Err(ResourceGovernorError::Refused(message)) => {
            return Err(ModelActivationError::Failed(message));
        }
        Err(err) => return Err(ModelActivationError::Failed(err.to_string())),
    };
    audit::record(
        storage,
        audit::ADMISSION_RESERVED,
        format!(
            "model={} backend={} ram={} vram={} evictions={}",
            target.logical_model_id(),
            target.driver_resolution().resolved_backend_id,
            admission.reservation.ram_bytes,
            admission.reservation.vram_bytes,
            admission.evict_runtime_ids.len()
        ),
        AuditContext::default(),
    );
    for runtime_id in &admission.evict_runtime_ids {
        audit::record(
            storage,
            audit::ADMISSION_EVICTION_STARTED,
            format!(
                "target_model={} evict_runtime={}",
                target.logical_model_id(),
                runtime_id
            ),
            AuditContext::for_runtime(runtime_id),
        );
    }
    if admission.requires_loader_lock {
        resource_governor
            .try_acquire_loader_lock(&format!("activate {}", target.logical_model_id()))
            .map_err(|err| match err {
                ResourceGovernorError::Busy(message) => ModelActivationError::Busy(message),
                _ => ModelActivationError::Failed(err.to_string()),
            })?;
    }
    let activation = (|| {
        for runtime_id in &admission.evict_runtime_ids {
            runtime_registry
                .evict_runtime(storage, runtime_id)
                .map_err(|err| ModelActivationError::Failed(err.to_string()))?;
        }

        runtime_registry
            .activate_target(storage, target, admission.reservation)
            .map_err(|err| ModelActivationError::Failed(err.to_string()))
    })();
    if admission.requires_loader_lock {
        resource_governor.release_loader_lock();
    }
    let activation = activation?;
    resource_governor
        .mark_runtime_admitted(
            storage,
            &crate::runtimes::runtime_key_for_target(target),
            "runtime load admitted",
        )
        .map_err(|err| ModelActivationError::Failed(err.to_string()))?;
    let engine = runtime_registry
        .engine(&activation.runtime_id)
        .expect("activated runtime should expose engine");
    let summary = LoadedModelSummary {
        runtime_id: activation.runtime_id.clone(),
        family: engine.loaded_family(),
        loaded_model_id: target.logical_model_id(),
        loaded_target_kind: target.target_kind().to_string(),
        loaded_provider_id: target.provider_id().map(ToString::to_string),
        loaded_remote_model_id: target.remote_model_id().map(ToString::to_string),
        backend_id: engine.loaded_backend_id().to_string(),
        backend_class: engine.loaded_backend_class(),
        backend_capabilities: engine.loaded_backend_capabilities(),
        driver_source: engine.driver_resolution_source().to_string(),
        driver_rationale: engine.driver_resolution_rationale().to_string(),
        path: target.display_path().to_path_buf(),
        architecture: target.architecture(),
        load_mode: match engine.loaded_backend_class() {
            BackendClass::ResidentLocal => "resident_local_adapter".to_string(),
            BackendClass::RemoteStateless => "remote_stateless".to_string(),
        },
        remote_model: target.remote_model_view(),
    };

    if let Some(model_id) = target.local_model_id() {
        let _ = model_catalog.set_selected(model_id);
    }

    Ok(summary)
}
