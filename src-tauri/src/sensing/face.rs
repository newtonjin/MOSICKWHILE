//! Adaptive high-precision webcam gaze proxy.
//!
//! - Auto-calibrates neutral pose
//! - Learns each user's head/eye range online (so corners become reachable)
//! - Asymmetric down-gain for lower UI (chat) — webcam-above-monitor geometry
//! - Scales gains by live display aspect ratio / size

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::one_euro::OneEuro;
use super::pupils::{detect_pupil_gaze, PupilGaze};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FaceSample {
    pub ok: bool,
    pub status: String,
    /// Gaze in screen space [0,1] — driven primarily by pupils.
    pub face_x: f32,
    pub face_y: f32,
    pub face_size: f32,
    pub present: bool,
    pub confidence: f32,
    pub head_roll: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub calibrated: bool,
    /// True when both/one pupil locked — gaze is deterministic.
    pub pupils_ok: bool,
    pub pupil_confidence: f32,
    /// Composite pupil quality 0–1 (contrast × circularity × agreement).
    pub pupil_quality: f32,
    /// Dual-eye gaze agreement 0–1 (live accuracy proxy).
    pub pupil_agreement: f32,
    /// EMA of frames with pupil lock (session precision proxy).
    pub pupil_lock_rate: f32,
    /// Gaze jitter when still (lower = more stable); screen units.
    pub pupil_jitter: f32,
}

pub struct FaceSensor {
    latest: Mutex<FaceSample>,
    running: AtomicBool,
    enabled: AtomicBool,
    /// MediaPipe (frontend) owns the webcam — skip Seeta/nokhwa loop.
    external: AtomicBool,
    last_external: Mutex<Instant>,
    /// Physical display size (pixels) for aspect-aware gains.
    display_w: AtomicU32,
    display_h: AtomicU32,
}

impl FaceSensor {
    pub fn new() -> Arc<Self> {
        let (w, h) = query_display_size();
        Arc::new(Self {
            latest: Mutex::new(FaceSample {
                status: "idle".into(),
                face_x: 0.5,
                face_y: 0.5,
                ..Default::default()
            }),
            running: AtomicBool::new(false),
            enabled: AtomicBool::new(false),
            external: AtomicBool::new(false),
            last_external: Mutex::new(Instant::now() - Duration::from_secs(30)),
            display_w: AtomicU32::new(w),
            display_h: AtomicU32::new(h),
        })
    }

    pub fn latest(&self) -> FaceSample {
        self.latest.lock().clone()
    }

    /// Refresh display geometry (call when overlay fits a monitor).
    pub fn set_display_size(&self, w: u32, h: u32) {
        if w >= 640 && h >= 480 {
            self.display_w.store(w, Ordering::Relaxed);
            self.display_h.store(h, Ordering::Relaxed);
        }
    }

    /// Frontend MediaPipe driver: ingest gaze samples and keep Seeta camera off.
    pub fn set_external(&self, on: bool) {
        self.external.store(on, Ordering::SeqCst);
        if on {
            // Stop legacy webcam loop so only one consumer holds the camera.
            self.running.store(false, Ordering::SeqCst);
        }
    }

    pub fn ingest(&self, sample: FaceSample) {
        self.external.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        *self.last_external.lock() = Instant::now();
        *self.latest.lock() = sample;
    }

    pub fn set_enabled(self: &Arc<Self>, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
        if !on {
            self.running.store(false, Ordering::SeqCst);
            return;
        }
        // Prefer MediaPipe external path; only fall back to Seeta if no
        // external samples arrived recently.
        if self.external.load(Ordering::SeqCst) {
            return;
        }
        let stale = self
            .last_external
            .lock()
            .elapsed()
            > Duration::from_secs(4);
        if stale && !self.running.load(Ordering::SeqCst) {
            self.start();
        }
    }

    pub fn stop(&self) {
        self.enabled.store(false, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        self.external.store(false, Ordering::SeqCst);
    }

    fn start(self: &Arc<Self>) {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let this = Arc::clone(self);
        std::thread::spawn(move || {
            if let Err(e) = run_loop(&this) {
                *this.latest.lock() = FaceSample {
                    ok: false,
                    status: e,
                    face_x: 0.5,
                    face_y: 0.5,
                    ..Default::default()
                };
            }
            this.running.store(false, Ordering::SeqCst);
        });
    }
}

fn query_display_size() -> (u32, u32) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        let w = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(640) as u32;
        let h = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(480) as u32;
        return (w, h);
    }
    #[cfg(not(windows))]
    {
        (1920, 1080)
    }
}

