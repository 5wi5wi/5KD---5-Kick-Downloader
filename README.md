<div align="center">
  <img src="public/assets/5kd-icon.svg" alt="5KD Logo" width="96" height="96" />
  <h1>5KD — 5wi's Kick Downloader</h1>
  <p><strong>High-performance native Rust desktop application for extracting and downloading Kick.com VODs &amp; clips with lossless time slicing and real-time frame/duration progress.</strong></p>
</div>

---

## Features

- **Integrated Kick VOD & Clip Extractor**:
  - Automatically resolves Kick VOD URLs (`https://kick.com/{channel}/videos/{uuid}`), Clip URLs (`https://kick.com/{channel}/clips/{clip_id}` or `?clip=...`), and direct `.m3u8` URLs.
  - Supports legacy `v1/video/{uuid}` VODs, modern **UUIDv7** `v2/channels/{channel}/videos` VODs via 48-bit hex timestamp matching, and Kick Clip API v2.
  - Reconstructs subscriber-only / hidden source streams from thumbnail session/segment IDs and scans known CDN prefixes (`prefixes.json`) automatically, with silent `curl` and `yt-dlp` fallbacks.
- **Precise Frame- & Duration-Based Progress Tracking**:
  - Probes the selected HLS M3U8 variant's `#EXTINF` segment durations & `FRAME-RATE` (or uses `ffprobe` for exact `nb_frames`, FPS, and duration on MP4 clips) before downloading.
  - Automatically recalculates target duration and total frames when Start/End time slicing (`-ss` / `-to`) is active.
  - Streams live `\r`/`\n` `ffmpeg` progress updates with 1-decimal percentage precision (`0.0%`–`99.9%` → `100%`) and live `Frame X / Y · HH:MM:SS / HH:MM:SS · speed` stats.
- **Multi-Threaded HLS Engine (`yt-dlp` + `ffmpeg`)**:
  - Downloads HLS segments concurrently (`-N 8`) with custom browser headers (`Origin`, `Referer`, `User-Agent`) to prevent HTTP 403 blocks.
  - Automatically sanitizes and names saved files after the actual Kick VOD or Clip title (with automatic duplicate disambiguation).
- **Custom Time-Range Slicing**:
  - Download specific sections of any broadcast without downloading the entire VOD using **Start (`-ss`)** and **End (`-to`)** (`HH:MM:SS`).
- **Resolution & Container Selection**:
  - Choose from `1080p60 (Source)`, `1080p`, `720p60`, `720p`, `480p`, `360p`, or `Audio Only`.
  - Remux directly into `.MKV`, `.MP4`, or `.TS` containers.
- **Unified Geometric App Icon**:
  - Anti-aliased `#00E701` geometric "5" badge on `#0D0E10` across the in-app brand mark, runtime window/taskbar icon (`128×128`), and Windows executable & setup installer (`16×16`–`256×256` multi-size `.ico` with transparent rounded corners).
- **In-App Auto-Updater**:
  - Checks GitHub Releases automatically on launch (configurable) or manually from the **About** tab, downloads the matching platform package in-app, installs silently, cleans up temporary installer files, and restarts automatically.
- **Persistent Preferences, Queue Controls & Live Terminal**:
  - Remembers your chosen output folder, quality, container format, UI zoom (`100%`–`150%`), and update settings across restarts.
  - Includes clipboard **Paste**, clean process-tree **Cancel** on active downloads, **Open Folder** / reveal file on completed downloads, and real-time stdout/stderr inspection in the **Terminal** tab.

---

## Prerequisites

