//! High-precision pupil-in-socket gaze.
//!
//! Pipeline per eye:
//!   1. Adaptive dark percentile threshold (not fixed fraction of range)
//!   2. Intensity-weighted centroid with center prior
//!   3. Sub-pixel refine in local neighborhood
//!   4. Circularity / compactness gate (reject brows / shadows)
//!
//! Binocular fusion:
//!   Dual-eye agreement is the live accuracy proxy (no screen GT available).
//!   Quality = f(contrast, circularity, agreement, IPD sanity).

#[derive(Clone, Copy, Debug, Default)]
pub struct PupilGaze {
    /// Horizontal gaze in eye space: -1 = looking left (camera), +1 = right.
    pub gaze_x: f32,
    /// Vertical gaze: -1 = up, +1 = down.
    pub gaze_y: f32,
    pub roll: f32,
    /// Per-frame detection confidence 0–1.
    pub confidence: f32,
    pub ok: bool,
    /// Both pupils locked this frame.
    pub binocular: bool,
    /// 1 − normalized |L−R| gaze disagreement (live accuracy proxy).
    pub agreement: f32,
    /// Composite quality 0–1 (contrast × circularity × agreement × IPD).
    pub quality: f32,
}

/// Detect pupils inside a face bounding box and return binocular gaze + quality.
pub fn detect_pupil_gaze(
    gray: &[u8],
    stride: u32,
    img_h: u32,
    bx: i32,
    by: i32,
    bw: u32,
    bh: u32,
) -> PupilGaze {
    if bw < 48 || bh < 48 || gray.len() < (stride * img_h) as usize {
        return PupilGaze::default();
    }

    let left = eye_roi(bx, by, bw, bh, true, stride, img_h);
    let right = eye_roi(bx, by, bw, bh, false, stride, img_h);

    let lp = find_pupil(gray, stride, left);
    let rp = find_pupil(gray, stride, right);

    match (lp, rp) {
        (Some(l), Some(r)) => fuse_binocular(l, r, bw),
        (Some(l), None) | (None, Some(l)) => {
            if l.conf < 0.32 || l.circularity < 0.35 {
                return PupilGaze::default();
            }
            let quality = (l.conf * 0.55 + l.circularity * 0.45) * 0.62;
            PupilGaze {
                gaze_x: l.nx.clamp(-1.5, 1.5),
                gaze_y: l.ny.clamp(-1.5, 1.5),
                roll: 0.0,
                confidence: l.conf * 0.65,
                ok: quality >= 0.28,
                binocular: false,
                agreement: 0.45, // unknown without second eye
                quality,
            }
        }
        _ => PupilGaze::default(),
    }
}

fn fuse_binocular(l: PupilHit, r: PupilHit, face_w: u32) -> PupilGaze {
    let conf = ((l.conf + r.conf) * 0.5).clamp(0.0, 1.0);
    let circ = ((l.circularity + r.circularity) * 0.5).clamp(0.0, 1.0);
    if conf < 0.20 || circ < 0.28 {
        return PupilGaze::default();
    }

    // Gaze disagreement — primary live accuracy signal.
    let dx = (l.nx - r.nx).abs();
    let dy = (l.ny - r.ny).abs();
    // Eyes looking same direction: expect |Δ| ≲ 0.35 in normalized eye space.
    let agree = (1.0 - (dx * 0.9 + dy * 1.1) * 0.85).clamp(0.0, 1.0);

    // Inter-pupillary distance in face-width units (typical 0.22–0.42).
    let ipd = (r.px - l.px).abs() / (face_w as f32).max(1.0);
    let ipd_ok = if (0.18..=0.48).contains(&ipd) {
        1.0
    } else if (0.12..=0.55).contains(&ipd) {
        0.55
    } else {
        0.15
    };

    // Soft-reject gross disagreement (likely one false lock).
    if agree < 0.22 && conf < 0.55 {
        return PupilGaze::default();
    }

    // Weight by per-eye quality so a weak eye doesn't drag.
    let wl = (l.conf * l.circularity).max(0.05);
    let wr = (r.conf * r.circularity).max(0.05);
    let wsum = wl + wr;
    let gaze_x = (l.nx * wl + r.nx * wr) / wsum;
    let gaze_y = (l.ny * wl + r.ny * wr) / wsum;
    let roll = (r.py - l.py).atan2((r.px - l.px).max(1.0));

    let quality = (conf * 0.28 + circ * 0.27 + agree * 0.30 + ipd_ok * 0.15).clamp(0.0, 1.0);

    PupilGaze {
        gaze_x: gaze_x.clamp(-1.5, 1.5),
        gaze_y: gaze_y.clamp(-1.5, 1.5),
        roll,
        confidence: (conf * (0.55 + agree * 0.45)).clamp(0.0, 1.0),
        ok: quality >= 0.30,
        binocular: true,
        agreement: agree,
        quality,
    }
}

