//! MOSICKWHILE Present bridge — draws mitigations inside the game's swapchain.
//! Works in exclusive fullscreen because it hooks DXGI Present (DX9/11/12).

mod shared;

use std::time::Instant;

use hudhook::hooks::dx11::ImguiDx11Hooks;
use hudhook::hooks::dx12::ImguiDx12Hooks;
use hudhook::hooks::dx9::ImguiDx9Hooks;
use hudhook::windows::Win32::Foundation::{HINSTANCE, POINT};
use hudhook::windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use hudhook::windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use hudhook::*;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

use shared::{SharedBlock, SHM_MAGIC, SHM_NAME, SHM_SIZE};

struct Shm {
    ptr: *mut SharedBlock,
}

// SAFETY: single mapped view, accessed under MUTEX in render thread of the game.
unsafe impl Send for Shm {}
unsafe impl Sync for Shm {}

static SHM: Lazy<Mutex<Option<Shm>>> = Lazy::new(|| Mutex::new(None));

fn open_shm() -> Option<Shm> {
    use hudhook::windows::core::PCWSTR;
    use hudhook::windows::Win32::Foundation::HANDLE;
    use hudhook::windows::Win32::System::Memory::{
        MapViewOfFile, OpenFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS,
    };

    let name: Vec<u16> = SHM_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let handle = OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(name.as_ptr())).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let view: MEMORY_MAPPED_VIEW_ADDRESS =
            MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, SHM_SIZE);
        if view.Value.is_null() {
            return None;
        }
        let ptr = view.Value as *mut SharedBlock;
        let block = &mut *ptr;
        if block.magic != 0 && block.magic != SHM_MAGIC {
            return None;
        }
        if block.magic == 0 {
            *block = SharedBlock::default();
        }
        block.hooked = 1;
        // Intentionally leak HANDLE — lives for process lifetime of the game.
        let _ = HANDLE(handle.0);
        Some(Shm { ptr })
    }
}

fn with_shm<R>(f: impl FnOnce(&mut SharedBlock) -> R) -> Option<R> {
    let mut guard = SHM.lock();
    if guard.is_none() {
        *guard = open_shm();
    }
    let shm = guard.as_ref()?;
    Some(f(unsafe { &mut *shm.ptr }))
}

pub struct MosickRenderLoop {
    smoothed: f32,
    vignette_out: f32,
    rest_out: f32,
    blur_out: f32,
    side_mask: f32,
    side_sign: f32,
    yaw: f32,
    pred_boost: f32,
    last_mouse: Option<(i32, i32)>,
    last_tick: Instant,
    frames: u32,
    fps_timer: Instant,
    present_fps: f32,
}

struct InputSample {
    motion: f32,
    yaw: f32,
    pred: f32,
}

impl MosickRenderLoop {
    fn new() -> Self {
        Self {
            smoothed: 0.0,
            vignette_out: 0.0,
            rest_out: 0.0,
            blur_out: 0.0,
            side_mask: 0.0,
            side_sign: -1.0,
            yaw: 0.0,
            pred_boost: 0.0,
            last_mouse: None,
            last_tick: Instant::now(),
            frames: 0,
            fps_timer: Instant::now(),
            present_fps: 0.0,
        }
    }

    /// Mouse look deltas + WASD. Teleports (alt-tab / click other window) rejected.
    fn sample_input(&mut self, dt: f32) -> InputSample {
        const MAX_LOOK_DELTA_PX: f32 = 96.0;
        let mut pt = POINT::default();
        let mut teleported = false;
        let (mouse_mag, yaw_raw) = if unsafe { GetCursorPos(&mut pt) }.is_ok() {
            let cur = (pt.x, pt.y);
            let (dx, dy) = if let Some((lx, ly)) = self.last_mouse {
                let dx = (cur.0 - lx) as f32;
                let dy = (cur.1 - ly) as f32;
                if (dx * dx + dy * dy).sqrt() > MAX_LOOK_DELTA_PX {
                    teleported = true;
                    (0.0, 0.0)
                } else {
                    (dx, dy)
                }
            } else {
                (0.0, 0.0)
            };
            self.last_mouse = Some(cur);
            if teleported {
                (0.0, 0.0)
            } else {
                let scale = (36.0 * (dt * 60.0).max(0.25)).max(1.0);
                let mag = ((dx * dx + dy * dy).sqrt() / scale).clamp(0.0, 1.0);
                let yaw = (dx / scale).clamp(-1.0, 1.0);
                (mag.powf(1.15), yaw)
            }
        } else {
            (0.0, 0.0)
        };

        let attack = 1.0 - (-dt / 0.055).exp();
        let release = 1.0 - (-dt / 0.22).exp();
        if teleported {
            self.pred_boost *= 1.0 - release;
            self.yaw *= 1.0 - release;
        } else if mouse_mag > self.pred_boost {
            self.pred_boost += (mouse_mag - self.pred_boost) * attack;
        } else {
            self.pred_boost += (mouse_mag - self.pred_boost) * release;
        }
        if !teleported {
            let ya = if yaw_raw.abs() > self.yaw.abs() {
                attack
            } else {
                release
            };
            self.yaw += (yaw_raw - self.yaw) * ya;
        }

        use hudhook::windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        let key_bits = [0x57u16, 0x41, 0x53, 0x44, 0x20];
        let mut keys = 0.0f32;
        for vk in key_bits {
            if unsafe { GetAsyncKeyState(vk as i32) } < 0 {
                keys += 0.12;
            }
        }
        InputSample {
            motion: (0.75 * mouse_mag + keys).clamp(0.0, 1.0),
            yaw: self.yaw,
            pred: self.pred_boost,
        }
    }
}

