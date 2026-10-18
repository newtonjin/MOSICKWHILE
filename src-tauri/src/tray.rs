//! System tray — minimize/close parks the panel in the notification area.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "tray-show", "Abrir MOSICKWHILE", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .unwrap_or_else(|| tauri::include_image!("icons/32x32.png").to_owned());

    let _tray = TrayIconBuilder::with_id("mosickwhile")
        .icon(icon)
        .tooltip("MOSICKWHILE — clique para abrir")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-show" => {
                let _ = restore_main(app);
            }
            "tray-quit" => {
                crate::shutdown_app(app);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = restore_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

pub fn restore_main(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    if let Some(state) = app.try_state::<std::sync::Arc<AppState>>() {
        state.panel_visible.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}

pub fn hide_to_tray(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    if let Some(state) = app.try_state::<std::sync::Arc<AppState>>() {
        state
            .panel_visible
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
