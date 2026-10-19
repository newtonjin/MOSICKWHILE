import { listen } from "@tauri-apps/api/event";
import type { OverlayFrame, RestFrameStyle } from "./types";

const canvas = document.getElementById("overlay") as HTMLCanvasElement;
const ctx = canvas.getContext("2d", { alpha: true })!;
const blurRing = document.getElementById("blur-ring") as HTMLDivElement;

let frame: OverlayFrame = {
  enabled: true,
  motion: 0,
  vignetteAmount: 0,
  vignetteSoftness: 0.48,
  biasX: 0,
  biasY: 0,
  groundClear: 0.25,
  restFrameStyle: "earth_horizon",
  restFrameOpacity: 0,
  blurAmount: 0,
  horizonRoll: 0,
  clearX: 0.5,
  clearY: 0.5,
  clearRadius: 0.38,
  foveatedClear: true,
  virtualNose: false,
  flowDampen: 0,
  sideMask: 0,
  sideSign: -1,
  realBlur: true,
  blurPx: 0,
  faceOk: false,
  facePresent: false,
  faceStatus: "idle",
  metrics: {
    motion: 0,
    peripheralFlow: 0,
    centralFlow: 0,
    rotational: 0,
    translational: 0,
    flowX: 0,
    flowY: 0,
    comfortIndex: 1,
    fovRestriction: 0,
    latencyMs: 0,
    analyzeFps: 0,
    captureOk: false,
    captureStatus: "init",
    sessionPeak: 0,
    framesAnalyzed: 0,
    sessionSecs: 0,
    pupilQuality: 0,
    pupilAgreement: 0,
    pupilLockRate: 0,
    pupilJitter: 0,
    gamingActive: false,
    gamingScore: 0,
  },
};

function resize() {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.floor(window.innerWidth * dpr);
  canvas.height = Math.floor(window.innerHeight * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
}

function updateBlurRing(
  clearX: number,
  clearY: number,
  clearRadius: number,
  blurPx: number,
  realBlur: boolean,
) {
  if (!realBlur || blurPx < 0.4) {
    blurRing.style.opacity = "0";
    blurRing.style.setProperty("backdrop-filter", "blur(0px)");
    blurRing.style.setProperty("-webkit-backdrop-filter", "blur(0px)");
    return;
  }
  const inner = Math.round(clearRadius * 100);
  const mid = Math.min(inner + 28, 92);
  const mask = `radial-gradient(circle at ${clearX * 100}% ${clearY * 100}%, transparent 0%, transparent ${inner}%, #000 ${mid}%, #000 100%)`;
  blurRing.style.setProperty("-webkit-mask-image", mask);
  blurRing.style.maskImage = mask;
  const px = Math.min(22, blurPx).toFixed(1);
  const filter = `blur(${px}px) saturate(0.82)`;
  blurRing.style.setProperty("backdrop-filter", filter);
  blurRing.style.setProperty("-webkit-backdrop-filter", filter);
  blurRing.style.opacity = String(Math.min(1, 0.35 + blurPx / 28));
}

function drawVignette(
  w: number,
  h: number,
  amount: number,
  softness: number,
  groundClear: number,
  clearX: number,
  clearY: number,
  clearRadius: number,
  foveated: boolean,
) {
  if (amount <= 0.001) return;

  const cx = w * clearX;
  const cy = h * clearY;
  const minDim = Math.min(w, h);
  const edgeProx = Math.max(
    Math.abs(clearX - 0.5) * 2,
    Math.abs(clearY - 0.5) * 2,
  );
  let inner = foveated
    ? minDim * clearRadius
    : minDim * (0.58 - amount * 0.38);

  // When gaze is near a corner/edge, force the clear hole past the looked-at
  // screen corner — kills the dark crescent that stuck until the mouse moved.
  if (foveated && edgeProx > 0.35) {
    const toEdgeX = clearX < 0.5 ? cx : w - cx;
    const toEdgeY = clearY < 0.5 ? cy : h - cy;
    const toLookedCorner = Math.hypot(toEdgeX, toEdgeY);
    const cover =
      toLookedCorner + minDim * (0.05 + edgeProx * 0.08);
    const t = ((edgeProx - 0.35) / 0.65) ** 0.85;
    inner = Math.max(inner, cover * t);
    inner = Math.min(inner, minDim * 0.92);
  }

  // Softer falloff — less hard tunnel edge, better “attached” feel on gaze.
  const outer = Math.max(inner * 1.35, minDim * (0.88 + softness * 0.55));

  const gradient = ctx.createRadialGradient(
    cx,
    cy,
    Math.max(inner * 0.85, 0),
    cx,
    cy,
    outer,
  );
  gradient.addColorStop(0, "rgba(0,0,0,0)");
  gradient.addColorStop(0.35, `rgba(0,0,0,${amount * 0.12})`);
  gradient.addColorStop(0.55, `rgba(0,0,0,${amount * 0.32})`);
  gradient.addColorStop(0.78, `rgba(0,0,0,${amount * 0.62})`);
  gradient.addColorStop(1, `rgba(0,0,0,${amount * 0.9})`);

  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, w, h);

  // Extra lift on the looked-at edge(s) so split panes stay readable.
  if (edgeProx > 0.4) {
    const liftA = Math.min(0.95, (edgeProx - 0.4) * 1.4);
    ctx.globalCompositeOperation = "destination-out";
    if (clearX < 0.35) {
      const g = ctx.createLinearGradient(0, 0, w * 0.28, 0);
      g.addColorStop(0, `rgba(0,0,0,${liftA})`);
      g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w * 0.28, h);
    } else if (clearX > 0.65) {
      const g = ctx.createLinearGradient(w, 0, w * 0.72, 0);
      g.addColorStop(0, `rgba(0,0,0,${liftA})`);
      g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = g;
      ctx.fillRect(w * 0.72, 0, w * 0.28, h);
    }
    if (clearY < 0.35) {
      const g = ctx.createLinearGradient(0, 0, 0, h * 0.28);
      g.addColorStop(0, `rgba(0,0,0,${liftA * 0.9})`);
      g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h * 0.28);
    } else if (clearY > 0.65) {
      const g = ctx.createLinearGradient(0, h, 0, h * 0.72);
      g.addColorStop(0, `rgba(0,0,0,${liftA * 0.9})`);
      g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = g;
      ctx.fillRect(0, h * 0.72, w, h * 0.28);
    }
    ctx.globalCompositeOperation = "source-over";
  }

  if (groundClear > 0.01) {
    const lift = ctx.createLinearGradient(0, h * 0.62, 0, h);
    lift.addColorStop(0, "rgba(0,0,0,0)");
    lift.addColorStop(1, `rgba(0,0,0,${amount * groundClear * 0.55})`);
    ctx.globalCompositeOperation = "destination-out";
    ctx.fillStyle = lift;
    ctx.fillRect(0, h * 0.58, w, h * 0.42);
    ctx.globalCompositeOperation = "source-over";
  }
}

