import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppConfig, LiveMetrics, OverlayFrame, Preset } from "./types";

function $(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`Missing #${id}`);
  return el;
}

function $input(id: string): HTMLInputElement {
  return $(id) as HTMLInputElement;
}

function $select(id: string): HTMLSelectElement {
  return $(id) as HTMLSelectElement;
}

function pct(v: number): string {
  return `${Math.round(v * 100)}%`;
}

let config: AppConfig | null = null;
let overlayOn = false;
let syncing = false;
let pendingMetrics: LiveMetrics | null = null;
let metricsFlush = 0;

async function loadConfig() {
  config = await invoke<AppConfig>("get_config");
  applyConfigToUi(config);
  overlayOn = await invoke<boolean>("is_overlay_active");
  setOverlayUi(overlayOn);
}

function setOverlayUi(active: boolean) {
  overlayOn = active;
  const badge = $("overlay-status");
  const power = $("btn-power");
  badge.textContent = active ? "ON" : "Off";
  badge.classList.toggle("on", active);
  badge.classList.toggle("off", !active);
  power.textContent = active ? "ON" : "OFF";
  power.classList.toggle("is-off", !active);
}

function applyConfigToUi(cfg: AppConfig) {
  syncing = true;
  $input("intensity").value = String(Math.round(cfg.intensity * 100));
  $("intensity-out").textContent = pct(cfg.intensity);
  $input("manual-motion").value = String(Math.round(cfg.manualMotion * 100));
  $("manual-out").textContent = pct(cfg.manualMotion);
  $input("vignette").checked = cfg.vignetteEnabled;
  $input("vignette-strength").value = String(
    Math.round(cfg.vignetteStrength * 100),
  );
  $("vig-str-out").textContent = pct(cfg.vignetteStrength);
  $input("asymmetric").checked = cfg.asymmetricFov;
  $input("ground").checked = cfg.groundVisible;
  $input("rest-frame").checked = cfg.restFrameEnabled;
  $select("rest-style").value = cfg.restFrameStyle;
  $input("blur").checked = cfg.peripheralBlurEnabled;
  $input("foveated").checked = cfg.foveatedClearEnabled;
  $input("face-sensing").checked = cfg.faceSensingEnabled;
  $input("comfort-ramp").checked = cfg.comfortAutoRamp;
  $input("flow-dampen").checked = cfg.peripheralFlowDampen;
  $input("predictive").checked = cfg.predictiveFov;
  $input("side-restrictor").checked = cfg.sideRestrictor;
  $input("real-blur").checked = cfg.realPeripheralBlur;
  $input("auto-start").checked = cfg.autoStartOverlay;
  $input("pin-game-monitor").checked = cfg.pinToGameMonitor ?? true;
  $input("long-session").checked = cfg.longSessionComfort ?? true;
  $select("motion-source").value = cfg.motionSource;
  $("manual-block").hidden = cfg.motionSource !== "manual";

  document.querySelectorAll<HTMLButtonElement>("[data-preset]").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.preset === cfg.preset);
  });
  $("btn-gaming").classList.toggle("active", cfg.gamingMode);

  $("btn-toggle-engine").textContent = cfg.enabled
    ? "Pausar motor"
    : "Retomar motor";
  syncing = false;
}

async function pushConfig(patch: Partial<AppConfig>) {
  if (!config || syncing) return;
  const next: AppConfig = {
    ...config,
    ...patch,
    preset: patch.preset ?? "custom",
  };
  config = await invoke<AppConfig>("set_config", { config: next });
  applyConfigToUi(config);
}

function queueMetrics(m: LiveMetrics) {
  pendingMetrics = m;
  const now = performance.now();
  if (now - metricsFlush < 100) return;
  metricsFlush = now;
  flushMetrics();
}

function flushMetrics() {
  const m = pendingMetrics;
  if (!m) return;
  pendingMetrics = null;

  $("m-motion").textContent = pct(m.motion);
  $("motion-bar").style.width = pct(m.motion);
  $("m-fov").textContent = pct(m.fovRestriction);
  $("fov-bar").style.width = pct(m.fovRestriction);
  $("comfort-readout").textContent = pct(m.comfortIndex);
  $("comfort-bar").style.width = pct(m.comfortIndex);

  $("m-peri").textContent = pct(m.peripheralFlow);
  $("m-cent").textContent = pct(m.centralFlow);
  $("m-rot").textContent = pct(m.rotational);
  $("m-trans").textContent = pct(m.translational);
  $("m-lat").textContent = m.latencyMs > 0 ? `${m.latencyMs.toFixed(0)}ms` : "—";
  $("m-fps").textContent = m.analyzeFps > 0 ? m.analyzeFps.toFixed(0) : "—";
  $("m-peak").textContent = pct(m.sessionPeak);
  $("m-secs").textContent = `${Math.floor(m.sessionSecs)}s`;
  $("m-pupil-q").textContent = pct(m.pupilQuality ?? 0);
  $("m-pupil-agree").textContent = pct(m.pupilAgreement ?? 0);
  $("m-pupil-lock").textContent = pct(m.pupilLockRate ?? 0);
  $("m-pupil-jit").textContent =
    m.pupilJitter != null ? m.pupilJitter.toFixed(3) : "—";

  const gameBadge = $("gaming-status");
  const gaming = !!m.gamingActive;
  gameBadge.hidden = !gaming;
  gameBadge.classList.toggle("on", gaming);
  gameBadge.classList.toggle("off", !gaming);

  const hint = $("cap-hint");
  hint.textContent = m.captureOk ? m.captureStatus : m.captureStatus;
  hint.classList.toggle("ok", m.captureOk);
  hint.classList.toggle("bad", !m.captureOk);
}

