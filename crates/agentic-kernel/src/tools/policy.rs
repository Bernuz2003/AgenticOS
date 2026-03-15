use crate::backend::http::HttpEndpoint;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::config::{env_bool, env_u64, env_usize, kernel_config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SandboxMode {
    Host,
    Container,
    Wasm,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SysCallConfig {
    pub(crate) mode: SandboxMode,
    pub(crate) allow_host_fallback: bool,
    pub(crate) timeout_s: u64,
    pub(crate) max_calls_per_window: usize,
    pub(crate) window_s: u64,
    pub(crate) error_burst_kill: usize,
}

#[derive(Debug, Clone)]
struct RateState {
    calls_in_window: VecDeque<Instant>,
    consecutive_errors: usize,
}

/// Per-process syscall rate-limiting state — owned by Kernel, no global statics.
pub(crate) struct SyscallRateMap {
    states: HashMap<u64, RateState>,
}

impl SyscallRateMap {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }
}

pub(crate) fn syscall_config() -> SysCallConfig {
    let tools = &kernel_config().tools;
    let mode = match std::env::var("AGENTIC_SANDBOX_MODE")
        .unwrap_or_else(|_| tools.sandbox_mode.clone())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "container" => SandboxMode::Container,
        "wasm" => SandboxMode::Wasm,
        _ => SandboxMode::Host,
    };

    SysCallConfig {
        mode,
        allow_host_fallback: env_bool("AGENTIC_ALLOW_HOST_FALLBACK", tools.allow_host_fallback),
        timeout_s: env_u64("AGENTIC_SYSCALL_TIMEOUT_S", tools.timeout_s),
        max_calls_per_window: env_usize(
            "AGENTIC_SYSCALL_MAX_PER_WINDOW",
            tools.max_calls_per_window,
        ),
        window_s: env_u64("AGENTIC_SYSCALL_WINDOW_S", tools.window_s),
        error_burst_kill: env_usize("AGENTIC_SYSCALL_ERROR_BURST_KILL", tools.error_burst_kill),
    }
}

pub(crate) fn rate_limit_precheck(
    pid: u64,
    cfg: SysCallConfig,
    rate_map: &mut SyscallRateMap,
) -> Result<(), String> {
    let now = Instant::now();
    let state = rate_map.states.entry(pid).or_insert_with(|| RateState {
        calls_in_window: VecDeque::new(),
        consecutive_errors: 0,
    });

    let max_age = Duration::from_secs(cfg.window_s.max(1));
    while let Some(front) = state.calls_in_window.front().copied() {
        if now.duration_since(front) > max_age {
            state.calls_in_window.pop_front();
        } else {
            break;
        }
    }

    if state.calls_in_window.len() >= cfg.max_calls_per_window.max(1) {
        return Err(format!(
            "SysCall Error: Rate limit exceeded (>{} calls in {}s).",
            cfg.max_calls_per_window.max(1),
            cfg.window_s.max(1)
        ));
    }

    state.calls_in_window.push_back(now);
    Ok(())
}

pub(crate) fn rate_limit_postcheck(
    pid: u64,
    success: bool,
    cfg: SysCallConfig,
    rate_map: &mut SyscallRateMap,
) -> bool {
    let state = rate_map.states.entry(pid).or_insert_with(|| RateState {
        calls_in_window: VecDeque::new(),
        consecutive_errors: 0,
    });

    if success {
        state.consecutive_errors = 0;
        false
    } else {
        state.consecutive_errors += 1;
        state.consecutive_errors >= cfg.error_burst_kill.max(1)
    }
}

pub(crate) fn enforce_remote_http_policy(
    tool_name: &str,
    endpoint: &HttpEndpoint,
) -> Result<(), String> {
    let allowed_hosts = remote_http_allowed_hosts();
    let host = endpoint.host.trim().to_ascii_lowercase();
    if !allowed_hosts.iter().any(|allowed| allowed == &host) {
        return Err(format!(
            "Remote tool '{}' endpoint host '{}' is not allowlisted.",
            tool_name, endpoint.host
        ));
    }

    let addr = format!("{}:{}", endpoint.host, endpoint.port);
    let resolved: Vec<_> = addr
        .to_socket_addrs()
        .map_err(|e| {
            format!(
                "Remote tool '{}' failed to resolve '{}': {}",
                tool_name, addr, e
            )
        })?
        .collect();

    if resolved.is_empty() {
        return Err(format!(
            "Remote tool '{}' failed to resolve endpoint '{}'.",
            tool_name, addr
        ));
    }

    let declared_ip = endpoint.host.parse::<IpAddr>().ok();
    for socket_addr in resolved {
        let resolved_ip = socket_addr.ip();
        if is_disallowed_remote_ip(resolved_ip) && declared_ip != Some(resolved_ip) {
            return Err(format!(
                "Remote tool '{}' resolved host '{}' to disallowed address '{}'. Use an explicitly declared literal IP if this endpoint is intentional.",
                tool_name,
                endpoint.host,
                resolved_ip
            ));
        }
    }

    Ok(())
}

pub(crate) fn is_disallowed_remote_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(addr) => {
            addr.is_private()
                || addr.is_loopback()
                || addr.is_link_local()
                || addr.is_broadcast()
                || addr.is_documentation()
                || addr.is_multicast()
                || addr.is_unspecified()
        }
        IpAddr::V6(addr) => {
            addr.is_loopback()
                || addr.is_unspecified()
                || addr.is_unique_local()
                || addr.is_unicast_link_local()
                || addr.is_multicast()
        }
    }
}

pub(crate) fn remote_http_allowed_hosts() -> Vec<String> {
    if let Ok(value) = std::env::var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS") {
        return value
            .split(',')
            .map(|item| item.trim().to_ascii_lowercase())
            .filter(|item| !item.is_empty())
            .collect();
    }

    kernel_config().tools.remote_http_allowed_hosts.clone()
}

pub(crate) fn remote_http_max_request_bytes() -> usize {
    std::env::var("AGENTIC_REMOTE_TOOL_MAX_REQUEST_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.max(256))
        .unwrap_or(kernel_config().tools.remote_http_max_request_bytes)
}

pub(crate) fn remote_http_max_response_bytes() -> usize {
    std::env::var("AGENTIC_REMOTE_TOOL_MAX_RESPONSE_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.max(256))
        .unwrap_or(kernel_config().tools.remote_http_max_response_bytes)
}
