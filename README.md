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
Every file reference inside any viewer — a `.dds` in a material, an `.ao` in an object definition, an `.et` in a dungeon graph — is a link that opens that file.

- **DAT / DATC64**: Schema-driven table view for PoE 1 & 2 (correct per-game table pick) with sortable columns, row filter, enum names, foreign keys resolved to the target row's `Id` (click to jump), `@file` paths that open the referenced asset, a row-detail panel, column hiding, and JSON/CSV export.
- **Schema discovery**: Tables missing from the community schema get their column types guessed from the data. When a table *is* in the schema but the game changed its layout, the schema is re-fitted onto the file automatically — names and references carry over, new columns show by offset. Foreign-key columns without a target get likely targets suggested from row counts and column names (right-click the header to set one). **Edit columns** lets you rename, retype, reference, insert or delete columns and save the result as a custom layout (`schema_overrides.json`, in dat-schema format so it can go straight into a poe-tool-dev pull request).
- **Textures**: DDS (all BC/DXT variants), PNG, JPG, WebP — with zoom, pan, and fit-to-window controls.
- **Audio**: Built-in OGG/WAV/MP3 player with volume control.
- **FMOD Banks**: `.bank` files (`FMOD/` folder) open with a full stream listing — play any stream in-app, save individual streams as WAV, or export the whole bank at once.
- **Video (BK2)**: Header metadata display (codec, resolution, FPS, duration, audio tracks). Playback via RAD Video Tools `binkplay.exe`, `ffplay`, or your system default.
- **CSD (stat descriptions)**: Searchable table of every description line (stat ids, value condition, text with placeholders highlighted, value functions), language switch, `include` links, and a detail panel that renders the line for values you type in.
- **Object definitions** (`.ao`, `.ot`, `.it`, `.act`, `.epk`): Components as collapsible sections with key/value grids, the `extends` chain resolved as links, and an **Inherited** toggle that merges every ancestor's components into the view.
- **Materials** (`.mat`): The textures a material samples as thumbnails, its fxgraph instances with their parameters (curves plotted, colours as swatches), and the full document.
- **Timelines** (`.atl`): Each animation's events on a time strip and in a table, with the effect packs and sounds they trigger as links.
- **Particles & trails** (`.pet`, `.trl`): Every keyframe and sampled curve plotted per emitter or trail block.
- **Dungeon graphs** (`.dgr`): The room grid drawn with its nodes and connections; click a node for its details and room sets.
- **PSG (skill trees)**: Renders the character, atlas, Chayula and Royale skill graphs the way the game lays them out — every asset comes from the GGPK: centre ring and class illustration, class-start plates, ascendancy plates relocated onto the outer ring (with a class/ascendancy picker that dims the others), textured orbit arcs and connectors from the game's sprite sheets, per-context node frames (character/ascendancy/atlas/Breach, plus per-node overrides), group backgrounds, atlas subtree art and blockers. Hover any node for its name, stats and flavour text.
- **JSON**: Interactive, collapsible tree viewer (`.json`, `.hideout`, `.env`, JSON-bodied `.pet`) with file links, colour swatches and inline plots for `points` curves.
- **Shaders**: Syntax-highlighted view for `.hlsl`, `.fx`, `.vshader`, `.pshader`.
- **Text / Config**: Every other PoE text format (`.tst`, `.rs`, `.mtd`, `.tsi`, `.arm`, `.sm`, `.amd`, `.ui`, …) in a filterable view with file references as links and a Raw toggle for the plain editor; UTF-16 with or without a BOM.
- **Models**: `.fmt` / `.tgm` meshes and `.smd` skinned meshes in a 3D preview (orbit, zoom, pan, wireframe, per-shape selection), `.ast` skeletons as bone lines, plus the structured summary with geometry stats and full JSON export.
- **DDS headers**: PoE 2 `.dds.header` streaming stubs render as thumbnails.
- **Hex Viewer**: Adaptive layout for raw binary inspection of any file.

### Export
- Right-click any file or folder in the tree to export.
- Exports individual files or entire directory trees to disk.
- Progress tracking with per-file status for large folder exports.

### UI
- Collapsible sidebar, resizable panels.
- Dark, VSCode-like theme.
- Multilingual font fallback for CJK (Chinese, Japanese, Korean) and Thai characters.
- Settings window: configure data source paths, schema updates, CDN patch version, cache management, and whether the ~2.8M `shadercache*/` entries are hidden (default: hidden).

## Requirements

### Playback (optional)
`.bk2` video playback requires an external player. The app checks in this order:

| Platform | Players tried |
|----------|--------------|
| Windows  | RAD Video Tools `binkplay.exe` (`Program Files\RADVideo\` etc.) → game-dir `binkplay.exe` → `ffplay` → system default |
| Linux / macOS | `ffplay` → `mpv` → `vlc` → system default (`xdg-open` / `open`) |

[RAD Video Tools](https://www.radgametools.com/bnkdown.htm) — free download from RAD Game Tools.  
[FFmpeg](https://ffmpeg.org/download.html) — includes `ffplay`, free and open source.

## Troubleshooting

### Where are the logs?

| Log | Location | Contents |
|-----|----------|----------|
| `crash.log` | Windows: `%APPDATA%\ggpk-explorer\crash.log`<br>Linux: the directory the app was launched from | Appended on every crash (panic) with version, location, message, and a backtrace. Attach this when reporting a crash. |
| `export_errors.log` | The destination folder you exported to | Written incrementally during an export — one line per file that failed, plus a summary. |

Release builds hide the console window, so errors are never printed to a terminal — the files above are the only place they land. If the app dies with no new `crash.log` entry, it was likely killed by the OS (e.g. out of memory); please report what you were doing along with your `crash.log` anyway.

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