impl ImguiRenderLoop for MosickRenderLoop {
    fn before_render<'a>(
        &'a mut self,
        ctx: &mut imgui::Context,
        _render_context: &'a mut dyn RenderContext,
    ) {
        let io = ctx.io_mut();
        io.mouse_draw_cursor = false;
        io.config_flags |= imgui::ConfigFlags::NO_MOUSE;
    }

    fn message_filter(&self, _io: &imgui::Io) -> MessageFilter {
        // Never steal game input.
        MessageFilter::empty()
    }

    fn render(&mut self, ui: &mut imgui::Ui) {
        self.frames += 1;
        if self.fps_timer.elapsed().as_secs_f32() >= 1.0 {
            self.present_fps = self.frames as f32 / self.fps_timer.elapsed().as_secs_f32();
            self.frames = 0;
            self.fps_timer = Instant::now();
        }

        let dt = self.last_tick.elapsed().as_secs_f32().clamp(0.001, 0.1);
        self.last_tick = Instant::now();

        let input = self.sample_input(dt);

        let mut cfg = SharedBlock::default();
        let enabled = with_shm(|b| {
            cfg = *b;
            b.hooked = 1;
            b.present_fps = self.present_fps;
            b.enabled != 0
        })
        .unwrap_or(true);

        if !enabled {
            self.smoothed = (self.smoothed - dt / cfg.release_secs.max(0.35)).max(0.0);
            self.vignette_out = (self.vignette_out - 0.9 * dt).max(0.0);
            self.rest_out = (self.rest_out - dt / 0.55).max(0.0);
            self.blur_out = (self.blur_out - dt / 0.45).max(0.0);
            self.side_mask = (self.side_mask - dt * 2.0).max(0.0);
            let _ = with_shm(|b| {
                b.motion = self.smoothed;
                b.fov_amount = self.vignette_out;
                b.comfort = 1.0 - self.smoothed;
            });
            return;
        }

        let mut target = input.motion;
        let pred_lead = cfg.predictive_fov != 0 && input.pred > input.motion + 0.08;
        if cfg.predictive_fov != 0 {
            if input.pred > target {
                target = (target * 0.4 + input.pred * 0.85).clamp(0.0, 1.0);
            } else {
                target = target.max(input.pred * 0.55);
            }
        }
        if target < cfg.motion_deadzone {
            target = 0.0;
        } else {
            target = ((target - cfg.motion_deadzone) / (1.0 - cfg.motion_deadzone)).clamp(0.0, 1.0);
        }

        let tau = if target > self.smoothed {
            if pred_lead {
                (cfg.attack_secs * 0.45).max(0.06)
            } else {
                cfg.attack_secs.max(0.14)
            }
        } else {
            cfg.release_secs.max(0.5)
        };
        let alpha = 1.0 - (-dt / tau).exp();
        self.smoothed += (target - self.smoothed) * alpha;

        // Side restrictor from local yaw (host may also push side_mask).
        if cfg.side_restrictor != 0 {
            let yaw_abs = input.yaw.abs();
            let side_t = (yaw_abs * 1.35).clamp(0.0, 1.0) * self.smoothed;
            self.side_mask += (side_t - self.side_mask) * (1.0 - (-dt / 0.1).exp());
            if yaw_abs > 0.08 {
                self.side_sign = -input.yaw.signum();
            }
        } else {
            self.side_mask *= 0.9;
        }
        let side_mask = self.side_mask.max(cfg.side_mask);
        let side_sign = if cfg.side_mask > 0.05 {
            cfg.side_sign
        } else {
            self.side_sign
        };

        let drive = (self.smoothed * cfg.intensity).clamp(0.0, 1.0);
        let curved = drive.powf(1.25);
        let mut vignette_target = curved * cfg.vignette_strength;
        if pred_lead {
            vignette_target =
                (vignette_target + input.pred * 0.18 * cfg.vignette_strength).clamp(0.0, 1.0);
        }
        let slew_up = if pred_lead { 2.2 } else { 1.2 };
        let dv = vignette_target - self.vignette_out;
        self.vignette_out += dv.clamp(-0.9 * dt, slew_up * dt);

        let rest_target = if cfg.rest_style == 0 {
            0.0
        } else {
            cfg.rest_opacity * (0.72 + 0.28 * curved)
        };
        self.rest_out += (rest_target - self.rest_out) * (1.0 - (-dt / 0.55).exp());
        let blur_t = curved * cfg.blur_strength;
        self.blur_out += (blur_t - self.blur_out) * (1.0 - (-dt / 0.4).exp());

        let _ = with_shm(|b| {
            b.motion = self.smoothed;
            b.peri_flow = input.motion.max(input.pred);
            b.fov_amount = self.vignette_out;
            b.comfort =
                (1.0 - self.smoothed * (1.0 - self.vignette_out * 0.85)).clamp(0.0, 1.0);
            b.present_fps = self.present_fps;
            b.hooked = 1;
            b.side_mask = side_mask;
            b.side_sign = side_sign;
            b.blur_px = if cfg.real_blur != 0 {
                (self.blur_out * 18.0).clamp(0.0, 22.0)
            } else {
                0.0
            };
        });

        let [w, h] = ui.io().display_size;
        if w < 16.0 || h < 16.0 {
            return;
        }

        let draw = ui.get_background_draw_list();
        // Clear zone from host gaze proxy — never yanked by side-restrictor / mouse.
        let cx = w * cfg.clear_x.clamp(0.04, 0.96);
        let cy = h * cfg.clear_y.clamp(0.04, 0.96);
        draw_vignette(
            &draw,
            w,
            h,
            self.vignette_out,
            cfg.vignette_softness,
            cfg.ground_visible != 0,
            cx,
            cy,
            cfg.clear_radius,
            cfg.foveated_clear != 0,
        );
        if side_mask > 0.02 {
            draw_side_restrictor(&draw, w, h, side_mask, side_sign, self.vignette_out);
        }
        let edge = (self.blur_out + cfg.flow_dampen).clamp(0.0, 1.0);
        if edge > 0.01 {
            if cfg.real_blur != 0 {
                draw_peripheral_frost(&draw, w, h, edge, cx, cy, cfg.clear_radius);
            } else {
                draw_edge_contrast(&draw, w, h, edge, cx, cy, cfg.clear_radius);
            }
        }
        draw_rest_frame(
            &draw,
            w,
            h,
            cfg.rest_style,
            self.rest_out,
            cfg.horizon_roll,
        );
    }
}

