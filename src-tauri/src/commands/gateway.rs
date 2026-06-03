//! Gateway control commands: start/stop the in-process messaging server and
//! report its status. The gateway is real (WebChat + Discord/Telegram/Slack all
//! run the agent), so a desktop Start/Stop affordance is honest.

use crate::state::AppCore;
use open_assistant::config;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct GatewayStatusDto {
    pub running: bool,
    /// e.g. "http://0.0.0.0:3000" when running.
    pub address: Option<String>,
}

/// Start the gateway in-process. Errors if it's already running or the port is
/// in use. Returns the URL it's listening on.
#[tauri::command(rename_all = "snake_case")]
pub async fn gateway_start(state: State<'_, AppCore>) -> Result<String, String> {
    let mut g = state.gateway.lock().await;
    if g.as_ref().map_or(false, |h| h.is_running()) {
        return Err("Gateway is already running.".into());
    }
    let cfg = config::load().await.map_err(|e| e.to_string())?;
    let handle = open_assistant::gateway::start_gateway_handle(cfg)
        .await
        .map_err(|e| e.to_string())?;
    let url = format!("http://{}", handle.addr);
    *g = Some(handle);
    Ok(url)
}

/// Stop the gateway if running. No-op if it isn't.
#[tauri::command(rename_all = "snake_case")]
pub async fn gateway_stop(state: State<'_, AppCore>) -> Result<(), String> {
    let mut g = state.gateway.lock().await;
    if let Some(handle) = g.take() {
        handle.stop();
    }
    Ok(())
}

/// Current gateway run status.
#[tauri::command(rename_all = "snake_case")]
pub async fn gateway_status(state: State<'_, AppCore>) -> Result<GatewayStatusDto, String> {
    let g = state.gateway.lock().await;
    match g.as_ref() {
        Some(h) if h.is_running() => Ok(GatewayStatusDto {
            running: true,
            address: Some(format!("http://{}", h.addr)),
        }),
        _ => Ok(GatewayStatusDto { running: false, address: None }),
    }
}
