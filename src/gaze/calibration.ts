/** 9-point gaze → screen map (ridge regression) + drift offset. */

export type Point = { x: number; y: number };

export type GazeFeatures = {
  /** Fused iris ⊕ ONNX gaze proxy −1…1 */
  gx: number;
  gy: number;
  /** Head pose from MediaPipe matrix */
  yaw: number;
  pitch: number;
  /** Appearance gaze from ONNX (radians, mild-normalized) */
  onnxYaw: number;
  onnxPitch: number;
};

export type CalibSample = GazeFeatures & Point;

export type GazeMap = {
  /** sx = bx · feats(f) */
  bx: number[];
  by: number[];
  driftX: number;
  driftY: number;
  updatedAt: number;
  version: 2;
};

const STORAGE_KEY = "mosickwhile.gazeCalib.v2";
const FEAT_N = 8;

export const CALIB_TARGETS: Point[] = [
  { x: 0.5, y: 0.5 },
  { x: 0.12, y: 0.12 },
  { x: 0.5, y: 0.12 },
  { x: 0.88, y: 0.12 },
  { x: 0.12, y: 0.5 },
  { x: 0.88, y: 0.5 },
  { x: 0.12, y: 0.88 },
  { x: 0.5, y: 0.88 },
  { x: 0.88, y: 0.88 },
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
  ];
}

/** Solve (XᵀX + λI) β = Xᵀy with tiny ridge. */
function ridge(samples: CalibSample[], axis: "x" | "y", lambda = 0.1): number[] {
  const xtx = Array.from({ length: FEAT_N }, () => new Array<number>(FEAT_N).fill(0));
  const xty = new Array<number>(FEAT_N).fill(0);

  for (const s of samples) {
    const x = feats(s);
    const y = axis === "x" ? s.x : s.y;
    for (let i = 0; i < FEAT_N; i++) {
      xty[i] += x[i] * y;
      for (let j = 0; j < FEAT_N; j++) {
        xtx[i][j] += x[i] * x[j];
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
  if (samples.length < 5) return null;
  return {
    bx: ridge(samples, "x"),
    by: ridge(samples, "y"),
    driftX: 0,
    driftY: 0,
    updatedAt: Date.now(),
    version: 2,
  };
}

/** Uncalibrated: blend iris socket + ONNX angles → rough screen point. */
export function mapGaze(map: GazeMap | null, f: GazeFeatures): Point {
  if (!map || map.bx.length !== FEAT_N) {
    const ox = Math.sin(f.onnxYaw) * Math.cos(f.onnxPitch);
    const oy = Math.sin(f.onnxPitch);
    return {
      x: clamp01(0.5 + f.gx * 0.28 + ox * 0.38 - f.yaw * 0.1),
      y: clamp01(0.5 + f.gy * 0.26 + oy * 0.36 + f.pitch * 0.08),
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
    if (!m?.bx?.length || m.bx.length !== FEAT_N || m.version !== 2) return null;
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

/** Soft recenter: pull drift so current features map nearer to screen center. */
export function applyRecenter(map: GazeMap, f: GazeFeatures): GazeMap {
  const p = mapGaze({ ...map, driftX: 0, driftY: 0 }, f);
  return {
    ...map,
    driftX: clamp(map.driftX + (0.5 - p.x) * 0.65, -0.2, 0.2),
    driftY: clamp(map.driftY + (0.5 - p.y) * 0.65, -0.2, 0.2),
    updatedAt: Date.now(),
  };
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

function clamp(v: number, a: number, b: number): number {
  return Math.min(b, Math.max(a, v));
}
