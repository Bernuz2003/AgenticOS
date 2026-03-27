use std::hash::{Hash, Hasher};

use crate::backend::BackendClass;
use crate::model_catalog::ResolvedModelTarget;
use crate::prompting::PromptFamily;

use super::registry::RuntimeHandle;

pub(crate) fn runtime_key_for_target(target: &ResolvedModelTarget) -> String {
    if is_family_scoped_local_runtime(target) {
        return format!(
            "{}|{}|{}|{:?}",
            target.target_kind(),
            target.driver_resolution().resolved_backend_id,
            target.provider_id().unwrap_or("-"),
            target.family()
        );
    }

    format!(
        "{}|{}|{}|{}",
        target.target_kind(),
        target.driver_resolution().resolved_backend_id,
        target.provider_id().unwrap_or("-"),
        target.display_path().display()
    )
}

pub(super) fn runtime_target_changed(handle: &RuntimeHandle, target: &ResolvedModelTarget) -> bool {
    is_family_scoped_local_runtime(target)
        && handle.descriptor.runtime_reference != target.runtime_reference()
}

pub(super) fn runtime_id_from_key(runtime_key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    runtime_key.hash(&mut hasher);
    format!("rt-{:016x}", hasher.finish())
}

fn is_family_scoped_local_runtime(target: &ResolvedModelTarget) -> bool {
    matches!(target, ResolvedModelTarget::Local(_))
        && target.driver_resolution().backend_class == BackendClass::ResidentLocal
        && target.driver_resolution().resolved_backend_id == "external-llamacpp"
        && !matches!(target.family(), PromptFamily::Unknown)
}
