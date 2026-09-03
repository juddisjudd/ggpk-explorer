//! Read-only access to a game install for the data export, over whichever of
//! GGPK, a Steam `Bundles2` folder or the patch CDN is available.

use crate::bundles::cdn::CdnBundleLoader;
use crate::bundles::index::{fnv1a64, murmur_hash64a, Index};
use crate::bundles::steam::SteamBundleLoader;
use crate::dat::relational::FileSource;
use crate::ggpk::reader::GgpkReader;
use std::cell::RefCell;
use std::sync::Arc;

/// Decompressed bundles kept around so tables sharing a bundle only pay for
/// it once. The whole `Data` folder lives in a handful of bundles.
const BUNDLE_CACHE_SIZE: usize = 6;

pub struct GameFiles {
    pub reader: Option<Arc<GgpkReader>>,
    pub index: Arc<Index>,
    pub steam: Option<SteamBundleLoader>,
    pub cdn: Option<CdnBundleLoader>,
    bundles: RefCell<Vec<(u32, Arc<Vec<u8>>)>>,
}

impl GameFiles {
    pub fn new(
        reader: Option<Arc<GgpkReader>>,
        index: Arc<Index>,
        steam: Option<SteamBundleLoader>,
        cdn: Option<CdnBundleLoader>,
    ) -> Self {
        Self { reader, index, steam, cdn, bundles: RefCell::new(Vec::new()) }
    }

    /// Index entry for a path. The index keys files by the hash of the
    /// lower-cased path, with two hash functions in circulation.
    pub fn lookup(&self, path: &str) -> Option<&crate::bundles::index::FileInfo> {
        if path.is_empty() {
            return None;
        }
        let lower = path.to_ascii_lowercase();
        [murmur_hash64a(lower.as_bytes()), fnv1a64(lower.as_bytes()), fnv1a64(path.as_bytes())]
            .iter()
            .find_map(|h| self.index.files.get(h))
            .filter(|f| f.path.eq_ignore_ascii_case(path))
    }

    pub fn exists(&self, path: &str) -> bool {
        self.lookup(path).is_some()
    }

    /// Every indexed path under `prefix` (case-insensitive), sorted.
    pub fn list_dir(&self, prefix: &str) -> Vec<String> {
        let lower = prefix.to_ascii_lowercase();
        let mut out: Vec<String> = self
            .index
            .files
            .values()
            .filter(|f| f.path.to_ascii_lowercase().starts_with(&lower))
            .map(|f| f.path.clone())
            .collect();
        out.sort();
        out
    }

    fn bundle(&self, bundle_index: u32) -> Option<Arc<Vec<u8>>> {
        if let Some(hit) = self.bundles.borrow().iter().find(|(i, _)| *i == bundle_index) {
            return Some(Arc::clone(&hit.1));
        }
        let info = self.index.bundles.get(bundle_index as usize)?;
        let raw = self.raw_bundle(&info.name)?;
        let mut cursor = std::io::Cursor::new(raw);
        let header = crate::bundles::bundle::Bundle::read_header(&mut cursor).ok()?;
        let data = Arc::new(header.decompress(&mut cursor).ok()?);
        let mut cache = self.bundles.borrow_mut();
        if cache.len() >= BUNDLE_CACHE_SIZE {
            cache.remove(0);
        }
        cache.push((bundle_index, Arc::clone(&data)));
        Some(data)
    }

    fn raw_bundle(&self, name: &str) -> Option<Vec<u8>> {
        if let Some(reader) = self.reader.as_deref() {
            let hit = [format!("Bundles2/{}", name), format!("Bundles2/{}.bundle.bin", name)]
                .iter()
                .find_map(|c| {
                    reader.read_file_by_path(c).ok().flatten().and_then(|rec| {
                        reader.get_data_slice(rec.data_offset, rec.data_length).ok().map(|d| d.to_vec())
                    })
                });
            if hit.is_some() {
                return hit;
            }
        }
        if let Some(steam) = self.steam.as_ref() {
            if let Ok(data) = steam.fetch_bundle(name) {
                return Some(data);
            }
        }
        self.cdn.as_ref().and_then(|cdn| cdn.fetch_bundle(name).ok())
    }
}

impl FileSource for GameFiles {
    fn fetch(&self, path: &str) -> Option<Vec<u8>> {
        let info = self.lookup(path)?;
        let data = self.bundle(info.bundle_index)?;
        let start = info.file_offset as usize;
        let end = start.checked_add(info.file_size as usize)?;
        data.get(start..end).map(|s| s.to_vec())
    }
}
