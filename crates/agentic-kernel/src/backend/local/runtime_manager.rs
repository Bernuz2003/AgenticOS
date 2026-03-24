use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::backend::http::HttpEndpoint;
use crate::model_catalog::{
    infer_metadata_path, load_model_metadata, LocalLoadTarget, ModelMetadata,
};
use crate::prompting::PromptFamily;

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
struct RequestedLocalRuntime {
    family: PromptFamily,
    model_path: PathBuf,
    logical_model_id: String,
    context_window_tokens: Option<usize>,
}

#[cfg(test)]
type TestSpawnHook = std::sync::Arc<
    dyn Fn(TestSpawnRequest) -> Result<ManagedRuntimeProcess, String> + Send + Sync,
>;

#[cfg(test)]
#[derive(Debug, Clone)]
struct TestSpawnRequest {
    model_path: PathBuf,
    port: u16,
    context_window_tokens: usize,
    slot_save_dir: PathBuf,
    family: PromptFamily,
}

impl RequestedLocalRuntime {
    fn from_target(target: &LocalLoadTarget) -> Result<Self, String> {
        let model_path = normalize_model_path(&target.display_path);
        Ok(Self {
            family: target.family,
            model_path,
            logical_model_id: target
                .model_id
                .clone()
                .unwrap_or_else(|| target.display_path.display().to_string()),
            context_window_tokens: target
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.max_context_tokens),
        })
    }

    fn from_reference(reference: &str, family: PromptFamily) -> Result<Self, String> {
        let fallback_path = PathBuf::from(reference);
        let model_path = normalize_model_path(&fallback_path);
        let metadata = load_local_runtime_metadata(&model_path);
        Ok(Self {
            family,
            model_path: model_path.clone(),
            logical_model_id: reference.to_string(),
            context_window_tokens: metadata.and_then(|entry| entry.max_context_tokens),
        })
    }
}

#[derive(Debug)]
struct ManagedLocalRuntimeEntry {
    family: PromptFamily,
    logical_model_id: String,
    model_path: PathBuf,
    endpoint: String,
    port: u16,
    slot_save_dir: PathBuf,
    state: ManagedLocalRuntimeState,
    context_window_tokens: Option<usize>,
    last_error: Option<String>,
    managed_by_kernel: bool,
    process: Option<ManagedRuntimeProcess>,
    updated_at_ms: i64,
}

impl ManagedLocalRuntimeEntry {
    fn lease(&self) -> Result<ManagedLocalRuntimeLease, String> {
        Ok(ManagedLocalRuntimeLease {
            endpoint: HttpEndpoint::parse(&self.endpoint).map_err(|err| err.to_string())?,
            family: self.family,
        })
    }

    fn view(&self) -> ManagedLocalRuntimeView {
        ManagedLocalRuntimeView {
            family: family_label(self.family).to_string(),
            logical_model_id: self.logical_model_id.clone(),
            display_path: self.model_path.display().to_string(),
            state: self.state.as_str().to_string(),
            endpoint: self.endpoint.clone(),
            port: self.port,
            context_window_tokens: self.context_window_tokens,
            slot_save_dir: self.slot_save_dir.display().to_string(),
            managed_by_kernel: self.managed_by_kernel,
            last_error: self.last_error.clone(),
        }
    }
}

#[derive(Debug)]
enum ManagedRuntimeProcess {
    Child(Child),
    #[cfg(test)]
    Test(TestManagedRuntimeProcess),
}

