import { FaceLandmarker, FilesetResolver } from "@mediapipe/tasks-vision";
import { invoke } from "@tauri-apps/api/core";
import {
  applyRecenter,
  fitMap,
  loadMap,
  mapGaze,
  median,
  saveMap,
  type CalibSample,
  type GazeFeatures,
  type GazeMap,
} from "./calibration";
import {
  extractGaze,
  L_BOT,
  L_INNER,
  L_OUTER,
  L_TOP,
  R_BOT,
  R_INNER,
  R_OUTER,
  R_TOP,
} from "./landmarks";
import { onnxGaze } from "./onnxGaze";
import { pupilLocalizer } from "./pupilLocalizer";

export type GazeStatus = {
  running: boolean;
  calibrated: boolean;
  status: string;
  trust: number;
  screenX: number;
  screenY: number;
  onnx: boolean;
  pupilQ: number;
  glassesLikely: boolean;
};

type FacePush = {
  ok: boolean;
  status: string;
  faceX: number;
  faceY: number;
  faceSize: number;
  present: boolean;
  confidence: number;
  headRoll: number;
  headYaw: number;
  headPitch: number;
  calibrated: boolean;
  pupilsOk: boolean;
  pupilConfidence: number;
  pupilQuality: number;
  pupilAgreement: number;
  pupilLockRate: number;
  pupilJitter: number;
};

export class GazeTracker {
  private landmarker: FaceLandmarker | null = null;
  private video: HTMLVideoElement | null = null;
  private stream: MediaStream | null = null;
  private raf = 0;
  private running = false;
  private map: GazeMap | null = loadMap();
  private lastTs = -1;
  private lockRate = 0;
  private prevX = 0.5;
  private prevY = 0.5;
  private jitter = 0;
  private lastFeat: GazeFeatures | null = null;
  private status = "idle";
  private trust = 0;
  private sx = 0.5;
  private sy = 0.5;
  private pushAcc = 0;
  private onnxYaw = 0;
  private onnxPitch = 0;
  private onnxConf = 0;
  private onnxBusy = false;
  private pupilQ = 0;
  private glare = 0;
  private pupilTick = 0;
  onStatus?: (s: GazeStatus) => void;

  getStatus(): GazeStatus {
    return {
      running: this.running,
      calibrated: !!this.map,
      status: this.status,
      trust: this.trust,
      screenX: this.sx,
      screenY: this.sy,
      onnx: onnxGaze.ok,
      pupilQ: this.pupilQ,
      glassesLikely: this.glare > 0.08,
    };
  }

  isCalibrated(): boolean {
    return !!this.map;
  }

  async start(): Promise<void> {
    if (this.running) return;
    this.status = "loading";
    this.emit();
    try {
      // Release any Rust/Seeta MSMF handle BEFORE getUserMedia (Windows exclusive).
      try {
        await invoke("release_camera");
      } catch {
        await invoke("set_gaze_external", { external: true }).catch(() => undefined);
        await sleep(350);
      }

      // Drop a leftover stream from a previous failed start.
      this.stream?.getTracks().forEach((t) => t.stop());
      this.stream = null;

      const vision = await FilesetResolver.forVisionTasks("/mediapipe/wasm");
      const opts = {
        runningMode: "VIDEO" as const,
        numFaces: 1,
        outputFaceBlendshapes: true,
        outputFacialTransformationMatrixes: true,
      };
      try {
        this.landmarker = await FaceLandmarker.createFromOptions(vision, {
          baseOptions: {
            modelAssetPath: "/models/face_landmarker.task",
            delegate: "GPU",
          },
          ...opts,
        });
      } catch {
        this.landmarker = await FaceLandmarker.createFromOptions(vision, {
          baseOptions: {
            modelAssetPath: "/models/face_landmarker.task",
            delegate: "CPU",
          },
          ...opts,
        });
      }

      void onnxGaze.init().then((ok) => {
        if (ok) this.status = this.map ? "pupil+onnx" : "needs-calib";
        this.emit();
      });

      this.video = document.createElement("video");
      this.video.setAttribute("playsinline", "true");
      this.video.muted = true;
      this.video.width = 640;
      this.video.height = 480;

      this.stream = await openCameraWithRetry();
      this.video.srcObject = this.stream;
      await this.video.play();

      await invoke("set_gaze_external", { external: true });

      this.running = true;
      this.status = this.map ? "pupil+onnx" : "needs-calib";
      this.emit();
      this.loop();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      const busy =
        /Device in use|NotReadableError|Could not start video|in use/i.test(msg);
      this.status = busy
        ? "cam-busy — feche Zoom/Teams/OBS e reabra"
        : `cam-fail: ${msg}`;
      this.running = false;
      this.emit();
      await this.stop();
      throw e;
    }
  }