fn draw_vignette(
    draw: &imgui::DrawListMut,
    w: f32,
    h: f32,
    amount: f32,
    softness: f32,
    ground_visible: bool,
    cx: f32,
    cy: f32,
    clear_radius: f32,
    foveated: bool,
) {
    if amount <= 0.002 {
        return;
    }
    let bands = 14;
    let min_dim = w.min(h);
    let clear_r = if foveated {
        min_dim * clear_radius.clamp(0.2, 0.6)
    } else {
        min_dim * (0.58 - amount * 0.38)
    };
    let outer = min_dim * (0.78 + softness * 0.4);
    for i in 0..bands {
        let t0 = i as f32 / bands as f32;
        let t1 = (i + 1) as f32 / bands as f32;
        let a = (amount * t1.powf(1.35) * 0.95).clamp(0.0, 0.92);
        let col = imgui::ImColor32::from_rgba_f32s(0.0, 0.0, 0.0, a);
        let r0 = clear_r + (outer - clear_r) * t0;
        let r1 = clear_r + (outer - clear_r) * t1;

        // Approximate radial FOV rings as axis-aligned frames (imgui has no filled ring).
        let inset0 = (r0 - clear_r).max(0.0);
        let inset1 = (r1 - clear_r).max(0.0) + min_dim * 0.02 * amount;

        draw.add_rect([0.0, 0.0], [inset1.max(cx - clear_r + inset1 * 0.15), h], col)
            .filled(true)
            .build();
        draw.add_rect(
            [w - inset1.max(w - cx - clear_r + inset1 * 0.15), 0.0],
            [w, h],
            col,
        )
        .filled(true)
        .build();
        draw.add_rect([0.0, 0.0], [w, inset1.max(cy - clear_r + inset1 * 0.1)], col)
            .filled(true)
            .build();

        let bottom_a = if ground_visible { a * 0.35 } else { a };
        let col_b = imgui::ImColor32::from_rgba_f32s(0.0, 0.0, 0.0, bottom_a);
        draw.add_rect(
            [0.0, h - inset1.max(h - cy - clear_r + inset1 * 0.1)],
            [w, h],
            col_b,
        )
        .filled(true)
        .build();

        let _ = (inset0, r0);
    }
}

