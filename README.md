# MOSICKWHILE

**Mo**tion **Sick**ness **Whi**le Gaming — overlay Windows que reduz cinetose em jogos (1ª/3ª pessoa).

## Usar (plug and play)

Não precisa instalar Node, Rust nem dependências.

1. Abra a página de **[Releases](../../releases/latest)**
2. Baixe **`MOSICKWHILE-windows-x64.zip`** (portátil) **ou** o instalador `.exe`
3. Extraia (se zip) e execute **`MOSICKWHILE.exe`**

Pronto. Webcam (opcional, pupilas), tray ao minimizar, hotkeys abaixo.

| Atalho | Ação |
|--------|------|
| `Ctrl+Shift+M` | Liga/desliga overlay |
| `Ctrl+Shift+=` / `-` | Intensidade |

### Fullscreen exclusivo

1. Abra o jogo  
2. No painel: **Anexar ao jogo (3s)** e foque o jogo  
3. A bridge desenha no Present (DX9/11/12)

> Anti-cheat (EAC / BattlEye / Vanguard) pode bloquear injeção. Use por sua conta e risco.

### Modo gaming

Detecta jogo em foco / bridge ativa: FOV mais rápido; viradas e olhadas de relance mantêm a zona clara no **centro** (sem depender do cursor).

---

## O que faz

- FOV / vignette dinâmica (e preditiva)
- Zona foveal guiada por **pupilas** (webcam)
- Rest frame (horizonte terra, etc.)
- Blur periférico, side restrictor, comfort ramp
- Dual-monitor + tray

## Desenvolvimento (só quem vai compilar)

```bash
npm install
npm run tauri build
```

Artefatos: `target/release/mosickwhile.exe` e instalador NSIS em `target/release/bundle/nsis/`.

```bash
npm run tauri dev
```

## Licença / privacidade

Código-fonte neste repositório. Sem telemetria remota, sem API keys, sem contas. O app roda local no seu PC.
