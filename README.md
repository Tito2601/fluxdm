# ⚡ FluxDM — Modern Parallel Download Manager

> Fast, AI-assisted, privacy-first, cross-platform.

Built with **Tauri 2 + Rust + React 18 + TypeScript**.

---

## Features

| Feature | Status |
|---|---|
| Multi-segment parallel HTTP download | ✅ |
| Pause / resume with on-disk segment recovery | ✅ |
| SQLite download history + queue | ✅ |
| AI file categorization (rule-based) | ✅ |
| Threat scoring + per-factor risk breakdown | ✅ |
| Smart filename cleaner | ✅ |
| Browser extension (Chrome/Firefox) + native host | ✅ |
| HLS/DASH stream downloader | ✅ |
| System tray + desktop notifications | ✅ |
| LLM-powered renaming (local model) | ✅ |
| **BitTorrent — magnet links and `.torrent` files** | ✅ |
| **Scheduler — time window, CPU load, battery guards** | ✅ |

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
npm install
cargo tauri dev      # development, hot-reload
cargo tauri build    # production build
```

> **Windows note:** Git ships a `link.exe` that shadows the MSVC linker. If `cargo`
> fails at the link step, prepend the MSVC toolchain to `PATH` or build from a
> Visual Studio Developer Prompt.

---

## Torrents

Paste a magnet link or pick a `.torrent` file. FluxDM waits for swarm metadata
before the row appears, so it shows the real name and size rather than a placeholder.

Torrents deliberately bypass the HTTP queue. A swarm spends most of its time
waiting on peers rather than saturating a connection, so counting torrents against
`max_parallel_downloads` would starve ordinary downloads for no benefit.

A completed torrent keeps **seeding** until you remove it — the UI shows this as a
distinct state, with upload speed, uploaded bytes, and share ratio. Peer counts are
reported as *connected* and *discovered*; the underlying engine does not distinguish
seeders from leechers, so FluxDM does not invent that number.

---

## Scheduler

Three guards, each independent, any of which can hold downloads:

| Guard | Behaviour |
|---|---|
| **Time window** | Only download between a start and stop time. A window whose stop precedes its start (e.g. `22:00 → 06:00`) wraps past midnight. |
| **CPU load** | Hold while system CPU is above a threshold. |
| **Battery** | Hold below a charge threshold. Ignored while plugged in or on a desktop. |

They are independent by design: wanting "never download below 20% battery" has
nothing to do with wanting "only download overnight".

When the gate closes, running downloads stop **cooperatively** — each worker
finishes its current chunk, flushes the partial file, and unwinds. Bytes on disk are
kept, and the transfer resumes from the exact offset when the gate reopens. Downloads
the *user* paused are never auto-resumed by the scheduler.

---

## Project Structure

```
FluxDM/
├── src-tauri/                  # Rust backend
│   └── src/
│       ├── engine/
│       │   ├── control.rs      # cooperative pause/cancel signals
│       │   ├── downloader.rs   # segmented HTTP orchestrator
│       │   ├── segment.rs      # byte-range worker with resume
│       │   ├── resume.rs       # reconciles DB segments with temp files on disk
│       │   ├── queue.rs        # concurrency + scheduler gate
│       │   ├── scheduler.rs    # time / CPU / battery guards
│       │   ├── torrent.rs      # BitTorrent session (librqbit)
│       │   ├── hls.rs dash.rs stream.rs
│       │   └── merger.rs
│       ├── storage/            # SQLite via rusqlite (+ column migrations)
│       ├── ai/                 # categorizer, threat scorer, renamer, LLM
│       ├── bridge/ server/     # browser-extension IPC
│       └── commands.rs         # Tauri command handlers
├── src/                        # React frontend
│   ├── components/             # Sidebar, Toolbar, DownloadTable, DetailPanel…
│   ├── store/                  # Zustand state
│   └── types/                  # shared TypeScript types
├── extension/                  # Chrome/Firefox extension
└── scripts/                    # extension install helpers
```

---

## Architecture Decisions

| Decision | Choice | Why |
|---|---|---|
| Framework | Tauri 2 | ~10 MB binary vs ~150 MB Electron |
| Engine | Rust (tokio + reqwest) | Native async I/O, memory safe |
| Torrents | librqbit | Pure-Rust, no native BitTorrent dependency |
| Database | SQLite (bundled) | Zero config, local-first |
| Parallelism | tokio multi-task | Segment tasks run truly in parallel |
| Stopping work | Cooperative polling | Aborting a task would strand temp files and DB rows |
| State | Zustand | Simpler than Redux, less boilerplate |
| Styling | Tailwind | Fast, dark mode built in |

---

## Browser Extension Setup

```powershell
# Windows — after loading the extension in Chrome:
.\scripts\install-extension.ps1 -ExtensionId YOUR_CHROME_EXT_ID

# macOS / Linux:
chmod +x scripts/install-extension.sh
./scripts/install-extension.sh YOUR_CHROME_EXT_ID /path/to/fluxdm
```

---

## Tests

```powershell
cargo test --lib     # 34 unit tests
npx tsc --noEmit     # frontend type check
```

---

## License

MIT — © FluxDev 2026