  async stop(): Promise<void> {
    this.running = false;
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.landmarker?.close();
    this.landmarker = null;
    this.stream?.getTracks().forEach((t) => t.stop());
    this.stream = null;
    if (this.video) {
      this.video.srcObject = null;
      this.video = null;
    }
    this.status = "off";
    this.emit();
    try {
      await invoke("set_gaze_external", { external: false });
    } catch {
      /* ignore */
    }
  }

  /**
   * Quality-gated sampling: median of good frames only.
   * Glasses → accept ONNX-strong frames even if pupilQ is low.
   */
  async sampleFor(
    ms: number,
    onProgress?: (t: number) => void,
  ): Promise<GazeFeatures | null> {
    const t0 = performance.now();
    const buf: GazeFeatures[] = [];
    while (performance.now() - t0 < ms) {
      const f = this.lastFeat;
      if (f) {
        const good =
          f.pupilQ >= 0.32 ||
          (onnxGaze.ok && Math.abs(f.onnxYaw) + Math.abs(f.onnxPitch) > 0.02);
        if (good) buf.push(f);
      }
      onProgress?.((performance.now() - t0) / ms);
      await sleep(33);
    }
    if (buf.length < 5) return null;
    // Use upper half of samples (after settle).
    const slice = buf.slice(Math.floor(buf.length * 0.4));
    const pick = (k: keyof GazeFeatures) => median(slice.map((f) => f[k]));
    return {
      gx: pick("gx"),
      gy: pick("gy"),
      yaw: pick("yaw"),
      pitch: pick("pitch"),
      onnxYaw: pick("onnxYaw"),
      onnxPitch: pick("onnxPitch"),
      pupilQ: pick("pupilQ"),
    };
  }

  finishCalibration(samples: CalibSample[]): boolean {
    // Weight edge samples higher (right/left bias with glasses).
    const weighted = samples.map((s) => {
      const edge = Math.max(Math.abs(s.x - 0.5), Math.abs(s.y - 0.5)) * 2;
      const w = 1 + edge * 0.85 + (1 - s.pupilQ) * 0.35;
      return { ...s, weight: w };
    });
    const map = fitMap(weighted);
    if (!map) return false;
    this.map = map;
    saveMap(map);
    this.status = "pupil+onnx";
    this.emit();
    return true;
  }

  /** Quick validation: mean |error| at a few hold points. */
  async validateMap(
    points: { x: number; y: number }[],
  ): Promise<number> {
    if (!this.map) return 1;
    let err = 0;
    for (const p of points) {
      const feat = await this.sampleFor(700);
      if (!feat) {
        err += 0.25;
        continue;
      }
      const m = mapGaze(this.map, feat);
      err += Math.hypot(m.x - p.x, m.y - p.y);
    }
    return err / Math.max(points.length, 1);
  }

  recenter(): void {
    if (!this.map || !this.lastFeat) return;
    this.map = applyRecenter(this.map, this.lastFeat);
    saveMap(this.map);
    this.status = "recentered";
    this.emit();
  }

