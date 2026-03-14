//! System configuration definition and initialization.

pub mod models;
pub mod parser;

pub use models::*;
pub use parser::*;

use std::path::PathBuf;
use std::sync::OnceLock;

static KERNEL_CONFIG: OnceLock<KernelConfig> = OnceLock::new();

pub fn initialize() -> Result<&'static KernelConfig, String> {
    if let Some(config) = KERNEL_CONFIG.get() {
        return Ok(config);
    }

    let config = load_kernel_config()?;
    let _ = KERNEL_CONFIG.set(config);
    Ok(KERNEL_CONFIG.get().expect("kernel config initialized"))
}

pub fn kernel_config() -> &'static KernelConfig {
    KERNEL_CONFIG.get_or_init(|| {
        load_kernel_config().unwrap_or_else(|err| {
            eprintln!("AgenticOS config warning: {err}. Falling back to built-in defaults.");
            KernelConfig::default()
        })
    })
}

#[allow(dead_code)]
pub fn config_file_path() -> PathBuf {
    resolve_config_bootstrap_paths().primary_config_path()
}