#[cfg(test)]
#[derive(Debug)]
struct TestManagedRuntimeProcess {
    stop_tx: Option<std::sync::mpsc::Sender<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ManagedRuntimeProcess {
    fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>, String> {
        match self {
            Self::Child(child) => child.try_wait().map_err(|err| err.to_string()),
            #[cfg(test)]
            Self::Test(_) => Ok(None),
        }
    }

    fn stop(&mut self) {
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

struct LocalRuntimeManager {
    entries: BTreeMap<String, ManagedLocalRuntimeEntry>,
}

impl LocalRuntimeManager {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    fn views(&self) -> Vec<ManagedLocalRuntimeView> {
        self.entries.values().map(ManagedLocalRuntimeEntry::view).collect()
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
                entry.last_error = Some("Managed local runtime became unhealthy; restarting.".to_string());
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

        let entry = self.entries.entry(key.clone()).or_insert(ManagedLocalRuntimeEntry {
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

fn manager() -> &'static Mutex<LocalRuntimeManager> {
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

pub(crate) fn managed_runtime_views() -> Vec<ManagedLocalRuntimeView> {
    manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .views()
}

pub(crate) fn diagnostic_endpoint() -> Option<String> {
    if let Some(endpoint) = legacy_endpoint_override() {
        return Some(endpoint);
    }

    manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entries
        .values()
        .next()
        .map(|entry| entry.endpoint.clone())
}

pub(crate) fn runtime_driver_available() -> bool {
    runtime_driver_unavailability_reason().is_none()
}

pub(crate) fn runtime_driver_unavailability_reason() -> Option<String> {
    if legacy_endpoint_override().is_some() {
        return None;
    }

    let configured = crate::config::kernel_config()
        .external_llamacpp
        .executable
        .trim()
        .to_string();
    if configured.is_empty() {
        return Some(
            "Local runtime backend is unavailable: [external_llamacpp].executable is empty and no legacy endpoint override is configured."
                .to_string(),
        );
    }

    let configured_path = PathBuf::from(&configured);
    if configured_path.components().count() > 1 {
        if configured_path.exists() {
            return None;
        }
        return Some(format!(
            "Local runtime backend is unavailable: configured llama-server executable '{}' does not exist. Set [external_llamacpp].executable in config/kernel/local.toml or AGENTIC_LLAMACPP_EXECUTABLE to the real binary path.",
            configured_path.display()
        ));
    }

    if resolve_llama_server_executable().is_some() {
        return None;
    }

    Some(format!(
        "Local runtime backend is unavailable: executable '{}' was not found on PATH. Install llama.cpp's llama-server or set [external_llamacpp].executable in config/kernel/local.toml (or AGENTIC_LLAMACPP_EXECUTABLE) to its absolute path.",
        configured
    ))
}

pub(crate) fn shutdown_all() {
    manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .shutdown_all();
}

fn wait_until_runtime_ready(
    entry: &mut ManagedLocalRuntimeEntry,
    expected_model_path: &Path,
) -> Result<(), String> {
    let timeout = Duration::from_millis(
        crate::config::kernel_config()
            .external_llamacpp
            .startup_timeout_ms
            .max(1),
    );
    let poll = Duration::from_millis(
        crate::config::kernel_config()
            .external_llamacpp
            .health_poll_ms
            .max(10),
    );
    let start = Instant::now();

    loop {
        if let Some(process) = entry.process.as_mut() {
            if let Some(status) = process.try_wait()? {
                return Err(format!(
                    "Local runtime '{}' exited before becoming healthy (status: {}).",
                    family_label(entry.family),
                    status
                ));
            }
        }

        match probe_runtime(&entry.endpoint, expected_model_path) {
            Ok(()) => {
                entry.state = if entry.managed_by_kernel {
                    ManagedLocalRuntimeState::Ready
                } else {
                    ManagedLocalRuntimeState::ExternalOverride
                };
                entry.last_error = None;
                entry.updated_at_ms = current_timestamp_ms();
                return Ok(());
            }
            Err(err) if start.elapsed() < timeout => {
                entry.last_error = Some(err);
                std::thread::sleep(poll);
            }
            Err(err) => {
                return Err(format!(
                    "Local runtime '{}' failed to become healthy within {} ms: {}",
                    family_label(entry.family),
                    timeout.as_millis(),
                    err
                ));
            }
        }
    }
}

fn probe_runtime(endpoint: &str, expected_model_path: &Path) -> Result<(), String> {
    let endpoint = HttpEndpoint::parse(endpoint).map_err(|err| err.to_string())?;
    let timeout_ms = crate::config::kernel_config().external_llamacpp.timeout_ms;
    let health = endpoint
        .request_json("GET", &endpoint.joined_path("/health"), None, timeout_ms)
        .map_err(|err| format!("health check failed: {}", err))?;
    if health.status_code != 200 {
        return Err(format!("health returned {}", health.status_line));
    }

    let props = endpoint
        .request_json("GET", &endpoint.joined_path("/props"), None, timeout_ms)
        .map_err(|err| format!("props check failed: {}", err))?;
    if props.status_code != 200 {
        return Err(format!("props returned {}", props.status_line));
    }

    let reported_model = props
        .json
        .as_ref()
        .and_then(|value| value.get("model_path"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| "props response is missing model_path".to_string())?;
    if !same_model_path(Path::new(reported_model), expected_model_path) {
        return Err(format!(
            "runtime is serving '{}' instead of '{}'",
            reported_model,
            expected_model_path.display()
        ));
    }

    Ok(())
}

fn spawn_llama_server(
    executable: &Path,
    model_path: &Path,
    port: u16,
    context_window_tokens: usize,
    slot_save_dir: &Path,
    family: PromptFamily,
) -> Result<ManagedRuntimeProcess, String> {
    #[cfg(test)]
    if let Some(spawn_hook) = test_spawn_hook_get() {
        return spawn_hook(TestSpawnRequest {
            model_path: model_path.to_path_buf(),
            port,
            context_window_tokens,
            slot_save_dir: slot_save_dir.to_path_buf(),
            family,
        });
    }

    let log_path = log_path_for_family(family);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create runtime log directory '{}': {}",
                parent.display(),
                err
            )
        })?;
    }
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| format!("Failed to open runtime log '{}': {}", log_path.display(), err))?;
    let stderr = stdout
        .try_clone()
        .map_err(|err| format!("Failed to clone runtime log '{}': {}", log_path.display(), err))?;

    let child = Command::new(executable)
        .arg("-m")
        .arg(model_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("--slots")
        .arg("--slot-save-path")
        .arg(slot_save_dir)
        .arg("--ctx-size")
        .arg(context_window_tokens.to_string())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|err| {
            format!(
                "Failed to spawn local runtime '{}' for family '{}': {}",
                executable.display(),
                family_label(family),
                err
            )
        })?;

    Ok(ManagedRuntimeProcess::Child(child))
}

fn stop_runtime_entry(entry: &mut ManagedLocalRuntimeEntry) {
    if let Some(process) = entry.process.as_mut() {
        process.stop();
    }
    entry.process = None;
    entry.updated_at_ms = current_timestamp_ms();
}

fn resolve_llama_server_executable() -> Option<PathBuf> {
    #[cfg(test)]
    if test_spawn_hook_get().is_some() {
        return Some(PathBuf::from("test-llama-server"));
    }

    let configured = crate::config::kernel_config()
        .external_llamacpp
        .executable
        .trim()
        .to_string();
    if configured.is_empty() {
        return None;
    }

    let configured_path = PathBuf::from(&configured);
    if configured_path.components().count() > 1 {
        return configured_path.exists().then_some(configured_path);
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(&configured);
            candidate.exists().then_some(candidate)
        })
    })
}

