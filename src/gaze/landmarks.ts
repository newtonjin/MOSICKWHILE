import type { GazeFeatures } from "./calibration";

type NormalizedLandmark = { x: number; y: number; z: number; visibility?: number };

/** MediaPipe Face Landmarker iris indices. */
const L_IRIS = [468, 469, 470, 471, 472] as const;
const R_IRIS = [473, 474, 475, 476, 477] as const;
// Eye corners / lids
const L_OUTER = 33;
const L_INNER = 133;
const L_TOP = 159;
const L_BOT = 145;
const R_OUTER = 263;
const R_INNER = 362;
const R_TOP = 386;
const R_BOT = 374;

export type RawGaze = GazeFeatures & {
  present: boolean;
  irisOk: boolean;
  blink: number;
  roll: number;
  trust: number;
  agreement: number;
};

function avg(lms: NormalizedLandmark[], idxs: readonly number[]) {
  let x = 0;
  let y = 0;
  let z = 0;
  for (const i of idxs) {
    x += lms[i].x;
    y += lms[i].y;
    z += lms[i].z;
  }
  const n = idxs.length;
  return { x: x / n, y: y / n, z: z / n };
}

function eyeVector(
  iris: { x: number; y: number },
  outer: NormalizedLandmark,
  inner: NormalizedLandmark,
  top: NormalizedLandmark,
  bot: NormalizedLandmark,
) {
  const midX = (outer.x + inner.x) * 0.5;
  const midY = (top.y + bot.y) * 0.5;
  const halfW = Math.max(Math.abs(inner.x - outer.x) * 0.5, 1e-4);
  const halfH = Math.max(Math.abs(bot.y - top.y) * 0.5, 1e-4);
  // Camera is mirrored relative to user looking at screen.
  const gx = -((iris.x - midX) / halfW);
  const gy = (iris.y - midY) / halfH;
  return {
    gx: clamp(gx, -1.6, 1.6),
    gy: clamp(gy, -1.6, 1.6),
  };
}

/** Extract yaw/pitch/roll (approx rad) from 4×4 row-major facial matrix. */
export function poseFromMatrix(m: number[]): { yaw: number; pitch: number; roll: number } {
  if (!m || m.length < 16) return { yaw: 0, pitch: 0, roll: 0 };
  // MediaPipe facialTransformationMatrixes are column-major 4x4.
  const r00 = m[0];
  const r01 = m[4];
  const r02 = m[8];
  const r10 = m[1];
  const r11 = m[5];
  const r12 = m[9];
  const r20 = m[2];
  const r21 = m[6];
  const r22 = m[10];
  const pitch = Math.asin(clamp(-r21, -1, 1));
  const yaw = Math.atan2(r20, r22);
  const roll = Math.atan2(r01, r11);
  // Unused but keep for stability checks
  void r00;
  void r02;
  void r10;
  void r12;
  return {
    yaw: clamp(-yaw, -1.2, 1.2),
    pitch: clamp(pitch, -1.0, 1.0),
    roll: clamp(-roll, -0.8, 0.8),
  };
}

export function extractGaze(
  landmarks: NormalizedLandmark[],
  matrix?: number[],
  blinkL = 0,
  blinkR = 0,
): RawGaze {
  if (!landmarks || landmarks.length < 478) {
    return {
      present: false,
      irisOk: false,
      gx: 0,
      gy: 0,
      yaw: 0,
      pitch: 0,
      roll: 0,
      blink: 1,
      trust: 0,
      agreement: 0,
    };
  }

  const li = avg(landmarks, L_IRIS);
  const ri = avg(landmarks, R_IRIS);
  const left = eyeVector(
    li,
    landmarks[L_OUTER],
    landmarks[L_INNER],
    landmarks[L_TOP],
    landmarks[L_BOT],
  );
  const right = eyeVector(
    ri,
    landmarks[R_OUTER],
    landmarks[R_INNER],
    landmarks[R_TOP],
    landmarks[R_BOT],
  );

  const agree =
    1 -
    Math.min(
      1,
      Math.hypot(left.gx - right.gx, left.gy - right.gy) * 0.55,
    );
  const gx = (left.gx + right.gx) * 0.5;
  const gy = (left.gy + right.gy) * 0.5;
  const pose = poseFromMatrix(matrix ?? []);
  const blink = Math.max(blinkL, blinkR);
  const irisOk = agree > 0.28 && blink < 0.55;
  // Pose-light residual: calibration absorbs most; keep mild compensation uncalibrated.
  const trust = clamp(
    agree * 0.55 + (1 - blink) * 0.25 + (irisOk ? 0.2 : 0),
    0,
    1,
  );

  return {
    present: true,
    irisOk,
    gx,
    gy,
    yaw: pose.yaw,
    pitch: pose.pitch,
    roll: pose.roll,
    blink,
    trust,
    agreement: agree,
  };
}

function clamp(v: number, a: number, b: number): number {
  return Math.min(b, Math.max(a, v));
}
