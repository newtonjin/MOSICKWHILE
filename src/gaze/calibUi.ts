import {
  CALIB_TARGETS,
  type CalibSample,
} from "./calibration";
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
  title.textContent = "Calibrar olhar (9 pontos)";
  card.appendChild(title);

  const lead = document.createElement("p");
  lead.className = "calib-lead";
  lead.textContent =
    "Olhe cada ponto sem mexer a cabeça. Isso ensina o FOV onde você realmente olha.";
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

    for (let i = 0; i < CALIB_TARGETS.length; i++) {
      const t = CALIB_TARGETS[i];
      dot.style.left = `${t.x * 100}%`;
      dot.style.top = `${t.y * 100}%`;
      dot.classList.add("pulse");
      prog.textContent = `${i + 1} / ${CALIB_TARGETS.length}`;
      status.textContent = "Olhe o ponto verde…";

      await sleep(450);
      const feat = await gazeTracker.sampleFor(900, (p) => {
        status.textContent = `Segurando… ${Math.round(p * 100)}%`;
        dot.style.transform = `translate(-50%, -50%) scale(${1 + p * 0.35})`;
      });
      dot.style.transform = "translate(-50%, -50%) scale(1)";

      if (!feat) {
        status.textContent = "Não vi o olhar — tentando de novo";
        i -= 1;
        await sleep(600);
        continue;
      }
      samples.push({ ...feat, x: t.x, y: t.y });
    }

    const ok = gazeTracker.finishCalibration(samples);
    status.textContent = ok
      ? "Calibração salva — FOV segue seu olhar"
      : "Falhou — tente de novo com mais luz";
    await sleep(900);
    return ok;
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
