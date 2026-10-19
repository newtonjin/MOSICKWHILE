/**
 * Appearance-based gaze (MobileOne-S0 ONNX — yakhyo/gaze-estimation).
 * Face crop → yaw/pitch radians (Gaze360-style 90-bin softmax expectation).
 */
import ort from "onnxruntime-web";

const MODEL_URL = "/models/mobileone_s0_gaze.onnx";
const SIZE = 448;
const MEAN = [0.485, 0.456, 0.406];
const STD = [0.229, 0.224, 0.225];
const BINS = 90;
const BIN_WIDTH = 4;
const ANGLE_OFFSET = 180;

export type OnnxGaze = {
  yaw: number;
  pitch: number;
  /** Confidence proxy from peak sharpness of the softmax. */
  confidence: number;
};

export class OnnxGazeEstimator {
  // ort.InferenceSession — typed loosely via ort-shim
  private session: {
    inputNames: string[];
    outputNames: string[];
    run: (feeds: Record<string, unknown>) => Promise<Record<string, { data: Float32Array }>>;
  } | null = null;
  private canvas: HTMLCanvasElement | null = null;
  private ctx: CanvasRenderingContext2D | null = null;
  private idx: Float32Array = new Float32Array(BINS);
  private inputName = "input";
  private yawName = "";
  private pitchName = "";
  private lastRun = 0;
  private smYaw = 0;
  private smPitch = 0;
  private ready = false;
  private failed = false;

  get ok(): boolean {
    return this.ready && !this.failed;
  }

  async init(): Promise<boolean> {
    if (this.ready) return true;
    if (this.failed) return false;
    try {
      ort.env.wasm.wasmPaths = "/ort/";
      ort.env.wasm.numThreads = 1;
      ort.env.wasm.simd = true;
      const session = await ort.InferenceSession.create(MODEL_URL, {
        executionProviders: ["wasm"],
        graphOptimizationLevel: "all",
      });
      this.session = session;
      this.inputName = session.inputNames[0] ?? "input";
      this.yawName = session.outputNames[0] ?? "";
      this.pitchName = session.outputNames[1] ?? "";
      for (let i = 0; i < BINS; i++) this.idx[i] = i;
      this.canvas = document.createElement("canvas");
      this.canvas.width = SIZE;
      this.canvas.height = SIZE;
      this.ctx = this.canvas.getContext("2d", { willReadFrequently: true });
      this.ready = !!this.ctx;
      return this.ready;
    } catch (e) {
      console.warn("MOSICKWHILE ONNX gaze init failed", e);
      this.failed = true;
      this.ready = false;
      return false;
    }
  }

  /**
   * Run at most every `minIntervalMs`. Returns smoothed yaw/pitch in radians.
   * Landmark coords are normalized 0–1 in video space.
   */
  async estimate(
    video: HTMLVideoElement,
    landmarks: { x: number; y: number }[],
    minIntervalMs = 90,
  ): Promise<OnnxGaze | null> {
    if (!this.session || !this.ctx || !this.canvas || landmarks.length < 100) {
      return null;
    }
    const now = performance.now();
    if (now - this.lastRun < minIntervalMs) {
      return {
        yaw: this.smYaw,
        pitch: this.smPitch,
        confidence: 0.55,
      };
    }
    this.lastRun = now;

    const vw = video.videoWidth;
    const vh = video.videoHeight;
    if (vw < 16 || vh < 16) return null;

    let minX = 1;
    let minY = 1;
    let maxX = 0;
    let maxY = 0;
    // Face oval + dense mesh subset is enough; using all landmarks is fine.
    for (let i = 0; i < Math.min(landmarks.length, 468); i++) {
      const p = landmarks[i];
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x);
      maxY = Math.max(maxY, p.y);
    }
    const cx = (minX + maxX) * 0.5 * vw;
    const cy = (minY + maxY) * 0.5 * vh;
    const side = Math.max((maxX - minX) * vw, (maxY - minY) * vh) * 1.35;
    const x0 = cx - side * 0.5;
    const y0 = cy - side * 0.5;

    this.ctx.save();
    // Webcam is mirrored vs user; model trained on unmirrored crops — flip.
    this.ctx.translate(SIZE, 0);
    this.ctx.scale(-1, 1);
    this.ctx.drawImage(video, x0, y0, side, side, 0, 0, SIZE, SIZE);
    this.ctx.restore();

    const img = this.ctx.getImageData(0, 0, SIZE, SIZE);
    const tensor = this.toNchw(img.data);
    const input = new ort.Tensor("float32", tensor, [1, 3, SIZE, SIZE]);

    try {
      const out = await this.session.run({ [this.inputName]: input });
      const yawLogits = out[this.yawName].data as Float32Array;
      const pitchLogits = out[this.pitchName].data as Float32Array;
      const { angle: yaw, peak: yawPeak } = decodeBins(yawLogits, this.idx);
      const { angle: pitch, peak: pitchPeak } = decodeBins(pitchLogits, this.idx);
      // Flip yaw back to screen/user space (we mirrored the crop).
      const yawScreen = -yaw;
      const a = 0.35;
      this.smYaw += (yawScreen - this.smYaw) * a;
      this.smPitch += (pitch - this.smPitch) * a;
      return {
        yaw: this.smYaw,
        pitch: this.smPitch,
        confidence: Math.min(1, (yawPeak + pitchPeak) * 0.55),
      };
    } catch (e) {
      console.warn("MOSICKWHILE ONNX gaze run failed", e);
      return null;
    }
  }

  private toNchw(rgba: Uint8ClampedArray): Float32Array {
    const n = SIZE * SIZE;
    const out = new Float32Array(3 * n);
    for (let i = 0; i < n; i++) {
      const r = rgba[i * 4] / 255;
      const g = rgba[i * 4 + 1] / 255;
      const b = rgba[i * 4 + 2] / 255;
      out[i] = (r - MEAN[0]) / STD[0];
      out[n + i] = (g - MEAN[1]) / STD[1];
      out[2 * n + i] = (b - MEAN[2]) / STD[2];
    }
    return out;
  }
}

function decodeBins(
  logits: Float32Array,
  idx: Float32Array,
): { angle: number; peak: number } {
  let max = -1e9;
  for (let i = 0; i < BINS; i++) max = Math.max(max, logits[i]);
  let sum = 0;
  let expSum = 0;
  const probs = new Float32Array(BINS);
  for (let i = 0; i < BINS; i++) {
    const e = Math.exp(logits[i] - max);
    probs[i] = e;
    expSum += e;
  }
  let peak = 0;
  for (let i = 0; i < BINS; i++) {
    probs[i] /= expSum;
    peak = Math.max(peak, probs[i]);
    sum += probs[i] * idx[i];
  }
  const deg = sum * BIN_WIDTH - ANGLE_OFFSET;
  return { angle: (deg * Math.PI) / 180, peak };
}

export const onnxGaze = new OnnxGazeEstimator();
