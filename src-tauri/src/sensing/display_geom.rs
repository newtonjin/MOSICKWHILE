//! Multi-monitor geometry helpers (overlay rect + cursor → local [0,1]).

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

#[derive(Debug, Clone, Copy)]
pub struct DisplayRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Default for DisplayRect {
    fn default() -> Self {
        let (w, h) = primary_size();
        Self { x: 0, y: 0, w, h }
    }
}

impl DisplayRect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.w as i32
            && py < self.y + self.h as i32
    }

    /// Normalize a desktop point into this rect [0,1], clamped.
    pub fn normalize(&self, px: i32, py: i32) -> (f32, f32) {
        let nx = (px - self.x) as f32 / self.w.max(1) as f32;
        let ny = (py - self.y) as f32 / self.h.max(1) as f32;
        (nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0))
    }
}

/// Shared overlay / active-monitor rectangle (updated by overlay fit).
pub struct SharedDisplay {
    x: AtomicI32,
    y: AtomicI32,
    w: AtomicU32,
    h: AtomicU32,
}

impl SharedDisplay {
    pub fn new() -> Self {
        let d = DisplayRect::default();
        Self {
            x: AtomicI32::new(d.x),
            y: AtomicI32::new(d.y),
            w: AtomicU32::new(d.w),
            h: AtomicU32::new(d.h),
        }
    }

    pub fn set(&self, r: DisplayRect) {
        if r.w < 640 || r.h < 480 {
            return;
        }
        self.x.store(r.x, Ordering::Relaxed);
        self.y.store(r.y, Ordering::Relaxed);
        self.w.store(r.w, Ordering::Relaxed);
        self.h.store(r.h, Ordering::Relaxed);
    }

    pub fn get(&self) -> DisplayRect {
        DisplayRect {
            x: self.x.load(Ordering::Relaxed),
            y: self.y.load(Ordering::Relaxed),
            w: self.w.load(Ordering::Relaxed).max(640),
            h: self.h.load(Ordering::Relaxed).max(480),
        }
    }
}

fn primary_size() -> (u32, u32) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        let w = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(640) as u32;
        let h = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(480) as u32;
        (w, h)
    }
    #[cfg(not(windows))]
    {
        (1920, 1080)
    }
}

/// Monitor of the foreground window (game pin — dual/multi-monitor).
/// Skips MOSICKWHILE itself so alt-tabbing to the panel doesn't yank the FOV.
pub fn monitor_of_foreground() -> Option<DisplayRect> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{HWND, RECT};
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        };

        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.0.is_null() || IsIconic(hwnd).as_bool() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == std::process::id() {
                return None;
            }

            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if mon.is_invalid() {
                return None;
            }
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                rcMonitor: RECT::default(),
                rcWork: RECT::default(),
                dwFlags: 0,
            };
            if !GetMonitorInfoW(mon, &mut info).as_bool() {
                return None;
            }
            let r = info.rcMonitor;
            let w = (r.right - r.left).max(640) as u32;
            let h = (r.bottom - r.top).max(480) as u32;
            Some(DisplayRect {
                x: r.left,
                y: r.top,
                w,
                h,
            })
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Monitor under the cursor (dual/multi-monitor aware).
pub fn monitor_under_cursor() -> Option<DisplayRect> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

        let mut pt = POINT::default();
        unsafe { GetCursorPos(&mut pt) }.ok()?;
        let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
        if mon.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        if !unsafe { GetMonitorInfoW(mon, &mut info) }.as_bool() {
            return None;
        }
        let r = info.rcMonitor;
        let w = (r.right - r.left).max(640) as u32;
        let h = (r.bottom - r.top).max(480) as u32;
        return Some(DisplayRect {
            x: r.left,
            y: r.top,
            w,
            h,
        });
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Cursor position normalized to `overlay` rect. `on_monitor` false if on another screen.
pub fn cursor_on_display(overlay: DisplayRect) -> (f32, f32, bool) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            return (0.5, 0.5, false);
        }
        let on = overlay.contains(pt.x, pt.y);
        if on {
            let (nx, ny) = overlay.normalize(pt.x, pt.y);
            return (nx, ny, true);
        }
        // Off-monitor: project to nearest edge (gaze toward that side of this screen).
        let (nx, ny) = overlay.normalize(pt.x, pt.y);
        return (nx, ny, false);
    }
    #[cfg(not(windows))]
    {
        let _ = overlay;
        (0.5, 0.5, false)
    }
}
