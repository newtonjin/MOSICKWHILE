//! Shared-memory layout mirrored in the host (`src-tauri/src/ipc.rs`).

pub const SHM_NAME: &str = "Local\\MOSICKWHILE_BRIDGE_v3";
pub const SHM_MAGIC: u32 = 0x4D53_4B33; // MSK3

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SharedBlock {
    pub magic: u32,
    pub seq: u32,
    pub enabled: u32,
    pub hooked: u32,
    pub intensity: f32,
    pub vignette_strength: f32,
    pub vignette_softness: f32,
    pub asymmetric: u32,
    pub ground_visible: u32,
    /// 0 none, 1 crosshair, 2 horizon, 3 grid, 4 earth_horizon, 5 virtual_nose
    pub rest_style: u32,
    pub rest_opacity: f32,
    pub blur_strength: f32,
    pub attack_secs: f32,
    pub release_secs: f32,
    pub motion_deadzone: f32,
    pub horizon_roll: f32,
    pub clear_x: f32,
    pub clear_y: f32,
    pub clear_radius: f32,
    pub foveated_clear: u32,
    pub flow_dampen: f32,
    pub predictive_fov: u32,
    pub side_restrictor: u32,
    pub real_blur: u32,
    pub side_mask: f32,
    pub side_sign: f32,
    pub blur_px: f32,
    /// Written by DLL (telemetry)
    pub motion: f32,
    pub comfort: f32,
    pub peri_flow: f32,
    pub fov_amount: f32,
    pub present_fps: f32,
}

impl Default for SharedBlock {
    fn default() -> Self {
        Self {
            magic: SHM_MAGIC,
            seq: 0,
            enabled: 1,
            hooked: 0,
            intensity: 0.7,
            vignette_strength: 0.82,
            vignette_softness: 0.48,
            asymmetric: 1,
            ground_visible: 1,
            rest_style: 4,
            rest_opacity: 0.4,
            blur_strength: 0.45,
            attack_secs: 0.14,
            release_secs: 0.5,
            motion_deadzone: 0.04,
            horizon_roll: 0.0,
            clear_x: 0.5,
            clear_y: 0.5,
            clear_radius: 0.38,
            foveated_clear: 1,
            flow_dampen: 0.0,
            predictive_fov: 1,
            side_restrictor: 1,
            real_blur: 1,
            side_mask: 0.0,
            side_sign: -1.0,
            blur_px: 0.0,
            motion: 0.0,
            comfort: 1.0,
            peri_flow: 0.0,
            fov_amount: 0.0,
            present_fps: 0.0,
        }
    }
}

pub const SHM_SIZE: usize = std::mem::size_of::<SharedBlock>();
