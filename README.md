# GGPK Explorer

A high-performance GGPK and PoE 2 Bundle explorer written in Rust.

<img width="1920" height="1079" alt="ggpk-explorer_Ti8U2Nh2kG" src="https://github.com/user-attachments/assets/112c14e5-6425-4a81-8293-afdb1fb45d15" />

## Features

### Core Explorer
- **Hybrid Support**: Seamlessly browse PoE 1 GGPK files and PoE 2 Bundle formats.
- **CDN Fallback**: Automatically fetches missing bundles from the official CDN when not found locally.
- **Advanced Search**:
    - Fast, background-threaded search.
    - Category filtering (Texture, Audio, Text, Data, etc.).
    - Smart result expansion with "Load More" for large datasets.

### specialized Viewers
- **DAT Viewer**:
    - Full schema support for PoE 1 & 2.
    - Cross-referencing and foreign key resolution.
    - JSON export.
- **Code & Text**:
    - Syntax highlighting for Shaders (`.hlsl`, `.fx`, `.vshader`, `.pshader`).
    - Auto-detection for standard text formats (`.txt`, `.xml`, `.ini`, `.csv`).
- **Media**:
    - **Textures**: DDS support with Zoom/Pan controls.
    - **Audio**: Built-in OGG player with volume control.
- **Data Formats**:
    - **CSD**: Specialized viewer for Client String Data with language filtering and JSON export.
    - **PSG**: Tree-view visualization for PSG files.
    - **JSON**: Interactive, collapsible tree viewer.
    - **Hex Viewer**: Responsive, adaptive layout for raw binary inspection.

### UI & UX
- **Multilingual Support**: Built-in font fallback for CJK (Chinese, Japanese, Korean) and Thai characters.
- **Export Tools**: Right-click to export individual files or entire folders to disk.
- **Theme**: Dark, VSCode-like aesthetic.

## Building and Running

This project uses Oodle for decompression, which requires the `ooz` library.

1. Clone the repository with submodules:
   ```bash
   git clone --recursive https://github.com/juddisjudd/ggpk-explorer.git
   ```
   Or if already cloned:
   ```bash
   git submodule update --init --recursive
   ```
2. Build and Run:
   ```bash
   cargo run --release
   ```

## Credits

This project utilizes logic and resources from the community:

- **[ooz](https://github.com/zao/ooz)**: For Oodle decompression support.
- **[dat-schema](https://github.com/poe-tool-dev/dat-schema)**: Source for community-maintained DAT schemas.
- **[poe-dat-viewer](https://github.com/SnosMe/poe-dat-viewer)**: Inspiration for DAT file structure and viewing logic.
- **[LibGGPK3](https://github.com/aianlinb/LibGGPK3)**: Reference for GGPK file format handling.

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/P5P57KRR9)
