# mDm — Media Download Manager

<p align="center">
  <img src="mdm-app/src-tauri/icons/128x128.png" width="96" height="96" alt="mDm Logo">
</p>

<p align="center">
  <b>A fast, unified dual-engine media download manager for Linux & Windows.</b><br>
  Built with <b>Rust</b>, <b>Tauri v2</b>, <b>yt-dlp</b> (1,000+ streaming sites), and <b>aria2</b> (high-throughput multi-connection HTTP, BitTorrent & Magnet).
</p>

<p align="center">
  <a href="https://github.com/habitual69/mdm/releases"><img src="https://img.shields.io/github/v/release/habitual69/mdm?color=brightgreen&style=flat-square" alt="Latest Release"></a>
  <a href="https://github.com/habitual69/mdm/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/habitual69/mdm/ci.yml?branch=master&label=CI&style=flat-square" alt="CI Status"></a>
  <img src="https://img.shields.io/badge/Platform-Linux%20%7C%20Windows-blue?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/Rust-1.80%2B-orange?style=flat-square&logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/Tauri-v2-24C8DB?style=flat-square&logo=tauri" alt="Tauri">
  <img src="https://img.shields.io/badge/Engines-yt--dlp%20%7C%20aria2%20%7C%20ffmpeg-success?style=flat-square" alt="Engines">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-green?style=flat-square" alt="License"></a>
</p>

---

## Overview

**mDm** combines the extraction power of **yt-dlp** with the raw download throughput of **aria2** into a single, cohesive desktop experience and headless daemon. Whether you are downloading a single 4K video, grabbing an entire 200-song playlist with embedded album artwork, downloading large ISOs over 16 parallel connections, or managing torrents and magnet links, mDm handles it all seamlessly.

mDm operates as a standalone native desktop app (via Tauri v2), a headless background daemon with an embedded Web UI and REST API, and integrates directly with your web browser via a companion Chromium extension.

---

## Screenshots

<div align="center">

### Home & Instant Link Addition
*Paste any URL to automatically detect format, stream quality, playlists, or direct file downloads.*
<br><br>
<img src="docs/screenshots/home.png" width="92%" alt="mDm Home View">

<br><br>

### Active Downloads & Real-Time Speed Activity
*Live bandwidth sparklines, multi-engine progress tracking, pause/resume, and category classification.*
<br><br>
<img src="docs/screenshots/downloads.png" width="92%" alt="mDm Downloads View">

<br><br>

### Settings & Customization
*Tailor appearance themes, accent colors, floating widget shapes, aria2 split counts, and media formats.*
<br><br>
<img src="docs/screenshots/settings.png" width="92%" alt="mDm Settings View">

<br><br>

### Desktop Floating Speed Widget
*Frameless, always-on-top, draggable desktop pill with live aggregate download/upload speeds.*
<br><br>
<img src="docs/screenshots/widget.png" alt="mDm Desktop Floating Speed Widget">

</div>

---

## Key Features

- **Unified Dual-Engine Download Core**:
  - **yt-dlp**: Download video & audio from over 1,000 websites (YouTube, Instagram, TikTok, Twitter/X, Twitch, SoundCloud, Vimeo, Reddit, Bilibili, etc.).
  - **aria2**: High-throughput multi-connection engine for direct files (HTTP/HTTPS/FTP), BitTorrent (`.torrent`), and Magnet URIs.
  - **Smart Acceleration & Fallback**: Offload stream chunks from yt-dlp to aria2 for up to 16 parallel connections, with automatic fallback to native yt-dlp downloader on network retry.

- **Desktop Floating Speed Widgets**:
  - Frameless, always-on-top, draggable mini-widgets displaying live download/upload speeds and progress.
  - Three switchable styles:
    - **Pill**: Ultra-sleek capsule (`[ ● ↓ 0 B/s ↑ 0 B/s | ———— 0% ✕ ]`).
    - **Card**: Compact dashboard card with title, speed graph, and active count.
    - **Minimalistic**: Clean, distraction-free speed badge.
  - Double-click to restore main window; zero compositor shadow/ghosting artifacts.

- **Companion Browser Extension (Chromium)**:
  - Manifest V3 extension for Google Chrome, Microsoft Edge, Brave, and Chromium browsers.
  - **IDM-Style Floating Video Button**: Automatically detects playable video elements on web pages and displays a 1-click download bar.
  - Right-click context menu integration to quickly download links, images, or media through mDm.

- **Interactive Playlist & Batch Inspector**:
  - Automatically recognizes media playlists when pasting a URL.
  - Fetches and displays all items with titles, authors, durations, and thumbnails.
  - Flexible controls: select all, custom range checkboxes, title search filter, and format selection (Video or Audio).

