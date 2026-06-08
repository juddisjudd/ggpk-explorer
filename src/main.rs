#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ggpk;
mod dat;
mod ooz;
pub mod bundles;
mod ui;
pub mod settings;
pub mod cli;
pub mod update;
pub mod export;
pub mod parsers;
pub mod adapters;

fn main() -> eframe::Result<()> {
    env_logger::init();
    

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "inspect" {

        if let Err(e) = cli::run_inspect() {
            eprintln!("Inspection failed: {}", e);
        }
        return Ok(());
    }


    ui::run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_murmur_hash64a(key: &[u8], seed: u64) -> u64 {
        let m: u64 = 0xc6a4a7935bd1e995;
        let r: u8 = 47;
        let len = key.len() as u64;
        let mut h: u64 = seed ^ (len.wrapping_mul(m));
        let n_blocks = len / 8;
        let md = key;
        for i in 0..n_blocks {
            let idx = (i * 8) as usize;
            let mut k: u64 = u64::from_le_bytes(md[idx..idx+8].try_into().unwrap());
            k = k.wrapping_mul(m);
            k ^= k >> r;
            k = k.wrapping_mul(m);
            h ^= k;
            h = h.wrapping_mul(m);
        }
        let remainder_idx = (n_blocks * 8) as usize;
        let remaining_len = (len & 7) as usize;
        if remaining_len > 0 {
            let mut k: u64 = 0;
            for i in 0..remaining_len {
                 k ^= (md[remainder_idx + i] as u64) << (8 * i);
            }
            h ^= k;
            h = h.wrapping_mul(m);
        }
        h ^= h >> r;
        h = h.wrapping_mul(m);
        h ^= h >> r;
        h
    }

    #[test]
    #[ignore]
    fn test_find_psg() {
        let settings = settings::AppSettings::load();
        let mut msg = format!(
            "Settings GGPK: {:?}\nSettings Steam: {:?}\n",
            settings.ggpk_path, settings.steam_path
        );
        
        let file_bytes = if let Some(steam_path) = &settings.steam_path {
            let steam = crate::bundles::steam::SteamBundleLoader::new(std::path::PathBuf::from(steam_path));
            if let Ok(index_bytes) = steam.load_index_bytes() {
                let mut cursor = std::io::Cursor::new(&index_bytes);
                if let Ok(bundle) = crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                    if let Ok(decompressed) = bundle.decompress(&mut cursor) {
                        if let Ok(idx) = crate::bundles::index::Index::read(&decompressed) {
                            let hash = local_murmur_hash64a(b"metadata/passiveskillgraph.psg", 0x1337b33f);
                            if let Some(file_info) = idx.files.get(&hash) {
                                if let Some(bundle_info) = idx.bundles.get(file_info.bundle_index as usize) {
                                    if let Ok(bundle_data) = steam.fetch_bundle(&bundle_info.name) {
                                        let mut cursor = std::io::Cursor::new(bundle_data);
                                        if let Ok(b) = crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                                            if let Ok(decomp) = b.decompress(&mut cursor) {
                                                let start = file_info.file_offset as usize;
                                                let end = start + file_info.file_size as usize;
                                                Some(decomp[start..end].to_vec())
                                            } else { msg.push_str("Steam: Decompress bundle failed\n"); None }
                                        } else { msg.push_str("Steam: Read bundle header failed\n"); None }
                                    } else { msg.push_str("Steam: Fetch bundle failed\n"); None }
                                } else { msg.push_str("Steam: Bundle info not found\n"); None }
                            } else { msg.push_str("Steam: File hash not in index\n"); None }
                        } else { msg.push_str("Steam: Index read failed\n"); None }
                    } else { msg.push_str("Steam: Decompress index failed\n"); None }
                } else { msg.push_str("Steam: Read index header failed\n"); None }
            } else { msg.push_str("Steam: Load index bytes failed\n"); None }
        } else if let Some(ggpk_path) = &settings.ggpk_path {
            match crate::ggpk::reader::GgpkReader::open(ggpk_path) {
                Ok(reader) => {
                    let mut found_bytes = None;
                    match reader.read_file_by_path("Bundles2/_.index.bin") {
                        Ok(Some(index_rec)) => {
                            match reader.get_data_slice(index_rec.data_offset, index_rec.data_length) {
                                Ok(data) => {
                                    let mut cursor = std::io::Cursor::new(data);
                                    match crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                                        Ok(bundle) => {
                                            match bundle.decompress(&mut cursor) {
                                                Ok(decompressed) => {
                                                    match crate::bundles::index::Index::read(&decompressed) {
                                                        Ok(idx) => {
                                                            let hash = local_murmur_hash64a(b"metadata/passiveskillgraph.psg", 0x1337b33f);
                                                            if let Some(file_info) = idx.files.get(&hash) {
                                                                if let Some(bundle_info) = idx.bundles.get(file_info.bundle_index as usize) {
                                                                    let candidates = vec![
                                                                        format!("Bundles2/{}", bundle_info.name),
                                                                        format!("Bundles2/{}.bundle.bin", bundle_info.name),
                                                                        bundle_info.name.clone(),
                                                                        format!("{}.bundle.bin", bundle_info.name),
                                                                    ];
                                                                    let mut b_rec = None;
                                                                    for cand in &candidates {
                                                                        if let Ok(Some(rec)) = reader.read_file_by_path(cand) {
                                                                            b_rec = Some(rec);
                                                                            break;
                                                                        }
                                                                    }
                                                                    if let Some(rec) = b_rec {
                                                                        match reader.get_data_slice(rec.data_offset, rec.data_length) {
                                                                            Ok(b_data) => {
                                                                                let mut cursor = std::io::Cursor::new(b_data);
                                                                                match crate::bundles::bundle::Bundle::read_header(&mut cursor) {
                                                                                    Ok(b) => {
                                                                                        match b.decompress(&mut cursor) {
                                                                                            Ok(decomp) => {
                                                                                                let start = file_info.file_offset as usize;
                                                                                                let end = start + file_info.file_size as usize;
                                                                                                found_bytes = Some(decomp[start..end].to_vec());
                                                                                            }
                                                                                            Err(e) => msg.push_str(&format!("GGPK: Decompress bundle failed: {}\n", e)),
                                                                                        }
                                                                                    }
                                                                                    Err(e) => msg.push_str(&format!("GGPK: Read bundle header failed: {}\n", e)),
                                                                                }
                                                                            }
                                                                            Err(e) => msg.push_str(&format!("GGPK: Get data slice failed for bundle: {}\n", e)),
                                                                        }
                                                                    } else {
                                                                        msg.push_str(&format!("GGPK: Bundle candidates not found: {:?}\n", candidates));
                                                                    }
                                                                } else { msg.push_str("GGPK: Bundle info not found\n"); }
                                                            } else { msg.push_str("GGPK: File hash not in index\n"); }
                                                        }
                                                        Err(e) => msg.push_str(&format!("GGPK: Index read failed: {}\n", e)),
                                                    }
                                                }
                                                Err(e) => msg.push_str(&format!("GGPK: Decompress index failed: {}\n", e)),
                                            }
                                        }
                                        Err(e) => msg.push_str(&format!("GGPK: Read index header failed: {}\n", e)),
                                    }
                                }
                                Err(e) => msg.push_str(&format!("GGPK: Get data slice failed for index: {}\n", e)),
                            }
                        }
                        Ok(std::option::Option::None) => msg.push_str("GGPK: Bundles2/_.index.bin not found in GGPK\n"),
                        Err(e) => msg.push_str(&format!("GGPK: Read index file from GGPK failed: {}\n", e)),
                    }
                    found_bytes
                }
                Err(e) => { msg.push_str(&format!("GGPK: Open reader failed: {}\n", e)); None }
            }
        } else {
            None
        };

        if let Some(bytes) = file_bytes {
            msg.push_str(&format!("Loaded game passiveskillgraph.psg: size={} bytes\n", bytes.len()));
            match crate::dat::psg::parse_psg(&bytes) {
                Ok(psg) => {
                    msg.push_str(&format!("Parsed successfully! Roots: {}, Groups: {}\n", psg.roots.len(), psg.groups.len()));
                }
                Err(e) => {
                    msg.push_str(&format!("Failed to parse: {}\n", e));
                }
            }
        } else {
            msg.push_str("Could not load metadata/passiveskillgraph.psg.\n");
        }
        panic!("{}", msg);
    }

    #[test]
    fn test_ooz_link() {
        println!("Testing ooz linking...");
        unsafe {
            let ptr = ooz::sys::BunMemAlloc(10);
            assert!(!ptr.is_null());
            ooz::sys::BunMemFree(ptr);
        }
    }
}
