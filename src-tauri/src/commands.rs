use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::engine::{AppConfig, Preset};
use crate::inject::{foreground_pid, inject_pid};
use crate::ipc::BridgeState;
use crate::overlay::{hide_overlay, show_overlay};
use crate::sensing::FaceSample;
use crate::state::AppState;

#[tauri::command]
pub fn get_config(state: State<'_, std::sync::Arc<AppState>>) -> AppConfig {
    state.config.lock().clone()
}

#[tauri::command]
pub fn set_config(
    app: AppHandle,
    state: State<'_, std::sync::Arc<AppState>>,
    config: AppConfig,
) -> Result<AppConfig, String> {
    let cfg = config.clamp();
    *state.config.lock() = cfg.clone();
    let _ = app.emit("config-updated", &cfg);
    Ok(cfg)
}

#[tauri::command]
pub fn apply_preset(
    app: AppHandle,
    state: State<'_, std::sync::Arc<AppState>>,
    preset: Preset,
) -> Result<AppConfig, String> {
    let mut cfg = AppConfig::from_preset(preset);
    {
        let current = state.config.lock();
        cfg.enabled = current.enabled;
        cfg.auto_start_overlay = current.auto_start_overlay;
        cfg.motion_source = current.motion_source;
        cfg.manual_motion = current.manual_motion;
    }
    *state.config.lock() = cfg.clone();
    let _ = app.emit("config-updated", &cfg);
    Ok(cfg)
}

#[tauri::command]
pub fn toggle_enabled(
    app: AppHandle,
    state: State<'_, std::sync::Arc<AppState>>,
) -> Result<AppConfig, String> {
    let mut cfg = state.config.lock();
    cfg.enabled = !cfg.enabled;
    let out = cfg.clone();
    drop(cfg);
    let _ = app.emit("config-updated", &out);
    Ok(out)
}

#[tauri::command]
pub fn start_overlay(
    app: AppHandle,
    state: State<'_, std::sync::Arc<AppState>>,
) -> Result<(), String> {
    show_overlay(&app).map_err(|e| e.to_string())?;
    *state.overlay_visible.lock() = true;
    Ok(())
}

#[tauri::command]
pub fn stop_overlay(
    app: AppHandle,
    state: State<'_, std::sync::Arc<AppState>>,
) -> Result<(), String> {
    hide_overlay(&app).map_err(|e| e.to_string())?;
    *state.overlay_visible.lock() = false;
    Ok(())
}

#[tauri::command]
pub fn get_overlay_frame(
    state: State<'_, std::sync::Arc<AppState>>,
) -> crate::engine::OverlayFrame {
    state.last_frame.lock().clone()
}

#[tauri::command]
pub fn is_overlay_active(state: State<'_, std::sync::Arc<AppState>>) -> bool {
    *state.overlay_visible.lock()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    pub attached_pid: Option<u32>,
    pub hooked: bool,
    pub motion: f32,
    pub comfort: f32,
    pub fov_amount: f32,
    pub present_fps: f32,
    pub peri_flow: f32,
}

#[tauri::command]
pub fn get_bridge_status(bridge: State<'_, std::sync::Arc<BridgeState>>) -> BridgeStatus {
    let snap = bridge.ipc.snapshot();
    BridgeStatus {
        attached_pid: *bridge.attached_pid.lock(),
        hooked: snap.hooked != 0,
        motion: snap.motion,
        comfort: snap.comfort,
        fov_amount: snap.fov_amount,
        present_fps: snap.present_fps,
        peri_flow: snap.peri_flow,
    }
}

/// Inject the Present bridge into the current foreground game.
/// Focus the game window first, then call this (or use the UI button with a short delay).
#[tauri::command]
pub fn attach_foreground_game(
    app: AppHandle,
    bridge: State<'_, std::sync::Arc<BridgeState>>,
) -> Result<BridgeStatus, String> {
    let pid = foreground_pid()?;
    inject_pid(pid)?;
    *bridge.attached_pid.lock() = Some(pid);
    // Prefer in-game Present path: hide desktop overlay to avoid double vignette
    // when the game is borderless. Exclusive FS ignores the desktop overlay anyway.
    let _ = hide_overlay(&app);
    if let Some(state) = app.try_state::<std::sync::Arc<AppState>>() {
        *state.overlay_visible.lock() = false;
        let _ = app.emit("overlay-toggled", false);
    }
    let _ = app.emit("bridge-attached", pid);
    Ok(get_bridge_status(bridge))
}

#[tauri::command]
pub fn attach_game_pid(
    app: AppHandle,
    bridge: State<'_, std::sync::Arc<BridgeState>>,
    pid: u32,
) -> Result<BridgeStatus, String> {
    if pid == std::process::id() {
        return Err("cannot attach to MOSICKWHILE itself".into());
    }
    inject_pid(pid)?;
    *bridge.attached_pid.lock() = Some(pid);
    let _ = hide_overlay(&app);
    if let Some(state) = app.try_state::<std::sync::Arc<AppState>>() {
        *state.overlay_visible.lock() = false;
        let _ = app.emit("overlay-toggled", false);
    }
    let _ = app.emit("bridge-attached", pid);
    Ok(get_bridge_status(bridge))
}

/// MediaPipe gaze driver (frontend) pushes iris/pose samples here.
#[tauri::command]
pub fn push_gaze_sample(
    state: State<'_, std::sync::Arc<AppState>>,
    sample: FaceSample,
) -> Result<(), String> {
    state.face.ingest(sample);
    Ok(())
}

#[tauri::command]
pub fn set_gaze_external(
    state: State<'_, std::sync::Arc<AppState>>,
    external: bool,
) -> Result<(), String> {
    state.face.set_external(external);
    if external {
        state.face.release_camera();
    }
    Ok(())
}

/// Release native webcam so the MediaPipe frontend can open it exclusively.
#[tauri::command]
pub fn release_camera(state: State<'_, std::sync::Arc<AppState>>) -> Result<(), String> {
    state.face.release_camera();
    // Brief yield so MSMF tears down the capture graph before getUserMedia.
    std::thread::sleep(std::time::Duration::from_millis(350));
    Ok(())
}
