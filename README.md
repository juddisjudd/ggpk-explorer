# GGPK Explorer

A high-performance GGPK and PoE 2 Bundle explorer written in Rust.

## Features

- **GGPK & Bundle Support**: Browse standard PoE 1 GGPK files and newer PoE 2 Bundle formats.
- **Dynamic Content Loading**: Access remote bundles via standard CDN or local cache.
- **Advanced DAT Viewer**:
    - View game data in a tabular format.
    - Automatic schema application for column naming.
    - Correct handling of array columns and references.
    - JSON export for selected data files.
- **Media Preview**:
    - Preview textures (DDS, converted to WebP for display).
    - Play audio files (OGG/WAV).
- **Settings & Updates**:
    - Configurable PoE 2 Patch Version with auto-detect.
    - Automatic schema updates from remote source.

## Building and Running

Ensure you have Rust installed.

```bash
cargo run --release
```

## Credits

This project utilizes logic and resources from the community:

- **[ooz](https://github.com/zao/ooz)**: For Oodle decompression support.
- **[dat-schema](https://github.com/poe-tool-dev/dat-schema)**: Source for community-maintained DAT schemas.
- **[poe-dat-viewer](https://github.com/SnosMe/poe-dat-viewer)**: Inspiration for DAT file structure and viewing logic.
- **[LibGGPK3](https://github.com/aianlinb/LibGGPK3)**: Reference for GGPK file format handling.
