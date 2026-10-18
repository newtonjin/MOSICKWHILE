use serde::{Deserialize, Serialize};

/// How motion magnitude is produced for the mitigation engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MotionSource {
    /// Live DXGI desktop capture + peripheral optical flow (production mode).
    #[default]
    Capture,
    /// Synthetic oscillating motion — useful for previewing mitigations.
    Demo,
    /// Fixed motion level from the intensity slider (forces constant vignette).
    Manual,
}

/// Visual rest-frame style drawn in screen space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RestFrameStyle {
    None,
    Crosshair,
    /// Screen-fixed horizon (classic).
    #[default]
    Horizon,
    /// Earth-fixed horizon counter-rotated by estimated visual roll
    /// (Frontiers 2020: only earth-fixed horizon significantly cut VIMS).
    EarthHorizon,
    Grid,
    /// Head-fixed virtual nose rest frame (Whittinghill et al. style).
    VirtualNose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Light,
    #[default]
    Medium,
    Aggressive,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub enabled: bool,
    pub auto_start_overlay: bool,
    pub preset: Preset,
    pub intensity: f32,
    pub motion_source: MotionSource,
    pub manual_motion: f32,
    pub vignette_enabled: bool,
    pub vignette_strength: f32,
    pub vignette_softness: f32,
    pub asymmetric_fov: bool,
    pub ground_visible: bool,
    pub rest_frame_enabled: bool,
    pub rest_frame_style: RestFrameStyle,
    pub rest_frame_opacity: f32,
    pub peripheral_blur_enabled: bool,
    pub peripheral_blur_strength: f32,
    pub attack_secs: f32,
    pub release_secs: f32,
    pub motion_deadzone: f32,
    pub vignette_slew_up: f32,
    pub vignette_slew_down: f32,

    // —— Advanced (state-of-the-art PC-feasible) ——
    /// Foveated clear window + peripheral rings (works without eye tracker).
    pub foveated_clear_enabled: bool,
    /// Clear-zone radius as fraction of min(screen) (0.25–0.55 typical).
    pub foveated_clear_radius: f32,
    /// Webcam face sensing: bias clear zone toward face / presence gate.
    pub face_sensing_enabled: bool,
    /// Auto-raise intensity during sustained high motion (comfort ramp).
    pub comfort_auto_ramp: bool,
    /// Soften peripheral optical-flow sensation (velocity-dampen style opacity).
    pub peripheral_flow_dampen: bool,
    /// Attack FOV on mouse-look onset before visual flow peaks (predictive).
    pub predictive_fov: bool,
    /// Wu & Rosenberg style: darken the high-flow side during yaw turns.
    pub side_restrictor: bool,
    /// Real peripheral blur (backdrop-filter on overlay / frost layers in bridge).
    pub real_peripheral_blur: bool,
    /// Manual gaming mode: faster FOV + clear-zone pinned to center on turns/glances.
    /// External overlay only — never uses process injection.
    #[serde(alias = "gamingModeAuto")]
    pub gaming_mode: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::from_preset(Preset::Medium)
    }
}

impl AppConfig {
    pub fn from_preset(preset: Preset) -> Self {
        let mut cfg = Self {
            enabled: true,
            auto_start_overlay: true,
            preset,
            intensity: 0.7,
            motion_source: MotionSource::Capture,
            manual_motion: 0.45,
            vignette_enabled: true,
            vignette_strength: 0.82,
            vignette_softness: 0.48,
            asymmetric_fov: true,
            ground_visible: true,
            rest_frame_enabled: true,
            rest_frame_style: RestFrameStyle::EarthHorizon,
            rest_frame_opacity: 0.42,
            peripheral_blur_enabled: true,
            peripheral_blur_strength: 0.5,
            attack_secs: 0.28,
            release_secs: 0.85,
            motion_deadzone: 0.06,
            vignette_slew_up: 1.2,
            vignette_slew_down: 0.9,
            foveated_clear_enabled: true,
            foveated_clear_radius: 0.38,
            face_sensing_enabled: true,
            comfort_auto_ramp: true,
            peripheral_flow_dampen: true,
            predictive_fov: true,
            side_restrictor: true,
            real_peripheral_blur: true,
            gaming_mode: false,
        };

        match preset {
            Preset::Light => {
                cfg.intensity = 0.45;
                cfg.vignette_strength = 0.55;
                cfg.rest_frame_opacity = 0.28;
                cfg.peripheral_blur_strength = 0.28;
                cfg.attack_secs = 0.35;
                cfg.release_secs = 1.0;
                cfg.vignette_slew_up = 0.9;
                cfg.vignette_slew_down = 0.7;
                cfg.rest_frame_style = RestFrameStyle::Horizon;
                cfg.foveated_clear_radius = 0.45;
            }
            Preset::Medium => {}
            Preset::Aggressive => {
                cfg.intensity = 0.92;
                cfg.vignette_strength = 0.95;
                cfg.vignette_softness = 0.36;
                cfg.rest_frame_opacity = 0.55;
                cfg.rest_frame_style = RestFrameStyle::EarthHorizon;
                cfg.peripheral_blur_enabled = true;
                cfg.peripheral_blur_strength = 0.7;
                cfg.attack_secs = 0.18;
                cfg.release_secs = 0.6;
                cfg.motion_deadzone = 0.04;
                cfg.vignette_slew_up = 1.8;
                cfg.vignette_slew_down = 1.2;
                cfg.foveated_clear_radius = 0.32;
                cfg.peripheral_flow_dampen = true;
            }
            Preset::Custom => {
                cfg.preset = Preset::Custom;
            }
        }

        cfg
    }

    pub fn clamp(mut self) -> Self {
        self.intensity = self.intensity.clamp(0.0, 1.0);
        self.manual_motion = self.manual_motion.clamp(0.0, 1.0);
        self.vignette_strength = self.vignette_strength.clamp(0.0, 1.0);
        self.vignette_softness = self.vignette_softness.clamp(0.05, 1.0);
        self.rest_frame_opacity = self.rest_frame_opacity.clamp(0.0, 1.0);
        self.peripheral_blur_strength = self.peripheral_blur_strength.clamp(0.0, 1.0);
        self.attack_secs = self.attack_secs.clamp(0.08, 2.0);
        self.release_secs = self.release_secs.clamp(0.2, 3.0);
        self.motion_deadzone = self.motion_deadzone.clamp(0.0, 0.3);
        self.vignette_slew_up = self.vignette_slew_up.clamp(0.2, 5.0);
        self.vignette_slew_down = self.vignette_slew_down.clamp(0.2, 5.0);
        self.foveated_clear_radius = self.foveated_clear_radius.clamp(0.2, 0.6);
        self
    }
}
