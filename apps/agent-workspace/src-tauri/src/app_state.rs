use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::kernel::client::KernelBridge;
use crate::kernel::stream::TimelineStore;

#[derive(Debug)]
pub struct AppState {
    pub workspace_root: PathBuf,
    pub kernel_addr: String,
    pub bridge: Arc<Mutex<KernelBridge>>,
    pub timeline_store: Arc<Mutex<TimelineStore>>,
}

impl Default for AppState {
    fn default() -> Self {
        let workspace_root =
            std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap_or_else(|_| PathBuf::from("../../.."));
        let kernel_addr = std::env::var("AGENTIC_PORT")
            .ok()
            .and_then(|port| port.parse::<u16>().ok())
            .map(|port| format!("127.0.0.1:{port}"))
            .unwrap_or_else(|| "127.0.0.1:6380".to_string());

        Self {
            bridge: Arc::new(Mutex::new(KernelBridge::new(
                kernel_addr.clone(),
                workspace_root.clone(),
            ))),
            timeline_store: Arc::new(Mutex::new(TimelineStore::default())),
            workspace_root,
            kernel_addr,
        }
    }
}
