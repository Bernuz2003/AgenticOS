use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Mutex, OnceLock};

use crate::backend::HttpEndpoint;
use crate::model_catalog::LocalLoadTarget;
use crate::prompting::PromptFamily;

use super::health::{probe_runtime, wait_until_runtime_ready};
use super::paths::{
    current_timestamp_ms, family_key, family_label, port_for_family, same_model_path,
    slot_save_dir_for_family,
};
use super::spawn::{
    legacy_endpoint_override, resolve_llama_server_executable, spawn_llama_server,
    stop_runtime_entry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagedLocalRuntimeState {
    Starting,
    Ready,
    Unhealthy,
    Restarting,
    Failed,
    ExternalOverride,
}

impl ManagedLocalRuntimeState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Unhealthy => "unhealthy",
            Self::Restarting => "restarting",
            Self::Failed => "failed",
            Self::ExternalOverride => "external_override",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedLocalRuntimeLease {
    pub(crate) endpoint: HttpEndpoint,
    pub(crate) family: PromptFamily,
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedLocalRuntimeView {
    pub(crate) family: String,
    pub(crate) logical_model_id: String,
    pub(crate) display_path: String,
    pub(crate) state: String,
    pub(crate) endpoint: String,
    pub(crate) port: u16,
    pub(crate) context_window_tokens: Option<usize>,
    pub(crate) slot_save_dir: String,
    pub(crate) managed_by_kernel: bool,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct RequestedLocalRuntime {
    pub(super) family: PromptFamily,
    pub(super) model_path: PathBuf,
    pub(super) logical_model_id: String,
    pub(super) context_window_tokens: Option<usize>,
}

#[cfg(test)]
type TestSpawnHook =
    std::sync::Arc<dyn Fn(TestSpawnRequest) -> Result<ManagedRuntimeProcess, String> + Send + Sync>;

#[cfg(test)]
#[derive(Debug, Clone)]
pub(super) struct TestSpawnRequest {
    pub(super) model_path: PathBuf,
    pub(super) port: u16,
    pub(super) context_window_tokens: usize,
    pub(super) slot_save_dir: PathBuf,
    pub(super) family: PromptFamily,
}

#[derive(Debug)]
pub(super) struct ManagedLocalRuntimeEntry {
    pub(super) family: PromptFamily,
    pub(super) logical_model_id: String,
    pub(super) model_path: PathBuf,
    pub(super) endpoint: String,
    pub(super) port: u16,
    pub(super) slot_save_dir: PathBuf,
    pub(super) state: ManagedLocalRuntimeState,
    pub(super) context_window_tokens: Option<usize>,
    pub(super) last_error: Option<String>,
    pub(super) managed_by_kernel: bool,
    pub(super) process: Option<ManagedRuntimeProcess>,
    pub(super) updated_at_ms: i64,
}

impl ManagedLocalRuntimeEntry {
    fn lease(&self) -> Result<ManagedLocalRuntimeLease, String> {
        Ok(ManagedLocalRuntimeLease {
            endpoint: HttpEndpoint::parse(&self.endpoint).map_err(|err| err.to_string())?,
            family: self.family,
        })
    }
}

#[derive(Debug)]
pub(super) enum ManagedRuntimeProcess {
    Child(Child),
    #[cfg(test)]
    Test(TestManagedRuntimeProcess),
}

#[cfg(test)]
#[derive(Debug)]
pub(super) struct TestManagedRuntimeProcess {
    stop_tx: Option<std::sync::mpsc::Sender<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ManagedRuntimeProcess {
    pub(super) fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>, String> {
        match self {
            Self::Child(child) => child.try_wait().map_err(|err| err.to_string()),
            #[cfg(test)]
            Self::Test(_) => Ok(None),
        }
    }

    pub(super) fn stop(&mut self) {
        match self {
            Self::Child(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            #[cfg(test)]
            Self::Test(process) => {
                if let Some(stop_tx) = process.stop_tx.take() {
                    let _ = stop_tx.send(());
                }
                if let Some(handle) = process.handle.take() {
                    let _ = handle.join();
                }
            }
        }
    }
}

pub(super) struct LocalRuntimeManager {
    pub(super) entries: BTreeMap<String, ManagedLocalRuntimeEntry>,
}

impl LocalRuntimeManager {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    fn ensure_for_request(
        &mut self,
        request: RequestedLocalRuntime,
    ) -> Result<ManagedLocalRuntimeLease, String> {
        if let Some(endpoint) = legacy_endpoint_override() {
            return self.ensure_legacy_override(request, &endpoint);
        }

        let executable = resolve_llama_server_executable().ok_or_else(|| {
            format!(
                "Local runtime backend is unavailable: '{}' is not on PATH and [external_llamacpp].legacy_endpoint_override is empty.",
                crate::config::kernel_config().external_llamacpp.executable
            )
        })?;

        let key = family_key(request.family);
        let desired_port = port_for_family(request.family);
        let endpoint = format!("http://127.0.0.1:{desired_port}");
        let slot_save_dir = slot_save_dir_for_family(request.family);
        fs::create_dir_all(&slot_save_dir).map_err(|err| {
            format!(
                "Failed to prepare slot-save directory '{}': {}",
                slot_save_dir.display(),
                err
            )
        })?;

        if let Some(entry) = self.entries.get_mut(&key) {
            let same_model = same_model_path(&entry.model_path, &request.model_path);
            if same_model {
                if probe_runtime(&entry.endpoint, &request.model_path).is_ok() {
                    entry.state = ManagedLocalRuntimeState::Ready;
                    entry.last_error = None;
                    entry.updated_at_ms = current_timestamp_ms();
                    return entry.lease();
                }

                entry.state = ManagedLocalRuntimeState::Unhealthy;
                stop_runtime_entry(entry);
                entry.state = ManagedLocalRuntimeState::Restarting;
                entry.last_error =
                    Some("Managed local runtime became unhealthy; restarting.".to_string());
            } else {
                stop_runtime_entry(entry);
                entry.logical_model_id = request.logical_model_id.clone();
                entry.model_path = request.model_path.clone();
                entry.context_window_tokens = request.context_window_tokens;
                entry.state = ManagedLocalRuntimeState::Restarting;
                entry.last_error = Some(format!(
                    "Switching family runtime '{}' to model '{}'.",
                    family_label(request.family),
                    request.model_path.display()
                ));
            }
        } else if probe_runtime(&endpoint, &request.model_path).is_ok() {
            let entry = ManagedLocalRuntimeEntry {
                family: request.family,
                logical_model_id: request.logical_model_id.clone(),
                model_path: request.model_path.clone(),
                endpoint: endpoint.clone(),
                port: desired_port,
                slot_save_dir: slot_save_dir.clone(),
                state: ManagedLocalRuntimeState::Ready,
                context_window_tokens: request.context_window_tokens,
                last_error: None,
                managed_by_kernel: false,
                process: None,
                updated_at_ms: current_timestamp_ms(),
            };
            self.entries.insert(key.clone(), entry);
            return self
                .entries
                .get(&key)
                .expect("adopted runtime should exist")
                .lease();
        }

        let context_window_tokens = request.context_window_tokens.ok_or_else(|| {
            format!(
                "Local model '{}' is missing metadata.max_context_tokens; runtime manager cannot start llama-server without a maximum context window.",
                request.model_path.display()
            )
        })?;

        let process = spawn_llama_server(
            &executable,
            &request.model_path,
            desired_port,
            context_window_tokens,
            &slot_save_dir,
            request.family,
        )?;

        let entry = self
            .entries
            .entry(key.clone())
            .or_insert(ManagedLocalRuntimeEntry {
                family: request.family,
                logical_model_id: request.logical_model_id.clone(),
                model_path: request.model_path.clone(),
                endpoint: endpoint.clone(),
                port: desired_port,
                slot_save_dir: slot_save_dir.clone(),
                state: ManagedLocalRuntimeState::Starting,
                context_window_tokens: Some(context_window_tokens),
                last_error: None,
                managed_by_kernel: true,
                process: None,
                updated_at_ms: current_timestamp_ms(),
            });
        entry.logical_model_id = request.logical_model_id;
        entry.model_path = request.model_path.clone();
        entry.endpoint = endpoint.clone();
        entry.port = desired_port;
        entry.slot_save_dir = slot_save_dir;
        entry.state = ManagedLocalRuntimeState::Starting;
        entry.context_window_tokens = Some(context_window_tokens);
        entry.last_error = None;
        entry.managed_by_kernel = true;
        entry.process = Some(process);
        entry.updated_at_ms = current_timestamp_ms();

        match wait_until_runtime_ready(entry, &request.model_path) {
            Ok(()) => entry.lease(),
            Err(err) => {
                entry.state = ManagedLocalRuntimeState::Failed;
                entry.last_error = Some(err.clone());
                entry.updated_at_ms = current_timestamp_ms();
                Err(err)
            }
        }
    }

    fn ensure_legacy_override(
        &mut self,
        request: RequestedLocalRuntime,
        endpoint: &str,
    ) -> Result<ManagedLocalRuntimeLease, String> {
        let key = family_key(request.family);
        let endpoint = endpoint.trim_end_matches('/').to_string();
        let port = HttpEndpoint::parse(&endpoint)
            .ok()
            .map(|parsed| parsed.port)
            .unwrap_or(0);
        self.entries.insert(
            key.clone(),
            ManagedLocalRuntimeEntry {
                family: request.family,
                logical_model_id: request.logical_model_id,
                model_path: request.model_path,
                endpoint,
                port,
                slot_save_dir: slot_save_dir_for_family(request.family),
                state: ManagedLocalRuntimeState::ExternalOverride,
                context_window_tokens: request.context_window_tokens,
                last_error: None,
                managed_by_kernel: false,
                process: None,
                updated_at_ms: current_timestamp_ms(),
            },
        );
        self.entries
            .get(&key)
            .expect("legacy override runtime should exist")
            .lease()
    }

    fn ensure_runtime_ready_for_family(&mut self, family: PromptFamily) -> Result<(), String> {
        #[cfg(test)]
        if let Some(false) = test_external_runtime_ready_override_get() {
            return Err(format!(
                "Managed local runtime for family '{}' is unavailable (test override).",
                family_label(family)
            ));
        }

        let Some(entry) = self.entries.get_mut(family_key(family).as_str()) else {
            return Err(format!(
                "No managed local runtime is active for family '{}'.",
                family_label(family)
            ));
        };

        if !entry.managed_by_kernel {
            entry.state = ManagedLocalRuntimeState::ExternalOverride;
            entry.last_error = None;
            entry.updated_at_ms = current_timestamp_ms();
            return Ok(());
        }

        probe_runtime(&entry.endpoint, &entry.model_path)?;
        entry.state = if entry.managed_by_kernel {
            ManagedLocalRuntimeState::Ready
        } else {
            ManagedLocalRuntimeState::ExternalOverride
        };
        entry.last_error = None;
        entry.updated_at_ms = current_timestamp_ms();
        Ok(())
    }

    fn shutdown_all(&mut self) {
        for entry in self.entries.values_mut() {
            stop_runtime_entry(entry);
        }
    }
}

pub(super) fn manager() -> &'static Mutex<LocalRuntimeManager> {
    static LOCAL_RUNTIME_MANAGER: OnceLock<Mutex<LocalRuntimeManager>> = OnceLock::new();
    LOCAL_RUNTIME_MANAGER.get_or_init(|| Mutex::new(LocalRuntimeManager::new()))
}

pub(crate) fn ensure_runtime_for_target(
    target: &LocalLoadTarget,
) -> Result<ManagedLocalRuntimeLease, String> {
    let request = RequestedLocalRuntime::from_target(target)?;
    manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .ensure_for_request(request)
}

pub(crate) fn ensure_runtime_for_reference(
    reference: &str,
    family: PromptFamily,
) -> Result<ManagedLocalRuntimeLease, String> {
    let key = family_key(family);
    let mut guard = manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let request = if let Some(entry) = guard.entries.get(&key) {
        RequestedLocalRuntime {
            family,
            model_path: entry.model_path.clone(),
            logical_model_id: entry.logical_model_id.clone(),
            context_window_tokens: entry.context_window_tokens,
        }
    } else {
        RequestedLocalRuntime::from_reference(reference, family)?
    };
    guard.ensure_for_request(request)
}

pub(crate) fn ensure_runtime_ready_for_family(family: PromptFamily) -> Result<(), String> {
    manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .ensure_runtime_ready_for_family(family)
}

pub(crate) fn shutdown_all() {
    manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .shutdown_all();
}

#[cfg(test)]
pub(super) fn test_external_endpoint_override_get() -> Option<String> {
    let cell = test_external_endpoint_override_cell();
    cell.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[cfg(test)]
fn test_external_endpoint_override_set(value: Option<String>) {
    let cell = test_external_endpoint_override_cell();
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

#[cfg(test)]
fn test_external_endpoint_override_cell() -> &'static Mutex<Option<String>> {
    static TEST_EXTERNAL_ENDPOINT_OVERRIDE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    TEST_EXTERNAL_ENDPOINT_OVERRIDE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(super) fn test_spawn_hook_get() -> Option<TestSpawnHook> {
    let cell = test_spawn_hook_cell();
    cell.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[cfg(test)]
fn test_spawn_hook_set(value: Option<TestSpawnHook>) {
    let cell = test_spawn_hook_cell();
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

#[cfg(test)]
fn test_spawn_hook_cell() -> &'static Mutex<Option<TestSpawnHook>> {
    static TEST_SPAWN_HOOK: OnceLock<Mutex<Option<TestSpawnHook>>> = OnceLock::new();
    TEST_SPAWN_HOOK.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(super) fn test_port_base_override_get() -> Option<u16> {
    let cell = test_port_base_override_cell();
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
fn test_port_base_override_set(value: Option<u16>) {
    let cell = test_port_base_override_cell();
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

#[cfg(test)]
fn test_port_base_override_cell() -> &'static Mutex<Option<u16>> {
    static TEST_PORT_BASE_OVERRIDE: OnceLock<Mutex<Option<u16>>> = OnceLock::new();
    TEST_PORT_BASE_OVERRIDE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn test_external_endpoint_override_lock() -> &'static Mutex<()> {
    static TEST_EXTERNAL_ENDPOINT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TEST_EXTERNAL_ENDPOINT_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub(crate) struct TestExternalEndpointOverrideGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<String>,
}

#[cfg(test)]
impl TestExternalEndpointOverrideGuard {
    pub(crate) fn set(value: &str) -> Self {
        Self::set_option(Some(value.to_string()))
    }

    pub(crate) fn clear() -> Self {
        Self::set_option(None)
    }

    fn set_option(value: Option<String>) -> Self {
        let lock = test_external_endpoint_override_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = test_external_endpoint_override_get();
        test_external_endpoint_override_set(value);
        reset_for_tests();
        Self {
            _lock: lock,
            previous,
        }
    }
}

#[cfg(test)]
impl Drop for TestExternalEndpointOverrideGuard {
    fn drop(&mut self) {
        test_external_endpoint_override_set(self.previous.clone());
        reset_for_tests();
    }
}

#[cfg(test)]
struct TestManagedRuntimeProvisionGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous_spawn_hook: Option<TestSpawnHook>,
    previous_port_base: Option<u16>,
}

#[cfg(test)]
impl TestManagedRuntimeProvisionGuard {
    fn set(port_base: u16, spawn_hook: TestSpawnHook) -> Self {
        let lock = test_external_endpoint_override_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_spawn_hook = test_spawn_hook_get();
        let previous_port_base = test_port_base_override_get();
        test_external_endpoint_override_set(None);
        test_spawn_hook_set(Some(spawn_hook));
        test_port_base_override_set(Some(port_base));
        reset_for_tests();
        Self {
            _lock: lock,
            previous_spawn_hook,
            previous_port_base,
        }
    }
}

#[cfg(test)]
impl Drop for TestManagedRuntimeProvisionGuard {
    fn drop(&mut self) {
        test_spawn_hook_set(self.previous_spawn_hook.clone());
        test_port_base_override_set(self.previous_port_base);
        reset_for_tests();
    }
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    let mut guard = manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.shutdown_all();
    guard.entries.clear();
}

#[cfg(test)]
fn test_external_runtime_ready_override_get() -> Option<bool> {
    let cell = test_external_runtime_ready_override_cell();
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
fn test_external_runtime_ready_override_set(value: Option<bool>) {
    let cell = test_external_runtime_ready_override_cell();
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

#[cfg(test)]
fn test_external_runtime_ready_override_cell() -> &'static Mutex<Option<bool>> {
    static TEST_EXTERNAL_RUNTIME_READY_OVERRIDE: OnceLock<Mutex<Option<bool>>> = OnceLock::new();
    TEST_EXTERNAL_RUNTIME_READY_OVERRIDE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn test_external_runtime_ready_override_lock() -> &'static Mutex<()> {
    static TEST_EXTERNAL_RUNTIME_READY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TEST_EXTERNAL_RUNTIME_READY_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub(super) fn test_driver_unavailability_override_get() -> Option<Option<String>> {
    let cell = test_driver_unavailability_override_cell();
    cell.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[cfg(test)]
fn test_driver_unavailability_override_set(value: Option<Option<String>>) {
    let cell = test_driver_unavailability_override_cell();
    *cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

#[cfg(test)]
fn test_driver_unavailability_override_cell() -> &'static Mutex<Option<Option<String>>> {
    static TEST_DRIVER_UNAVAILABILITY_OVERRIDE: OnceLock<Mutex<Option<Option<String>>>> =
        OnceLock::new();
    TEST_DRIVER_UNAVAILABILITY_OVERRIDE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(crate) struct TestExternalRuntimeReadyGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<bool>,
}

#[cfg(test)]
impl TestExternalRuntimeReadyGuard {
    pub(crate) fn unavailable() -> Self {
        Self::set(Some(false))
    }

    fn set(value: Option<bool>) -> Self {
        let lock = test_external_runtime_ready_override_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = test_external_runtime_ready_override_get();
        test_external_runtime_ready_override_set(value);
        reset_for_tests();
        Self {
            _lock: lock,
            previous,
        }
    }
}

#[cfg(test)]
impl Drop for TestExternalRuntimeReadyGuard {
    fn drop(&mut self) {
        test_external_runtime_ready_override_set(self.previous);
        reset_for_tests();
    }
}

#[cfg(test)]
pub(crate) struct TestRuntimeDriverAvailabilityGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<Option<String>>,
}

#[cfg(test)]
impl TestRuntimeDriverAvailabilityGuard {
    pub(crate) fn unavailable(reason: &str) -> Self {
        Self::set(Some(Some(reason.to_string())))
    }

    fn set(value: Option<Option<String>>) -> Self {
        let lock = test_external_runtime_ready_override_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = test_driver_unavailability_override_get();
        test_driver_unavailability_override_set(value);
        reset_for_tests();
        Self {
            _lock: lock,
            previous,
        }
    }
}

#[cfg(test)]
impl Drop for TestRuntimeDriverAvailabilityGuard {
    fn drop(&mut self) {
        test_driver_unavailability_override_set(self.previous.clone());
        reset_for_tests();
    }
}

#[cfg(test)]
mod tests {
    use super::super::managed_runtime_views;
    use super::*;
    use crate::backend::{BackendCapabilities, BackendClass, DriverResolution};
    use crate::model_catalog::{LocalLoadTarget, ModelMetadata};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn test_driver_resolution() -> DriverResolution {
        DriverResolution {
            resolved_backend_id: "external-llamacpp".to_string(),
            backend_class: BackendClass::ResidentLocal,
            capabilities: BackendCapabilities::default(),
            resolution_source: "test",
            resolution_rationale: "test".to_string(),
            available: true,
            load_supported: true,
        }
    }

    fn test_target(
        family: PromptFamily,
        logical_model_id: &str,
        max_context_tokens: Option<usize>,
    ) -> LocalLoadTarget {
        let dir = crate::config::kernel_config()
            .paths
            .workspace_dir
            .join("tests")
            .join("local-runtime-manager");
        fs::create_dir_all(&dir).expect("create local runtime manager test dir");
        let model_path = dir.join(format!("{logical_model_id}.gguf"));
        if !model_path.exists() {
            fs::write(&model_path, b"test").expect("create fake model artifact");
        }

        LocalLoadTarget {
            model_id: Some(logical_model_id.to_string()),
            display_path: model_path.clone(),
            runtime_reference: model_path.display().to_string(),
            family,
            tokenizer_path: None,
            metadata: Some(ModelMetadata {
                family: Some(family_label(family).to_string()),
                max_context_tokens,
                ..ModelMetadata::default()
            }),
            driver_resolution: test_driver_resolution(),
        }
    }

    fn reserve_port_base() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
        listener.local_addr().expect("free port addr").port()
    }

    fn mock_runtime_spawn_hook(spawn_count: Arc<AtomicUsize>) -> TestSpawnHook {
        Arc::new(move |request: TestSpawnRequest| {
            spawn_count.fetch_add(1, Ordering::SeqCst);
            let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
            let handle = std::thread::spawn(move || {
                let listener =
                    TcpListener::bind(("127.0.0.1", request.port)).expect("bind mock llama-server");
                listener
                    .set_nonblocking(true)
                    .expect("set mock llama-server nonblocking");

                loop {
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }

                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let mut buffer = [0_u8; 4096];
                            let read = match stream.read(&mut buffer) {
                                Ok(read) => read,
                                Err(_) => continue,
                            };
                            let request_line = String::from_utf8_lossy(&buffer[..read]);
                            let path = request_line
                                .lines()
                                .next()
                                .and_then(|line| line.split_whitespace().nth(1))
                                .unwrap_or("/");
                            let (status, body) = match path {
                                "/health" => ("HTTP/1.1 200 OK", r#"{"status":"ok"}"#.to_string()),
                                "/props" => (
                                    "HTTP/1.1 200 OK",
                                    serde_json::json!({
                                        "model_path": request.model_path.display().to_string(),
                                        "total_slots": 4,
                                        "ctx_size": request.context_window_tokens,
                                        "slot_save_path": request.slot_save_dir.display().to_string(),
                                        "family": family_label(request.family),
                                    })
                                    .to_string(),
                                ),
                                "/slots" => ("HTTP/1.1 200 OK", r#"[]"#.to_string()),
                                _ => (
                                    "HTTP/1.1 404 Not Found",
                                    r#"{"error":"unexpected path"}"#.to_string(),
                                ),
                            };
                            let response = format!(
                                "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(response.as_bytes());
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });

            Ok(ManagedRuntimeProcess::Test(TestManagedRuntimeProcess {
                stop_tx: Some(stop_tx),
                handle: Some(handle),
            }))
        })
    }

    #[test]
    fn managed_runtime_provisions_once_and_reuses_family_slot_runtime() {
        let spawn_count = Arc::new(AtomicUsize::new(0));
        let port_base = reserve_port_base();
        let _guard = TestManagedRuntimeProvisionGuard::set(
            port_base,
            mock_runtime_spawn_hook(spawn_count.clone()),
        );
        let target = test_target(PromptFamily::Qwen, "qwen-test-runtime", Some(131_072));

        let first = ensure_runtime_for_target(&target).expect("provision qwen runtime");
        let second = ensure_runtime_for_target(&target).expect("reuse qwen runtime");
        let views = managed_runtime_views();

        assert_eq!(spawn_count.load(Ordering::SeqCst), 1);
        assert_eq!(first.endpoint.host, second.endpoint.host);
        assert_eq!(first.endpoint.port, second.endpoint.port);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].family, "Qwen");
        assert_eq!(views[0].state, "ready");
        assert!(views[0].managed_by_kernel);
        assert_eq!(views[0].port, port_base);
        assert_eq!(views[0].context_window_tokens, Some(131_072));
    }

    #[test]
    fn managed_runtime_restarts_after_shutdown_when_same_family_is_requested_again() {
        let spawn_count = Arc::new(AtomicUsize::new(0));
        let port_base = reserve_port_base();
        let _guard = TestManagedRuntimeProvisionGuard::set(
            port_base,
            mock_runtime_spawn_hook(spawn_count.clone()),
        );
        let target = test_target(PromptFamily::Llama, "llama-test-runtime", Some(131_072));

        ensure_runtime_for_target(&target).expect("initial provision");
        shutdown_all();
        ensure_runtime_for_target(&target).expect("restart provision after shutdown");

        assert_eq!(spawn_count.load(Ordering::SeqCst), 2);
        let views = managed_runtime_views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].family, "Llama");
        assert_eq!(views[0].state, "ready");
    }

    #[test]
    fn managed_runtime_requires_max_context_metadata_to_spawn() {
        let spawn_count = Arc::new(AtomicUsize::new(0));
        let port_base = reserve_port_base();
        let _guard = TestManagedRuntimeProvisionGuard::set(
            port_base,
            mock_runtime_spawn_hook(spawn_count.clone()),
        );
        let target = test_target(PromptFamily::Qwen, "qwen-missing-context", None);

        let err = ensure_runtime_for_target(&target)
            .expect_err("runtime provisioning should fail without max context metadata");

        assert!(err.contains("missing metadata.max_context_tokens"));
        assert_eq!(spawn_count.load(Ordering::SeqCst), 0);
    }
}
