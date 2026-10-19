import { CALIB_TARGETS, type CalibSample } from "./calibration";
import { gazeTracker } from "./tracker";

export async function runCalibrationWizard(
  root: HTMLElement,
): Promise<boolean> {
  root.hidden = false;
  root.innerHTML = "";
  root.className = "calib-overlay";

  const card = document.createElement("div");
  card.className = "calib-card";
  root.appendChild(card);

  const title = document.createElement("h2");
  title.textContent = "Calibrar olhar (13 pontos)";
  card.appendChild(title);

  const lead = document.createElement("p");
  lead.className = "calib-lead";
  lead.textContent =
    "Com óculos: luz frontal suave, sem reflexo forte. Olhe cada ponto sem mexer a cabeça — pupila + ONNX.";
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

  try {
    if (!gazeTracker.getStatus().running) {
      status.textContent = "Abrindo câmera…";
      await gazeTracker.start();
    }

    // Warm-up so ONNX + pupil settle.
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
      // Retry edge points once if pupilQ collapses.
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

    // Validate center + right + left.
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
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
