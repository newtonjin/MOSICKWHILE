//! Shared-memory IPC with the in-game Present bridge (exclusive fullscreen path).

use parking_lot::Mutex;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS,
    PAGE_READWRITE,
};

pub const SHM_NAME: &str = "Local\\MOSICKWHILE_BRIDGE_v3";
pub const SHM_MAGIC: u32 = 0x4D53_4B33;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
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

pub struct BridgeIpc {
    _handle: HANDLE,
    ptr: *mut SharedBlock,
}

unsafe impl Send for BridgeIpc {}
unsafe impl Sync for BridgeIpc {}

impl BridgeIpc {
    pub fn create() -> Result<Self, String> {
        let name: Vec<u16> = OsStr::new(SHM_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let size = std::mem::size_of::<SharedBlock>() as u32;
        unsafe {
            let handle = CreateFileMappingW(
                windows::Win32::Foundation::INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                size,
                PCWSTR(name.as_ptr()),
            )
            .map_err(|e| format!("CreateFileMappingW: {e}"))?;

            let view: MEMORY_MAPPED_VIEW_ADDRESS =
                MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size as usize);
            if view.Value.is_null() {
                return Err("MapViewOfFile failed".into());
            }
            let ptr = view.Value as *mut SharedBlock;
            *ptr = SharedBlock::default();
            Ok(Self {
                _handle: handle,
                ptr,
            })
        }
    }

    pub fn write_from_config(&self, cfg: &crate::engine::AppConfig, engine_enabled: bool) {
        let rest_style = match cfg.rest_frame_style {
            crate::engine::RestFrameStyle::None => 0,
            crate::engine::RestFrameStyle::Crosshair => 1,
            crate::engine::RestFrameStyle::Horizon => 2,
            crate::engine::RestFrameStyle::Grid => 3,
            crate::engine::RestFrameStyle::EarthHorizon => 4,
            crate::engine::RestFrameStyle::VirtualNose => 5,
        };
        unsafe {
            let b = &mut *self.ptr;
            b.magic = SHM_MAGIC;
            b.seq = b.seq.wrapping_add(1);
            b.enabled = u32::from(engine_enabled && cfg.enabled);
            b.intensity = cfg.intensity;
            b.vignette_strength = if cfg.vignette_enabled {
                cfg.vignette_strength
            } else {
                0.0
            };
            b.vignette_softness = cfg.vignette_softness;
            b.asymmetric = u32::from(cfg.asymmetric_fov);
            b.ground_visible = u32::from(cfg.ground_visible);
            b.rest_style = if cfg.rest_frame_enabled { rest_style } else { 0 };
            b.rest_opacity = cfg.rest_frame_opacity;
            b.blur_strength = if cfg.peripheral_blur_enabled || cfg.real_peripheral_blur {
                cfg.peripheral_blur_strength.max(0.35)
            } else {
                0.0
            };
            b.attack_secs = cfg.attack_secs;
            b.release_secs = cfg.release_secs;
            b.motion_deadzone = cfg.motion_deadzone;
            b.clear_radius = cfg.foveated_clear_radius;
            b.foveated_clear = u32::from(cfg.foveated_clear_enabled);
            b.predictive_fov = u32::from(cfg.predictive_fov);
            b.side_restrictor = u32::from(cfg.side_restrictor);
            b.real_blur = u32::from(cfg.real_peripheral_blur);
        }
    }

    pub fn write_frame_hints(&self, frame: &crate::engine::OverlayFrame) {
        unsafe {
            let b = &mut *self.ptr;
            b.horizon_roll = frame.horizon_roll;
            b.clear_x = frame.clear_x;
            b.clear_y = frame.clear_y;
            b.clear_radius = frame.clear_radius;
            b.foveated_clear = u32::from(frame.foveated_clear);
            b.flow_dampen = frame.flow_dampen;
            b.side_mask = frame.side_mask;
            b.side_sign = frame.side_sign;
            b.blur_px = frame.blur_px;
            b.real_blur = u32::from(frame.real_blur);
        }
    }

    pub fn snapshot(&self) -> SharedBlock {
        unsafe { *self.ptr }
    }
}

pub struct BridgeState {
    pub ipc: BridgeIpc,
    pub attached_pid: Mutex<Option<u32>>,
}

impl BridgeState {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            ipc: BridgeIpc::create()?,
            attached_pid: Mutex::new(None),
        })
    }
}