  private loop = () => {
    if (!this.running || !this.landmarker || !this.video) return;
    const video = this.video;
    if (video.readyState >= 2) {
      const ts = performance.now();
      if (ts !== this.lastTs) {
        this.lastTs = ts;
        const res = this.landmarker.detectForVideo(video, ts);
        this.consume(res);
      }
    }
    this.raf = requestAnimationFrame(this.loop);
  };

  private consume(res: Awaited<ReturnType<FaceLandmarker["detectForVideo"]>>) {
    const face = res.faceLandmarks?.[0];
    if (!face || !this.video) {
      this.lockRate *= 0.92;
      this.trust *= 0.85;
      this.status = "no-face";
      this.push(false);
      this.emit();
      return;
    }

    const matrix = res.facialTransformationMatrixes?.[0]?.data as
      | number[]
      | undefined;
    let blinkL = 0;
    let blinkR = 0;
    const cats = res.faceBlendshapes?.[0]?.categories;
    if (cats) {
      for (const c of cats) {
        if (c.categoryName === "eyeBlinkLeft") blinkL = c.score;
        if (c.categoryName === "eyeBlinkRight") blinkR = c.score;
      }
    }

    const iris = extractGaze(face, matrix, blinkL, blinkR);

    // —— Dedicated pupil localization on each eye crop (~every other frame) ——
    this.pupilTick++;
    let pupilGx = iris.gx;
    let pupilGy = iris.gy;
    let pupilQ = iris.trust * 0.5;
    let glare = this.glare;
    if (this.pupilTick % 2 === 0 && iris.blink < 0.45) {
      const L = pupilLocalizer.locate(
        this.video,
        face[L_OUTER],
        face[L_INNER],
        face[L_TOP],
        face[L_BOT],
      );
      const R = pupilLocalizer.locate(
        this.video,
        face[R_OUTER],
        face[R_INNER],
        face[R_TOP],
        face[R_BOT],
      );
      glare = (L.glare + R.glare) * 0.5;
      this.glare = this.glare * 0.7 + glare * 0.3;
      if (L.ok || R.ok) {
        const wl = L.ok ? L.quality : 0.05;
        const wr = R.ok ? R.quality : 0.05;
        pupilGx = (L.gx * wl + R.gx * wr) / (wl + wr);
        pupilGy = (L.gy * wl + R.gy * wr) / (wl + wr);
        pupilQ = Math.max(L.quality, R.quality);
        // Blend a little MediaPipe iris when both agree.
        if (iris.irisOk && iris.agreement > 0.55) {
          pupilGx = pupilGx * 0.72 + iris.gx * 0.28;
          pupilGy = pupilGy * 0.72 + iris.gy * 0.28;
          pupilQ = Math.max(pupilQ, iris.trust * 0.75);
        }
      } else if (iris.irisOk) {
        pupilGx = iris.gx;
        pupilGy = iris.gy;
        pupilQ = iris.trust * 0.55;
      }
    }
    this.pupilQ = this.pupilQ * 0.75 + pupilQ * 0.25;

    // ONNX appearance gaze — stronger when glasses/glare ruin pupil.
    if (onnxGaze.ok && !this.onnxBusy && iris.blink < 0.5) {
      this.onnxBusy = true;
      void onnxGaze
        .estimate(this.video, face, this.glare > 0.1 ? 70 : 95)
        .then((g) => {
          if (g) {
            this.onnxYaw = g.yaw;
            this.onnxPitch = g.pitch;
            this.onnxConf = g.confidence;
          }
        })
        .finally(() => {
          this.onnxBusy = false;
        });
    }

    const onnxGx = Math.sin(this.onnxYaw) * Math.cos(this.onnxPitch);
    const onnxGy = Math.sin(this.onnxPitch);
    // Glasses / low pupilQ → trust ONNX more (fixes right-edge stuck vignette).
    const glasses = this.glare > 0.08 || this.pupilQ < 0.34;
    let ow = 0;
    if (onnxGaze.ok) {
      ow = glasses
        ? 0.62 + this.onnxConf * 0.22
        : 0.38 + this.onnxConf * 0.18 + (1 - this.pupilQ) * 0.2;
      ow = Math.min(0.88, ow);
    }
    const iw = 1 - ow;
    const gx = pupilGx * iw + onnxGx * ow;
    const gy = pupilGy * iw + onnxGy * ow;

    this.lastFeat = {
      gx,
      gy,
      yaw: iris.yaw,
      pitch: iris.pitch,
      onnxYaw: this.onnxYaw,
      onnxPitch: this.onnxPitch,
      pupilQ: this.pupilQ,
    };

    if (iris.blink > 0.55) {
      this.lockRate = this.lockRate * 0.9 + 0.05;
      this.push(true);
      this.status = "blink";
      this.emit();
      return;
    }

    const screen = mapGaze(this.map, this.lastFeat);
    const step = Math.hypot(screen.x - this.prevX, screen.y - this.prevY);
    this.jitter = this.jitter * 0.85 + step * 0.15;
    this.prevX = screen.x;
    this.prevY = screen.y;
    this.sx = screen.x;
    this.sy = screen.y;
    this.trust = Math.max(this.pupilQ, this.onnxConf * (glasses ? 0.95 : 0.8));
    this.lockRate =
      this.lockRate * 0.88 +
      (this.pupilQ > 0.3 || onnxGaze.ok ? 0.12 : 0.02);
    this.status = this.map
      ? glasses
        ? "glasses+onnx"
        : "pupil+onnx"
      : "needs-calib";

    this.push(
      true,
      iris.agreement,
      iris.roll,
      iris.yaw,
      iris.pitch,
      this.pupilQ > 0.28 || onnxGaze.ok,
    );
    this.emit();
  }