#[derive(Clone, Copy)]
struct Roi {
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
}

#[derive(Clone, Copy)]
struct PupilHit {
    px: f32,
    py: f32,
    nx: f32,
    ny: f32,
    conf: f32,
    circularity: f32,
}

fn eye_roi(bx: i32, by: i32, bw: u32, bh: u32, left: bool, stride: u32, img_h: u32) -> Roi {
    // Eyes: upper-mid face; each ~30% face width, ~18% face height.
    let y0 = (by as f32 + bh as f32 * 0.26).round() as i32;
    let y1 = (by as f32 + bh as f32 * 0.50).round() as i32;
    let (x0, x1) = if left {
        let a = (bx as f32 + bw as f32 * 0.10).round() as i32;
        let b = (bx as f32 + bw as f32 * 0.46).round() as i32;
        (a, b)
    } else {
        let a = (bx as f32 + bw as f32 * 0.54).round() as i32;
        let b = (bx as f32 + bw as f32 * 0.90).round() as i32;
        (a, b)
    };
    Roi {
        x0: x0.max(0) as u32,
        x1: x1.min(stride as i32).max(0) as u32,
        y0: y0.max(0) as u32,
        y1: y1.min(img_h as i32).max(0) as u32,
    }
}

/// Find pupil: adaptive dark mass → centroid → subpixel → circularity gate.
fn find_pupil(gray: &[u8], stride: u32, roi: Roi) -> Option<PupilHit> {
    if roi.x1 <= roi.x0 + 6 || roi.y1 <= roi.y0 + 4 {
        return None;
    }
    let iw = (roi.x1 - roi.x0) as f32;
    let ih = (roi.y1 - roi.y0) as f32;
    // Inset avoids lids / brows / canthi.
    let x0 = roi.x0 + (iw * 0.14) as u32;
    let x1 = roi.x1 - (iw * 0.14) as u32;
    let y0 = roi.y0 + (ih * 0.20) as u32;
    let y1 = roi.y1 - (ih * 0.16) as u32;
    if x1 <= x0 + 5 || y1 <= y0 + 4 {
        return None;
    }

    let rw = (x1 - x0) as usize;
    let rh = (y1 - y0) as usize;
    let n = rw * rh;
    if n < 24 {
        return None;
    }

    // —— Stats + histogram for percentile threshold ——
    let mut hist = [0u32; 256];
    let mut min_v = 255u8;
    let mut max_v = 0u8;
    let mut sum = 0u64;
    for y in y0..y1 {
        let row = (y * stride) as usize;
        for x in x0..x1 {
            let v = gray[row + x as usize];
            hist[v as usize] += 1;
            min_v = min_v.min(v);
            max_v = max_v.max(v);
            sum += v as u64;
        }
    }
    if max_v <= min_v + 10 {
        return None;
    }
    let mean = sum as f32 / n as f32;
    let contrast = (max_v - min_v) as f32 / 255.0;

    // Darkest ~11% of pixels (pupil+inner iris); clamp into useful band.
    let target = ((n as f32) * 0.11).max(8.0) as u32;
    let mut acc = 0u32;
    let mut pct_thresh = min_v;
    for (i, &c) in hist.iter().enumerate() {
        acc += c;
        if acc >= target {
            pct_thresh = i as u8;
            break;
        }
    }
    // Also require darker than mean − margin (reject flat lids).
    let thresh = (pct_thresh as f32)
        .min(mean - 10.0)
        .max(min_v as f32 + 2.0);

    let ecx = (x0 + x1) as f32 * 0.5;
    let ecy = (y0 + y1) as f32 * 0.5;
    let half_w = (iw * 0.5).max(1.0);
    let half_h = (ih * 0.5).max(1.0);

    let mut wsum = 0.0f32;
    let mut sx = 0.0f32;
    let mut sy = 0.0f32;
    let mut sxx = 0.0f32;
    let mut syy = 0.0f32;
    let mut sxy = 0.0f32;
    let mut dark_n = 0u32;

    for y in y0..y1 {
        let row = (y * stride) as usize;
        for x in x0..x1 {
            let v = gray[row + x as usize] as f32;
            if v > thresh {
                continue;
            }
            let dx = (x as f32 - ecx) / half_w;
            let dy = (y as f32 - ecy) / half_h;
            // Soft elliptical prior — pupils sit near eye center.
            let prior = (1.0 - (dx * dx * 0.55 + dy * dy * 0.70)).clamp(0.15, 1.0);
            let wgt = (thresh - v + 1.5).powf(2.15) * prior;
            wsum += wgt;
            sx += x as f32 * wgt;
            sy += y as f32 * wgt;
            sxx += x as f32 * x as f32 * wgt;
            syy += y as f32 * y as f32 * wgt;
            sxy += x as f32 * y as f32 * wgt;
            dark_n += 1;
        }
    }
    if wsum < 1.0 || dark_n < 5 {
        return None;
    }

    let mut px = sx / wsum;
    let mut py = sy / wsum;

    // Blob size gate.
    let frac = dark_n as f32 / n as f32;
    if !(0.018..=0.42).contains(&frac) {
        return None;
    }

    // Circularity from weighted covariance eigenvalues (λ1≈λ2 → round).
    let var_x = (sxx / wsum) - px * px;
    let var_y = (syy / wsum) - py * py;
    let cov_xy = (sxy / wsum) - px * py;
    let trace = var_x + var_y;
    let det = (var_x * var_y - cov_xy * cov_xy).max(0.0);
    let disc = (trace * trace * 0.25 - det).max(0.0).sqrt();
    let l1 = (trace * 0.5 + disc).max(1e-3);
    let l2 = (trace * 0.5 - disc).max(1e-3);
    let axis_ratio = (l2 / l1).sqrt().clamp(0.0, 1.0); // 1 = circle
    // Compactness vs expected pupil area ~ π r² with r from mean eigenvalue.
    let expected_r2 = ((l1 + l2) * 0.5).max(0.5);
    let area_score = (1.0 - (frac * n as f32 / (expected_r2 * 3.4) - 1.0).abs() * 0.45)
        .clamp(0.0, 1.0);
    let circularity = (axis_ratio * 0.7 + area_score * 0.3).clamp(0.0, 1.0);
    if circularity < 0.22 {
        return None;
    }

    // —— Sub-pixel refine: local intensity centroid in 7×7 ——
    refine_subpixel(gray, stride, &mut px, &mut py, x0, x1, y0, y1);

    // Reject if landed on a specular highlight (bright vs local dark).
    let sample = sample_gray(gray, stride, px, py);
    if sample > mean - 4.0 {
        // Nudge toward darkest neighbor in 5×5.
        if let Some((dx, dy)) = darkest_offset(gray, stride, px, py, x0, x1, y0, y1) {
            px += dx;
            py += dy;
        }
    }

    let nx = ((px - ecx) / (iw * 0.30).max(1.0)).clamp(-1.65, 1.65);
    let ny = ((py - ecy) / (ih * 0.26).max(1.0)).clamp(-1.65, 1.65);

    // Confidence: contrast + ideal blob fraction (~0.10) + circularity.
    let frac_score = (1.0 - (frac - 0.10).abs() * 2.2).clamp(0.0, 1.0);
    let conf = (contrast * 1.25 * 0.40 + frac_score * 0.25 + circularity * 0.35).clamp(0.0, 1.0);

    Some(PupilHit {
        px,
        py,
        nx,
        ny,
        conf,
        circularity,
    })
}

