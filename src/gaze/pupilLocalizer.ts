/**
 * Per-eye pupil localization from webcam crops.
 * Designed for glasses: reject specular glare, adaptive dark mass, subpixel centroid.
 */

export type EyeRoi = {
  /** Pixel rect in video space */
  x: number;
  y: number;
  w: number;
  h: number;
};

export type PupilHit = {
  /** Gaze in eye socket: −1…1 (already screen-mirrored) */
  gx: number;
  gy: number;
  quality: number;
  glare: number;
  ok: boolean;
};

const EYE_W = 96;
const EYE_H = 64;

export class PupilLocalizer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private scratch: Uint8ClampedArray | null = null;

  constructor() {
    this.canvas = document.createElement("canvas");
    this.canvas.width = EYE_W;
    this.canvas.height = EYE_H;
    const ctx = this.canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) throw new Error("2d context");
    this.ctx = ctx;
  }

  locate(
    video: HTMLVideoElement,
    outer: { x: number; y: number },
    inner: { x: number; y: number },
    top: { x: number; y: number },
    bot: { x: number; y: number },
  ): PupilHit {
    const vw = video.videoWidth;
    const vh = video.videoHeight;
    if (vw < 32 || vh < 32) return fail();

    const midX = ((outer.x + inner.x) * 0.5) * vw;
    const midY = ((top.y + bot.y) * 0.5) * vh;
    const eyeW = Math.max(Math.abs(inner.x - outer.x) * vw * 1.35, 18);
    const eyeH = Math.max(Math.abs(bot.y - top.y) * vh * 1.55, 12);
    const x0 = midX - eyeW * 0.5;
    const y0 = midY - eyeH * 0.5;

    this.ctx.drawImage(video, x0, y0, eyeW, eyeH, 0, 0, EYE_W, EYE_H);
    const img = this.ctx.getImageData(0, 0, EYE_W, EYE_H);
    const gray = this.toGray(img.data);

    // Specular glare (glasses reflections) — bright outliers.
    let bright = 0;
    let sum = 0;
    let minV = 255;
    let maxV = 0;
    const n = EYE_W * EYE_H;
    for (let i = 0; i < n; i++) {
      const v = gray[i];
      sum += v;
      minV = Math.min(minV, v);
      maxV = Math.max(maxV, v);
      if (v > 220) bright++;
    }
    const mean = sum / n;
    const glare = bright / n;
    const contrast = (maxV - minV) / 255;
    if (contrast < 0.08) return fail(glare);

    // Suppress glare pixels (treat as missing) before dark search.
    const masked = new Uint8Array(n);
    for (let i = 0; i < n; i++) {
      masked[i] = gray[i] > 210 ? 255 : gray[i];
    }

    // Adaptive dark threshold: darkest ~12% among non-glare pixels.
    const hist = new Array<number>(256).fill(0);
    let valid = 0;
    for (let i = 0; i < n; i++) {
      if (masked[i] >= 250) continue;
      hist[masked[i]]++;
      valid++;
    }
    if (valid < 40) return fail(glare);

    const target = Math.max(8, Math.floor(valid * 0.12));
    let acc = 0;
    let thresh = minV;
    for (let i = 0; i < 256; i++) {
      acc += hist[i];
      if (acc >= target) {
        thresh = i;
        break;
      }
    }
    thresh = Math.min(thresh, Math.floor(mean - 8));
    thresh = Math.max(thresh, minV + 2);

    // Soft elliptical prior near eye center (pupils sit mid-socket).
    let wsum = 0;
    let sx = 0;
    let sy = 0;
    let darkN = 0;
    const cx = EYE_W * 0.5;
    const cy = EYE_H * 0.52;
    for (let y = 4; y < EYE_H - 4; y++) {
      for (let x = 6; x < EYE_W - 6; x++) {
        const i = y * EYE_W + x;
        const v = masked[i];
        if (v > thresh || v >= 250) continue;
        const dx = (x - cx) / (EYE_W * 0.42);
        const dy = (y - cy) / (EYE_H * 0.42);
        const prior = Math.max(0.12, 1 - (dx * dx * 0.55 + dy * dy * 0.75));
        const w = (thresh - v + 1.5) ** 2.1 * prior;
        wsum += w;
        sx += x * w;
        sy += y * w;
        darkN++;
      }
    }
    if (wsum < 1 || darkN < 6) return fail(glare);

    let px = sx / wsum;
    let py = sy / wsum;

    // Subpixel refine in 5×5 around centroid (ignore glare).
    let rSum = 0;
    let rx = 0;
    let ry = 0;
    for (let y = Math.floor(py) - 2; y <= Math.floor(py) + 2; y++) {
      for (let x = Math.floor(px) - 2; x <= Math.floor(px) + 2; x++) {
        if (x < 0 || y < 0 || x >= EYE_W || y >= EYE_H) continue;
        const v = masked[y * EYE_W + x];
        if (v >= 250) continue;
        const w = Math.max(0, 40 - v);
        rSum += w;
        rx += x * w;
        ry += y * w;
      }
    }
    if (rSum > 1) {
      px = rx / rSum;
      py = ry / rSum;
    }

    const frac = darkN / n;
    if (frac < 0.015 || frac > 0.45) return fail(glare);

    // Circularity proxy via second moments.
    let sxx = 0;
    let syy = 0;
    for (let y = 4; y < EYE_H - 4; y++) {
      for (let x = 6; x < EYE_W - 6; x++) {
        const i = y * EYE_W + x;
        const v = masked[i];
        if (v > thresh || v >= 250) continue;
        const dx = x - px;
        const dy = y - py;
        const w = thresh - v + 1;
        sxx += dx * dx * w;
        syy += dy * dy * w;
      }
    }
    const ratio =
      Math.min(sxx, syy) / Math.max(sxx, syy, 1e-3);
    const circularity = clamp(ratio, 0, 1);

    // Gaze in socket (camera space) then mirror X for screen.
    const gxCam = (px - cx) / (EYE_W * 0.42);
    const gyCam = (py - cy) / (EYE_H * 0.42);
    const gx = -clamp(gxCam, -1.7, 1.7);
    const gy = clamp(gyCam, -1.7, 1.7);

    // Glasses: high glare lowers quality — caller will lean on ONNX.
    const quality = clamp(
      circularity * 0.4 +
        (1 - glare * 3.2) * 0.35 +
        contrast * 0.25 +
        (frac > 0.03 && frac < 0.3 ? 0.1 : 0),
      0,
      1,
    );

    return {
      gx,
      gy,
      quality,
      glare,
      ok: quality >= 0.28 && glare < 0.22,
    };
  }

  private toGray(rgba: Uint8ClampedArray): Uint8Array {
    const n = EYE_W * EYE_H;
    if (!this.scratch || this.scratch.length !== n) {
      this.scratch = new Uint8ClampedArray(n);
    }
    const out = this.scratch;
    for (let i = 0; i < n; i++) {
      const r = rgba[i * 4];
      const g = rgba[i * 4 + 1];
      const b = rgba[i * 4 + 2];
      // Luma; slightly favor darker channel (pupil under warm LEDs).
      out[i] = (r * 0.25 + g * 0.55 + b * 0.2) | 0;
    }
    return out as unknown as Uint8Array;
  }
}

function fail(glare = 1): PupilHit {
  return { gx: 0, gy: 0, quality: 0, glare, ok: false };
}

function clamp(v: number, a: number, b: number): number {
  return Math.min(b, Math.max(a, v));
}

export const pupilLocalizer = new PupilLocalizer();