function drawSideRestrictor(
  w: number,
  h: number,
  amount: number,
  sign: number,
  vignette: number,
) {
  if (amount <= 0.01) return;
  const a = Math.min(0.85, amount * (0.35 + vignette * 0.45));
  const width = w * (0.18 + amount * 0.22);
  const g =
    sign < 0
      ? ctx.createLinearGradient(0, 0, width, 0)
      : ctx.createLinearGradient(w, 0, w - width, 0);
  g.addColorStop(0, `rgba(0,0,0,${a})`);
  g.addColorStop(1, "rgba(0,0,0,0)");
  ctx.fillStyle = g;
  if (sign < 0) ctx.fillRect(0, 0, width, h);
  else ctx.fillRect(w - width, 0, width, h);
}

function drawPeripheralBlurHint(
  w: number,
  h: number,
  amount: number,
  clearX: number,
  clearY: number,
  clearRadius: number,
) {
  if (amount <= 0.001) return;
  const cx = w * clearX;
  const cy = h * clearY;
  const minDim = Math.min(w, h);
  const inner = minDim * clearRadius * 0.92;
  const outer = minDim * 0.95;
  const g = ctx.createRadialGradient(cx, cy, inner, cx, cy, outer);
  g.addColorStop(0, "rgba(6,12,10,0)");
  g.addColorStop(0.55, `rgba(6,12,10,${amount * 0.12})`);
  g.addColorStop(1, `rgba(6,12,10,${amount * 0.38})`);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, w, h);
}

function drawHorizonLine(w: number, h: number, roll: number, _clearY = 0.5) {
  const cx = w / 2;
  // Fixed screen band — never floats with gaze (rest frame must be stable).
  const cy = h * 0.48;
  const c = Math.cos(roll);
  const s = Math.sin(roll);
  const half = w * 0.34;

  ctx.beginPath();
  ctx.moveTo(cx - half * c, cy - half * s);
  ctx.lineTo(cx + half * c, cy + half * s);
  ctx.stroke();

  for (let i = -4; i <= 4; i++) {
    if (i === 0) continue;
    const along = i * w * 0.065;
    const px = cx + along * c;
    const py = cy + along * s;
    const tick = Math.abs(i) === 4 ? 9 : 5;
    ctx.beginPath();
    ctx.moveTo(px + tick * s, py - tick * c);
    ctx.lineTo(px - tick * s, py + tick * c);
    ctx.stroke();
  }

  ctx.beginPath();
  ctx.moveTo(cx, cy - 4);
  ctx.lineTo(cx + 4, cy);
  ctx.lineTo(cx, cy + 4);
  ctx.lineTo(cx - 4, cy);
  ctx.closePath();
  ctx.stroke();
}

