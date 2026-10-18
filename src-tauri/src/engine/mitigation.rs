use serde::Serialize;

use super::config::{AppConfig, RestFrameStyle};
use super::gaming::GamingState;
use super::metrics::LiveMetrics;
use super::motion::MotionReading;
use crate::sensing::{FaceSample, OneEuro};

/// Frame state pushed to the overlay each tick.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayFrame {
    pub enabled: bool,
    pub motion: f32,
    pub vignette_amount: f32,
    pub vignette_softness: f32,
    pub bias_x: f32,
    pub bias_y: f32,
    pub ground_clear: f32,
    pub rest_frame_style: RestFrameStyle,
    pub rest_frame_opacity: f32,
    pub blur_amount: f32,
    /// Earth-horizon tilt in radians (counter-roll).
    pub horizon_roll: f32,
    /// Foveated clear-zone center (0–1 screen space).
    pub clear_x: f32,
    pub clear_y: f32,
    pub clear_radius: f32,
    pub foveated_clear: bool,
    pub virtual_nose: bool,
    pub flow_dampen: f32,
    /// Side-restrictor strength 0–1.
    pub side_mask: f32,
    /// -1 darken left (look right), +1 darken right (look left).
    pub side_sign: f32,
    pub real_blur: bool,
    pub blur_px: f32,
    pub face_ok: bool,
    pub face_present: bool,
    pub face_status: String,
    pub metrics: LiveMetrics,
}

pub struct MitigationEngine {
    smoothed_motion: f32,
    vignette_out: f32,
    blur_out: f32,
    rest_out: f32,
    smoothed_bias_x: f32,
    smoothed_bias_y: f32,
    smoothed_roll: f32,
    clear_x: f32,
    clear_y: f32,
    comfort_boost: f32,
    high_motion_secs: f32,
    side_mask: f32,
    side_sign: f32,
    yaw_smooth: f32,
    clear_fx: OneEuro,
    clear_fy: OneEuro,
    clear_inited: bool,
    /// Gaming: how hard clear-zone is pinned to screen center (turns/glances).
    center_pin: f32,
    prev_gaze_x: f32,
    prev_gaze_y: f32,
}

impl Default for MitigationEngine {
    fn default() -> Self {
        Self {
            smoothed_motion: 0.0,
            vignette_out: 0.0,
            blur_out: 0.0,
            rest_out: 0.0,
            smoothed_bias_x: 0.0,
            smoothed_bias_y: 0.0,
            smoothed_roll: 0.0,
            clear_x: 0.5,
            clear_y: 0.5,
            comfort_boost: 0.0,
            high_motion_secs: 0.0,
            side_mask: 0.0,
            side_sign: -1.0,
            yaw_smooth: 0.0,
            clear_fx: OneEuro::clear_zone(),
            clear_fy: OneEuro::clear_zone(),
            clear_inited: false,
            center_pin: 0.0,
            prev_gaze_x: 0.5,
            prev_gaze_y: 0.5,
        }
    }
}

