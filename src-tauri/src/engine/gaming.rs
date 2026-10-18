//! Auto gaming-mode detection + tuning hints.
//!
//! Active when: Present bridge hooked, or foreground looks like a game
//! (near-fullscreen, non-shell process) with game-like motion.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Default)]
pub struct GamingState {
    pub active: bool,
    /// 0–1 confidence the user is in a game session.
    pub score: f32,
}

pub struct GameDetector {
    score: f32,
    active: bool,
    last_fg: Instant,
    fg_game_like: bool,
    hold_until: Instant,
}

impl Default for GameDetector {
    fn default() -> Self {
        Self {
            score: 0.0,
            active: false,
            last_fg: Instant::now() - Duration::from_secs(5),
            fg_game_like: false,
            hold_until: Instant::now(),
        }
    }
}

impl GameDetector {
    pub fn update(
        &mut self,
        bridge_hooked: bool,
        peri_flow: f32,
        rotational: f32,
        mouse_boost: f32,
        dt: f32,
    ) -> GamingState {
        // Refresh foreground classification ~4 Hz (Win32 calls are cheap but not free).
        if self.last_fg.elapsed() > Duration::from_millis(250) {
            self.last_fg = Instant::now();
            self.fg_game_like = foreground_looks_like_game();
        }

        let mut impulse = 0.0f32;
        if bridge_hooked {
            impulse += 0.55;
        }
        if self.fg_game_like {
            impulse += 0.38;
        }
        // Game-like visual motion (turns / camera).
        let motion = (peri_flow * 0.45 + rotational * 0.55 + mouse_boost * 0.35).clamp(0.0, 1.0);
        if motion > 0.2 {
            impulse += (motion - 0.2) * 0.5;
        }

        // Fast attack into gaming, slower release (avoid flicker when alt-tabbing).
        let attack = 1.0 - (-dt / 0.35).exp();
        let release = 1.0 - (-dt / 1.8).exp();
        if impulse > self.score {
            self.score += (impulse.min(1.0) - self.score) * attack;
        } else {
            self.score += (impulse - self.score) * release;
        }
        self.score = self.score.clamp(0.0, 1.0);

        // Hysteresis + short hold so brief menus don't drop mode.
        if self.score > 0.42 {
            self.active = true;
            self.hold_until = Instant::now() + Duration::from_millis(900);
        } else if Instant::now() > self.hold_until && self.score < 0.28 {
            self.active = false;
        }

        GamingState {
            active: self.active,
            score: self.score,
        }
    }
}

fn foreground_looks_like_game() -> bool {
    #[cfg(windows)]
    {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic,
        };

        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.0.is_null() {
                return false;
            }
            if IsIconic(hwnd).as_bool() {
                return false;
            }

            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == std::process::id() {
                return false;
            }

            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 260];
                let mut size = buf.len() as u32;
                let ok = QueryFullProcessImageNameW(
                    handle,
                    Default::default(),
                    PWSTR(buf.as_mut_ptr()),
                    &mut size,
                );
                let _ = CloseHandle(handle);
                if ok.is_ok() && size > 0 {
                    let path = String::from_utf16_lossy(&buf[..size as usize]);
                    let name = path
                        .rsplit(['\\', '/'])
                        .next()
                        .unwrap_or(&path)
                        .to_ascii_lowercase();
                    if is_shell_or_desktop_app(&name) {
                        return false;
                    }
                }
            }

            let mut wr = RECT::default();
            if GetWindowRect(hwnd, &mut wr).is_err() {
                return false;
            }
            let ww = (wr.right - wr.left).max(1);
            let wh = (wr.bottom - wr.top).max(1);

            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(mon, &mut mi).as_bool() {
                return ww >= 1280 && wh >= 720;
            }
            let mw = (mi.rcMonitor.right - mi.rcMonitor.left).max(1);
            let mh = (mi.rcMonitor.bottom - mi.rcMonitor.top).max(1);
            let cover = (ww as f32 / mw as f32) * (wh as f32 / mh as f32);
            // Borderless / exclusive-ish coverage.
            cover >= 0.82 && ww >= 1024 && wh >= 600
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn is_shell_or_desktop_app(exe: &str) -> bool {
    const BLOCK: &[&str] = &[
        "explorer.exe",
        "searchhost.exe",
        "startmenuexperiencehost.exe",
        "shellexperiencehost.exe",
        "applicationframehost.exe",
        "textinputhost.exe",
        "runtimebroker.exe",
        "dwm.exe",
        "taskmgr.exe",
        "systemsettings.exe",
        "cmd.exe",
        "powershell.exe",
        "windowsterminal.exe",
        "code.exe",
        "cursor.exe",
        "devenv.exe",
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "opera.exe",
        "brave.exe",
        "notepad.exe",
        "notepad++.exe",
        "winword.exe",
        "excel.exe",
        "powerpnt.exe",
        "outlook.exe",
        "spotify.exe",
        "mosickwhile.exe",
    ];
    BLOCK.iter().any(|b| exe == *b)
}
