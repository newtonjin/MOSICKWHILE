import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
import {
  currentMonitor,
  getCurrentWindow,
  primaryMonitor,
} from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { CALIB_TARGETS, type CalibSample } from "./calibration";
import { gazeTracker } from "./tracker";

/**
 * Run 13-point calibration on the FULL monitor (not the tiny panel).
 * Expands the main window to cover the active display, then restores it.
 */
export async function runCalibrationWizard(
  root: HTMLElement,
): Promise<boolean> {
  const win = getCurrentWindow();
  const restore = await captureWindowState(win);

  root.hidden = false;
  root.innerHTML = "";
  root.className = "calib-overlay";

  const card = document.createElement("div");
  card.className = "calib-card";
  root.appendChild(card);

  const title = document.createElement("h2");
  title.textContent = "Calibrar olhar — tela cheia";
  card.appendChild(title);

  const lead = document.createElement("p");
  lead.className = "calib-lead";
  lead.textContent =
    "Os pontos cobrem o MONITOR inteiro. Olhe cada um sem mexer a cabeça. Óculos: luz frontal, sem reflexo.";
  card.appendChild(lead);

  const dot = document.createElement("div");
  dot.className = "calib-dot";
  root.appendChild(dot);

  const prog = document.createElement("div");
  prog.className = "calib-prog";
  card.appendChild(prog);

  const status = document.createElement("p");
  status.className = "calib-status";
  card.appendChild(status);

  const samples: CalibSample[] = [];
  // Remember FOV state — we hide it during calib and must restore after.
  let wasOverlayOn = false;
  try {
    wasOverlayOn = await invoke<boolean>("is_overlay_active");
  } catch {
    wasOverlayOn = true; // badge usually ON; prefer restoring
  }

  try {
    status.textContent = "Expandindo para o monitor…";
    // Hide FOV overlay so dots aren't under the vignette.
    try {
      await invoke("stop_overlay");
    } catch {
      /* ignore */
    }
    await expandToMonitor(win);
    await sleep(200);

    if (!gazeTracker.getStatus().running) {
      status.textContent = "Abrindo câmera…";
      await gazeTracker.start();
    }

    status.textContent = "Preparando sensores…";
    await sleep(700);

    for (let i = 0; i < CALIB_TARGETS.length; i++) {
      const t = CALIB_TARGETS[i];
      dot.style.left = `${t.x * 100}%`;
      dot.style.top = `${t.y * 100}%`;
      dot.classList.add("pulse");
      prog.textContent = `${i + 1} / ${CALIB_TARGETS.length}`;
      status.textContent =
        t.x > 0.85
          ? "Olhe bem à DIREITA no ponto…"
          : t.x < 0.15
            ? "Olhe bem à ESQUERDA no ponto…"
            : "Olhe o ponto verde…";

      await sleep(550);
      let feat = await gazeTracker.sampleFor(1400, (p) => {
        status.textContent = `Segurando… ${Math.round(p * 100)}%`;
        dot.style.transform = `translate(-50%, -50%) scale(${1 + p * 0.35})`;
      });
      dot.style.transform = "translate(-50%, -50%) scale(1)";

      if (!feat) {
        status.textContent = "Sinal fraco (óculos/luz) — de novo";
        i -= 1;
        await sleep(700);
        continue;
      }
      if ((t.x > 0.85 || t.x < 0.15) && feat.pupilQ < 0.2 && !feat.onnxYaw) {
        status.textContent = "Reforçando borda…";
        const again = await gazeTracker.sampleFor(1200);
        if (again) feat = again;
      }
      samples.push({ ...feat, x: t.x, y: t.y });
    }

    const ok = gazeTracker.finishCalibration(samples);
    if (!ok) {
      status.textContent = "Falhou — mais luz frontal, menos reflexo";
      await sleep(1400);
      return false;
    }

    status.textContent = "Validando… olhe o centro";
    dot.style.left = "50%";
    dot.style.top = "50%";
    await sleep(400);
    status.textContent = "Validando… olhe a DIREITA";
    dot.style.left = "92%";
    dot.style.top = "50%";
    const err = await gazeTracker.validateMap([
      { x: 0.5, y: 0.5 },
      { x: 0.92, y: 0.5 },
      { x: 0.08, y: 0.5 },
    ]);

    if (err > 0.18) {
      status.textContent = `Validação fraca (${(err * 100).toFixed(0)}%) — tente de novo`;
      await sleep(1600);
      return false;
    }

    status.textContent = `OK — erro ~${(err * 100).toFixed(0)}% · FOV segue o olhar`;
    await sleep(1100);
    return true;
  } catch (e) {
    status.textContent =
      e instanceof Error ? e.message : "Erro na calibração";
    await sleep(1600);
    return false;
  } finally {
    root.hidden = true;
    root.innerHTML = "";
    await restoreWindowState(win, restore);
    // Bring FOV / rest frame back — stop_overlay left them hidden while UI stayed "ON".
    if (wasOverlayOn) {
      try {
        await sleep(120);
        await invoke("start_overlay");
      } catch (e) {
        console.warn("restore overlay after calib failed", e);
      }
    }
  }
}

type WinState = {
  fullscreen: boolean;
  x: number;
  y: number;
  w: number;
  h: number;
};

async function captureWindowState(
  win: ReturnType<typeof getCurrentWindow>,
): Promise<WinState> {
  const pos = await win.outerPosition();
  const size = await win.outerSize();
  const fullscreen = await win.isFullscreen().catch(() => false);
  // Convert physical → logical for restore on DPI-aware Windows.
  const scale = await win.scaleFactor();
  return {
    fullscreen,
    x: pos.x / scale,
    y: pos.y / scale,
    w: size.width / scale,
    h: size.height / scale,
  };
}

async function expandToMonitor(
  win: ReturnType<typeof getCurrentWindow>,
): Promise<void> {
  const mon = (await currentMonitor()) ?? (await primaryMonitor());
  if (!mon) {
    await win.setFullscreen(true);
    return;
  }
  await win.setFullscreen(false);
  await win.setDecorations(false);
  const scale = mon.scaleFactor;
  // Monitor position/size are physical pixels in Tauri 2.
  const x = mon.position.x / scale;
  const y = mon.position.y / scale;
  const w = mon.size.width / scale;
  const h = mon.size.height / scale;
  await win.setPosition(new LogicalPosition(x, y));
  await win.setSize(new LogicalSize(w, h));
  await win.setAlwaysOnTop(true);
  await win.setFocus();
}

async function restoreWindowState(
  win: ReturnType<typeof getCurrentWindow>,
  s: WinState,
): Promise<void> {
  try {
    await win.setAlwaysOnTop(false);
    await win.setFullscreen(false);
    await win.setDecorations(true);
    await win.setPosition(new LogicalPosition(s.x, s.y));
    await win.setSize(new LogicalSize(s.w, s.h));
    if (s.fullscreen) {
      await win.setFullscreen(true);
    }
  } catch (e) {
    console.warn("restore window failed", e);
    try {
      await win.setFullscreen(false);
      await win.setDecorations(true);
      await win.setSize(new LogicalSize(372, 640));
    } catch {
      /* ignore */
    }
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