/// Wu & Rosenberg side restrictor: darken the high-flow side during yaw.
fn draw_side_restrictor(
    draw: &imgui::DrawListMut,
    w: f32,
    h: f32,
    amount: f32,
    sign: f32,
    vignette: f32,
) {
    let a = (amount * (0.35 + vignette * 0.45)).clamp(0.0, 0.85);
    if a < 0.01 {
        return;
    }
    let width = w * (0.18 + amount * 0.22);
    let bands = 10;
    for i in 0..bands {
        let t = (i as f32 + 1.0) / bands as f32;
        let col = imgui::ImColor32::from_rgba_f32s(0.0, 0.0, 0.0, a * t.powf(1.2));
        let band_w = width * t;
        if sign < 0.0 {
            // Darken left
            draw.add_rect([0.0, 0.0], [band_w, h], col)
                .filled(true)
                .build();
        } else {
            draw.add_rect([w - band_w, 0.0], [w, h], col)
                .filled(true)
                .build();
        }
    }
}

/// Multi-layer frost approximating peripheral Gaussian blur (imgui can't sample RT).
/// Desaturates / softens high-frequency peripheral detail that drives vection.
fn draw_peripheral_frost(
    draw: &imgui::DrawListMut,
    w: f32,
    h: f32,
    amount: f32,
    cx: f32,
    cy: f32,
    clear_radius: f32,
) {
    let keep = w.min(h) * clear_radius.clamp(0.2, 0.6) * 0.88;
    let layers = 8;
    for i in 0..layers {
        let t = (i as f32 + 1.0) / layers as f32;
        let inset = keep + (w.min(h) * 0.5 - keep) * t;
        let a = (amount * 0.07 * t.powf(1.1)).clamp(0.0, 0.22);
        // Warm-neutral frost (reduces contrast without pure black tunnel).
        let col = imgui::ImColor32::from_rgba_f32s(0.55, 0.58, 0.56, a);
        // Left
        let left = (cx - inset).max(0.0);
        if left > 2.0 {
            draw.add_rect([0.0, 0.0], [left, h], col)
                .filled(true)
                .build();
        }
        let right = (cx + inset).min(w);
        if w - right > 2.0 {
            draw.add_rect([right, 0.0], [w, h], col)
                .filled(true)
                .build();
        }
        let top = (cy - inset).max(0.0);
        if top > 2.0 {
            draw.add_rect([left, 0.0], [right, top], col)
                .filled(true)
                .build();
        }
        let bottom = (cy + inset).min(h);
        if h - bottom > 2.0 {
            draw.add_rect([left, bottom], [right, h], col)
                .filled(true)
                .build();
        }
    }
}

fn draw_edge_contrast(
    draw: &imgui::DrawListMut,
    w: f32,
    h: f32,
    amount: f32,
    cx: f32,
    cy: f32,
    clear_radius: f32,
) {
    let a = (amount * 0.28).clamp(0.0, 0.4);
    let col = imgui::ImColor32::from_rgba_f32s(0.02, 0.04, 0.03, a);
    let keep = w.min(h) * clear_radius.clamp(0.2, 0.6) * 0.85;
    let m = w.min(h) * 0.18;
    let left = (cx - keep).max(m * 0.35);
    let right = (w - (cx + keep)).max(m * 0.35);
    let top = (cy - keep).max(m * 0.25);
    let bottom = (h - (cy + keep)).max(m * 0.25);
    draw.add_rect([0.0, 0.0], [left, h], col).filled(true).build();
    draw.add_rect([w - right, 0.0], [w, h], col)
        .filled(true)
        .build();
    draw.add_rect([0.0, 0.0], [w, top], col).filled(true).build();
    draw.add_rect([0.0, h - bottom], [w, h], col)
        .filled(true)
        .build();
}

