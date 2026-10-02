//! Reading single files out of the bundle system, from whichever source holds them.

use super::bundle::Bundle;
use super::cdn::CdnBundleLoader;
use super::index::{BundleInfo, FileInfo, Index, GGPK_LOOSE_FILE_SENTINEL};
use super::steam::{SteamBundleLoader, LOOSE_FILE_SENTINEL};
use crate::ggpk::reader::GgpkReader;

pub fn find_file_info_by_path<'a>(index: &'a Index, path: &str) -> Option<&'a FileInfo> {
    index.files.values().find(|f| f.path.eq_ignore_ascii_case(path))
}

/// Synchronously extracts one file from the bundle system without going through the cache.
pub fn extract_bundle_file_sync(
    file_info: &FileInfo,
    index: &Index,
    reader: Option<&GgpkReader>,
    steam_loader: Option<&SteamBundleLoader>,
) -> Option<Vec<u8>> {
    if file_info.bundle_index == GGPK_LOOSE_FILE_SENTINEL {
        return read_ggpk_record(reader?, &file_info.path);
    }
    let data = decompress_bundle(file_info.bundle_index, index, reader, steam_loader)?;
    slice_file(&data, file_info).map(<[u8]>::to_vec)
}

/// Compressed bytes of one bundle, from the GGPK if it has them, else the Steam install.
pub fn fetch_bundle_raw(
    bundle_info: &BundleInfo,
    reader: Option<&GgpkReader>,
    steam_loader: Option<&SteamBundleLoader>,
) -> Option<Vec<u8>> {
    let from_ggpk = reader.and_then(|reader| {
        let candidates = [
            format!("Bundles2/{}", bundle_info.name),
            format!("Bundles2/{}.bundle.bin", bundle_info.name),
            bundle_info.name.clone(),
            format!("{}.bundle.bin", bundle_info.name),
        ];
        candidates.iter().find_map(|c| read_ggpk_record(reader, c))
    });
    from_ggpk.or_else(|| steam_loader.and_then(|s| s.fetch_bundle(&bundle_info.name).ok()))
}

/// Decompresses a whole bundle (the raw payload for `bundle_index`).
pub fn decompress_bundle(
    bundle_index: u32,
    index: &Index,
    reader: Option<&GgpkReader>,
    steam_loader: Option<&SteamBundleLoader>,
) -> Option<Vec<u8>> {
    let bundle_info = index.bundles.get(bundle_index as usize)?;
    decompress(fetch_bundle_raw(bundle_info, reader, steam_loader)?)
}

/// The viewer's read of a file: the GGPK or Steam install first, then the
/// patch CDN, and last the GGPK's own copy of the file outside any bundle.
/// Blocks on the network when it reaches the CDN, so call it off the UI thread.
pub fn read_file(
    file_info: &FileInfo,
    index: &Index,
    reader: Option<&GgpkReader>,
    steam_loader: Option<&SteamBundleLoader>,
    cdn_loader: Option<&CdnBundleLoader>,
) -> Result<Vec<u8>, String> {
    if file_info.bundle_index == LOOSE_FILE_SENTINEL {
        let steam = steam_loader.ok_or_else(|| format!("No Steam install to read {} from", file_info.path))?;
        let path = steam
            .loose_file_path(&file_info.path)
            .ok_or_else(|| format!("Loose file not found on disk: {}", file_info.path))?;
        return std::fs::read(&path).map_err(|e| format!("Failed to read loose file: {}", e));
    }
    if file_info.bundle_index == GGPK_LOOSE_FILE_SENTINEL {
        return reader
            .and_then(|r| read_ggpk_record(r, &file_info.path))
            .ok_or_else(|| format!("Failed to read loose GGPK file: {}", file_info.path));
    }
    let bundle_info = index
        .bundles
        .get(file_info.bundle_index as usize)
        .ok_or_else(|| format!("No bundle {} in the index for {}", file_info.bundle_index, file_info.path))?;

    let mut cdn_error = None;
    let raw = fetch_bundle_raw(bundle_info, reader, steam_loader).or_else(|| match cdn_loader {
        Some(cdn) => {
            println!("Bundle missing from GGPK. Attempting CDN fetch for: {}", bundle_info.name);
            cdn.fetch_bundle(&bundle_info.name)
                .map_err(|e| cdn_error = Some(format!("CDN fetch failed: {}", e)))
                .ok()
        }
        None => {
            cdn_error = Some("bundle not in the install and no CDN loader".to_string());
            None
        }
    });

    if let Some(data) = raw.and_then(decompress) {
        return slice_file(&data, file_info)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| format!("Decompressed bounds check failed for '{}'", file_info.path));
    }

    // Some GGPKs still carry the file itself outside its bundle.
    if let Some(data) = reader.and_then(|r| read_ggpk_record(r, &file_info.path)) {
        let size = file_info.file_size as usize;
        return match data.get(..size) {
            Some(file) => Ok(file.to_vec()),
            None => Err(format!("Decompressed bounds check failed for '{}'", file_info.path)),
        };
    }

    let mut msg = format!("Failed to load or decompress data for '{}' (bundle not found and GGPK lookup failed)", file_info.path);
    if let Some(e) = cdn_error {
        msg = format!("{}: {}", msg, e);
    }
    Err(msg)
}

/// DAT-stored texture paths under `Art/2DArt/UIImages/...` (group
/// backgrounds, node frames) are missing a `Textures/Interface/2D/` segment
/// that the actual bundle path has — confirmed against the real index:
/// `Art/2DArt/UIImages/InGame/PassiveSkillScreenGroupBackgroundSmall` in the
/// DAT resolves to
/// `Art/Textures/Interface/2D/2DArt/UIImages/InGame/PassiveSkillScreenGroupBackgroundSmall.dds`
/// on disk. Icon (`SkillIcons`) and connector (`PassiveTree`) paths don't
/// need this — only try it for the `UIImages` case.
pub fn dds_path_candidates(path: &str) -> Vec<String> {
    let mut candidates = vec![path.to_string(), format!("{}.dds", path)];
    let lower = path.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("art/2dart/uiimages") {
        let suffix = &path[path.len() - rest.len()..];
        let corrected = format!("Art/Textures/Interface/2D/2DArt/UIImages{}", suffix);
        candidates.push(format!("{}.dds", corrected));
        candidates.push(corrected);
    }
    candidates
}

/// Index entry for a DAT-style texture path (with or without `.dds`, with or
/// without the `Textures/Interface/2D` segment).
pub fn resolve_texture_path<'a>(index: &'a Index, path: &str) -> Option<&'a FileInfo> {
    dds_path_candidates(path)
        .iter()
        .find_map(|candidate| find_file_info_by_path(index, candidate))
}

fn read_ggpk_record(reader: &GgpkReader, path: &str) -> Option<Vec<u8>> {
    let rec = reader.read_file_by_path(path).ok().flatten()?;
    reader.get_data_slice(rec.data_offset, rec.data_length).ok().map(|d| d.to_vec())
}

fn decompress(raw: Vec<u8>) -> Option<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(raw);
    let header = Bundle::read_header(&mut cursor).ok()?;
    header.decompress(&mut cursor).ok()
}

fn slice_file<'a>(bundle: &'a [u8], file_info: &FileInfo) -> Option<&'a [u8]> {
    let start = file_info.file_offset as usize;
    bundle.get(start..start + file_info.file_size as usize)
}
