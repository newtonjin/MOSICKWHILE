//! Peripheral-weighted optical flow for cybersickness drive estimation.
//!
//! Critical distinction (literature + computer vision):
//! - Cybersickness is driven by **vection** from coherent self-motion optical flow
//! - Global brightness / exposure / muzzle-flash / scene cuts are **photometric**
//!   changes, NOT vection. Treating them as motion makes FOV restriction flicker,
//!   which itself increases discomfort (unstable FOV is a known anti-pattern;
//!   adaptive FOV papers smooth parameters ~0.5s).
//!
//! This analyzer therefore:
//! 1. Mean-normalizes luminance between frames (photometric invariance)
//! 2. Uses zero-mean block matching (ZNCC-lite via mean-subtracted SAD)
//! 3. Attenuates drive when global brightness jumps without spatial coherence
//! 4. Rewards directionally coherent peripheral flow (true locomotion/turn)

const ANALYZE_W: usize = 160;
const ANALYZE_H: usize = 90;
const BLOCK: usize = 8;
const SEARCH: i32 = 4;

#[derive(Debug, Clone, Default)]
pub struct FlowSample {
    pub magnitude: f32,
    pub peripheral_flow: f32,
    pub central_flow: f32,
    pub flow_x: f32,
    pub flow_y: f32,
    pub rotational: f32,
    pub translational: f32,
    /// Estimated camera/scene roll in ≈[-1, 1] from opposing vertical flow
    /// on left vs right (for earth-fixed horizon).
    pub roll: f32,
}

pub struct OpticalFlowAnalyzer {
    prev: Option<Vec<u8>>,
    prev_mean: f32,
    width: usize,
    height: usize,
    scratch: Vec<u8>,
}

impl Default for OpticalFlowAnalyzer {
    fn default() -> Self {
        Self {
            prev: None,
            prev_mean: 0.0,
            width: ANALYZE_W,
            height: ANALYZE_H,
            scratch: vec![0u8; ANALYZE_W * ANALYZE_H],
        }
    }
}

impl OpticalFlowAnalyzer {
    /// Ingest a BGRA8 framebuffer; returns None on the first frame (no baseline).
    pub fn push_bgra(&mut self, bgra: &[u8], src_w: usize, src_h: usize) -> Option<FlowSample> {
        if src_w == 0 || src_h == 0 || bgra.len() < src_w * src_h * 4 {
            return None;
        }

        downsample_bgra_luma(bgra, src_w, src_h, &mut self.scratch, self.width, self.height);
        let mean = mean_luma(&self.scratch);

        let sample = if let Some(prev) = &self.prev {
            // Photometric match: shift current so means align before matching.
            let mut matched = self.scratch.clone();
            let delta = self.prev_mean - mean;
            for p in &mut matched {
                *p = (*p as f32 + delta).clamp(0.0, 255.0) as u8;
            }

            let global_brightness_jump = (mean - self.prev_mean).abs() / 255.0;
            let mut flow = block_match_flow(prev, &matched, self.width, self.height);

            // Scene cuts / flashes: large global ΔL + low directional coherence
            // → suppress (not vection).
            let coherence = flow_coherence(flow.flow_x, flow.flow_y, flow.peripheral_flow);
            let photo_gate = (1.0 - (global_brightness_jump * 2.4 - coherence * 0.6).clamp(0.0, 0.95))
                .clamp(0.05, 1.0);
            flow.magnitude *= photo_gate;
            flow.peripheral_flow *= photo_gate;
            flow.rotational *= photo_gate;
            flow.translational *= photo_gate;
            flow.roll *= photo_gate;
            Some(flow)
        } else {
            None
        };

        self.prev = Some(self.scratch.clone());
        self.prev_mean = mean;
        sample
    }

    pub fn reset(&mut self) {
        self.prev = None;
        self.prev_mean = 0.0;
    }
}

fn mean_luma(buf: &[u8]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    let sum: u32 = buf.iter().map(|&v| v as u32).sum();
    sum as f32 / buf.len() as f32
}

/// How much the mean flow vector agrees with "there is real directed motion".
fn flow_coherence(fx: f32, fy: f32, peri: f32) -> f32 {
    let vec_mag = (fx * fx + fy * fy).sqrt();
    if peri < 0.02 {
        return 1.0; // quiet — don't punish
    }
    (vec_mag / peri.max(0.02)).clamp(0.0, 1.0)
}

fn downsample_bgra_luma(
    bgra: &[u8],
    src_w: usize,
    src_h: usize,
    out: &mut [u8],
    dst_w: usize,
    dst_h: usize,
) {
    for y in 0..dst_h {
        let sy = y * src_h / dst_h;
        for x in 0..dst_w {
            let sx = x * src_w / dst_w;
            let i = (sy * src_w + sx) * 4;
            let b = bgra[i] as u32;
            let g = bgra[i + 1] as u32;
            let r = bgra[i + 2] as u32;
            out[y * dst_w + x] = ((25 * b + 129 * g + 66 * r + 128) >> 8) as u8;
        }
    }
}