impl MitigationEngine {
    pub fn tick(
        &mut self,
        reading: &MotionReading,
        cfg: &AppConfig,
        dt: f32,
        metrics_seed: LiveMetrics,
        face: &FaceSample,
        gaming: GamingState,
    ) -> OverlayFrame {
        let mut metrics = metrics_seed;
        let gaming_on = cfg.gaming_mode_auto && gaming.active;
        metrics.gaming_active = gaming_on;
        metrics.gaming_score = gaming.score;

        if !cfg.enabled {
            self.decay_outputs(dt, cfg);
            metrics.motion = self.smoothed_motion;
            metrics.comfort_index = 1.0 - self.smoothed_motion;
            metrics.fov_restriction = self.vignette_out;
            metrics.pupil_quality = face.pupil_quality;
            metrics.pupil_agreement = face.pupil_agreement;
            metrics.pupil_lock_rate = face.pupil_lock_rate;
            metrics.pupil_jitter = face.pupil_jitter;
            return self.pack(cfg, false, metrics, face);
        }

        let mut target = reading.raw_drive.clamp(0.0, 1.0);

        // Predictive FOV: mouse onset leads visual flow — blend and attack faster.
        if cfg.predictive_fov {
            let pred = reading.pred.boost;
            if pred > target {
                target = (target * 0.4 + pred * 0.85).clamp(0.0, 1.0);
            } else {
                target = target.max(pred * 0.55);
            }
        }

        if target < cfg.motion_deadzone {
            target = 0.0;
        } else {
            target = ((target - cfg.motion_deadzone) / (1.0 - cfg.motion_deadzone)).clamp(0.0, 1.0);
        }

        if cfg.face_sensing_enabled && face.ok && !face.present {
            target *= 0.35;
        }

        // Gaming: FOV restriction rises much faster (turns must not lag).
        let pred_lead = cfg.predictive_fov && reading.pred.boost > reading.raw_drive + 0.08;
        let attack_scale = if gaming_on { 0.38 } else { 1.0 };
        let release_scale = if gaming_on { 0.72 } else { 1.0 };
        let tau = if target > self.smoothed_motion {
            if pred_lead || gaming_on {
                (cfg.attack_secs * 0.35 * attack_scale).max(0.045)
            } else {
                (cfg.attack_secs * attack_scale).max(0.08)
            }
        } else {
            (cfg.release_secs * release_scale).max(0.22)
        };
        let alpha = 1.0 - (-dt / tau).exp();
        self.smoothed_motion += (target - self.smoothed_motion) * alpha;

        if cfg.comfort_auto_ramp {
            if self.smoothed_motion > 0.55 {
                self.high_motion_secs += dt;
            } else {
                self.high_motion_secs = (self.high_motion_secs - dt * 0.5).max(0.0);
            }
            let ramp = (self.high_motion_secs / 12.0).clamp(0.0, 1.0) * 0.18;
            self.comfort_boost += (ramp - self.comfort_boost) * (1.0 - (-dt / 1.2).exp());
        } else {
            self.comfort_boost *= 0.95;
        }

        let intensity = (cfg.intensity + self.comfort_boost).clamp(0.0, 1.0);

        // Yaw for side-restrictor only — ignore teleports / zero mouse boost.
        let yaw_raw = if reading.pred.teleport_rejected || reading.pred.boost < 0.04 {
            reading.flow.flow_x * 0.65
        } else {
            reading.pred.yaw
        };
        let yaw_alpha = 1.0 - (-dt / 0.14).exp();
        self.yaw_smooth += (yaw_raw.clamp(-1.0, 1.0) - self.yaw_smooth) * yaw_alpha;

        // —— Clear zone = pupils (primary) ⊕ weak mouse assist ——
        let mut face_cx = 0.5;
        let mut face_cy = 0.5;
        let mut face_w = 0.0;
        if cfg.face_sensing_enabled && face.ok && (face.present || face.confidence > 0.1) {
            // Pupils locked → high weight; face-only → medium.
            let base = if face.pupils_ok {
                0.78 + face.pupil_confidence * 0.18
            } else {
                face.confidence * 0.55
            };
            face_w = (base * if face.present { 1.0 } else { 0.55 }).clamp(0.0, 1.0);
            face_cx = face.face_x;
            face_cy = face.face_y;
        }

        // Mouse assist — almost off in gaming (relative look / hidden cursor).
        let mouse_cx = reading.pred.cursor_x;
        let mouse_cy = reading.pred.cursor_y;
        let mut mouse_w = if gaming_on {
            0.0
        } else if face.pupils_ok {
            if reading.pred.cursor_on_monitor {
                0.08 + (1.0 - face_w) * 0.04
            } else {
                0.10 + (1.0 - face_w) * 0.05
            }
        } else if reading.pred.cursor_on_monitor {
            0.22 + (1.0 - face_w) * 0.28
        } else {
            0.16 + (1.0 - face_w) * 0.22
        };
        if !gaming_on && !face.pupils_ok && (!face.calibrated || face_w < 0.35) {
            mouse_w = mouse_w.max(0.45);
        }
        let mouse_cap = if gaming_on {
            0.0
        } else if face.pupils_ok {
            0.12
        } else {
            0.55
        };
        mouse_w = mouse_w.clamp(0.0, mouse_cap);

        let rem = (1.0 - face_w - mouse_w).max(0.0);
        let mut target_cx = face_cx * face_w + mouse_cx * mouse_w + 0.5 * rem;
        let mut target_cy = face_cy * face_w + mouse_cy * mouse_w + 0.5 * rem;

        // Gaming: rapid turns / glance saccades → keep clear zone at screen center
        // so visibility stays where the crosshair is (no cursor required).
        if gaming_on {
            let gaze_step = (face_cx - self.prev_gaze_x).hypot(face_cy - self.prev_gaze_y);
            let turn = reading
                .pred
                .boost
                .max(self.yaw_smooth.abs())
                .max(reading.flow.rotational);
            let rapid = turn > 0.14 || gaze_step > 0.045;
            let pin_target = if rapid {
                (0.72 + turn * 0.35 + gaze_step * 2.5).clamp(0.72, 0.97)
            } else {
                0.0
            };
            let pin_a = if pin_target > self.center_pin {
                1.0 - (-dt / 0.06).exp() // snap on
            } else {
                1.0 - (-dt / 0.28).exp() // brief hold then release
            };
            self.center_pin += (pin_target - self.center_pin) * pin_a;
            let p = self.center_pin;
            target_cx = target_cx * (1.0 - p) + 0.5 * p;
            target_cy = target_cy * (1.0 - p) + 0.5 * p;
        } else {
            self.center_pin *= 0.9;
        }
        self.prev_gaze_x = face_cx;
        self.prev_gaze_y = face_cy;

        // Never let scene flow shove the clear zone when pupils drive gaze
        // (video optical flow was a major FOV jitter source).
        if cfg.asymmetric_fov && face_w < 0.4 && !face.pupils_ok {
            target_cx += reading.flow.flow_x * 0.03;
            target_cy += reading.flow.flow_y * 0.02;
        }

        // Side restrictor: darken high-flow side only — do NOT relocate clear zone.
        if cfg.side_restrictor && !reading.pred.teleport_rejected {
            let yaw_abs = self.yaw_smooth.abs();
            let side_target = (yaw_abs * 1.15).clamp(0.0, 1.0) * self.smoothed_motion;
            // Soft side mask — no hard edge pops.
            self.side_mask += (side_target - self.side_mask) * (1.0 - (-dt / 0.22).exp());
            if yaw_abs > 0.12 {
                self.side_sign = -self.yaw_smooth.signum();
            }
        } else {
            self.side_mask *= 0.92;
        }

        // Allow true corners (chat bottom-right, etc.).
        target_cx = target_cx.clamp(0.04, 0.96);
        target_cy = target_cy.clamp(0.04, 0.96);

        let bias_alpha = 1.0 - (-dt / 0.22).exp();
        let target_bx = (target_cx - 0.5) * 2.0;
        let target_by = (target_cy - 0.5) * 2.0;
        self.smoothed_bias_x += (target_bx - self.smoothed_bias_x) * bias_alpha;
        self.smoothed_bias_y += (target_by - self.smoothed_bias_y) * bias_alpha;

        // Earth horizon: ONLY real head tilt. Never optical-flow roll (video
        // content made the line wobble). Deadzone + slow settle = rock solid.
        let mut face_roll = if cfg.face_sensing_enabled && face.present {
            face.head_roll
        } else {
            0.0
        };
        // Ignore micro pupil/lid wobble (~1°).
        const ROLL_DEAD: f32 = 0.018;
        if face_roll.abs() < ROLL_DEAD {
            face_roll = 0.0;
        } else {
            face_roll = face_roll.signum() * (face_roll.abs() - ROLL_DEAD);
        }
        // Soft clamp — large noisy spikes shouldn't tip the horizon hard.
        let roll_target = face_roll.clamp(-0.28, 0.28);
        let roll_alpha = 1.0 - (-dt / 1.15).exp(); // ~1.15s settle
        self.smoothed_roll += (roll_target - self.smoothed_roll) * roll_alpha;

        // Anti-jitter: deadzone + video hold before One Euro.
        // Watching video → high peripheral flow but gaze barely moves; hold FOV.
        if self.clear_inited {
            let dx = target_cx - self.clear_x;
            let dy = target_cy - self.clear_y;
            let dist = (dx * dx + dy * dy).sqrt();
            // ~1.6% of screen — kills pupil/compression micro-wobble.
            const DEAD: f32 = 0.016;
            if dist < DEAD {
                target_cx = self.clear_x;
                target_cy = self.clear_y;
            } else {
                // Only the excess beyond the deadzone pulls the zone.
                let scale = ((dist - DEAD) / dist).clamp(0.0, 1.0);
                target_cx = self.clear_x + dx * scale;
                target_cy = self.clear_y + dy * scale;
            }

            let peri = reading.flow.peripheral_flow;
            let gaze_step = dist;
            if peri > 0.22 && gaze_step < 0.045 {
                // Content motion ≠ eye motion: freeze clear zone hard.
                let hold = 0.88;
                target_cx = self.clear_x * hold + target_cx * (1.0 - hold);
                target_cy = self.clear_y * hold + target_cy * (1.0 - hold);
            } else if face.pupils_ok && face.pupil_jitter < 0.012 && gaze_step < 0.03 {
                // High-quality still gaze: extra sticky.
                let hold = 0.72;
                target_cx = self.clear_x * hold + target_cx * (1.0 - hold);
                target_cy = self.clear_y * hold + target_cy * (1.0 - hold);
            }
        }

        // One Euro clear-zone: tracks saccades, calm when gaze rests.
        if !self.clear_inited {
            self.clear_fx.reset(target_cx);
            self.clear_fy.reset(target_cy);
            self.clear_x = target_cx;
            self.clear_y = target_cy;
            self.clear_inited = true;
        } else {
            self.clear_x = self.clear_fx.filter(target_cx, dt).clamp(0.04, 0.96);
            self.clear_y = self.clear_fy.filter(target_cy, dt).clamp(0.04, 0.96);
        }

        let drive = (self.smoothed_motion * intensity).clamp(0.0, 1.0);
        let curved = drive.powf(1.25);

        let mut vignette_target = if cfg.vignette_enabled {
            curved * cfg.vignette_strength
        } else {
            0.0
        };
        // Predictive: allow a brief overshoot so FOV is already up when flow arrives.
        if cfg.predictive_fov && pred_lead {
            vignette_target = (vignette_target + reading.pred.boost * 0.18 * cfg.vignette_strength)
                .clamp(0.0, 1.0);
        }

        let slew_up = if gaming_on {
            cfg.vignette_slew_up * 2.4
        } else if pred_lead {
            cfg.vignette_slew_up * 1.8
        } else {
            cfg.vignette_slew_up
        };
        let max_up = slew_up * dt;
        let max_down = cfg.vignette_slew_down * dt;
        let dv = vignette_target - self.vignette_out;
        self.vignette_out += dv.clamp(-max_down, max_up);

        let blur_target = if cfg.peripheral_blur_enabled
            || cfg.foveated_clear_enabled
            || cfg.real_peripheral_blur
        {
            curved
                * if cfg.peripheral_blur_enabled || cfg.real_peripheral_blur {
                    cfg.peripheral_blur_strength.max(0.35)
                } else {
                    0.4
                }
        } else {
            0.0
        };
        self.blur_out += (blur_target - self.blur_out) * (1.0 - (-dt / 0.45).exp());

        let rest_target = if cfg.rest_frame_enabled {
            // Earth horizon stays near-constant opacity (pulsing felt like "floating").
            if matches!(cfg.rest_frame_style, RestFrameStyle::EarthHorizon) {
                cfg.rest_frame_opacity.clamp(0.0, 1.0)
            } else {
                let base = cfg.rest_frame_opacity * 0.72;
                let boost = cfg.rest_frame_opacity * 0.28 * curved;
                (base + boost).clamp(0.0, 1.0)
            }
        } else {
            0.0
        };
        let rest_tau = if matches!(cfg.rest_frame_style, RestFrameStyle::EarthHorizon) {
            0.85
        } else {
            0.55
        };
        self.rest_out += (rest_target - self.rest_out) * (1.0 - (-dt / rest_tau).exp());

        let residual =
            (self.smoothed_motion * (1.0 - self.vignette_out * 0.85)).clamp(0.0, 1.0);
        metrics.motion = self.smoothed_motion;
        metrics.fov_restriction = self.vignette_out;
        metrics.comfort_index = (1.0 - residual).clamp(0.0, 1.0);
        metrics.pupil_quality = face.pupil_quality;
        metrics.pupil_agreement = face.pupil_agreement;
        metrics.pupil_lock_rate = face.pupil_lock_rate;
        metrics.pupil_jitter = face.pupil_jitter;

        self.pack(cfg, true, metrics, face)
    }

