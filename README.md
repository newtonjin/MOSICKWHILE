# MOSICKWHILE

**Mo**tion **Sick**ness **Whi**le Gaming — overlay Windows que reduz cinetose em jogos (1ª/3ª pessoa).

## Usar (plug and play)

Não precisa instalar Node, Rust nem dependências.

1. Abra **[Releases](https://github.com/newtonjin/MOSICKWHILE/releases/latest)**
2. Baixe **`MOSICKWHILE-windows-x64.zip`** (portátil) **ou** o instalador `.exe`
3. Extraia (se zip) e execute **`MOSICKWHILE.exe`**

| Atalho | Ação |
|--------|------|
| `Ctrl+Shift+M` | Liga/desliga overlay |
| `Ctrl+Shift+=` / `-` | Intensidade |

### Como jogar (sem crash)

- Overlay **externo** no monitor, sempre acima (sem injetar DLL no jogo)
- No jogo, use **Borderless / Janela sem bordas / Fullscreen windowed**
- Fullscreen exclusivo puro do DirectX não deixa janelas externas por cima — por isso não usamos inject (podia crashar o jogo)

### Modo gaming

Ative **manualmente** no painel. FOV mais rápido; viradas e olhadas de relance mantêm a zona clara no **centro**.

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

## Licença / privacidade

Código-fonte neste repositório. Sem telemetria remota, sem API keys, sem contas. O app roda local no seu PC.
