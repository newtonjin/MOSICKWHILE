export type MotionSource = "capture" | "demo" | "manual";
export type RestFrameStyle =
  | "none"
  | "crosshair"
  | "horizon"
  | "earth_horizon"
  | "grid"
  | "virtual_nose";
export type Preset = "light" | "medium" | "aggressive" | "custom";

export interface AppConfig {
  enabled: boolean;
  autoStartOverlay: boolean;
  preset: Preset;
  intensity: number;
  motionSource: MotionSource;
  manualMotion: number;
  vignetteEnabled: boolean;
  vignetteStrength: number;
  vignetteSoftness: number;
  asymmetricFov: boolean;
  groundVisible: boolean;
  restFrameEnabled: boolean;
  restFrameStyle: RestFrameStyle;
  restFrameOpacity: number;
  peripheralBlurEnabled: boolean;
  peripheralBlurStrength: number;
  attackSecs: number;
  releaseSecs: number;
  motionDeadzone: number;
  vignetteSlewUp: number;
  vignetteSlewDown: number;
  foveatedClearEnabled: boolean;
  foveatedClearRadius: number;
  faceSensingEnabled: boolean;
  comfortAutoRamp: boolean;
  peripheralFlowDampen: boolean;
  predictiveFov: boolean;
  sideRestrictor: boolean;
  realPeripheralBlur: boolean;
  /** Auto-detect games: faster FOV + clear zone centered on turns/glances. */
  gamingModeAuto: boolean;
}

export interface LiveMetrics {
  motion: number;
  peripheralFlow: number;
  centralFlow: number;
  rotational: number;
  translational: number;
  flowX: number;
  flowY: number;
  comfortIndex: number;
  fovRestriction: number;
  latencyMs: number;
  analyzeFps: number;
  captureOk: boolean;
  captureStatus: string;
  sessionPeak: number;
  framesAnalyzed: number;
  sessionSecs: number;
  /** Composite pupil quality 0–1 (not labeled GT accuracy). */
  pupilQuality: number;
  /** Dual-eye agreement — live accuracy proxy. */
  pupilAgreement: number;
  /** Recent frames with pupil lock. */
  pupilLockRate: number;
  /** Gaze jitter while still (lower better). */
  pupilJitter: number;
  gamingActive: boolean;
  gamingScore: number;
}

export interface OverlayFrame {
  enabled: boolean;
  motion: number;
  vignetteAmount: number;
  vignetteSoftness: number;
  biasX: number;
  biasY: number;
  groundClear: number;
  restFrameStyle: RestFrameStyle;
  restFrameOpacity: number;
  blurAmount: number;
  horizonRoll: number;
  clearX: number;
  clearY: number;
  clearRadius: number;
  foveatedClear: boolean;
  virtualNose: boolean;
  flowDampen: number;
  sideMask: number;
  sideSign: number;
  realBlur: boolean;
  blurPx: number;
  faceOk: boolean;
  facePresent: boolean;
  faceStatus: string;
  metrics: LiveMetrics;
}
