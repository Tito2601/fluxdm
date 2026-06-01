# ⚡ FluxDM — Modern Parallel Download Manager

> **Faster than IDM** — AI-powered, privacy-first, cross-platform.

Built with **Tauri 2 + Rust + React 18 + TypeScript**.

---

## Features

| Feature | Status |
|---|---|
| Multi-segment parallel HTTP download | ✅ Phase 1 |
| SQLite download history + queue | ✅ Phase 1 |
| AI file categorization (rule-based) | ✅ Phase 1 |
| Threat scoring + risk badges | ✅ Phase 1 |
| Smart filename cleaner | ✅ Phase 1 |
| React UI (dark theme, analytics) | ✅ Phase 1 |
| Browser extension (Chrome/Firefox) | 🔧 Phase 3 |
| HLS/DASH stream downloader | 📅 Phase 5 |
| System tray + notifications | 📅 Phase 6 |
| LLM-powered renaming (local Phi-3) | 📅 Phase 6 |

---

## Quick Start

### Prerequisites

| Tool | Version |
|---|---|
| Node.js | 18+ |
| Rust | 1.77+ |
| Tauri CLI | 2.x (`cargo install tauri-cli`) |

### Setup

```powershell
# 1. Install Node dependencies
npm install

# 2. Generate app icons (required for first build)
#    Provide any 1024×1024 PNG as the source:
cargo tauri icon assets/icon.png

# 3. Development mode (hot-reload)
cargo tauri dev

# 4. Production build
cargo tauri build
```

### Without Icons (Quick Dev Test)

If you don't have an icon yet, create a blank 32×32 PNG placeholder:

```powershell
# Using PowerShell + .NET to generate a minimal PNG
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap 32, 32
$bmp.Save("src-tauri\icons\32x32.png", [System.Drawing.Imaging.ImageFormat]::Png)
$bmp128 = New-Object System.Drawing.Bitmap 128, 128
$bmp128.Save("src-tauri\icons\128x128.png", [System.Drawing.Imaging.ImageFormat]::Png)
$bmp128.Save("src-tauri\icons\128x128@2x.png", [System.Drawing.Imaging.ImageFormat]::Png)
```

---

## Project Structure

```
FluxDM/
├── src-tauri/                  # Rust backend
│   └── src/
│       ├── engine/             # Download engine (parallel, queue, merge)
│       ├── storage/            # SQLite via rusqlite
│       ├── ai/                 # Categorizer, threat scorer, renamer
│       ├── bridge/             # Browser extension IPC (Phase 3)
│       └── commands.rs         # Tauri command handlers
├── src/                        # React frontend
│   ├── components/             # UI components
│   ├── store/                  # Zustand state
│   ├── hooks/                  # useTauriEvents
│   └── types/                  # TypeScript types
├── extension/                  # Chrome/Firefox extension
└── scripts/                    # install-extension.sh/.ps1
```

---

## Architecture Decisions

| Decision | Choice | Why |
|---|---|---|
| Framework | Tauri 2 | 10MB binary vs 150MB Electron |
| Engine | Rust (tokio + reqwest) | Native async I/O, memory safe |
| Database | SQLite (bundled) | Zero config, local-first |
| Parallelism | tokio multi-task | Segment tasks run truly in parallel |
| State | Zustand (not Redux) | Simpler, less boilerplate |
| Styling | Tailwind + shadcn/ui | Fast, dark mode built-in |

---

## Browser Extension Setup

```powershell
# Windows — after loading extension in Chrome:
.\scripts\install-extension.ps1 -ExtensionId YOUR_CHROME_EXT_ID

# macOS / Linux:
chmod +x scripts/install-extension.sh
./scripts/install-extension.sh YOUR_CHROME_EXT_ID /path/to/fluxdm
```

---

## Build Phases

- **Phase 1** ✅ — Core engine, queue, storage, AI layer, UI scaffold
- **Phase 2** — Full React UI (all components wired to Rust commands)
- **Phase 3** — Browser extension + native host
- **Phase 4** — AI enhancements (better categorization, renamer v2)
- **Phase 5** — HLS/DASH stream downloader
- **Phase 6** — System tray, notifications, local LLM (Phi-3)

---

## License

MIT — © FluxDev 2026
