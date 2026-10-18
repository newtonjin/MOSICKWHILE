use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::sensing::display_geom::{
    monitor_of_foreground, monitor_under_cursor, DisplayRect,
};

#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW,
    SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WDA_EXCLUDEFROMCAPTURE, WS_EX_APPWINDOW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

pub const OVERLAY_LABEL: &str = "overlay";

pub fn ensure_overlay(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        harden_overlay_window(&win)?;
        return Ok(win);
    }

    let window =
        WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("overlay.html".into()))
            .title("")
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .focused(false)
            .visible(false)
            .shadow(false)
            .fullscreen(false)
            .build()?;

    let _ = fit_overlay_to_active_monitor(app, &window);
    harden_overlay_window(&window)?;
    Ok(window)
}

pub fn show_overlay(app: &AppHandle) -> tauri::Result<()> {
    let win = ensure_overlay(app)?;
    let _ = fit_overlay_to_active_monitor(app, &win);
    win.show()?;
    let _ = win.set_always_on_top(true);
    harden_overlay_window(&win)?;
    reassert_topmost(&win);
    Ok(())
}

/// Keep the external overlay above games on the active monitor (no DLL inject).
/// Works with borderless / windowed / fullscreen-windowed. True exclusive FS
/// usually needs the game set to Borderless.
pub fn reassert_topmost(window: &tauri::WebviewWindow) {
    let _ = window.set_always_on_top(true);
    #[cfg(windows)]
    {
        let Ok(hwnd_raw) = window.hwnd() else {
            return;
        };
        let hwnd = HWND(hwnd_raw.0 as *mut _);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

pub fn hide_overlay(app: &AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        win.hide()?;
    }
    Ok(())
}

pub fn destroy_overlay(app: &AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = win.hide();
        win.close()?;
    }
    Ok(())
}

/// Place FOV on the right monitor: prefer focused game, else cursor (multi-screen).
pub fn fit_overlay_to_active_monitor(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
) -> tauri::Result<()> {
    let pin_game = app
        .try_state::<std::sync::Arc<crate::state::AppState>>()
        .map(|s| s.config.lock().pin_to_game_monitor)
        .unwrap_or(true);

    let fallback = || {
        window
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| window.primary_monitor().ok().flatten())
            .map(|m| {
                let p = *m.position();
                let s = *m.size();
                DisplayRect {
                    x: p.x,
                    y: p.y,
                    w: s.width,
                    h: s.height,
                }
            })
    };

    let rect = if pin_game {
        monitor_of_foreground()
            .or_else(monitor_under_cursor)
            .or_else(fallback)
    } else {
        monitor_under_cursor()
            .or_else(monitor_of_foreground)
            .or_else(fallback)
    };

    let Some(rect) = rect else {
        return Ok(());
    };

    let _ = window.set_fullscreen(false);
    let _ = window.set_decorations(false);
    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x: rect.x,
        y: rect.y,
    }))?;
    window.set_size(tauri::Size::Physical(tauri::PhysicalSize {
        width: rect.w,
        height: rect.h,
    }))?;

    if let Some(state) = app.try_state::<std::sync::Arc<crate::state::AppState>>() {
        state.display.set(rect);
        state.face.set_display_size(rect.w, rect.h);
    }

    Ok(())
}

fn harden_overlay_window(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let _ = window.set_ignore_cursor_events(true);
    let _ = window.set_title("");
    let _ = window.set_decorations(false);
    let _ = window.set_shadow(false);

    #[cfg(windows)]
    apply_win32_clickthrough(window);

    Ok(())
}

#[cfg(windows)]
fn apply_win32_clickthrough(window: &tauri::WebviewWindow) {
    let Ok(hwnd_raw) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(hwnd_raw.0 as *mut _);
    unsafe {
        let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        style |= (WS_EX_LAYERED.0
            | WS_EX_TRANSPARENT.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TOPMOST.0) as isize;
        style &= !(WS_EX_APPWINDOW.0 as isize);
        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        let _ = SetLayeredWindowAttributes(hwnd, Default::default(), 255, LWA_ALPHA);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }
}