1. **Rust Toolchain** ([rustup.rs](https://rustup.rs/)) — stable `cargo` and `rustc`.
2. **[yt-dlp](https://github.com/yt-dlp/yt-dlp)** — installed and available in your system `PATH`.
3. **[FFmpeg](https://ffmpeg.org/download.html)** (including `ffprobe`) — installed and available in your system `PATH`.

### Quick Dependency Install

#### Windows (Winget)
```powershell
winget install Rustlang.Rustup
winget install yt-dlp.yt-dlp
winget install Gyan.FFmpeg
```
*(Optional, for building the Windows Setup Wizard installer locally: `winget install JRSoftware.InnoSetup`)*

#### Linux (Ubuntu / Debian)
Install runtime tools plus the GTK3/OpenGL/Wayland/X11 development headers required by `eframe` and `rfd`:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt-get update
sudo apt-get install -y \
  yt-dlp ffmpeg pkg-config \
  libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libwayland-dev libgl1-mesa-dev
```

#### macOS (Homebrew)
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install yt-dlp ffmpeg
```
*(Make sure Xcode Command Line Tools are installed: `xcode-select --install`)*

---

## Building & Creating a Release

### Windows

#### Option 1: One-Click Local Release Build (`build_release.bat`)
Double-click **`build_release.bat`** (or run it from a terminal):
```bat
.\build_release.bat
```
This compiles `5KD.exe` with full LTO + symbol stripping (`[profile.release]`), embeds the `#00E701` geometric `5` multi-size icon via `build.rs`, builds the Inno Setup installer at **`dist\5KD-windows-x86_64-setup.exe`** (or copies `dist\5KD.exe` if Inno Setup 6 is not installed), and opens the `dist\` folder in Explorer.

#### Option 2: Manual Cargo Build (Windows)
```powershell
# Run directly in release mode
cargo run --release

# Or compile the standalone binary at target\release\5KD.exe
cargo build --release
```

---

### Linux

#### Option 1: Standard Cargo Build & Run
```bash
# Run directly in release mode
cargo run --release

# Or compile the release binary at target/release/5KD
cargo build --release
```

#### Option 2: Package Release Archive (`.tar.gz`)
```bash
cargo build --release
mkdir -p staging dist
cp target/release/5KD staging/5KD-linux-x86_64
chmod +x staging/5KD-linux-x86_64
tar -czf dist/5KD-linux-x86_64.tar.gz -C staging 5KD-linux-x86_64
```
The packaged release archive is created at **`dist/5KD-linux-x86_64.tar.gz`**.

---

### macOS

#### Option 1: Standard Cargo Build & Run
```bash
# Run directly in release mode
cargo run --release

# Or compile the release binary at target/release/5KD
cargo build --release
```

#### Option 2: Package Release Archive (Apple Silicon & Intel)
```bash
# Build for your current Mac architecture (arm64 on Apple Silicon, x86_64 on Intel)
ARCH=$(uname -m)
if [ "$ARCH" = "arm64" ]; then
  TARGET="aarch64-apple-darwin"
  ASSET="5KD-macos-arm64"
else
  TARGET="x86_64-apple-darwin"
  ASSET="5KD-macos-x86_64"
fi

rustup target add "$TARGET"
cargo build --release --target "$TARGET"
mkdir -p staging dist
cp "target/$TARGET/release/5KD" "staging/$ASSET"
chmod +x "staging/$ASSET"
tar -czf "dist/$ASSET.tar.gz" -C staging "$ASSET"
```
To cross-compile for the other macOS architecture on the same Mac, add the target (`aarch64-apple-darwin` or `x86_64-apple-darwin`) and pass `--target` to `cargo build --release`.

---

## Usage

1. **Select an Output Folder**: Click **Choose Folder** in the `Output Folder` bar to pick where your downloaded VODs and clips will be saved (remembered automatically on future launches).
2. **Configure Options (Optional)**:
   - Enter **Start (`-ss`)** and/or **End (`-to`)** in `HH:MM:SS` format to clip a specific segment.
   - Select your desired **Quality** (`1080p60 (Source)` by default) and **Format** (`.MKV`, `.MP4`, or `.TS`).
3. **Paste URL & Download**:
   - Paste any Kick VOD URL (`https://kick.com/channel/videos/uuid`), Clip URL, or direct `.m3u8` URL into the top input bar (or click **Paste**) and click **Download**.
   - Track exact 1-decimal percentage, frame progress, elapsed/total timestamp, and speed in **Active Queue**, or inspect live logs in the **Terminal** tab.
   - Click **Cancel** at any time to cleanly terminate an ongoing download.

---

## Project Structure

```text
├── Cargo.toml                     # Rust crate manifest & dependencies (eframe, egui, reqwest, rfd, serde, serde_json, arboard)
├── build.rs                       # Embeds multi-size geometric 5KD .ico on Windows & injects build version/repo metadata
├── installer.iss                  # Inno Setup 6 Windows Setup Wizard installer script
├── build_release.bat              # One-click Windows release binary & setup installer builder
├── .github/workflows/release.yml  # Multi-platform GitHub Release CI workflow (Windows, Linux, macOS)
├── src/
│   └── main.rs                    # Native Rust GUI, Kick VOD/Clip extractor, stream prober, progress engine, & auto-updater
└── public/
    └── assets/                    # Geometric SVG app icon (5kd-icon.svg) & visual assets
```

---

## Credits & Acknowledgments

- **M3U8 URL Extractor**: Adapted from [**kickvod-extractor**](https://github.com/mortyobnoxious/kickvod-extractor) by [@mortyobnoxious](https://github.com/mortyobnoxious).
- **Stream Downloading, Probing & Muxing**: Powered by [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [FFmpeg / ffprobe](https://ffmpeg.org/).
- **GUI Framework**: Built with [egui / eframe](https://github.com/emilk/egui).