- **Rich Audio Extraction & Album Artwork**:
  - Extracts audio to high-bitrate **MP3**, **FLAC**, **M4A**, **Opus**, or **WAV**.
  - Automatically embeds universal JPEG front cover artwork and ID3v2 tags (`Title`, `Artist`, `Album`, `Track Number`) compatible with all media players and file managers (GNOME Files, Windows Explorer, macOS Finder).

- **Zero-Config Self-Healing Engine Setup**:
  - Built-in installer (`mdm setup` and runtime daemon auto-init) downloads required dependencies (`yt-dlp`, `aria2c`, `ffmpeg`, `ffprobe`) into user space with SHA-256 validation. No root or administrative privileges required.

- **Smart Category Sorting**:
  - Automatically routes completed files into organized subfolders:
    - `Videos/` (`.mp4`, `.mkv`, `.webm`, `.avi`, etc.)
    - `Audios/` (`.mp3`, `.flac`, `.m4a`, `.opus`, etc.)
    - `Compress/` (`.zip`, `.rar`, `.7z`, `.tar`, `.gz`, etc.)
    - `Document/` (`.pdf`, `.doc`, `.docx`, `.xls`, `.md`, `.html`, etc.)
    - `Torrents/` (`.torrent`)
    - `Miscellaneous/` (other file formats)

- **System Tray & Live Speed Indicator**:
  - Native system tray icon on Ubuntu, Windows, and macOS.
  - Live upload and download speeds dynamically updated in the tray title and tooltip.

- **Embedded Web UI & REST API**:
  - Embedded web UI accessible on `http://127.0.0.1:5757` from any local browser or device on your network.
  - Complete REST API and real-time WebSocket event stream (`/api/events`) for automation and scripting.

---

## Platform Support

| Platform | Architecture | Supported Formats | Requirements | Notes |
| :--- | :---: | :--- | :--- | :--- |
| **Windows 10 / 11** | `x86_64` | • NSIS Installer (`.exe`)<br>• Portable Archive (`.zip`)<br>• Standalone CLI (`.exe`) | Microsoft Edge WebView2 (bundled bootstrapper fallback included) | Full system tray, floating speed widget, Start menu shortcuts, uninstaller |
| **Ubuntu / Debian / Mint / Pop!_OS** | `x86_64` | • Debian Package (`.deb`)<br>• Universal AppImage (`.AppImage`)<br>• Standalone CLI (`.tar.gz`) | WebKitGTK 4.1 (`libwebkit2gtk-4.1-dev`) | Native AppIndicator tray integration, desktop entry, auto-mime routing |
| **Fedora / RHEL / Arch / Generic Linux** | `x86_64` | • Universal AppImage (`.AppImage`)<br>• Standalone Binaries (`.tar.gz`) | Standard glibc, FUSE (for AppImage) | Runs standalone on any modern Linux desktop environment (GNOME, KDE, XFCE) |
| **Browser Extension** | Cross-platform | • Chromium Manifest V3 (`.zip`) | Chrome, Edge, Brave, Vivaldi, Opera | Unpacked developer mode loading with 1-click page media interception |
| **Web / Headless Daemon** | Any | • Web Dashboard (`http://localhost:5757`) | Any modern browser | Accessible locally or over LAN for remote server setups |

---

## Installation & Downloads

