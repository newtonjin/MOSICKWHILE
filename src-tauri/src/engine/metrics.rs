use serde::Serialize;

/// Live instrumentation emitted every engine tick.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveMetrics {
    /// Sickness-relevant drive after smoothing (0–1).
    pub motion: f32,
    /// Mean optical-flow magnitude in the periphery (normalized 0–1).
    pub peripheral_flow: f32,
    /// Mean optical-flow magnitude in the central FOV (normalized 0–1).
    pub central_flow: f32,
    /// Estimated rotational component 0–1.
    pub rotational: f32,
    /// Estimated translational component 0–1.
    pub translational: f32,
    /// Mean flow vector (screen space, normalized).
    pub flow_x: f32,
    pub flow_y: f32,
    /// 1 − sickness drive after mitigation response (higher = more comfort).
    pub comfort_index: f32,
    /// Current dynamic FOV restriction amount applied.
    pub fov_restriction: f32,
    /// Capture → analyze pipeline latency.
    pub latency_ms: f32,
    /// Optical-flow analysis rate.
    pub analyze_fps: f32,
    /// DXGI capture healthy.
    pub capture_ok: bool,
    pub capture_status: String,
    /// Peak motion this session.
    pub session_peak: f32,
    /// Frames analyzed this session.
    pub frames_analyzed: u64,
    /// Seconds since session start.
    pub session_secs: f32,
    /// Pupil composite quality 0–1 (live; not labeled GT accuracy).
    pub pupil_quality: f32,
    /// Dual-eye agreement 0–1 — primary live accuracy proxy.
    pub pupil_agreement: f32,
    /// Fraction of recent frames with pupil lock.
    pub pupil_lock_rate: f32,
    /// Gaze jitter while relatively still (screen units, lower better).
    pub pupil_jitter: f32,
    /// Gaming mode currently active (auto-detect or forced).
    pub gaming_active: bool,
    /// 0–1 confidence of game session.
    pub gaming_score: f32,
}