fn refine_subpixel(
    gray: &[u8],
    stride: u32,
    px: &mut f32,
    py: &mut f32,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
) {
    let cx = px.round() as i32;
    let cy = py.round() as i32;
    let mut wsum = 0.0f32;
    let mut sx = 0.0f32;
    let mut sy = 0.0f32;
    for dy in -3i32..=3 {
        for dx in -3i32..=3 {
            let x = cx + dx;
            let y = cy + dy;
            if x < x0 as i32 || x >= x1 as i32 || y < y0 as i32 || y >= y1 as i32 {
                continue;
            }
            let v = gray[(y as u32 * stride + x as u32) as usize] as f32;
            // Inverse intensity — darker pulls harder.
            let w = (255.0 - v).powf(2.4);
            wsum += w;
            sx += x as f32 * w;
            sy += y as f32 * w;
        }
    }
    if wsum > 1.0 {
        *px = sx / wsum;
        *py = sy / wsum;
    }
}

fn sample_gray(gray: &[u8], stride: u32, px: f32, py: f32) -> f32 {
    let x = px.round().clamp(0.0, (stride - 1) as f32) as u32;
    let y = py.round().max(0.0) as u32;
    let idx = (y * stride + x) as usize;
    if idx < gray.len() {
        gray[idx] as f32
    } else {
        255.0
    }
}

