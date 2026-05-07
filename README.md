# GGPK Explorer

A high-performance Path of Exile asset explorer for both the standalone (GGPK) and Steam (Bundles2) installs, written in Rust.

[![GitHub Downloads (all assets, latest release)](https://img.shields.io/github/downloads/juddisjudd/ggpk-explorer/latest/total)](https://github.com/juddisjudd/ggpk-explorer/releases) [![GitHub Release](https://img.shields.io/github/v/release/juddisjudd/ggpk-explorer)](https://github.com/juddisjudd/ggpk-explorer/releases) [![Release](https://github.com/juddisjudd/ggpk-explorer/actions/workflows/release.yml/badge.svg)](https://github.com/juddisjudd/ggpk-explorer/actions/workflows/release.yml)

<img width="1280" height="776" alt="image" src="https://github.com/user-attachments/assets/dc94a152-393d-420b-ab9e-9ed455c8a87e" />

## Features

### Data Sources
- **Standalone (GGPK)**: Open `content.ggpk` from the GGG standalone launcher install.
- **Steam**: Point directly at the `Bundles2/` directory from your Steam install — no GGPK required. Loose files (e.g. `Art/Videos/`) are discovered and merged automatically.
- **CDN Fallback**: Bundles not found locally are fetched automatically from the official CDN.
- **Session Memory**: The last-used data source (GGPK path or Steam directory) is remembered and reopened on launch.

### File Tree & Search
- Hierarchical tree view of the full bundle/GGPK structure.
- **Command Palette**: Keyboard-driven search across all file paths.
- Category filtering (Texture, Audio, Text, Data, Video, etc.).
- Fast background-threaded search with "Load More" for large result sets.

### Viewers
- **DAT / DAT64**: Full schema support for PoE 1 & 2, cross-referencing, foreign key resolution, JSON export.
- **Textures**: DDS (all BC/DXT variants), PNG, JPG, WebP — with zoom, pan, and fit-to-window controls.
- **Audio**: Built-in OGG/WAV/MP3 player with volume control.
- **Video (BK2)**: Header metadata display (codec, resolution, FPS, duration, audio tracks). Playback via RAD Video Tools `binkplay.exe`, `ffplay`, or your system default.
- **CSD**: Client String Data viewer with language filtering and JSON export.
- **PSG**: Particle/graph file viewer with tree visualization.
- **JSON**: Interactive, collapsible tree viewer.
- **Shaders**: Syntax-highlighted view for `.hlsl`, `.fx`, `.vshader`, `.pshader`.
- **Text / Config**: Auto-detected viewer for `.txt`, `.xml`, `.ini`, `.csv`, and dozens of PoE-specific text formats, with UTF-16 BOM support.
- **Hex Viewer**: Adaptive layout for raw binary inspection of any file.

### Export
- Right-click any file or folder in the tree to export.
- Exports individual files or entire directory trees to disk.
- Progress tracking with per-file status for large folder exports.

### UI
- Collapsible sidebar, resizable panels.
- Dark, VSCode-like theme.
- Multilingual font fallback for CJK (Chinese, Japanese, Korean) and Thai characters.
- Settings window: configure data source paths, schema updates, CDN patch version, and cache management.

## Requirements

### Playback (optional)
`.bk2` video playback requires an external player. The app checks in this order:

| Platform | Players tried |
|----------|--------------|
| Windows  | RAD Video Tools `binkplay.exe` (`Program Files\RADVideo\` etc.) → game-dir `binkplay.exe` → `ffplay` → system default |
| Linux / macOS | `ffplay` → `mpv` → `vlc` → system default (`xdg-open` / `open`) |

[RAD Video Tools](https://www.radgametools.com/bnkdown.htm) — free download from RAD Game Tools.  
[FFmpeg](https://ffmpeg.org/download.html) — includes `ffplay`, free and open source.

## Building

This project uses Oodle for decompression via the `ooz` native library.

1. Clone with submodules:
   ```bash
   git clone --recursive https://github.com/juddisjudd/ggpk-explorer.git
   ```
   Or if already cloned:
   ```bash
   git submodule update --init --recursive
   ```
2. Build and run:
   ```bash
   cargo run --release
   ```

## Credits

- **[ooz](https://github.com/zao/ooz)** — Oodle decompression.
- **[dat-schema](https://github.com/poe-tool-dev/dat-schema)** — Community-maintained DAT schemas.
- **[poe-dat-viewer](https://github.com/SnosMe/poe-dat-viewer)** — DAT file structure reference.
- **[LibGGPK3](https://github.com/aianlinb/LibGGPK3)** — GGPK format reference.

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/P5P57KRR9)
