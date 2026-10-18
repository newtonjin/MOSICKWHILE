use std::time::Instant;

use super::capture::ScreenMotionCapture;
use super::config::MotionSource;
use super::flow::FlowSample;
use super::input_pred::{InputPrediction, InputPredictor};
use super::metrics::LiveMetrics;
use crate::sensing::DisplayRect;

#[derive(Debug, Clone, Default)]
pub struct MotionReading {
    pub raw_drive: f32,
    pub flow: FlowSample,
    pub latency_ms: f32,
    pub capture_ok: bool,
    pub capture_status: String,
    pub analyze_fps: f32,
    /// Mouse-look prediction (host-side).
    pub pred: InputPrediction,
}

/// Produces a sickness-relevant motion drive in \[0, 1\].
pub struct MotionEstimator {
    started: Instant,
    capture: ScreenMotionCapture,
    predictor: InputPredictor,
    last_capture: MotionReading,
    session_peak: f32,
    frames_analyzed: u64,
    /// Hold last good capture sample briefly across DXGI timeouts.
    hold_secs: f32,
}

impl Default for MotionEstimator {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            capture: ScreenMotionCapture::new(),
            predictor: InputPredictor::default(),
            last_capture: MotionReading::default(),
            session_peak: 0.0,
            frames_analyzed: 0,
            hold_secs: 0.0,
        }
    }
}

impl MotionEstimator {
    pub fn sample(
        &mut self,
        source: MotionSource,
        manual: f32,
        dt: f32,
        overlay: DisplayRect,
    ) -> MotionReading {
        let pred = self.predictor.sample(dt, overlay);
        let mut reading = match source {
            MotionSource::Manual => MotionReading {
                raw_drive: manual.clamp(0.0, 1.0),
                flow: FlowSample {
                    magnitude: manual.clamp(0.0, 1.0),
                    ..FlowSample::default()
                },
                latency_ms: 0.0,
                capture_ok: true,
                capture_status: "manual".into(),
                analyze_fps: 0.0,
                pred: InputPrediction::default(),
            },
            MotionSource::Demo => {
                let drive = self.demo_sample();
                let t = self.started.elapsed().as_secs_f32();
                let yaw = (t * 1.1).sin() * drive;
                MotionReading {
                    raw_drive: drive,
                    flow: FlowSample {
                        magnitude: drive,
                        peripheral_flow: drive,
                        rotational: drive * 0.6,
                        translational: drive * 0.4,
                        flow_x: yaw,
                        flow_y: (t * 0.7).cos() * drive * 0.4,
                        ..FlowSample::default()
                    },
                    latency_ms: 0.0,
                    capture_ok: true,
                    capture_status: "demo".into(),
                    analyze_fps: 60.0,
                    pred: InputPrediction {
                        boost: drive * 0.85,
                        yaw,
                        pitch: 0.0,
                        teleport_rejected: false,
                        cursor_x: 0.5 + yaw * 0.2,
                        cursor_y: 0.5,
                        cursor_on_monitor: true,
                    },
                }
            }
            MotionSource::Capture => self.sample_capture(dt),
        };
        reading.pred = pred;

        if reading.raw_drive > self.session_peak {
            self.session_peak = reading.raw_drive;
        }
        if reading.capture_ok && reading.capture_status == "capturing" {
            self.frames_analyzed = self.frames_analyzed.saturating_add(1);
        }

        reading
    }

    pub fn session_metrics_base(&self) -> (f32, u64, f32) {
        (
            self.session_peak,
            self.frames_analyzed,
            self.started.elapsed().as_secs_f32(),
        )
    }

    fn sample_capture(&mut self, dt: f32) -> MotionReading {
        let cap = self.capture.poll();
        let fps = self.capture.analyze_fps();

        if cap.ok && cap.status == "capturing" {
            self.hold_secs = 0.08;
            self.last_capture = MotionReading {
                raw_drive: cap.flow.magnitude,
                flow: cap.flow,
                latency_ms: cap.latency_ms,
                capture_ok: true,
                capture_status: cap.status,
                analyze_fps: fps,
                pred: InputPrediction::default(),
            };
            return self.last_capture.clone();
        }

        // DXGI timeout: hold last sample briefly so mitigation doesn't flicker.
        if cap.ok && cap.status == "waiting_frame" {
            self.hold_secs = (self.hold_secs - dt).max(0.0);
            if self.hold_secs > 0.0 {
                let mut held = self.last_capture.clone();
                held.analyze_fps = fps;
                held.capture_status = "holding".into();
                return held;
            }
            return MotionReading {
                raw_drive: 0.0,
                flow: FlowSample::default(),
                latency_ms: cap.latency_ms,
                capture_ok: true,
                capture_status: "idle".into(),
                analyze_fps: fps,
                pred: InputPrediction::default(),
            };
        }

        MotionReading {
            raw_drive: 0.0,
            flow: FlowSample::default(),
            latency_ms: cap.latency_ms,
            capture_ok: false,
            capture_status: cap.status,
            analyze_fps: fps,
            pred: InputPrediction::default(),
        }
    }

    fn demo_sample(&self) -> f32 {
        let t = self.started.elapsed().as_secs_f32();
        let locomotion = ((t * 0.55).sin() * 0.5 + 0.5).powf(1.4);
        let turns = ((t * 1.35 + 0.8).sin().abs()).powf(2.2);
        let burst = if (t * 0.22).sin() > 0.65 { 0.35 } else { 0.0 };
        (0.35 * locomotion + 0.45 * turns + burst).clamp(0.0, 1.0)
    }

    pub fn fill_metrics(&self, reading: &MotionReading, metrics: &mut LiveMetrics) {
        let (peak, frames, secs) = self.session_metrics_base();
        metrics.peripheral_flow = reading.flow.peripheral_flow;
        metrics.central_flow = reading.flow.central_flow;
        metrics.rotational = reading.flow.rotational;
        metrics.translational = reading.flow.translational;
        metrics.flow_x = reading.flow.flow_x;
        metrics.flow_y = reading.flow.flow_y;
        metrics.latency_ms = reading.latency_ms;
        metrics.analyze_fps = reading.analyze_fps;
        metrics.capture_ok = reading.capture_ok;
        metrics.capture_status = reading.capture_status.clone();
        metrics.session_peak = peak;
        metrics.frames_analyzed = frames;
        metrics.session_secs = secs;
    }
}
