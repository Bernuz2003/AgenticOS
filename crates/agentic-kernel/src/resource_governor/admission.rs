/// Admission logic and budget calculations.
use crate::backend::BackendClass;
use crate::config::ResourceGovernorConfig;
use crate::model_catalog::ResolvedModelTarget;
use crate::runtimes::RuntimeReservation;
use std::path::Path;

pub(crate) fn estimate_reservation(
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

pub(crate) fn estimate_local_model_bytes(target: &ResolvedModelTarget) -> u64 {
    std::fs::metadata(target.display_path())
        .map(|metadata| metadata.len())
        .unwrap_or_else(|_| fallback_model_bytes(target.display_path(), &target.logical_model_id()))
}

pub(crate) fn fallback_model_bytes(path: &Path, logical_model_id: &str) -> u64 {
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

pub(crate) fn scaled_reservation_bytes(base_bytes: u64, scale: f64, overhead_bytes: u64) -> u64 {
    ((base_bytes as f64) * scale).ceil() as u64 + overhead_bytes
}

pub(crate) fn fits_with_headroom(
    config: &ResourceGovernorConfig,
    usage: RuntimeReservation,
    reservation: RuntimeReservation,
) -> bool {
    fits_with_headroom_after_free(config, usage, RuntimeReservation::default(), reservation)
}

pub(crate) fn fits_with_headroom_after_free(
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

pub(crate) fn effective_budget(budget: u64, headroom: u64) -> u64 {
    if budget == 0 {
        u64::MAX
    } else {
        budget.saturating_sub(headroom)
    }
}

pub(crate) fn budget_available(budget: u64, headroom: u64, used: u64) -> u64 {
    effective_budget(budget, headroom).saturating_sub(used)
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    if bytes == u64::MAX {
        "unbounded".to_string()
    } else {
        format!("{:.2}GiB", bytes as f64 / GIB)
    }
}