fn legacy_endpoint_override() -> Option<String> {
    #[cfg(test)]
    if let Some(value) = test_external_endpoint_override_get() {
        return Some(value);
    }

    let endpoint = crate::config::kernel_config()
        .external_llamacpp
        .legacy_endpoint_override
        .trim()
        .to_string();
    (!endpoint.is_empty()).then_some(endpoint)
}

fn normalize_model_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn load_local_runtime_metadata(model_path: &Path) -> Option<ModelMetadata> {
    infer_metadata_path(model_path)
        .as_ref()
        .and_then(|path| load_model_metadata(path))
}

fn same_model_path(left: &Path, right: &Path) -> bool {
    normalize_model_path(left) == normalize_model_path(right)
}

fn slot_save_dir_for_family(family: PromptFamily) -> PathBuf {
    crate::config::kernel_config()
        .paths
        .workspace_dir
        .join("local-runtimes")
        .join("slots")
        .join(family_key(family))
}

fn log_path_for_family(family: PromptFamily) -> PathBuf {
    crate::config::kernel_config()
        .paths
        .workspace_dir
        .join("local-runtimes")
        .join("logs")
        .join(format!("{}.log", family_key(family)))
}

fn port_for_family(family: PromptFamily) -> u16 {
    #[cfg(test)]
    let base = test_port_base_override_get()
        .unwrap_or(crate::config::kernel_config().external_llamacpp.port_base);
    #[cfg(not(test))]
    let base = crate::config::kernel_config().external_llamacpp.port_base;
    base.saturating_add(match family {
        PromptFamily::Qwen => 0,
        PromptFamily::Llama => 1,
        PromptFamily::Mistral => 2,
        PromptFamily::Unknown => 90,
    })
}

fn family_key(family: PromptFamily) -> String {
    family_label(family).to_ascii_lowercase()
}

fn family_label(family: PromptFamily) -> &'static str {
    match family {
        PromptFamily::Llama => "Llama",
        PromptFamily::Qwen => "Qwen",
        PromptFamily::Mistral => "Mistral",
        PromptFamily::Unknown => "Unknown",
    }
}

fn current_timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
fn test_external_endpoint_override_get() -> Option<String> {
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
fn test_spawn_hook_get() -> Option<TestSpawnHook> {
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
fn test_port_base_override_get() -> Option<u16> {
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
mod tests {
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
                let listener = TcpListener::bind(("127.0.0.1", request.port))
                    .expect("bind mock llama-server");
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
        let _guard =
            TestManagedRuntimeProvisionGuard::set(port_base, mock_runtime_spawn_hook(spawn_count.clone()));
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
        let _guard =
            TestManagedRuntimeProvisionGuard::set(port_base, mock_runtime_spawn_hook(spawn_count.clone()));
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
        let _guard =
            TestManagedRuntimeProvisionGuard::set(port_base, mock_runtime_spawn_hook(spawn_count.clone()));
        let target = test_target(PromptFamily::Qwen, "qwen-missing-context", None);

        let err = ensure_runtime_for_target(&target)
            .expect_err("runtime provisioning should fail without max context metadata");

        assert!(err.contains("missing metadata.max_context_tokens"));
        assert_eq!(spawn_count.load(Ordering::SeqCst), 0);
    }
}