    fn pack(
        &self,
        cfg: &AppConfig,
        enabled: bool,
        metrics: LiveMetrics,
        face: &FaceSample,
    ) -> OverlayFrame {
        let ground_clear = if cfg.ground_visible {
            0.22 + 0.18 * (1.0 - self.smoothed_motion)
        } else {
            0.0
        };
        let rest_style = if cfg.rest_frame_enabled {
            cfg.rest_frame_style
        } else {
            RestFrameStyle::None
        };
        // Gentle counter-roll only — mostly level, never fluttery.
        let horizon_roll = if matches!(rest_style, RestFrameStyle::EarthHorizon) {
            (-self.smoothed_roll * 0.55).clamp(-0.22, 0.22)
        } else {
            0.0
        };
        let flow_dampen = if cfg.peripheral_flow_dampen {
            self.blur_out * 0.85
        } else {
            0.0
        };
        // Map blur amount → CSS backdrop-filter px (perceptually useful range).
        let blur_px = if cfg.real_peripheral_blur {
            (self.blur_out * 18.0).clamp(0.0, 22.0)
        } else {
            0.0
        };

        OverlayFrame {
            enabled,
            motion: self.smoothed_motion,
            vignette_amount: self.vignette_out,
            vignette_softness: cfg.vignette_softness,
            bias_x: self.smoothed_bias_x,
            bias_y: self.smoothed_bias_y,
            ground_clear,
            rest_frame_style: rest_style,
            rest_frame_opacity: if enabled { self.rest_out } else { 0.0 },
            blur_amount: self.blur_out,
            horizon_roll,
            clear_x: self.clear_x,
            clear_y: self.clear_y,
            // Slightly smaller clear window near screen edges so corners feel “reached”.
            clear_radius: {
                let edge = (self.clear_x - 0.5)
                    .abs()
                    .max((self.clear_y - 0.5).abs())
                    .clamp(0.0, 0.5);
                let shrink = edge * 0.22;
                let boost = if metrics.gaming_active { 0.04 } else { 0.0 };
                (cfg.foveated_clear_radius - shrink + boost).clamp(0.18, 0.62)
            },
            foveated_clear: cfg.foveated_clear_enabled,
            virtual_nose: matches!(rest_style, RestFrameStyle::VirtualNose),
            flow_dampen,
            side_mask: if cfg.side_restrictor { self.side_mask } else { 0.0 },
            side_sign: self.side_sign,
            real_blur: cfg.real_peripheral_blur,
            blur_px,
            face_ok: face.ok,
            face_present: face.present,
            face_status: face.status.clone(),
            metrics,
        }
    }

    fn decay_outputs(&mut self, dt: f32, cfg: &AppConfig) {
        self.smoothed_motion =
            (self.smoothed_motion - dt / cfg.release_secs.max(0.35)).max(0.0);
        self.vignette_out = (self.vignette_out - cfg.vignette_slew_down * dt).max(0.0);
        self.blur_out = (self.blur_out - dt / 0.45).max(0.0);
        self.rest_out = (self.rest_out - dt / 0.55).max(0.0);
        self.comfort_boost *= 0.9;
        self.side_mask = (self.side_mask - dt * 2.0).max(0.0);
    }
}
