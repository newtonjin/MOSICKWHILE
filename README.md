# MOSICKWHILE

**Mo**tion **Sick**ness **Whi**le Gaming — overlay Windows feito para quem tem cinetose: **mais horas de jogo com menos enjoo e dor de cabeça**.

## Usar (plug and play)

1. **[Releases](https://github.com/newtonjin/MOSICKWHILE/releases/latest)** → baixe o zip ou o setup  
2. Execute **`MOSICKWHILE.exe`**  
3. No jogo: **Borderless / Janela sem bordas**  
4. Overlay **ON** · em FPS/ação ative **Gaming**  

| Atalho | Ação |
|--------|------|
| `Ctrl+Shift+M` | Liga/desliga overlay |
| `Ctrl+Shift+=` / `-` | Intensidade |

## Conforto (o que importa)

- **FOV dinâmica** corta optical flow periférico (principal gatilho de VIMS)
- **Olhar inteligente** — MediaPipe (íris + pose) + **calibração 9 pontos**
- **Hold longo** — quanto mais tempo com motion, mais a proteção segura
- **Multi-monitor** — FOV fixa no **monitor do jogo** em foco
- Overlay **externo** always-on-top (sem inject)

Na 1ª abertura: permita a webcam e rode **Calibrar 9 pontos**.

## Desenvolvimento

```bash
npm install
npm run tauri build
```

Sem telemetria remota. Roda só no seu PC.
