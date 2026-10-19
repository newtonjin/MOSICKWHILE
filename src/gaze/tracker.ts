import { FaceLandmarker, FilesetResolver } from "@mediapipe/tasks-vision";
import { invoke } from "@tauri-apps/api/core";
import {
  applyRecenter,
  fitMap,
  loadMap,
  mapGaze,
  saveMap,
  type CalibSample,
  type GazeFeatures,
  type GazeMap,
} from "./calibration";
import { extractGaze } from "./landmarks";
import { onnxGaze } from "./onnxGaze";

export type GazeStatus = {
  running: boolean;
  calibrated: boolean;
  status: string;
  trust: number;
  screenX: number;
  screenY: number;
  onnx: boolean;
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

      // Appearance gaze — best-effort (iris still works if ONNX fails).
      void onnxGaze.init().then((ok) => {
        if (ok) this.status = this.map ? "onnx+iris" : "needs-calib";
        this.emit();
      });

      this.video = document.createElement("video");
      this.video.setAttribute("playsinline", "true");
      this.video.muted = true;
      this.video.width = 640;
      this.video.height = 480;
      this.stream = await navigator.mediaDevices.getUserMedia({
        audio: false,
        video: {
          facingMode: "user",
          width: { ideal: 640 },
          height: { ideal: 480 },
          frameRate: { ideal: 30, max: 30 },
        },
      });
      this.video.srcObject = this.stream;
      await this.video.play();

      await invoke("set_gaze_external", { external: true });

      this.running = true;
      this.status = this.map
        ? onnxGaze.ok
          ? "onnx+iris"
          : "iris-lock"
        : "needs-calib";
      this.emit();
      this.loop();
    } catch (e) {
      this.status = `cam-fail: ${e instanceof Error ? e.message : String(e)}`;
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

  async sampleFor(
    ms: number,
    onProgress?: (t: number) => void,
  ): Promise<GazeFeatures | null> {
    const t0 = performance.now();
    const buf: GazeFeatures[] = [];
    while (performance.now() - t0 < ms) {
      if (this.lastFeat) buf.push(this.lastFeat);
      onProgress?.((performance.now() - t0) / ms);
      await sleep(33);
    }
    if (buf.length < 4) return null;
    const slice = buf.slice(Math.floor(buf.length * 0.35));
    const avg = (k: keyof GazeFeatures) =>
      slice.reduce((s, f) => s + f[k], 0) / slice.length;
    return {
      gx: avg("gx"),
      gy: avg("gy"),
      yaw: avg("yaw"),
      pitch: avg("pitch"),
      onnxYaw: avg("onnxYaw"),
      onnxPitch: avg("onnxPitch"),
    };
  }

  finishCalibration(samples: CalibSample[]): boolean {
    const map = fitMap(samples);
    if (!map) return false;
    this.map = map;
    saveMap(map);
    this.status = onnxGaze.ok ? "onnx+iris" : "iris-lock";
    this.emit();
    return true;
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
    if (!face) {
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

    const raw = extractGaze(face, matrix, blinkL, blinkR);

    // Kick ONNX asynchronously (~10 Hz) — never block the iris path.
    if (onnxGaze.ok && this.video && !this.onnxBusy && raw.blink < 0.5) {
      this.onnxBusy = true;
      void onnxGaze
        .estimate(this.video, face, 85)
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

    // Fuse iris socket with appearance angles into a single gaze proxy.
    const onnxGx = Math.sin(this.onnxYaw) * Math.cos(this.onnxPitch);
    const onnxGy = Math.sin(this.onnxPitch);
    const ow = onnxGaze.ok ? 0.52 + this.onnxConf * 0.2 : 0;
    const iw = 1 - ow;
    const gx = raw.gx * iw + onnxGx * ow;
    const gy = raw.gy * iw + onnxGy * ow;

    this.lastFeat = {
      gx,
      gy,
      yaw: raw.yaw,
      pitch: raw.pitch,
      onnxYaw: this.onnxYaw,
      onnxPitch: this.onnxPitch,
    };

    if (raw.blink > 0.55) {
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
    this.trust = Math.max(raw.trust, this.onnxConf * 0.85);
    this.lockRate =
      this.lockRate * 0.88 + (raw.irisOk || onnxGaze.ok ? 0.12 : 0.02);
    this.status = this.map
      ? onnxGaze.ok
        ? "onnx+iris"
        : raw.irisOk
          ? "iris-lock"
          : "face-only"
      : "needs-calib";

    this.push(
      true,
      raw.agreement,
      raw.roll,
      raw.yaw,
      raw.pitch,
      raw.irisOk || onnxGaze.ok,
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
    if (now - this.pushAcc < 45 && present) return;
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
      pupilQuality: this.trust,
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

export const gazeTracker = new GazeTracker();
