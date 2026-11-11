# RustTune

An audio-only, gamepad-first YouTube Music player built with Rust and Slint for the **Anbernic RG35XX H** running **ROCKNIX** and **PortMaster** (640x480). Ships as a single self-contained `aarch64` binary with pure-Rust AAC decoding (`symphonia`), embedded Svelte 5 + Vite Web UI, and zero bundled FFmpeg or mpv libraries.

> **Hardware Support Note:** Built and tested on ROCKNIX with an Anbernic RG35XX H. Currently I only have an RG35XX H with me, so I will not be able to test it on other handhelds.
>
> **AI / LLM Disclosure:** LLMs were used to varying degrees for the development of this project.
>
> **License:** MIT No Attribution (`MIT-0`), Copyright 2026 `dfrost16`. You are allowed to fork, distribute, and modify this code without attribution.

## Features

- **Handheld Studio UI (640x480):** Bottom dock (`Now`, `Vibes`, `Artists`, `Lib`, `Find`) with battery readout, Studio Dark and Studio Light themes, and slow readable marquee text.
- **Now Playing + Dynamic Cover Theme:** Extracts the dominant vibrant hue from each track's album art using a 12-bucket hue histogram and adapts UI contrast cleanly across light and dark modes.
- **Synced Lyrics & Up Next Radio:** 3-line time-synced lyrics via LRCLIB with YouTube Music plain lyrics fallback, plus automatic Up Next radio queue and song looping (`R3`).
- **Vibes & Lazy-Loaded Artists:** 3x2 genre/mood grid and paginated Artists directory with on-demand thumbnail loading, circular/rounded RGBA image masks, and Top Songs / Albums / Singles views.
- **Library (`Favs`, `Playlists`, `History`, `Downloads`):** Browse favorites, playlists, recently played history (last 100 tracks), and locally cached `.m4a` files. Includes `R3` library filtering, batch download confirmation prompts, single file removal (`Y`), and clear-all downloads.
- **Full Search Keyboard:** Bottom QWERTY keyboard with `123` number/symbol toggle (`R3` or `123` key), live search autocomplete chips, and direct YouTube/YTM URL support.
- **Pocket Lock (`L3`):** Pure black screen mode to save power in your pocket while audio keeps playing.
- **Local Web UI + 6-Digit PIN Security:** Built-in multi-threaded HTTP server (`0.0.0.0:8080`) serving two tailored layouts from the binary:
  - **Desktop (Command Workbench):** `Ctrl+K` URL/filter bar, YouTube playlist URL preview and importer, multi-select batch `.m4a` deletion, orphaned cache cleanup, playlist CRUD (create, rename, delete), playback control, and JSON library backup export.
  - **Phone (Apple HIG + AMOLED):** `#000000` AMOLED / Dark / Light themes, SF Symbols-style vector icons, YouTube playlist importer, playlist & offline storage deletion, and Home Screen support (`chrome://flags/#unsafely-treat-insecure-origin-as-secure` enables full PWA install on Android HTTP; iOS Safari supports Share -> Add to Home Screen).
  - **6-Digit Pairing PIN & Server Toggle:** Generates a random 6-digit access code when the server starts, with Settings controls to toggle the Local Web Server ON/OFF, toggle the PIN ON/OFF, or rotate a new PIN (`X`).

## Controls (RG35XX H)

| Screen | Button | Action |
|---|---|---|
| **Global** | `L1` / `R1` | Switch dock tabs (`Now`, `Vibes`, `Artists`, `Lib`, `Find`) |
| | `L2` / `R2` | Previous / Next track from any screen |
| | `SELECT` | Open / close Settings (Theme, Loop, Radio Autoplay, Swap A/B, Local Web Server, Web Security PIN, Quit) |
| | `L3` | Toggle Pocket Lock (`L3` or `START` unlocks) |
| | `START` | Add selected/current track to a Playlist |
| **Now Playing** | `A` | Play / Pause |
| | `D-Pad Left` / `Right` | Seek -10s / +10s |
| | `R-Stick Left` / `Right` | Seek -30s / +30s |
| | `D-Pad Up` / `Down` | Previous / Next track |
| | `R3` | Loop current song ON / OFF |
| | `X` / `Y` | Toggle Favorite / Download `.m4a` offline |
| **Vibes & Artists** | `D-Pad` | Navigate 3x2 grid cards |
| | `R-Stick Up` / `Down` | Jump pages fast |
| | `A` | Open Vibe mix or Artist page |
| | `X` / `Y` | Favorite card / Toggle Favorites Only |
| | `R3` | Open search/filter keyboard |
| **Library (`Lib`)** | `D-Pad Left` / `Right` or `R-Stick L/R` | Switch sub-tabs (`Favs`, `Playlists`, `History`, `Downloads`) |
| | `D-Pad Up` above row 0 | Focus header action (`Clear` all downloads in Downloads) |
| | `R-Stick Up` / `Down` | Jump +/- 5 rows |
| | `A` / `X` / `Y` | Play track / Toggle Favorite / Download (or Delete file in Downloads) |
| | `R3` | Filter current Library view |
| **Detail Pages** | `D-Pad Up` above row 0 | Focus `[Download All]` button (`A` opens confirmation modal) |
| | `D-Pad L/R` or `R-Stick L/R` | Switch `Songs / Albums / Singles` on Artist Detail |
| **Find / Keyboard** | `D-Pad` | Navigate keys (`Up` from top row enters Hits / Suggestions) |
| | `R3` | Toggle `ABC` / `123` number & symbol layout |
| | `A` / `X` / `Y` | Press key / Space / Backspace |

## Build & Cross-Compile Guide

### Prerequisites

- **Node.js (v20+) & npm:** Used to build the Svelte 5 + Vite web frontend before embedding it into the Rust binary.
- **Rust (`rustup`):**
  ```powershell
  rustup target add aarch64-unknown-linux-gnu
  ```
- **Zig + `cargo-zigbuild` (for Windows/macOS cross-compilation to ROCKNIX glibc 2.36):**
  ```powershell
  cargo install cargo-zigbuild
  # Plus install Zig (e.g., `winget install zig.zig` on Windows or `brew install zig` on macOS)
  ```
  *Why Zig?* `cargo-zigbuild` cross-compiles C dependencies (`rusqlite` bundled SQLite) to `aarch64-unknown-linux-gnu` without needing Docker or manual Linux sysroots, while pinning `glibc 2.36` ABI compatibility for ROCKNIX. On native ARM64 Linux or Ubuntu CI runners (`ubuntu-24.04-arm`), standard `cargo build --release` works as well.
- **Python 3:** Used by `build_port.py` to generate fallback cover icons, enforce LF line endings on `RustTune.sh`, and bundle `dist/rusttune.zip`.

### Build Steps

1. **Build the Web UI assets** (embedded at compile time by `ytmusic-rs/build.rs`):
   ```powershell
   cd webapp
   npm install
   npm run build
   cd ..
   ```

2. **Run tests and cross-compile the RG35XX H binary:**
   ```powershell
   cd ytmusic-rs
   cargo test
   cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.36
   cd ..
   ```

3. **Package PortMaster zip:**
   ```powershell
   python build_port.py
   ```
   Outputs `dist/rusttune.zip` ready for PortMaster (`autoinstall/` or manual extraction to your ports directory).
