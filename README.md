# GGPK Explorer

A Path of Exile asset explorer for the standalone (GGPK) and Steam (Bundles2) installs, written in Rust.

[![GitHub Downloads (all assets, latest release)](https://img.shields.io/github/downloads/juddisjudd/ggpk-explorer/latest/total)](https://github.com/juddisjudd/ggpk-explorer/releases) [![GitHub Release](https://img.shields.io/github/v/release/juddisjudd/ggpk-explorer)](https://github.com/juddisjudd/ggpk-explorer/releases) [![Release](https://github.com/juddisjudd/ggpk-explorer/actions/workflows/release.yml/badge.svg)](https://github.com/juddisjudd/ggpk-explorer/actions/workflows/release.yml)

<img width="1280" height="776" alt="image" src="https://github.com/user-attachments/assets/dc94a152-393d-420b-ab9e-9ed455c8a87e" />

## Features

### Data Sources
- **Standalone (GGPK)**: Open `content.ggpk` from the GGG standalone launcher install.
- **Steam**: Point directly at the `Bundles2/` directory from your Steam install, with no GGPK required. Loose files (e.g. `Art/Videos/`) are discovered and merged automatically.
- **CDN Fallback**: Bundles not found locally are fetched automatically from the official CDN. You can also read a patch you do not have installed.
- **Session Memory**: The last-used data source (GGPK path or Steam directory) is remembered and reopened on launch.
- **Patch-aware caches**: The index and tree caches record which patch they were built for, and any run clears them once the game updates. A patch rewrites bundle contents under unchanged names, so a cache kept across one reads back wrong data rather than merely stale data.

### File Tree & Search
- Hierarchical tree view of the full bundle/GGPK structure.
- **Command Palette**: Keyboard-driven search across all file paths.
- Category filtering (Texture, Audio, Text, Data, Video, etc.).
- Background-threaded search with "Load More" for large result sets.

### Viewers
Every file reference inside any viewer is a link that opens that file: a `.dds` in a material, an `.ao` in an object definition, an `.et` in a dungeon graph.

- **DAT / DATC64**: Schema-driven table view for PoE 1 & 2 (correct per-game table pick) with sortable columns, row filter, enum names, foreign keys resolved to the target row's `Id` (click to jump), `@file` paths that open the referenced asset, a row-detail panel, column hiding, and JSON/CSV export.
- **Schema discovery**: Tables missing from the community schema get their column types guessed from the data. When a table *is* in the schema but the game changed its layout, the schema is re-fitted onto the file automatically, so names and references carry over and new columns show by offset. Foreign-key columns without a target get likely targets suggested from row counts and column names (right-click the header to set one). **Edit columns** lets you rename, retype, reference, insert or delete columns and save the result as a custom layout (`schema_overrides.json`, in dat-schema format so it can go straight into a poe-tool-dev pull request).
- **Carrying names across a patch**: A patch that moves a table's columns makes the community schema read the wrong bytes, and nothing about that fails loudly. Click **Carry names from &lt;version&gt;** on the drift warning: the app reads that patch from the CDN, matches rows by `Id`, and moves each column's name to wherever its values went. Columns it cannot place are reported rather than guessed at, because bytes never say what a column is called.
- **Textures**: DDS (all BC/DXT variants), PNG, JPG and WebP, with zoom, pan and fit-to-window controls.
- **Audio**: Built-in OGG/WAV/MP3 player with volume control.
- **FMOD Banks**: `.bank` files (`FMOD/` folder) open with a full stream listing. Play any stream in-app, save individual streams as WAV, or export the whole bank at once.
- **Video (BK2)**: Header metadata display (codec, resolution, FPS, duration, audio tracks). Playback via RAD Video Tools `binkplay.exe`, `ffplay`, or your system default.
- **CSD (stat descriptions)**: Searchable table of every description line (stat ids, value condition, text with placeholders highlighted, value functions), language switch, `include` links, and a detail panel that renders the line for values you type in.
- **Object definitions** (`.ao`, `.ot`, `.it`, `.act`, `.epk`): Components as collapsible sections with key/value grids, the `extends` chain resolved as links, and an **Inherited** toggle that merges every ancestor's components into the view.
- **Materials** (`.mat`): The textures a material samples as thumbnails, its fxgraph instances with their parameters (curves plotted, colours as swatches), and the full document.
- **Timelines** (`.atl`): Each animation's events on a time strip and in a table, with the effect packs and sounds they trigger as links.
- **Particles & trails** (`.pet`, `.trl`): Every keyframe and sampled curve plotted per emitter or trail block.
- **Dungeon graphs** (`.dgr`): The room grid drawn with its nodes and connections; click a node for its details and room sets.
- **PSG (skill trees)**: Renders the character, atlas, Chayula and Royale skill graphs the way the game lays them out, with every asset taken from the GGPK: centre ring and class illustration, class-start plates, ascendancy plates relocated onto the outer ring (with a class/ascendancy picker that dims the others), textured orbit arcs and connectors from the game's sprite sheets, per-context node frames (character/ascendancy/atlas/Breach, plus per-node overrides), group backgrounds, atlas subtree art and blockers. Hover any node for its name, stats and flavour text.
- **JSON**: Interactive, collapsible tree viewer (`.json`, `.hideout`, `.env`, JSON-bodied `.pet`) with file links, colour swatches and inline plots for `points` curves.
- **Shaders**: Syntax-highlighted view for `.hlsl`, `.fx`, `.vshader`, `.pshader`.
- **Text / Config**: Every other PoE text format (`.tst`, `.rs`, `.mtd`, `.tsi`, `.arm`, `.sm`, `.amd`, `.ui`, …) in a filterable view with file references as links and a Raw toggle for the plain editor; UTF-16 with or without a BOM.
- **Models**: `.fmt` / `.tgm` meshes and `.smd` skinned meshes in a 3D preview (orbit, zoom, pan, wireframe, per-shape selection), `.ast` skeletons as bone lines, plus the structured summary with geometry stats and full JSON export.
- **DDS headers**: PoE 2 `.dds.header` streaming stubs render as thumbnails.
- **Hex Viewer**: Adaptive layout for raw binary inspection of any file.

### Comparing Patches
The **Diff** button records a snapshot of the current bundle index and compares any saved snapshot against the live one. It lists the files a patch added, removed, resized, and repacked. The app snapshots on its own when it notices a new patch, so you can still diff across an update you did not plan for.

### Export
- Right-click any file or folder in the tree to export it.
- Convert while exporting: textures to PNG or WebP, audio to WAV, DAT tables and stat descriptions to JSON.
- Progress tracking with per-file status for large folder exports.
- **Skill trees**: The PSG viewer's **Export tree…** button writes a tree in GGG's official web export layout: `data.json`, WebP sprite sheets, and a standalone `index.html` viewer.
- **Game data**: **File → Export Game Data…** writes RePoE-style JSON, one file per game concept rather than one per DAT table: `mods.json`, `skills.json`, `base_items.json`, `unique_details.json`, `stat_translations/`, and 19 others. A table whose layout no longer matches the schema is refused instead of exported wrong, and the run reports what it left out.

### Command Line
The same binary runs without the GUI:

| Command | What it does |
|---------|--------------|
| `ggpk-explorer inspect` | Print GGPK and bundle index diagnostics. |
| `ggpk-explorer export Art/2DArt -o out --textures png` | Extract a file or folder, converting as it goes. |
| `ggpk-explorer export-data -o data` | Write the RePoE-style JSON dumps. |
| `ggpk-explorer refit --old 4.5.4.11 --write` | Rebuild the table layouts a patch broke, using the patch before it. |
| `ggpk-explorer lint` | Check the schema's foreign keys and enum indices against the game files. |

`lint --schema <file>` checks a candidate schema, which is the way to test a dat-schema change before proposing it. `export-data --ls <prefix>` lists indexed paths and `--cat <path>` prints one file, which is the quickest way to check a format by hand. Pass `--help` to any subcommand for its full options.

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

[RAD Video Tools](https://www.radgametools.com/bnkdown.htm), a free download from RAD Game Tools.  
[FFmpeg](https://ffmpeg.org/download.html), which includes `ffplay`, free and open source.

## Troubleshooting

### Where are the logs?

| Log | Location | Contents |
|-----|----------|----------|
| `crash.log` | Windows: `%APPDATA%\ggpk-explorer\crash.log`<br>Linux: the directory the app was launched from | Appended on every crash (panic) with version, location, message, and a backtrace. Attach this when reporting a crash. |
| `export_errors.log` | The destination folder you exported to | Written incrementally during an export: one line per file that failed, plus a summary. |

Release builds hide the console window, so errors never reach a terminal. The files above are the only place they land. If the app dies with no new `crash.log` entry, it was likely killed by the OS (e.g. out of memory); please report what you were doing along with your `crash.log` anyway.

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

- **[ooz](https://github.com/zao/ooz)**: Oodle decompression.
- **[dat-schema](https://github.com/poe-tool-dev/dat-schema)**: Community-maintained DAT schemas.
- **[poe-dat-viewer](https://github.com/SnosMe/poe-dat-viewer)**: DAT file structure reference.
- **[LibGGPK3](https://github.com/aianlinb/LibGGPK3)**: GGPK format reference.

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/P5P57KRR9)
