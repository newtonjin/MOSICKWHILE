//! Predictive look from mouse deltas + absolute cursor as gaze assist.
//!
//! Absolute cursor (multi-monitor aware) helps FOV when face is off-center
//! (second screen) or detection is weak. Deltas still drive intensity.

use std::time::Instant;

use crate::sensing::display_geom::{cursor_on_display, DisplayRect};

#[cfg(windows)]
use windows::Win32::Foundation::POINT;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Max cursor travel (px) accepted as continuous look in one tick.
const MAX_LOOK_DELTA_PX: f32 = 120.0;

#[derive(Debug, Clone, Default)]
pub struct InputPrediction {
    pub boost: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub teleport_rejected: bool,
    /// Cursor on active overlay monitor, normalized [0,1].
    pub cursor_x: f32,
    pub cursor_y: f32,
    /// True when cursor is on the overlay's monitor (not a neighbor).
    pub cursor_on_monitor: bool,
}

pub struct InputPredictor {
    last_pos: Option<(i32, i32)>,
    last_tick: Instant,
    yaw: f32,
    pitch: f32,
    boost: f32,
    cursor_x: f32,
    cursor_y: f32,
}

impl Default for InputPredictor {
    fn default() -> Self {
        Self {
            last_pos: None,
            last_tick: Instant::now(),
            yaw: 0.0,
            pitch: 0.0,
            boost: 0.0,
            cursor_x: 0.5,
            cursor_y: 0.5,
        }
    }
}

impl InputPredictor {
    pub fn sample(&mut self, dt: f32, overlay: DisplayRect) -> InputPrediction {
        let (cx, cy, on_mon) = cursor_on_display(overlay);
        // Smooth absolute cursor (teleports between monitors get soft catch-up).
        let c_a = if on_mon { 0.35 } else { 0.2 };
        self.cursor_x += (cx - self.cursor_x) * c_a;
        self.cursor_y += (cy - self.cursor_y) * c_a;

        let (dx, dy, teleported) = self.read_delta();

        if teleported {
            let release = 1.0 - (-dt / 0.12).exp();
            self.boost *= 1.0 - release;
            self.yaw *= 1.0 - release;
            self.pitch *= 1.0 - release;
            return InputPrediction {
                boost: self.boost.clamp(0.0, 1.0),
                yaw: self.yaw.clamp(-1.0, 1.0),
                pitch: self.pitch.clamp(-1.0, 1.0),
                teleport_rejected: true,
                cursor_x: self.cursor_x.clamp(0.0, 1.0),
                cursor_y: self.cursor_y.clamp(0.0, 1.0),
                cursor_on_monitor: on_mon,
            };
        }

        let scale = (36.0 * (dt * 60.0).max(0.25)).max(1.0);
        let raw_yaw = (dx / scale).clamp(-1.2, 1.2);
        let raw_pitch = (dy / scale).clamp(-1.2, 1.2);
        let raw_mag = (raw_yaw * raw_yaw + raw_pitch * raw_pitch)
            .sqrt()
            .clamp(0.0, 1.0);
        let shaped = (raw_mag.powf(1.15)).clamp(0.0, 1.0);

        let attack = 1.0 - (-dt / 0.055).exp();
        let release = 1.0 - (-dt / 0.22).exp();
        if shaped > self.boost {
            self.boost += (shaped - self.boost) * attack;
        } else {
            self.boost += (shaped - self.boost) * release;
        }

        let yaw_alpha = if raw_yaw.abs() > self.yaw.abs() {
            attack
        } else {
            release
        };
        self.yaw += (raw_yaw.clamp(-1.0, 1.0) - self.yaw) * yaw_alpha;
        self.pitch += (raw_pitch.clamp(-1.0, 1.0) - self.pitch) * yaw_alpha;

        InputPrediction {
            boost: self.boost.clamp(0.0, 1.0),
            yaw: self.yaw.clamp(-1.0, 1.0),
            pitch: self.pitch.clamp(-1.0, 1.0),
            teleport_rejected: false,
            cursor_x: self.cursor_x.clamp(0.0, 1.0),
            cursor_y: self.cursor_y.clamp(0.0, 1.0),
            cursor_on_monitor: on_mon,
        }
    }

    fn read_delta(&mut self) -> (f32, f32, bool) {
        #[cfg(windows)]
        {
            let mut pt = POINT::default();
            if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                let cur = (pt.x, pt.y);
                let (dx, dy, teleported) = if let Some((lx, ly)) = self.last_pos {
                    let dx = (cur.0 - lx) as f32;
                    let dy = (cur.1 - ly) as f32;
                    let dist = (dx * dx + dy * dy).sqrt();
                    // Larger allowance — monitor hops are handled via absolute cursor.
                    if dist > MAX_LOOK_DELTA_PX {
                        (0.0, 0.0, true)
                    } else {
                        (dx, dy, false)
                    }
                } else {
                    (0.0, 0.0, false)
                };
                self.last_pos = Some(cur);
                self.last_tick = Instant::now();
                return (dx, dy, teleported);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = self.last_tick;
        }
        (0.0, 0.0, false)
    }
}