  private push(
    present: boolean,
    agreement = 0,
    roll = 0,
    yaw = 0,
    pitch = 0,
    irisOk = false,
  ) {
    const now = performance.now();
    if (now - this.pushAcc < 40 && present) return;
    this.pushAcc = now;

    const sample: FacePush = {
      ok: true,
      status: this.status,
      faceX: this.sx,
      faceY: this.sy,
      faceSize: 0.3,
      present,
      confidence: this.trust,
      headRoll: roll,
      headYaw: yaw,
      headPitch: pitch,
      calibrated: !!this.map,
      pupilsOk: irisOk && present,
      pupilConfidence: this.trust,
      pupilQuality: this.pupilQ,
      pupilAgreement: agreement,
      pupilLockRate: this.lockRate,
      pupilJitter: this.jitter,
    };
    void invoke("push_gaze_sample", { sample }).catch(() => {
      /* backend may be restarting */
    });
  }

  private emit() {
    this.onStatus?.(this.getStatus());
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

async function openCameraWithRetry(): Promise<MediaStream> {
  const constraints: MediaStreamConstraints = {
    audio: false,
    video: {
      facingMode: "user",
      width: { ideal: 640 },
      height: { ideal: 480 },
      frameRate: { ideal: 24, max: 30 },
    },
  };
  let last: unknown;
  for (let i = 0; i < 4; i++) {
    try {
      return await navigator.mediaDevices.getUserMedia(constraints);
    } catch (e) {
      last = e;
      const msg = e instanceof Error ? e.message : String(e);
      if (!/Device in use|NotReadableError|Could not start/i.test(msg)) {
        throw e;
      }
      // Ask Rust again + wait for MSMF teardown.
      try {
        await invoke("release_camera");
      } catch {
        await sleep(400);
      }
      await sleep(450 + i * 200);
    }
  }
  // Last resort: any camera, minimal constraints.
  try {
    return await navigator.mediaDevices.getUserMedia({
      audio: false,
      video: true,
    });
  } catch {
    throw last instanceof Error ? last : new Error(String(last));
  }
}

export const gazeTracker = new GazeTracker();