Download the latest release for your platform from the **[GitHub Releases](https://github.com/habitual69/mdm/releases)** page.

### Windows

#### Option 1: Self-Contained Installer (Recommended)
1. Download `mDm_Setup_<version>_x64.exe`.
2. Run the installer. It will:
   - Install the application into `%LOCALAPPDATA%\Programs\mDm Media Download Manager`.
   - Bundle all required engines (`yt-dlp`, `aria2c`, `ffmpeg`, `ffprobe`).
   - Create Start Menu and Desktop shortcuts.
   - Verify or silently install the Edge WebView2 runtime if missing.
   - Provide an optional uninstaller in Windows Add/Remove Programs.

#### Option 2: Portable ZIP
1. Download `mDm_Portable_<version>_x64.zip`.
2. Extract the archive to any folder.
3. Launch `mDm Media Download Manager.exe`.

#### Option 3: Standalone CLI
1. Download `mdm-cli-<version>-windows-x64.zip`.
2. Extract `mdm.exe` and add it to your system `PATH`.

---

### Linux

#### Option 1: Debian / Ubuntu Package (.deb)
```bash
# Download and install the .deb package
sudo dpkg -i "mDm Media Download Manager_<version>_amd64.deb"

# Or install with apt to automatically resolve any system dependencies
sudo apt-get install ./"mDm Media Download Manager_<version>_amd64.deb"
```

#### Option 2: Universal AppImage
```bash
# Make the AppImage executable and launch
chmod +x "mDm Media Download Manager_<version>_amd64.AppImage"
./"mDm Media Download Manager_<version>_amd64.AppImage"
```

#### Option 3: Standalone Tarball (CLI / Desktop)
```bash
# Extract CLI binary into your local bin path
tar -xzf "mdm-<version>-linux-x86_64.tar.gz"
mkdir -p ~/.local/bin
mv mdm ~/.local/bin/

# Ensure dependencies are downloaded
mdm setup
```

---

### Browser Extension (Chrome / Edge / Brave)

1. Download `mdm-browser-extension.zip` from [Releases](https://github.com/habitual69/mdm/releases) (or find it in your installed mDm folder under `extension/`).
2. Extract the ZIP into a local folder.
3. In Chrome/Brave/Edge, navigate to `chrome://extensions/` (or `edge://extensions/`).
4. Enable **Developer mode** (toggle in the top-right corner).
5. Click **Load unpacked** and select the extracted `extension` directory.
6. The mDm icon will appear in your browser toolbar, and video links will automatically display the floating download bar.

---

## Quick Start (CLI)

The `mdm` CLI can be run alongside the desktop app or as a standalone headless service:

```bash
# Setup and verify all dependencies (yt-dlp, aria2c, ffmpeg)
mdm setup

# Force reinstall or update a specific engine
mdm setup --force --only yt-dlp

# Download a video from YouTube, Twitter/X, TikTok, or 1000+ sites
mdm add "https://www.youtube.com/watch?v=dQw4w9WgXcQ"

# Download in audio-only mode (extracts to MP3 with album artwork)
mdm add "https://www.youtube.com/watch?v=dQw4w9WgXcQ" -k audio

# Download a direct file or ISO over multi-connection aria2
mdm add "https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso"

# Download a BitTorrent file or Magnet link
mdm add "magnet:?xt=urn:btih:..."

# List queue status and live progress (interactive watch mode)
mdm ls --watch

# Start the headless daemon manually
mdm daemon
```

### CLI Command Reference

| Command | Arguments / Flags | Description |
| :--- | :--- | :--- |
| `mdm setup` | `[--force] [--only <engine>]` | Verify and download required binaries into user space (`~/.local/share/mdm/bin` or `%APPDATA%\mdm\bin`). |
| `mdm add <url...>` | `[-f, --format <fmt>] [-k, --kind <kind>] [-o, --out <dir>]` | Queue one or more URLs for download. Auto-detects video vs direct vs torrent. |
| `mdm ls` | `[--watch]` | Display active and queued jobs. Use `--watch` for real-time console updates. |
| `mdm daemon` | — | Run the HTTP REST and WebSocket daemon server on `127.0.0.1:5757` in foreground. |
| `mdm --help` | — | Show help information and options. |

---

## Configuration

Configuration is stored in:
- **Linux**: `~/.local/share/mdm/config.toml` (or `~/.config/mdm/config.toml`)
- **Windows**: `%APPDATA%\mdm\config.toml`

Settings can be edited directly or modified on-the-fly via the Settings tab in the UI or via REST API:

```toml
# Storage & Queue
output_dir = "~/Downloads/mdm"        # Default download directory
max_concurrent = 4                  # Maximum concurrent downloads
max_retries = 3                     # Retry attempts on network errors
speed_limit = ""                    # Global bandwidth cap (e.g. "5M", "500K")
proxy = ""                          # HTTP / SOCKS5 proxy URL

# Appearance & Desktop Widgets
theme = "dark"                      # "dark" | "light" | "system"
accent_color = "#e8934a"            # Primary accent color (Hex)
widget_shape = "pill"               # "pill" | "card" | "minimalistic"

# aria2 Multi-Connection Tuning
aria2_connections = 8               # Connections per server (1-16)
aria2_splits = 8                    # Split file into N chunks
aria2_offload = true                # Offload yt-dlp stream chunks to aria2
aria2_fallback = true               # Fallback to yt-dlp native if aria2 chunking fails

# Media & yt-dlp Settings
ytdlp_audio_format = "mp3"          # "mp3" | "flac" | "m4a" | "opus" | "wav"
embed_metadata = true               # Embed ID3 tags & chapter markers
embed_thumbnail = true              # Embed front cover album art in audio files
```

---

## REST API & WebSocket Events

The embedded server exposes a full REST API on `http://127.0.0.1:5757`:

| Method | Endpoint | Description |
| :--- | :--- | :--- |
| `GET` | `/api/health` | Health check endpoint returning status and version |
| `GET` | `/api/jobs` | Retrieve all jobs (supports query parameter `?state=running`) |
| `POST` | `/api/jobs` | Submit a new download job (`{"url": "...", "kind": "video"}`) |
| `POST` | `/api/jobs/batch` | Batch submit multiple items (e.g., from playlist extractor) |
| `POST` | `/api/jobs/{id}/pause` | Pause an active download |
| `POST` | `/api/jobs/{id}/resume` | Resume a paused download |
| `DELETE` | `/api/jobs/{id}` | Cancel and remove a job from queue |
| `POST` | `/api/playlist-info` | Extract metadata and item list for a playlist URL |
| `GET` | `/api/config` | Fetch current runtime configuration |
| `PUT` | `/api/config` | Update configuration settings |
| `GET` | `/api/events` | Real-time WebSocket event stream (`job_created`, `job_progress`, `job_completed`) |
| `POST` | `/api/window/{action}` | Control desktop window (`minimize`, `toggle-maximize`, `close`, `toggle-floating`) |

---

## Project Structure & Architecture

```
mdm/
├── .github/workflows/     # CI/CD pipelines (Continuous Integration & Release publication)
├── crates/
│   ├── mdm-core/          # SQLite queue database, job models, configuration manager
│   ├── mdm-engine-ytdlp/  # yt-dlp driver, progress parser, playlist extractor, ID3 tagger
│   ├── mdm-engine-aria2/  # aria2 JSON-RPC daemon client, BitTorrent & multi-connection manager
│   ├── mdm-bins/          # Cross-platform dependency installer (SHA-256 verified)
│   ├── mdm-server/        # Axum HTTP REST server, WebSocket broadcaster, embedded Web UI
│   └── mdm-cli/           # Command-line interface (`mdm setup`, `mdm add`, `mdm ls`)
├── mdm-app/               # Tauri v2 native desktop shell with system tray & floating widget
│   └── src-tauri/         # Tauri backend, window controllers, platform configs
├── extension/             # Companion Chromium extension (IDM-style video detector)
├── scripts/               # Windows NSIS installer script & automated packaging pipeline
└── docs/                  # Application screenshots and documentation assets
```

---

## Building from Source

### Prerequisites

- **Rust toolchain** (1.80+ or 2024 edition): [rustup.rs](https://rustup.rs/)
- **Node.js** (v18+ or v20+): For running Tauri CLI bundler
- **Linux Packages** (Ubuntu / Debian):
  ```bash
  sudo apt-get update
  sudo apt-get install -y \
    build-essential \
    libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    patchelf \
    libssl-dev \
    libgtk-3-dev \
    squashfs-tools
  ```

### Build Instructions

```bash
# 1. Clone the repository
git clone https://github.com/habitual69/mdm.git
cd mdm

# 2. Run automated workspace tests
cargo test --workspace

# 3. Build release CLI binary
cargo build --release -p mdm-cli
# Result: target/release/mdm (or mdm.exe)

# 4. Run desktop application in development mode
cd mdm-app
npx @tauri-apps/cli dev

# 5. Build production Linux packages (.deb & .AppImage)
npx @tauri-apps/cli build --bundles deb,appimage
# Result: target/release/bundle/deb/ & target/release/bundle/appimage/
```

### Windows Packaging

To package the self-contained Windows NSIS installer and portable ZIP distribution:

```powershell
# Compile release binaries and package installer
.\scripts\package-windows.ps1

# Result:
# dist\mDm_Setup_<version>_x64.exe
# dist\mDm_Portable_<version>_x64.zip
```

---

## CI/CD Automation

mDm uses GitHub Actions for continuous integration and automated releases:

- **Continuous Integration (`ci.yml`)**: Runs on every push and pull request across `ubuntu-22.04` and `windows-latest`. Compiles all workspace crates and executes unit and integration tests.
- **Automated Releases (`release.yml`)**: Triggered whenever a version tag (`v*`) is pushed or manually via GitHub `workflow_dispatch`. Automatically compiles:
  - Linux `.deb` installer and universal `.AppImage`
  - Linux standalone CLI and desktop archives (`.tar.gz`)
  - Windows self-contained NSIS installer (`.exe`)
  - Windows portable ZIP archive (`.zip`)
  - Windows CLI archive (`.zip`)
  - Companion browser extension package (`.zip`)
  - Cryptographic checksum manifest (`SHA256SUMS.txt`)
  - GitHub Release with auto-generated release notes

---

## License

This project is licensed under the **[MIT License](LICENSE)**.

---

<p align="center">
  <b>mDm — Media Download Manager</b> • Crafted with Rust and Tauri
</p>
