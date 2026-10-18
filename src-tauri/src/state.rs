use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::engine::{
    AppConfig, GameDetector, GamingState, LiveMetrics, MitigationEngine, MotionEstimator,
    OverlayFrame, RestFrameStyle,
};
use crate::sensing::{FaceSensor, SharedDisplay};

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub engine: Mutex<MitigationEngine>,
    pub motion: Mutex<MotionEstimator>,
    pub last_frame: Mutex<OverlayFrame>,
    pub last_tick: Mutex<Instant>,
    pub overlay_visible: Mutex<bool>,
    pub running: AtomicBool,
    /// Main control panel visible (false when parked in tray).
    pub panel_visible: AtomicBool,
    pub face: Arc<FaceSensor>,
    pub display: Arc<SharedDisplay>,
    pub game_detector: Mutex<GameDetector>,
    pub gaming: Mutex<GamingState>,
}

impl AppState {
    pub fn new() -> Arc<Self> {
        let cfg = AppConfig::default();
        let display = Arc::new(SharedDisplay::new());
        let face = FaceSensor::new();
        if cfg.face_sensing_enabled {
            face.set_enabled(true);
        }
        Arc::new(Self {
            config: Mutex::new(cfg),
            engine: Mutex::new(MitigationEngine::default()),
            motion: Mutex::new(MotionEstimator::default()),
            last_frame: Mutex::new(OverlayFrame {
                enabled: true,
                motion: 0.0,
                vignette_amount: 0.0,
                vignette_softness: 0.48,
                bias_x: 0.0,
                bias_y: 0.0,
                ground_clear: 0.25,
                rest_frame_style: RestFrameStyle::EarthHorizon,
                rest_frame_opacity: 0.0,
                blur_amount: 0.0,
                horizon_roll: 0.0,
                clear_x: 0.5,
                clear_y: 0.5,
                clear_radius: 0.38,
                foveated_clear: true,
                virtual_nose: false,
                flow_dampen: 0.0,
                side_mask: 0.0,
                side_sign: -1.0,
                real_blur: true,
                blur_px: 0.0,
                face_ok: false,
                face_present: false,
                face_status: "idle".into(),
                metrics: LiveMetrics::default(),
            }),
            last_tick: Mutex::new(Instant::now()),
            overlay_visible: Mutex::new(false),
            running: AtomicBool::new(true),
            panel_visible: AtomicBool::new(true),
            face,
            display,
            game_detector: Mutex::new(GameDetector::default()),
            gaming: Mutex::new(GamingState::default()),
        })
    }

    pub fn update_gaming(
        &self,
        bridge_hooked: bool,
        peri_flow: f32,
        rotational: f32,
        mouse_boost: f32,
        dt: f32,
    ) -> GamingState {
        let st = self.game_detector.lock().update(
            bridge_hooked,
            peri_flow,
            rotational,
            mouse_boost,
            dt,
        );
        *self.gaming.lock() = st;
        st
    }

    pub fn request_stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.face.stop();
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn tick(&self, bridge_hooked: bool) -> OverlayFrame {
        let now = Instant::now();
        let mut last = self.last_tick.lock();
        let dt = now.duration_since(*last).as_secs_f32().clamp(0.001, 0.1);
        *last = now;
        drop(last);

        let cfg = self.config.lock().clone();
        self.face.set_enabled(cfg.face_sensing_enabled);

        let overlay = self.display.get();
        let geom = overlay;
        self.face.set_display_size(geom.w, geom.h);

        let reading = {
            let mut motion = self.motion.lock();
            let reading = motion.sample(cfg.motion_source, cfg.manual_motion, dt, overlay);
            let mut metrics = LiveMetrics::default();
            motion.fill_metrics(&reading, &mut metrics);
            (reading, metrics)
        };

        let gaming = if cfg.gaming_mode_auto {
            self.update_gaming(
                bridge_hooked,
                reading.0.flow.peripheral_flow,
                reading.0.flow.rotational,
                reading.0.pred.boost,
                dt,
            )
        } else {
            let off = GamingState::default();
            *self.gaming.lock() = off;
            off
        };

        let face = self.face.latest();
        let frame = {
            let mut engine = self.engine.lock();
            engine.tick(&reading.0, &cfg, dt, reading.1, &face, gaming)
        };

        *self.last_frame.lock() = frame.clone();
        frame
    }
}