fn model_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join("resources").join("seeta_fd_frontal_v1.0.bin"),
        dir.join("seeta_fd_frontal_v1.0.bin"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("seeta_fd_frontal_v1.0.bin"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}


/// Soft peak tracker — expands to cover new extremes, slowly forgets.
#[derive(Clone, Copy)]
struct SoftPeak {
    v: f32,
}

impl SoftPeak {
    fn new(init: f32) -> Self {
        Self { v: init }
    }

    fn observe(&mut self, sample: f32, attack: f32, release: f32) {
        let s = sample.abs().max(0.0);
        if s > self.v {
            self.v += (s - self.v) * attack;
        } else {
            self.v += (s - self.v) * release;
        }
        // Dual-monitor head turns can be large in camera space.
        self.v = self.v.clamp(0.04, 0.95);
    }
}

struct AdaptiveMapper {
    // Neutral
    ready: bool,
    frames: u32,
    sum_x: f32,
    sum_y: f32,
    sum_size: f32,
    sum_roll: f32,
    n: u32,
    cx: f32,
    cy: f32,
    size: f32,
    roll: f32,

    // Personal range (learned)
    peak_hx: SoftPeak,
    peak_hy_up: SoftPeak,
    peak_hy_down: SoftPeak,
    peak_ex: SoftPeak,
    peak_ey_up: SoftPeak,
    peak_ey_down: SoftPeak,
    peak_lean_down: SoftPeak,

    // Display
    aspect: f32,
    size_factor: f32,
}

impl AdaptiveMapper {
    fn new(display_w: u32, display_h: u32) -> Self {
        let aspect = (display_w as f32 / display_h as f32).max(0.5);
        // Normalize vs 1920×1080 reference; big / ultrawide panels need more X.
        let diag = ((display_w as f32).hypot(display_h as f32)) / 2203.0; // ~1080p diag
        Self {
            ready: false,
            frames: 0,
            sum_x: 0.0,
            sum_y: 0.0,
            sum_size: 0.0,
            sum_roll: 0.0,
            n: 0,
            cx: 0.5,
            cy: 0.42,
            size: 0.28,
            roll: 0.0,
            // Higher initial peaks = gentler gain until range is truly learned.
            peak_hx: SoftPeak::new(0.20),
            peak_hy_up: SoftPeak::new(0.16),
            peak_hy_down: SoftPeak::new(0.16),
            peak_ex: SoftPeak::new(0.55),
            peak_ey_up: SoftPeak::new(0.45),
            peak_ey_down: SoftPeak::new(0.50),
            peak_lean_down: SoftPeak::new(0.12),
            aspect,
            size_factor: diag.clamp(0.75, 1.6),
        }
    }

    fn update_display(&mut self, w: u32, h: u32) {
        if w < 640 || h < 480 {
            return;
        }
        self.aspect = w as f32 / h as f32;
        let diag = ((w as f32).hypot(h as f32)) / 2203.0;
        self.size_factor = diag.clamp(0.75, 1.6);
    }

    fn observe_neutral(&mut self, cam_x: f32, cam_y: f32, size: f32, roll: f32) {
        if self.ready {
            return;
        }
        self.frames += 1;
        if self.frames < 8 {
            return;
        }
        // Accept ANY stable face pose as neutral — dual-monitor / off-center webcam
        // means the user is rarely at camera center when looking at the main screen.
        self.sum_x += cam_x;
        self.sum_y += cam_y;
        self.sum_size += size;
        self.sum_roll += roll;
        self.n += 1;

        if self.n >= 20 {
            self.cx = self.sum_x / self.n as f32;
            self.cy = self.sum_y / self.n as f32;
            self.size = self.sum_size / self.n as f32;
            self.roll = self.sum_roll / self.n as f32;
            self.ready = true;
        } else if self.frames > 70 && self.n > 5 {
            self.cx = self.sum_x / self.n as f32;
            self.cy = self.sum_y / self.n as f32;
            self.size = self.sum_size / self.n as f32;
            self.roll = self.sum_roll / self.n as f32;
            self.ready = true;
        }
    }

    /// Map camera + pupils → screen gaze [0,1].
    /// Pupils are primary when locked; head pose is weak assist / fallback.
    fn map(
        &mut self,
        cam_x: f32,
        cam_y: f32,
        size: f32,
        pupil: PupilGaze,
    ) -> (f32, f32, f32, f32, f32) {
        let hx = cam_x - self.cx;
        let hy = cam_y - self.cy;
        let lean = size - self.size;
        let ex = if pupil.ok { pupil.gaze_x } else { 0.0 };
        let ey = if pupil.ok { pupil.gaze_y } else { 0.0 };

        // Slow peak learning — avoids FOV snaps from sudden remaps.
        self.peak_hx.observe(hx, 0.06, 0.001);
        if hy >= 0.0 {
            self.peak_hy_down.observe(hy, 0.07, 0.001);
        } else {
            self.peak_hy_up.observe(hy, 0.05, 0.001);
        }
        self.peak_ex.observe(ex, 0.08, 0.0012);
        if ey >= 0.0 {
            self.peak_ey_down.observe(ey, 0.09, 0.001);
        } else {
            self.peak_ey_up.observe(ey, 0.07, 0.0012);
        }
        if lean > 0.0 {
            self.peak_lean_down.observe(lean, 0.05, 0.001);
        }

        let aspect_x = if self.aspect >= 2.0 {
            1.28
        } else if self.aspect >= 1.7 {
            1.12
        } else if self.aspect < 1.4 {
            0.92
        } else {
            1.0
        };
        let aspect_y = if self.aspect >= 2.0 {
            0.95
        } else if self.aspect < 1.4 {
            1.18
        } else {
            1.05
        };
        let size_boost = 0.92 + self.size_factor * 0.12;

        let nx = (hx / self.peak_hx.v).clamp(-1.35, 1.35);
        let ny = if hy >= 0.0 {
            (hy / self.peak_hy_down.v).clamp(0.0, 1.4)
        } else {
            -((-hy) / self.peak_hy_up.v).clamp(0.0, 1.35)
        };
        let nex = (ex / self.peak_ex.v.max(0.08)).clamp(-1.45, 1.45);
        let ney = if ey >= 0.0 {
            (ey / self.peak_ey_down.v.max(0.08)).clamp(0.0, 1.55)
        } else {
            -((-ey) / self.peak_ey_up.v.max(0.08)).clamp(0.0, 1.35)
        };
        let nlean = if lean > 0.0 {
            (lean / self.peak_lean_down.v).clamp(0.0, 1.4)
        } else {
            (lean / 0.12).clamp(-0.8, 0.0)
        };

        let fx = nx.signum() * nx.abs().powf(0.92);
        let fy_up = if ny < 0.0 {
            -((-ny).powf(0.95))
        } else {
            0.0
        };
        let fy_down = if ny > 0.0 { ny.powf(0.82) } else { 0.0 };
        let fex = nex.signum() * nex.abs().powf(0.88);
        let fey_down = if ney > 0.0 { ney.powf(0.75) } else { 0.0 };
        let fey_up = if ney < 0.0 {
            -((-ney).powf(0.92))
        } else {
            0.0
        };

        let (sx, sy, yaw, pitch) = if pupil.ok {
            // —— Pupil-primary: where the iris looks IS the FOV center ——
            let conf = pupil.confidence.clamp(0.0, 1.0);
            let pw = 0.72 + conf * 0.22; // 0.72–0.94 pupil weight
            let hw = 1.0 - pw; // residual head for dual-monitor lean

            let down = (fey_down * pw + fy_down * hw * 0.55 + nlean.max(0.0) * 0.12).min(1.15);
            let up = (fey_up.abs() * pw + fy_up.abs() * hw * 0.5).min(1.1);

            // Extra tip gain at extreme pupil angles so split-pane / corner UI
            // is reachable without needing the mouse to drag the clear zone.
            let tip_x = fex.abs().powf(1.55);
            let tip_y = (fey_down.max(fey_up.abs())).powf(1.45);
            let sx = 0.5
                + (fex * pw * (0.52 + tip_x * 0.14) + fx * hw * 0.22)
                    * aspect_x
                    * size_boost;
            let sy = 0.5
                + (-up * (0.48 + tip_y * 0.10) * aspect_y)
                + (down * (0.50 + tip_y * 0.10) * aspect_y);
            let yaw = (nex * pw + nx * hw * 0.6).clamp(-1.0, 1.0);
            let pitch =
                (ney * pw + ny * hw * 0.55 + nlean.max(0.0) * 0.15).clamp(-1.0, 1.0);
            (sx, sy, yaw, pitch)
        } else {
            // Fallback: head pose only (no pupil lock).
            let down = (fy_down * 0.72 + nlean.max(0.0) * 0.28).min(1.0);
            let up = fy_up.abs().min(1.0);
            let sx = 0.5 + fx * 0.48 * aspect_x * size_boost;
            let sy = 0.5 + (-up * 0.40 * aspect_y) + (down * 0.44 * aspect_y);
            let yaw = nx.clamp(-1.0, 1.0);
            let pitch = (ny * 0.85 + nlean.max(0.0) * 0.25).clamp(-1.0, 1.0);
            (sx, sy, yaw, pitch)
        };

        (
            sx.clamp(0.02, 0.98),
            sy.clamp(0.02, 0.98),
            yaw,
            pitch,
            hy.max(0.0) + ey.max(0.0) * 0.55,
        )
    }
}

fn run_loop(sensor: &FaceSensor) -> Result<(), String> {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
    use nokhwa::Camera;
    use rustface::ImageData;

    let model = model_path().ok_or_else(|| "face model missing".to_string())?;
    let mut detector =
        rustface::create_detector(model.to_str().unwrap()).map_err(|e| format!("detector: {e}"))?;
    // More permissive for angled / off-center faces (multi-monitor head turns).
    detector.set_min_face_size(28);
    detector.set_score_thresh(1.15);
    detector.set_pyramid_scale_factor(0.68);
    detector.set_slide_window_step(2, 2);

    let format =
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
    let mut camera = Camera::new(CameraIndex::Index(0), format)
        .map_err(|e| format!("camera open failed: {e}"))?;
    camera
        .open_stream()
        .map_err(|e| format!("camera stream: {e}"))?;

    {
        let mut s = sensor.latest.lock();
        s.ok = true;
        s.status = "calibrating".into();
    }

    let mut last_hit = Instant::now() - Duration::from_secs(2);
    let mut last_filter = Instant::now();
    // Light pre-smooth kills single-frame detector spikes; One Euro does the rest.
    let mut pre_x = 0.5f32;
    let mut pre_y = 0.5f32;
    let mut filt_x = OneEuro::gaze();
    let mut filt_y = OneEuro::gaze();
    let mut out_x = 0.5f32;
    let mut out_y = 0.5f32;
    let mut sm_size = 0.25f32;
    let mut sm_conf = 0.0f32;
    let mut sm_roll = 0.0f32;
    let mut sm_yaw = 0.0f32;
    let mut sm_pitch = 0.0f32;
    let mut sm_pupil_conf = 0.0f32;
    let mut sm_pupil_q = 0.0f32;
    let mut sm_pupil_agree = 0.0f32;
    let mut sm_lock_rate = 0.0f32;
    let mut sm_jitter = 0.0f32;
    let mut prev_gaze_x = 0.5f32;
    let mut prev_gaze_y = 0.5f32;
    let mut blink_hold: Option<(f32, f32, Instant)> = None;
    let mut have_lock = false;
    let mut mapper = AdaptiveMapper::new(
        sensor.display_w.load(Ordering::Relaxed),
        sensor.display_h.load(Ordering::Relaxed),
    );
    let mut disp_tick = 0u32;

    while sensor.enabled.load(Ordering::Relaxed) {
        disp_tick = disp_tick.wrapping_add(1);
        if disp_tick % 45 == 0 {
            mapper.update_display(
                sensor.display_w.load(Ordering::Relaxed),
                sensor.display_h.load(Ordering::Relaxed),
            );
        }

        let frame = match camera.frame() {
            Ok(f) => f,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(28));
                continue;
            }
        };
        let decoded = match frame.decode_image::<RgbFormat>() {
            Ok(i) => i,
            Err(_) => continue,
        };
        let w = decoded.width();
        let h = decoded.height();
        // Cap analysis res — pupils don't need 1080p (big RAM/CPU win).
        let scale = (400.0 / w as f32).min(1.0);
        let dw = ((w as f32) * scale).max(1.0) as u32;
        let dh = ((h as f32) * scale).max(1.0) as u32;
        let small =
            image::imageops::resize(&decoded, dw, dh, image::imageops::FilterType::Triangle);
        let gray = image::DynamicImage::ImageRgb8(small).into_luma8();
        let pixels = gray.as_raw();
        let img = ImageData::new(pixels, dw, dh);
        let faces = detector.detect(&img);

        if let Some(best) = faces.iter().max_by(|a, b| {
            a.score()
                .partial_cmp(&b.score())
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            let bbox = best.bbox();
            let bx = bbox.x();
            let by = bbox.y();
            let bw = bbox.width();
            let bh = bbox.height();

            // Deterministic pupil-in-socket gaze (primary FOV driver).
            let mut pupil = detect_pupil_gaze(pixels, dw, dh, bx, by, bw, bh);
            // Webcam is mirror of user space — flip X / roll to screen coords.
            if pupil.ok {
                pupil.gaze_x = -pupil.gaze_x;
                pupil.roll = -pupil.roll;
            }

            // Blink hold: keep last good pupil gaze ~140ms through brief dropouts.
            if pupil.ok {
                blink_hold = Some((pupil.gaze_x, pupil.gaze_y, Instant::now()));
            } else if let Some((gx, gy, t)) = blink_hold {
                if t.elapsed() < Duration::from_millis(140) {
                    pupil.gaze_x = gx;
                    pupil.gaze_y = gy;
                    pupil.ok = true;
                    pupil.confidence = (pupil.confidence * 0.5).max(0.25);
                    pupil.quality = (pupil.quality * 0.5).max(0.22);
                }
            }

            let cam_x = 1.0 - ((bx as f32 + bw as f32 * 0.5) / dw as f32);
            let cam_y = (by as f32 + bh as f32 * 0.5) / dh as f32;
            let size = (bw.max(bh) as f32) / dw as f32;

            mapper.observe_neutral(
                cam_x.clamp(0.0, 1.0),
                cam_y.clamp(0.0, 1.0),
                size.clamp(0.0, 1.0),
                if pupil.ok { pupil.roll } else { 0.0 },
            );

            let (tx, ty, yaw, pitch, _hint) = mapper.map(
                cam_x.clamp(0.0, 1.0),
                cam_y.clamp(0.0, 1.0),
                size.clamp(0.0, 1.0),
                pupil,
            );

            let roll_raw = if pupil.ok {
                pupil.roll - mapper.roll
            } else {
                0.0
            };

            let dt = last_filter.elapsed().as_secs_f32().clamp(0.01, 0.08);
            last_filter = Instant::now();

            if !have_lock {
                pre_x = tx;
                pre_y = ty;
                filt_x.reset(tx);
                filt_y.reset(ty);
                out_x = tx;
                out_y = ty;
            } else {
                // Pre-smooth + deadzone: ignore micro pupil wobble (video flicker).
                let mut aim_x = tx;
                let mut aim_y = ty;
                let raw_step = (tx - pre_x).hypot(ty - pre_y);
                const GAZE_DEAD: f32 = 0.012;
                if raw_step < GAZE_DEAD {
                    aim_x = pre_x;
                    aim_y = pre_y;
                }
                pre_x += (aim_x - pre_x) * 0.32;
                pre_y += (aim_y - pre_y) * 0.32;
                out_x = filt_x.filter(pre_x, dt);
                out_y = filt_y.filter(pre_y, dt);
                // Cap step: tighter when pupils locked (stability > snap).
                let max_step = if pupil.ok { 0.055 } else { 0.10 };
                let (px, py) = {
                    let s = sensor.latest.lock();
                    (s.face_x, s.face_y)
                };
                out_x = px + (out_x - px).clamp(-max_step, max_step);
                out_y = py + (out_y - py).clamp(-max_step, max_step);
            }

            sm_size += (size.clamp(0.0, 1.0) - sm_size) * 0.15;
            sm_roll += (roll_raw.clamp(-0.45, 0.45) - sm_roll) * 0.18;
            sm_yaw += (yaw - sm_yaw) * 0.2;
            sm_pitch += (pitch - sm_pitch) * 0.2;
            let pupil_target = if pupil.ok { pupil.confidence } else { 0.0 };
            sm_pupil_conf += (pupil_target - sm_pupil_conf) * 0.35;
            sm_pupil_q += ((if pupil.ok { pupil.quality } else { 0.0 }) - sm_pupil_q) * 0.28;
            sm_pupil_agree +=
                ((if pupil.ok { pupil.agreement } else { 0.0 }) - sm_pupil_agree) * 0.25;
            // Lock rate: EMA of binary lock (session “precision” proxy).
            let lock_bit = if pupil.ok { 1.0 } else { 0.0 };
            sm_lock_rate += (lock_bit - sm_lock_rate) * 0.08;

            // Jitter: step size when gaze is relatively still (precision when holding).
            let step = (out_x - prev_gaze_x).hypot(out_y - prev_gaze_y);
            if pupil.ok && step < 0.035 {
                sm_jitter += (step - sm_jitter) * 0.12;
            } else if !pupil.ok {
                sm_jitter *= 0.98;
            }
            prev_gaze_x = out_x;
            prev_gaze_y = out_y;

            // Confidence: pupils lock dominates; face alone is weaker.
            let conf_target = if pupil.ok {
                0.55 + pupil.confidence * 0.45
            } else {
                0.35
            };
            sm_conf += (conf_target - sm_conf) * 0.28;
            have_lock = true;
            last_hit = Instant::now();

            let status = if !mapper.ready {
                "calibrating"
            } else if pupil.ok && pupil.binocular {
                "pupils"
            } else if pupil.ok {
                "pupil"
            } else {
                "face"
            };

            *sensor.latest.lock() = FaceSample {
                ok: true,
                status: status.into(),
                face_x: out_x,
                face_y: out_y,
                face_size: sm_size,
                present: true,
                confidence: sm_conf.clamp(0.0, 1.0),
                head_roll: sm_roll,
                head_yaw: sm_yaw,
                head_pitch: sm_pitch,
                calibrated: mapper.ready,
                pupils_ok: pupil.ok,
                pupil_confidence: sm_pupil_conf.clamp(0.0, 1.0),
                pupil_quality: sm_pupil_q.clamp(0.0, 1.0),
                pupil_agreement: sm_pupil_agree.clamp(0.0, 1.0),
                pupil_lock_rate: sm_lock_rate.clamp(0.0, 1.0),
                pupil_jitter: sm_jitter.clamp(0.0, 0.2),
            };
        } else {
            sm_conf *= 0.9;
            sm_pupil_conf *= 0.85;
            sm_pupil_q *= 0.9;
            sm_lock_rate += (0.0 - sm_lock_rate) * 0.06;
            if last_hit.elapsed() > Duration::from_millis(1400) {
                have_lock = false;
                pre_x += (0.5 - pre_x) * 0.04;
                pre_y += (0.5 - pre_y) * 0.04;
                out_x += (0.5 - out_x) * 0.04;
                out_y += (0.5 - out_y) * 0.04;
                filt_x.reset(out_x);
                filt_y.reset(out_y);
                sm_roll *= 0.92;
                *sensor.latest.lock() = FaceSample {
                    ok: true,
                    status: "no_face".into(),
                    face_x: out_x,
                    face_y: out_y,
                    face_size: sm_size,
                    present: false,
                    confidence: sm_conf.clamp(0.0, 1.0),
                    head_roll: sm_roll,
                    head_yaw: sm_yaw * 0.9,
                    head_pitch: sm_pitch * 0.9,
                    calibrated: mapper.ready,
                    pupils_ok: false,
                    pupil_confidence: sm_pupil_conf.clamp(0.0, 1.0),
                    pupil_quality: sm_pupil_q.clamp(0.0, 1.0),
                    pupil_agreement: sm_pupil_agree.clamp(0.0, 1.0),
                    pupil_lock_rate: sm_lock_rate.clamp(0.0, 1.0),
                    pupil_jitter: sm_jitter.clamp(0.0, 0.2),
                };
            } else {
                let mut s = sensor.latest.lock();
                s.ok = true;
                s.status = "holding".into();
                s.face_x = out_x;
                s.face_y = out_y;
                s.face_size = sm_size;
                s.present = true;
                s.confidence = sm_conf.clamp(0.0, 1.0);
                s.head_roll = sm_roll;
                s.head_yaw = sm_yaw;
                s.head_pitch = sm_pitch;
                s.calibrated = mapper.ready;
                s.pupils_ok = false;
                s.pupil_confidence = sm_pupil_conf.clamp(0.0, 1.0);
                s.pupil_quality = sm_pupil_q.clamp(0.0, 1.0);
                s.pupil_agreement = sm_pupil_agree.clamp(0.0, 1.0);
                s.pupil_lock_rate = sm_lock_rate.clamp(0.0, 1.0);
                s.pupil_jitter = sm_jitter.clamp(0.0, 0.2);
            }
        }

        // Adaptive cadence: slower when gaze is locked & still → less CPU/RAM.
        let sleep_ms = if sm_lock_rate > 0.8 && sm_jitter < 0.015 {
            42
        } else if sm_lock_rate > 0.5 {
            34
        } else {
            28
        };
        std::thread::sleep(Duration::from_millis(sleep_ms));
    }

    let _ = camera.stop_stream();
    Ok(())
}