fn block_match_flow(prev: &[u8], curr: &[u8], w: usize, h: usize) -> FlowSample {
    let mut peri_mag = 0.0f32;
    let mut peri_n = 0.0f32;
    let mut cent_mag = 0.0f32;
    let mut cent_n = 0.0f32;
    let mut sum_vx = 0.0f32;
    let mut sum_vy = 0.0f32;
    let mut sum_rot = 0.0f32;
    let mut sum_trans = 0.0f32;
    let mut vec_n = 0.0f32;
    let mut left_vy = 0.0f32;
    let mut left_n = 0.0f32;
    let mut right_vy = 0.0f32;
    let mut right_n = 0.0f32;

    let cx = (w as f32) * 0.5;
    let cy = (h as f32) * 0.5;
    let rx = (w as f32) * 0.28;
    let ry = (h as f32) * 0.28;

    let mut y = 0;
    while y + BLOCK <= h {
        let mut x = 0;
        while x + BLOCK <= w {
            let (vx, vy, sad) = match_block_zncc_lite(prev, curr, w, h, x, y);
            let mag = (vx * vx + vy * vy).sqrt();
            // Reject unstructured intensity noise (photometric leftovers).
            if sad < 6.0 && mag < 0.4 {
                x += BLOCK;
                continue;
            }
            // High residual with near-zero displacement = flash / fade, not motion.
            if mag < 0.5 && sad > 28.0 {
                x += BLOCK;
                continue;
            }

            let bx = x as f32 + BLOCK as f32 * 0.5;
            let by = y as f32 + BLOCK as f32 * 0.5;
            let nx = (bx - cx) / rx;
            let ny = (by - cy) / ry;
            let radial = (nx * nx + ny * ny).sqrt();
            let peripheral = radial > 1.0;

            let tx = -ny;
            let ty = nx;
            let tang = (vx * tx + vy * ty).abs();
            let rad_c = (vx * nx + vy * ny).abs();

            if peripheral {
                peri_mag += mag;
                peri_n += 1.0;
            } else {
                cent_mag += mag;
                cent_n += 1.0;
            }

            sum_vx += vx;
            sum_vy += vy;
            sum_rot += tang;
            sum_trans += rad_c;
            vec_n += 1.0;
            if bx < cx {
                left_vy += vy;
                left_n += 1.0;
            } else {
                right_vy += vy;
                right_n += 1.0;
            }

            x += BLOCK;
        }
        y += BLOCK;
    }

    let peri = if peri_n > 0.0 { peri_mag / peri_n } else { 0.0 };
    let cent = if cent_n > 0.0 { cent_mag / cent_n } else { 0.0 };
    let inv = if vec_n > 0.0 { 1.0 / vec_n } else { 0.0 };
    let flow_x = sum_vx * inv;
    let flow_y = sum_vy * inv;
    let rotational = sum_rot * inv;
    let translational = sum_trans * inv;
    let lvy = if left_n > 0.0 { left_vy / left_n } else { 0.0 };
    let rvy = if right_n > 0.0 { right_vy / right_n } else { 0.0 };
    // Opposing vertical shear ≈ roll of the visual field.
    let roll = ((rvy - lvy) / (2.0 * SEARCH as f32)).clamp(-1.0, 1.0);

    let norm = SEARCH as f32;
    let peri_n = (peri / norm).clamp(0.0, 1.0);
    let cent_n = (cent / norm).clamp(0.0, 1.0);
    let rot_n = (rotational / norm).clamp(0.0, 1.0);
    let trans_n = (translational / norm).clamp(0.0, 1.0);

    // Uniform "motion" everywhere (cut/flash) is not vection — down-weight.
    let peri_cent_ratio = peri_n / (cent_n + 0.08);
    let structure = peri_cent_ratio.clamp(0.35, 1.4) / 1.4;

    let magnitude = ((0.62 * peri_n + 0.28 * rot_n + 0.18 * trans_n) * structure
        - 0.08 * (1.0 - cent_n).max(0.0))
        .clamp(0.0, 1.0);

    FlowSample {
        magnitude,
        peripheral_flow: peri_n,
        central_flow: cent_n,
        flow_x: (flow_x / norm).clamp(-1.0, 1.0),
        flow_y: (flow_y / norm).clamp(-1.0, 1.0),
        rotational: rot_n,
        translational: trans_n,
        roll,
    }
}

/// Zero-mean SAD (lightweight ZNCC stand-in): subtract local block means so
/// global/local brightness offsets don't win the match.
fn match_block_zncc_lite(
    prev: &[u8],
    curr: &[u8],
    w: usize,
    h: usize,
    bx: usize,
    by: usize,
) -> (f32, f32, f32) {
    let mut curr_sum = 0i32;
    for j in 0..BLOCK {
        let row = (by + j) * w + bx;
        for i in 0..BLOCK {
            curr_sum += curr[row + i] as i32;
        }
    }
    let curr_mean = curr_sum / (BLOCK * BLOCK) as i32;

    let mut best_sad = i32::MAX;
    let mut best_dx = 0i32;
    let mut best_dy = 0i32;

    for dy in -SEARCH..=SEARCH {
        for dx in -SEARCH..=SEARCH {
            let sx = bx as i32 + dx;
            let sy = by as i32 + dy;
            if sx < 0 || sy < 0 || sx as usize + BLOCK > w || sy as usize + BLOCK > h {
                continue;
            }

            let mut prev_sum = 0i32;
            for j in 0..BLOCK {
                let row = (sy as usize + j) * w + sx as usize;
                for i in 0..BLOCK {
                    prev_sum += prev[row + i] as i32;
                }
            }
            let prev_mean = prev_sum / (BLOCK * BLOCK) as i32;

            let mut sad = 0i32;
            for j in 0..BLOCK {
                let row_a = (by + j) * w + bx;
                let row_b = (sy as usize + j) * w + sx as usize;
                for i in 0..BLOCK {
                    let a = curr[row_a + i] as i32 - curr_mean;
                    let b = prev[row_b + i] as i32 - prev_mean;
                    sad += (a - b).abs();
                }
            }
            if sad < best_sad {
                best_sad = sad;
                best_dx = dx;
                best_dy = dy;
            }
        }
    }

    let mean_sad = best_sad as f32 / (BLOCK * BLOCK) as f32;
    (best_dx as f32, best_dy as f32, mean_sad)
}
