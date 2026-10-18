mod commands;
mod engine;
mod inject;
mod ipc;
mod overlay;
mod sensing;
mod state;
mod tray;

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use commands::{
    apply_preset, attach_foreground_game, attach_game_pid, get_bridge_status, get_config,
    get_overlay_frame, is_overlay_active, set_config, start_overlay, stop_overlay, toggle_enabled,
};
use ipc::BridgeState;
use overlay::{destroy_overlay, fit_overlay_to_active_monitor, hide_overlay, show_overlay};
use state::AppState;

fn try_register_shortcuts(app: &tauri::App) {
    let toggle = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);
    let stronger = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Equal);
    let weaker = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Minus);

    let _ = app.global_shortcut().unregister(toggle.clone());
    let _ = app.global_shortcut().unregister(stronger.clone());
    let _ = app.global_shortcut().unregister(weaker.clone());

    if let Err(err) = app.global_shortcut().on_shortcut(toggle, {
        let app = app.handle().clone();
        move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let state = app.state::<std::sync::Arc<AppState>>();
            let visible = *state.overlay_visible.lock();
            if visible {
                let _ = hide_overlay(&app);
                *state.overlay_visible.lock() = false;
            } else {
                let _ = show_overlay(&app);
                *state.overlay_visible.lock() = true;
            }
            let _ = app.emit("overlay-toggled", *state.overlay_visible.lock());
        }
    }) {
        eprintln!("MOSICKWHILE: toggle hotkey unavailable: {err}");
    }

    if let Err(err) = app.global_shortcut().on_shortcut(stronger, {
        let app = app.handle().clone();
        move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let state = app.state::<std::sync::Arc<AppState>>();
            let mut cfg = state.config.lock();
            cfg.intensity = (cfg.intensity + 0.05).clamp(0.0, 1.0);
            cfg.preset = engine::Preset::Custom;
            let out = cfg.clone();
            drop(cfg);
            let _ = app.emit("config-updated", &out);
        }
    }) {
        eprintln!("MOSICKWHILE: intensify hotkey unavailable: {err}");
    }

    if let Err(err) = app.global_shortcut().on_shortcut(weaker, {
        let app = app.handle().clone();
        move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let state = app.state::<std::sync::Arc<AppState>>();
            let mut cfg = state.config.lock();
            cfg.intensity = (cfg.intensity - 0.05).clamp(0.0, 1.0);
            cfg.preset = engine::Preset::Custom;
            let out = cfg.clone();
            drop(cfg);
            let _ = app.emit("config-updated", &out);
        }
    }) {
        eprintln!("MOSICKWHILE: weaken hotkey unavailable: {err}");
    }
}

pub(crate) fn shutdown_app(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<std::sync::Arc<AppState>>() {
        state.request_stop();
        *state.overlay_visible.lock() = false;
    }
    let _ = destroy_overlay(app);
    let _ = app.global_shortcut().unregister_all();
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new();
    let bridge = std::sync::Arc::new(
        BridgeState::new().unwrap_or_else(|e| panic!("MOSICKWHILE bridge IPC failed: {e}")),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(app_state.clone())
        .manage(bridge.clone())
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            apply_preset,
            toggle_enabled,
            start_overlay,
            stop_overlay,
            get_overlay_frame,
            is_overlay_active,
            get_bridge_status,
            attach_foreground_game,
            attach_game_pid,
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                // × and minimize → tray (overlay keeps running).
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    tray::hide_to_tray(window.app_handle());
                }
                WindowEvent::Resized(_) => {
                    if window.is_minimized().unwrap_or(false) {
                        tray::hide_to_tray(window.app_handle());
                    }
                }
                WindowEvent::Destroyed => {
                    // Only if the process is tearing down for real.
                }
                _ => {}
            }
        })
        .setup(|app| {
            tray::setup_tray(app.handle())?;

            let handle = app.handle().clone();
            let state = app.state::<std::sync::Arc<AppState>>().inner().clone();
            let bridge = app.state::<std::sync::Arc<BridgeState>>().inner().clone();

            std::thread::spawn(move || {
                let mut geo_tick = 0u32;
                let mut ui_tick = 0u32;
                while state.is_running() {
                    geo_tick = geo_tick.wrapping_add(1);
                    ui_tick = ui_tick.wrapping_add(1);
                    // Dual-monitor: move FOV overlay onto the monitor under the cursor.
                    if geo_tick % 20 == 0 {
                        if let Some(win) = handle.get_webview_window(overlay::OVERLAY_LABEL) {
                            if *state.overlay_visible.lock() {
                                let _ = fit_overlay_to_active_monitor(&handle, &win);
                                let _ = win.set_always_on_top(true);
                            }
                        }
                    }

                    let cfg = state.config.lock().clone();
                    bridge.ipc.write_from_config(&cfg, cfg.enabled);

                    let snap_pre = bridge.ipc.snapshot();
                    let frame = state.tick(snap_pre.hooked != 0);
                    bridge.ipc.write_frame_hints(&frame);
                    let snap = bridge.ipc.snapshot();

                    // When Present bridge is live, surface its telemetry (exclusive FS).
                    let mut frame = frame;
                    if snap.hooked != 0 {
                        frame.metrics.motion = snap.motion;
                        frame.metrics.comfort_index = snap.comfort;
                        frame.metrics.fov_restriction = snap.fov_amount;
                        frame.metrics.peripheral_flow = snap.peri_flow;
                        frame.metrics.analyze_fps = snap.present_fps;
                        frame.metrics.capture_ok = true;
                        frame.metrics.capture_status = "bridge_present".into();
                        frame.motion = snap.motion;
                        frame.vignette_amount = snap.fov_amount;
                    }

                    // Overlay always needs frames (FOV). Panel telemetry throttled in tray.
                    let _ = handle.emit("overlay-frame", &frame);
                    let panel_on = state.panel_visible.load(Ordering::Relaxed);
                    if panel_on || ui_tick % 4 == 0 {
                        let _ = handle.emit(
                            "bridge-status",
                            commands::BridgeStatus {
                                attached_pid: *bridge.attached_pid.lock(),
                                hooked: snap.hooked != 0,
                                motion: snap.motion,
                                comfort: snap.comfort,
                                fov_amount: snap.fov_amount,
                                present_fps: snap.present_fps,
                                peri_flow: snap.peri_flow,
                            },
                        );
                    }

                    // Slightly longer sleep when panel is hidden → less CPU churn.
                    let sleep_ms = if panel_on { 22 } else { 26 };
                    std::thread::sleep(Duration::from_millis(sleep_ms));
                }
            });

            {
                let cfg = app.state::<std::sync::Arc<AppState>>().config.lock().clone();
                if cfg.auto_start_overlay {
                    let app = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(400));
                        let state = app.state::<std::sync::Arc<AppState>>();
                        if !state.is_running() {
                            return;
                        }
                        if show_overlay(&app).is_ok() {
                            *state.overlay_visible.lock() = true;
                            let _ = app.emit("overlay-toggled", true);
                        }
                    });
                }
            }

            try_register_shortcuts(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running MOSICKWHILE");
}