fn draw_rest_frame(
    draw: &imgui::DrawListMut,
    w: f32,
    h: f32,
    style: u32,
    opacity: f32,
    horizon_roll: f32,
) {
    if style == 0 || opacity <= 0.01 {
        return;
    }
    let col = imgui::ImColor32::from_rgba_f32s(0.92, 0.97, 0.94, opacity);
    let cx = w * 0.5;
    let cy = h * 0.5;

    match style {
        1 => {
            let arm = w.min(h) * 0.028;
            let gap = 6.0;
            draw.add_line([cx - arm, cy], [cx - gap, cy], col)
                .thickness(1.6)
                .build();
            draw.add_line([cx + gap, cy], [cx + arm, cy], col)
                .thickness(1.6)
                .build();
            draw.add_line([cx, cy - arm], [cx, cy - gap], col)
                .thickness(1.6)
                .build();
            draw.add_line([cx, cy + gap], [cx, cy + arm], col)
                .thickness(1.6)
                .build();
        }
        2 | 4 => {
            // Screen-fixed (2) or earth-fixed counter-rolled (4) horizon.
            let y = cy + h * 0.015;
            let roll = if style == 4 { horizon_roll } else { 0.0 };
            let (s, c) = roll.sin_cos();
            let half = w * 0.34;
            let x0 = cx - half * c;
            let y0 = y - half * s;
            let x1 = cx + half * c;
            let y1 = y + half * s;
            draw.add_line([x0, y0], [x1, y1], col)
                .thickness(1.7)
                .build();
            for i in -4i32..=4 {
                if i == 0 {
                    continue;
                }
                let along = i as f32 * w * 0.065;
                let px = cx + along * c;
                let py = y + along * s;
                let tick = if i.abs() == 4 { 9.0 } else { 5.0 };
                // Tick perpendicular to horizon.
                draw.add_line(
                    [px + tick * s, py - tick * c],
                    [px - tick * s, py + tick * c],
                    col,
                )
                .thickness(1.4)
                .build();
            }
        }
        3 => {
            let step = w.min(h) / 8.0;
            let gcol = imgui::ImColor32::from_rgba_f32s(0.92, 0.97, 0.94, opacity * 0.4);
            let mut x = step;
            while x < w {
                draw.add_line([x, 0.0], [x, h], gcol).thickness(1.0).build();
                x += step;
            }
            let mut y = step;
            while y < h {
                draw.add_line([0.0, y], [w, y], gcol).thickness(1.0).build();
                y += step;
            }
        }
        5 => {
            // Virtual nose — head-fixed rest frame at bottom-center.
            let tip_x = cx;
            let tip_y = h * 0.78;
            let base_y = h * 0.98;
            let half = w * 0.045;
            let nose = imgui::ImColor32::from_rgba_f32s(0.85, 0.9, 0.88, opacity * 0.55);
            draw.add_triangle([tip_x, tip_y], [cx - half, base_y], [cx + half, base_y], nose)
                .filled(true)
                .build();
            draw.add_line([tip_x, tip_y], [cx - half, base_y], col)
                .thickness(1.2)
                .build();
            draw.add_line([tip_x, tip_y], [cx + half, base_y], col)
                .thickness(1.2)
                .build();
        }
        _ => {}
    }
}

fn attach_hooks(hmodule: HINSTANCE) {
    // Install DX9/11/12 Present hooks. Only the API the game uses will fire.
    let result = Hudhook::builder()
        .with::<ImguiDx9Hooks>(MosickRenderLoop::new())
        .with::<ImguiDx11Hooks>(MosickRenderLoop::new())
        .with::<ImguiDx12Hooks>(MosickRenderLoop::new())
        .with_hmodule(hmodule)
        .build()
        .apply();

    if let Err(e) = result {
        eprintln!("MOSICKWHILE bridge: hook apply failed: {e:?}");
    } else {
        let _ = with_shm(|b| b.hooked = 1);
    }
}

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    hmodule: HINSTANCE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        let raw = hmodule.0 as usize;
        std::thread::spawn(move || {
            let hmodule = HINSTANCE(raw as _);
            // Small delay so the game's DXGI device exists.
            std::thread::sleep(std::time::Duration::from_millis(800));
            attach_hooks(hmodule);
        });
    }
    1
}
