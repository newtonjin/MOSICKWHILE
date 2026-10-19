/** Gaze → screen map (ridge regression) + drift offset. */

export type Point = { x: number; y: number };

export type GazeFeatures = {
  /** Fused pupil ⊕ ONNX gaze proxy −1…1 */
  gx: number;
  gy: number;
  /** Head pose from MediaPipe matrix */
  yaw: number;
  pitch: number;
  /** Appearance gaze from ONNX (radians) */
  onnxYaw: number;
  onnxPitch: number;
  /** Pupil localizer quality 0–1 (low with glasses glare) */
  pupilQ: number;
};

export type CalibSample = GazeFeatures & Point & { weight?: number };

export type GazeMap = {
  bx: number[];
  by: number[];
  driftX: number;
  driftY: number;
  updatedAt: number;
  version: 3;
};

const STORAGE_KEY = "mosickwhile.gazeCalib.v3";
const FEAT_N = 9;

/** 13 targets — denser on right/left edges (split UI / Discord). */
export const CALIB_TARGETS: Point[] = [
  { x: 0.5, y: 0.5 },
  { x: 0.08, y: 0.1 },
  { x: 0.5, y: 0.08 },
  { x: 0.92, y: 0.1 },
  { x: 0.08, y: 0.5 },
  { x: 0.92, y: 0.5 },
  { x: 0.08, y: 0.9 },
  { x: 0.5, y: 0.92 },
  { x: 0.92, y: 0.9 },
  // Extra lateral samples — glasses often bias these.
  { x: 0.96, y: 0.3 },
  { x: 0.96, y: 0.7 },
  { x: 0.04, y: 0.3 },
  { x: 0.04, y: 0.7 },
];

function feats(f: GazeFeatures): number[] {
  return [
    1,
    f.gx,
    f.gy,
    f.yaw,
    f.pitch,
    f.onnxYaw,
    f.onnxPitch,
    f.gx * f.onnxYaw,
    f.pupilQ,
  ];
}

function ridge(samples: CalibSample[], axis: "x" | "y", lambda = 0.12): number[] {
  const xtx = Array.from({ length: FEAT_N }, () => new Array<number>(FEAT_N).fill(0));
  const xty = new Array<number>(FEAT_N).fill(0);

  for (const s of samples) {
    const w = s.weight ?? 1;
    const x = feats(s);
    const y = axis === "x" ? s.x : s.y;
    for (let i = 0; i < FEAT_N; i++) {
      xty[i] += w * x[i] * y;
      for (let j = 0; j < FEAT_N; j++) {
        xtx[i][j] += w * x[i] * x[j];
      }
    }
  }
  for (let i = 0; i < FEAT_N; i++) {
    xtx[i][i] += lambda * (i === 0 ? 0.02 : 1);
  }
  return solve(xtx, xty);
}

function solve(a: number[][], b: number[]): number[] {
  const n = b.length;
  const m = a.map((row, i) => [...row, b[i]]);
  for (let col = 0; col < n; col++) {
    let pivot = col;
    for (let r = col + 1; r < n; r++) {
      if (Math.abs(m[r][col]) > Math.abs(m[pivot][col])) pivot = r;
    }
    [m[col], m[pivot]] = [m[pivot], m[col]];
    const div = m[col][col] || 1e-9;
    for (let c = col; c <= n; c++) m[col][c] /= div;
    for (let r = 0; r < n; r++) {
      if (r === col) continue;
      const f = m[r][col];
      for (let c = col; c <= n; c++) m[r][c] -= f * m[col][c];
    }
  }
  return m.map((row) => row[n]);
}

function applyBeta(beta: number[], f: GazeFeatures): number {
  const x = feats(f);
  let s = 0;
  for (let i = 0; i < FEAT_N; i++) s += beta[i] * x[i];
  return s;
}

export function fitMap(samples: CalibSample[]): GazeMap | null {
  if (samples.length < 7) return null;
  return {
    bx: ridge(samples, "x"),
    by: ridge(samples, "y"),
    driftX: 0,
    driftY: 0,
    updatedAt: Date.now(),
    version: 3,
  };
}

export function mapGaze(map: GazeMap | null, f: GazeFeatures): Point {
  if (!map || map.bx.length !== FEAT_N) {
    const ox = Math.sin(f.onnxYaw) * Math.cos(f.onnxPitch);
    const oy = Math.sin(f.onnxPitch);
    // Stronger lateral reach when pupilQ is low (glasses → lean ONNX).
    const irisW = 0.22 + f.pupilQ * 0.18;
    const onnxW = 0.42 + (1 - f.pupilQ) * 0.2;
    return {
      x: clamp01(0.5 + f.gx * irisW + ox * onnxW - f.yaw * 0.08),
      y: clamp01(0.5 + f.gy * irisW + oy * onnxW + f.pitch * 0.07),
    };
  }
  return {
    x: clamp01(applyBeta(map.bx, f) + map.driftX),
    y: clamp01(applyBeta(map.by, f) + map.driftY),
  };
}

export function loadMap(): GazeMap | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const m = JSON.parse(raw) as GazeMap;
    if (!m?.bx?.length || m.bx.length !== FEAT_N || m.version !== 3) return null;
    return m;
  } catch {
    return null;
  }
}

export function saveMap(map: GazeMap): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
}

export function clearMap(): void {
  localStorage.removeItem(STORAGE_KEY);
}

export function applyRecenter(map: GazeMap, f: GazeFeatures): GazeMap {
  const p = mapGaze({ ...map, driftX: 0, driftY: 0 }, f);
  return {
    ...map,
    driftX: clamp(map.driftX + (0.5 - p.x) * 0.7, -0.22, 0.22),
    driftY: clamp(map.driftY + (0.5 - p.y) * 0.7, -0.22, 0.22),
    updatedAt: Date.now(),
  };
}

/** Median of numeric samples. */
export function median(vals: number[]): number {
  if (!vals.length) return 0;
  const a = [...vals].sort((x, y) => x - y);
  const m = Math.floor(a.length / 2);
  return a.length % 2 ? a[m] : (a[m - 1] + a[m]) * 0.5;
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

function clamp(v: number, a: number, b: number): number {
  return Math.min(b, Math.max(a, v));
}