function wireUi() {
  $("btn-power").addEventListener("click", async () => {
    if (overlayOn) {
      await invoke("stop_overlay");
      setOverlayUi(false);
    } else {
      await invoke("start_overlay");
      setOverlayUi(true);
    }
  });

  $("btn-toggle-engine").addEventListener("click", async () => {
    config = await invoke<AppConfig>("toggle_enabled");
    applyConfigToUi(config);
  });

  document.querySelectorAll<HTMLButtonElement>("[data-preset]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const preset = btn.dataset.preset as Preset;
      config = await invoke<AppConfig>("apply_preset", { preset });
      applyConfigToUi(config);
    });
  });

  $input("intensity").addEventListener("input", (e) => {
    const v = Number((e.target as HTMLInputElement).value) / 100;
    $("intensity-out").textContent = pct(v);
    void pushConfig({ intensity: v });
  });

  $input("manual-motion").addEventListener("input", (e) => {
    const v = Number((e.target as HTMLInputElement).value) / 100;
    $("manual-out").textContent = pct(v);
    void pushConfig({ manualMotion: v });
  });

  $input("vignette-strength").addEventListener("input", (e) => {
    const v = Number((e.target as HTMLInputElement).value) / 100;
    $("vig-str-out").textContent = pct(v);
    void pushConfig({ vignetteStrength: v });
  });

  const boolMap: Array<[string, keyof AppConfig]> = [
    ["vignette", "vignetteEnabled"],
    ["asymmetric", "asymmetricFov"],
    ["ground", "groundVisible"],
    ["rest-frame", "restFrameEnabled"],
    ["blur", "peripheralBlurEnabled"],
    ["foveated", "foveatedClearEnabled"],
    ["face-sensing", "faceSensingEnabled"],
    ["comfort-ramp", "comfortAutoRamp"],
    ["flow-dampen", "peripheralFlowDampen"],
    ["predictive", "predictiveFov"],
    ["side-restrictor", "sideRestrictor"],
    ["real-blur", "realPeripheralBlur"],
    ["auto-start", "autoStartOverlay"],
    ["pin-game-monitor", "pinToGameMonitor"],
    ["long-session", "longSessionComfort"],
  ];

  for (const [id, key] of boolMap) {
    $input(id).addEventListener("change", (e) => {
      void pushConfig({
        [key]: (e.target as HTMLInputElement).checked,
      } as Partial<AppConfig>);
    });
  }

  $("btn-gaming").addEventListener("click", () => {
    if (!config) return;
    void pushConfig({ gamingMode: !config.gamingMode, preset: config.preset });
  });

  $select("rest-style").addEventListener("change", (e) => {
    void pushConfig({
      restFrameStyle: (e.target as HTMLSelectElement)
        .value as AppConfig["restFrameStyle"],
    });
  });

  $select("motion-source").addEventListener("change", (e) => {
    const motionSource = (e.target as HTMLSelectElement)
      .value as AppConfig["motionSource"];
    $("manual-block").hidden = motionSource !== "manual";
    void pushConfig({ motionSource });
  });
}

async function wireEvents() {
  await listen<OverlayFrame>("overlay-frame", (event) => {
    queueMetrics(event.payload.metrics);
    const f = event.payload;
    const faceEl = $("m-face");
    if (!f.faceOk) {
      faceEl.textContent = f.faceStatus || "off";
    } else if (f.facePresent) {
      const q = Math.round((f.metrics.pupilQuality ?? 0) * 100);
      faceEl.textContent = `${f.faceStatus} Q${q} ${Math.round(f.clearX * 100)},${Math.round(f.clearY * 100)}`;
    } else {
      faceEl.textContent = f.faceStatus || "—";
    }
  });

  await listen<AppConfig>("config-updated", (event) => {
    config = event.payload;
    applyConfigToUi(config);
  });

  await listen<boolean>("overlay-toggled", (event) => {
    setOverlayUi(event.payload);
  });

}

function wireComfortGate() {
  const key = "mosickwhile.comfortGate.v1";
  const gate = $("comfort-gate");
  if (!localStorage.getItem(key)) {
    gate.hidden = false;
  }
  $("gate-go").addEventListener("click", () => {
    localStorage.setItem(key, "1");
    gate.hidden = true;
  });
}

wireComfortGate();
wireUi();
void loadConfig();
void wireEvents();