fn darkest_offset(
    gray: &[u8],
    stride: u32,
    px: f32,
    py: f32,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
) -> Option<(f32, f32)> {
    let cx = px.round() as i32;
    let cy = py.round() as i32;
    let mut best = 255u8;
    let mut bx = 0i32;
    let mut by = 0i32;
    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            let x = cx + dx;
            let y = cy + dy;
            if x < x0 as i32 || x >= x1 as i32 || y < y0 as i32 || y >= y1 as i32 {
                continue;
            }
            let v = gray[(y as u32 * stride + x as u32) as usize];
            if v < best {
                best = v;
                bx = dx;
                by = dy;
            }
        }
    }
    if best >= 250 {
        return None;
    }
    Some((bx as f32, by as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paint a dark elliptical pupil on a mid-gray eye patch.
    fn synth_eye(w: u32, h: u32, cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<u8> {
        let mut img = vec![160u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let dx = (x as f32 - cx) / rx;
                let dy = (y as f32 - cy) / ry;
                let d2 = dx * dx + dy * dy;
                if d2 <= 1.0 {
                    // Soft pupil edge + darker core.
                    let t = (1.0 - d2).clamp(0.0, 1.0);
                    img[(y * w + x) as usize] = (28.0 + (1.0 - t) * 40.0) as u8;
                }
            }
        }
        // Mild noise.
        for (i, p) in img.iter_mut().enumerate() {
            let n = ((i * 17) % 7) as u8;
            *p = p.saturating_add(n).saturating_sub(3);
        }
        img
    }

    #[test]
    fn synthetic_pupil_centroid_subpixel() {
        let w = 64u32;
        let h = 40u32;
        let true_x = 18.4f32;
        let true_y = 15.2f32;
        let img = synth_eye(w, h, true_x, true_y, 5.5, 5.0);
        let roi = eye_roi(0, 0, w, h, true, w, h);
        let hit = find_pupil(&img, w, roi).expect("pupil should be found");
        let err = (hit.px - true_x).hypot(hit.py - true_y);
        assert!(
            err < 0.75,
            "centroid error {err:.3}px (got {},{} want {},{})",
            hit.px,
            hit.py,
            true_x,
            true_y
        );
        assert!(hit.circularity > 0.4, "circularity {}", hit.circularity);
        assert!(hit.conf > 0.35, "conf {}", hit.conf);
    }

    /// Bench: mean centroid error across a grid of known pupil positions.
    /// This is the lab accuracy number (synthetic GT) — not webcam accuracy.
    #[test]
    fn synthetic_grid_mean_error_under_one_pixel() {
        let w = 64u32;
        let h = 40u32;
        let roi = eye_roi(0, 0, w, h, true, w, h);
        let mut sum = 0.0f32;
        let mut n = 0u32;
        let mut max_e = 0.0f32;
        // Sweep inside the inset ROI with margin for pupil radius.
        for yi in 0..5 {
            for xi in 0..5 {
                let true_x = 12.0 + xi as f32 * 2.5;
                let true_y = 13.0 + yi as f32 * 1.2;
                let img = synth_eye(w, h, true_x, true_y, 4.8, 4.5);
                let Some(hit) = find_pupil(&img, w, roi) else {
                    continue;
                };
                let err = (hit.px - true_x).hypot(hit.py - true_y);
                sum += err;
                max_e = max_e.max(err);
                n += 1;
            }
        }
        assert!(n >= 18, "too few detections: {n}");
        let mean = sum / n as f32;
        assert!(
            mean < 1.0,
            "mean centroid error {mean:.3}px (max {max_e:.3}, n={n})"
        );
        assert!(max_e < 2.0, "max error {max_e:.3}px");
    }

    #[test]
    fn binocular_agreement_high_when_both_look_same() {
        let w = 120u32;
        let h = 80u32;
        let mut img = vec![165u8; (w * h) as usize];
        // Two pupils with same relative offset inside their eye boxes.
        // Left eye center ~ 0.28*w, right ~ 0.72*w; y ~ 0.38*h
        let ly = h as f32 * 0.38;
        let lx = w as f32 * 0.28 + 3.0; // looking slightly right in both
        let rx = w as f32 * 0.72 + 3.0;
        paint_disk(&mut img, w, lx, ly, 4.5, 22);
        paint_disk(&mut img, w, rx, ly, 4.5, 22);
        let g = detect_pupil_gaze(&img, w, h, 0, 0, w, h);
        assert!(g.ok, "should lock");
        assert!(g.binocular, "both eyes");
        assert!(
            g.agreement > 0.75,
            "agreement {} too low",
            g.agreement
        );
        assert!(g.quality > 0.45, "quality {}", g.quality);
    }

    fn paint_disk(img: &mut [u8], stride: u32, cx: f32, cy: f32, r: f32, val: u8) {
        let r2 = r * r;
        let x0 = (cx - r - 1.0).max(0.0) as u32;
        let x1 = (cx + r + 1.0).min(stride as f32 - 1.0) as u32;
        let y0 = (cy - r - 1.0).max(0.0) as u32;
        let h = img.len() as u32 / stride;
        let y1 = (cy + r + 1.0).min(h as f32 - 1.0) as u32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                if dx * dx + dy * dy <= r2 {
                    img[(y * stride + x) as usize] = val;
                }
            }
        }
    }
}
