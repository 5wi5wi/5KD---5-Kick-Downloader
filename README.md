<div align="center">
  <img src="public/assets/aistudio/Gemini_Generated_Image_z2ceqoz2ceqoz2ce__1__ef8ae8c7.png" alt="5KD Logo" width="96" height="96" />
  <h1>5KD — 5wi's Kick Downloader</h1>
  <p><strong>High-performance native Rust desktop application for extracting and downloading Kick.com VODs &amp; clips with lossless time slicing.</strong></p>
</div>

---

## Features

- **Integrated Kick VOD M3U8 Extractor**:
  - Automatically resolves Kick VOD URLs (`https://kick.com/{channel}/videos/{uuid}`) into their underlying AWS IVS Master HLS playlist (`master.m3u8`).
  - Supports both legacy `v1/video/{uuid}` VODs and modern **UUIDv7** `v2/channels/{channel}/videos` VODs via 48-bit hex timestamp matching.
  - Reconstructs subscriber-only / hidden source streams from thumbnail session/segment IDs and scans known CDN prefixes (`prefixes.json`) automatically.
- **Multi-Threaded HLS Engine (`yt-dlp` + `ffmpeg`)**:
  - Downloads HLS segments concurrently (`-N 8`) with custom browser headers (`Origin`, `Referer`, `User-Agent`) to prevent HTTP 403 blocks.
- **Custom Time-Range Slicing**:
  - Download specific sections of any broadcast without downloading the entire VOD using **Start Time (`-ss`)** and **End Time (`-to`)** (`HH:MM:SS`).
- **Resolution & Container Selection**:
  - Choose from `1080p60 (Source)`, `1080p`, `720p60`, `720p`, `480p`, `360p`, or `Audio Only`.
  - Remux directly into `.MKV`, `.MP4`, or `.TS` containers.
- **Safe Queue Management & Process Cancellation**:
  - Requires choosing a target output folder before starting downloads.
  - Includes a **Cancel** button on active downloads that cleanly terminates the underlying `yt-dlp` and `ffmpeg` process tree.
- **Live Terminal Output & UI Scaling**:
  - Real-time stdout/stderr stream inspection in the **Terminal** tab.
  - Built-in top-bar **Zoom** controls (`100%` to `150%`).

---

## Prerequisites

1. **Rust Toolchain** ([rustup.rs](https://rustup.rs/)) — stable `cargo` and `rustc`.
2. **[yt-dlp](https://github.com/yt-dlp/yt-dlp)** — installed and available in your system `PATH`.
3. **[FFmpeg](https://ffmpeg.org/download.html)** — installed and available in your system `PATH`.

### Quick Install on Windows (via Winget)

```powershell
winget install Rustlang.Rustup
winget install yt-dlp.yt-dlp
winget install Gyan.FFmpeg
```

---

## Building & Running

Clone the repository and build the optimized native release binary with Cargo:

```bash
# Run directly in release mode
cargo run --release

# Or build the standalone executable
cargo build --release
```

On Windows, `build.rs` automatically generates and embeds the custom `#00e701` pixel-art `5` application icon into the compiled `.exe` binary at `target/release/five_kd.exe`.

---

## Usage

1. **Select an Output Folder**: Click **Choose Folder** in the `Output Folder (-o)` bar to pick where your downloaded VODs will be saved.
2. **Configure Options (Optional)**:
   - Enter **Start Time (`-ss`)** and/or **End Time (`-to`)** in `HH:MM:SS` format to clip a specific segment.
   - Select your desired **Resolution** (`1080p60 (Source)` by default) and **Container Extension** (`.MKV`, `.MP4`, or `.TS`).
3. **Paste URL & Download**:
   - Paste any Kick VOD URL (e.g., `https://kick.com/channel/videos/uuid`) into the top input bar and click **Download**.
   - Monitor progress in the **Downloads** queue or inspect raw `yt-dlp` / `ffmpeg` logs in the **Terminal** tab.
   - Click **Cancel** at any time to abort an ongoing download.

---

## Project Structure

```text
├── Cargo.toml      # Rust crate manifest & dependencies (eframe, egui, reqwest, rfd, chrono, serde_json)
├── build.rs        # Windows resource script embedding the custom 5KD icon into the executable
├── src/
│   └── main.rs     # Native Rust GUI, Kick VOD M3U8 extractor, and yt-dlp/ffmpeg worker engine
└── public/
    └── assets/     # Application and navigation icons
```

---

## Credits & Acknowledgments

- **M3U8 URL Extractor**: Adapted from [**kickvod-extractor**](https://github.com/mortyobnoxious/kickvod-extractor) by [@mortyobnoxious](https://github.com/mortyobnoxious).
- **Stream Downloading & Muxing**: Powered by [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [FFmpeg](https://ffmpeg.org/).
- **GUI Framework**: Built with [egui / eframe](https://github.com/emilk/egui).
