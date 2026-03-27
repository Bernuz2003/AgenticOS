use tauri::State;

use super::run_blocking;
use crate::kernel::auth;
use crate::kernel::client::transport::{self, KernelBridgeError};
use crate::models::kernel::{KernelBootstrapState, LobbySnapshot};
use crate::state::AppState;

#[tauri::command]
pub fn bootstrap_state(state: State<'_, AppState>) -> KernelBootstrapState {
    KernelBootstrapState {
        kernel_addr: state.kernel_addr.clone(),
        workspace_root: state.workspace_root.display().to_string(),
        protocol_version: transport::default_protocol_version().to_string(),
        connection_mode: "tcp-authenticated-bridge".to_string(),
    }
}

#[tauri::command]
pub fn protocol_preview(state: State<'_, AppState>) -> String {
    let token_path = auth::kernel_token_path(&state.workspace_root);
    format!(
        "protocol={} token_path={}",
        transport::default_protocol_version(),
        token_path.display()
    )
}

#[tauri::command]
pub async fn fetch_lobby_snapshot(state: State<'_, AppState>) -> Result<LobbySnapshot, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        Ok(bridge.fetch_lobby_snapshot())
    })
    .await
}

#[tauri::command]
pub async fn ping_kernel(state: State<'_, AppState>) -> Result<String, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.ping().map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn shutdown_kernel(state: State<'_, AppState>) -> Result<String, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        match bridge.shutdown() {
            Ok(message) => Ok(message),
            Err(err) if is_expected_shutdown_disconnect(&err) => {
                Ok("Kernel shutdown requested".to_string())
            }
            Err(err) => Err(err.to_string()),
        }
    })
    .await
}

fn is_expected_shutdown_disconnect(err: &KernelBridgeError) -> bool {
    match err {
        KernelBridgeError::ConnectionClosed | KernelBridgeError::ConnectionUnavailable => true,
        KernelBridgeError::Io(io_err) => matches!(
            io_err.kind(),
            std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::UnexpectedEof
        ),
        _ => false,
    }
}