function drawVirtualNose(w: number, h: number, opacity: number) {
  const cx = w / 2;
  const tipY = h * 0.78;
  const baseY = h * 0.98;
  const half = w * 0.045;
  ctx.fillStyle = `rgba(210, 230, 220, ${opacity * 0.45})`;
  ctx.beginPath();
  ctx.moveTo(cx, tipY);
  ctx.lineTo(cx - half, baseY);
  ctx.lineTo(cx + half, baseY);
  ctx.closePath();
  ctx.fill();
  ctx.strokeStyle = `rgba(236, 248, 240, ${opacity})`;
  ctx.beginPath();
  ctx.moveTo(cx, tipY);
  ctx.lineTo(cx - half, baseY);
  ctx.moveTo(cx, tipY);
  ctx.lineTo(cx + half, baseY);
  ctx.stroke();
}

function drawRestFrame(
  w: number,
  h: number,
  style: RestFrameStyle,
  opacity: number,
  roll: number,
  virtualNose: boolean,
  clearY: number,
) {
  if (opacity <= 0.01 && !virtualNose) return;
  if (style === "none" && !virtualNose) return;

  ctx.save();
  ctx.globalAlpha = opacity;
  ctx.strokeStyle = "rgba(236, 248, 240, 0.95)";
  ctx.fillStyle = "rgba(236, 248, 240, 0.95)";
  ctx.lineWidth = 1.6;

  const cx = w / 2;
  const cy = h / 2;

  if (style === "crosshair") {
    const arm = Math.min(w, h) * 0.03;
    const gap = 7;
    ctx.beginPath();
    ctx.moveTo(cx - arm, cy);
    ctx.lineTo(cx - gap, cy);
    ctx.moveTo(cx + gap, cy);
    ctx.lineTo(cx + arm, cy);
    ctx.moveTo(cx, cy - arm);
    ctx.lineTo(cx, cy - gap);
    ctx.moveTo(cx, cy + gap);
    ctx.lineTo(cx, cy + arm);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(cx, cy, 2.4, 0, Math.PI * 2);
    ctx.fill();
  } else if (style === "horizon") {
    drawHorizonLine(w, h, 0, 0.5);
  } else if (style === "earth_horizon") {
    drawHorizonLine(w, h, roll, clearY);
  } else if (style === "grid") {
    ctx.globalAlpha = opacity * 0.4;
    const step = Math.min(w, h) / 8;
    ctx.beginPath();
    for (let x = step; x < w; x += step) {
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
    }
    for (let y = step; y < h; y += step) {
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
    }
    ctx.stroke();
  } else if (style === "virtual_nose" || virtualNose) {
    drawVirtualNose(w, h, opacity);
  }

  ctx.restore();
}

function render() {
  const w = window.innerWidth;
  const h = window.innerHeight;
  ctx.clearRect(0, 0, w, h);

  if (!frame.enabled) {
    updateBlurRing(0.5, 0.5, 0.4, 0, false);
    requestAnimationFrame(render);
    return;
  }

  // Clear zone follows engine gaze proxy (face) — never side-restrictor shift.
  const clearX = frame.foveatedClear
    ? frame.clearX
    : 0.5 + frame.biasX * 0.1;
  const clearY = frame.foveatedClear
    ? frame.clearY
    : 0.5 + frame.biasY * 0.08;
  const clearR = frame.foveatedClear ? frame.clearRadius : 0.4;

  updateBlurRing(clearX, clearY, clearR, frame.blurPx, frame.realBlur);

  if (!frame.realBlur || frame.blurPx < 0.5) {
    const peri = Math.max(frame.blurAmount, frame.flowDampen);
    drawPeripheralBlurHint(w, h, peri, clearX, clearY, clearR);
  }

  drawVignette(
    w,
    h,
    frame.vignetteAmount,
    frame.vignetteSoftness,
    frame.groundClear,
    clearX,
    clearY,
    clearR,
    frame.foveatedClear,
  );
  drawSideRestrictor(
    w,
    h,
    frame.sideMask,
    frame.sideSign,
    frame.vignetteAmount,
  );
  drawRestFrame(
    w,
    h,
    frame.restFrameStyle,
    frame.restFrameOpacity,
    frame.horizonRoll,
    frame.virtualNose,
    clearY,
  );

  requestAnimationFrame(render);
}

window.addEventListener("resize", resize);
resize();
requestAnimationFrame(render);

void listen<OverlayFrame>("overlay-frame", (event) => {
  frame = event.payload;
});
