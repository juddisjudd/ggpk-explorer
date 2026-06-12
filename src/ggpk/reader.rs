use super::record::{GgpkRecord, RecordHeader, RecordTag, DirectoryRecord, FileRecord};
use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::path::Path;

pub struct GgpkReader {
    mmap: Mmap,
    pub root_offset: u64,
    pub version: u32,
}

impl GgpkReader {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };


        if mmap.len() < 8 {
             return Err(io::Error::new(io::ErrorKind::InvalidData, "File too small"));
        }
        

        let header = RecordHeader::read(&mmap[0..8]);
        if header.tag != RecordTag::GGPK {
             return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid GGPK signature"));
        }

        let ggpk_rec = GgpkRecord::read(&mmap, 0)?;

        
        Ok(Self {
            mmap,
            root_offset: ggpk_rec.root_offset,
            version: ggpk_rec.version,
        })
    }

    pub fn read_record_header(&self, offset: u64) -> io::Result<RecordHeader> {
        let offset = offset as usize;
        if offset + 8 > self.mmap.len() {
             return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Offset out of bounds"));
        }
        Ok(RecordHeader::read(&self.mmap[offset..offset+8]))
    }

    pub fn read_directory(&self, offset: u64) -> io::Result<DirectoryRecord> {
        let header = self.read_record_header(offset)?;
        if header.tag != RecordTag::PDIR {
             return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Expected PDIR at {}, found {:?}", offset, header.tag)));
        }
        let data = self.get_slice(offset, header.length as u64)?;
        DirectoryRecord::read(data, offset, self.version)
    }

    pub fn read_file_record(&self, offset: u64) -> io::Result<FileRecord> {
        let header = self.read_record_header(offset)?;
        if header.tag != RecordTag::FILE {
             return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Expected FILE at {}, found {:?}", offset, header.tag)));
        }
        let data = self.get_slice(offset, header.length as u64)?;
        FileRecord::read(data, offset, self.version)
    }

    fn get_slice(&self, offset: u64, length: u64) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start + length as usize;
        if end > self.mmap.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Record length out of bounds"));
        }
        Ok(&self.mmap[start..end])
    }
    
    pub fn get_data_slice(&self, offset: u64, length: u64) -> io::Result<&[u8]> {
        self.get_slice(offset, length)
    }

    #[allow(dead_code)]
    pub fn is_poe2_heuristic(&self) -> bool {

        
        let root = match self.read_directory(self.root_offset) {
            Ok(r) => r,
            Err(_) => return false,
        };

        for entry in root.entries {
             if let Ok(header) = self.read_record_header(entry.offset) {
                 if header.tag == RecordTag::PDIR {
                     if let Ok(dir) = self.read_directory(entry.offset) {
                         if dir.name == "Data" {
                             // Found Data, look for Balance
                             for child in dir.entries {
                                 if let Ok(child_header) = self.read_record_header(child.offset) {
                                     if child_header.tag == RecordTag::PDIR {

                                         if let Ok(child_dir) = self.read_directory(child.offset) {
                                             if child_dir.name == "Balance" {
                                                 return true;
                                             }
                                         }
                                     }
                                 }
                             }
                             return false;
                         }
                     }
                 }
             }
        }
        
        false
    }

    /// Collects all loose FILE records stored directly in the GGPK (outside
    /// Bundles2/) as (virtual_path, data_length) pairs — e.g. FMOD/ sound
    /// banks and Media/ videos, which are not part of the bundle index.
    pub fn collect_loose_files(&self) -> Vec<(String, u64)> {
        // Reverse order so the live record of a stale/live duplicate pair is
        // collected first and wins the first-occurrence dedup in
        // `Index::add_ggpk_loose_files` (see `find_file_in_dir`).
        let mut out = Vec::new();
        if let Ok(root) = self.read_directory(self.root_offset) {
            for entry in root.entries.iter().rev() {
                let header = match self.read_record_header(entry.offset) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                match header.tag {
                    RecordTag::PDIR => {
                        if let Ok(dir) = self.read_directory(entry.offset) {
                            if dir.name.eq_ignore_ascii_case("Bundles2") {
                                continue;
                            }
                            let prefix = dir.name.clone();
                            self.collect_loose_dir(&dir, &prefix, &mut out);
                        }
                    }
                    RecordTag::FILE => {
                        if let Ok(file) = self.read_file_record(entry.offset) {
                            out.push((file.name, file.data_length));
                        }
                    }
                    _ => {}
                }
            }
        }
        out
    }

    fn collect_loose_dir(&self, dir: &DirectoryRecord, prefix: &str, out: &mut Vec<(String, u64)>) {
        for entry in dir.entries.iter().rev() {
            let header = match self.read_record_header(entry.offset) {
                Ok(h) => h,
                Err(_) => continue,
            };
            match header.tag {
                RecordTag::PDIR => {
                    if let Ok(sub) = self.read_directory(entry.offset) {
                        let sub_prefix = format!("{}/{}", prefix, sub.name);
                        self.collect_loose_dir(&sub, &sub_prefix, out);
                    }
                }
                RecordTag::FILE => {
                    if let Ok(file) = self.read_file_record(entry.offset) {
                        out.push((format!("{}/{}", prefix, file.name), file.data_length));
                    }
                }
                _ => {}
            }
        }
    }

    pub fn read_file_by_path(&self, path: &str) -> io::Result<Option<FileRecord>> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(self.find_file_in_dir(self.root_offset, &parts))
    }

    /// Recursive path resolution with backtracking: the GGPK can contain
    /// multiple sibling directories with the same name (e.g. two root Art/
    /// records), so on a dead end the search continues with the next match.
    ///
    /// Entries are scanned in REVERSE order: when the patcher leaves a stale
    /// duplicate behind, the live record is the later entry. Resolving the
    /// stale copy returns bundle data that no longer matches the index —
    /// corrupted/truncated files (issue #10).
    fn find_file_in_dir(&self, dir_offset: u64, parts: &[&str]) -> Option<FileRecord> {
        let (part, rest) = parts.split_first()?;
        let dir = self.read_directory(dir_offset).ok()?;

        for entry in dir.entries.iter().rev() {
            let header = match self.read_record_header(entry.offset) {
                Ok(h) => h,
                Err(_) => continue,
            };
            match header.tag {
                RecordTag::PDIR if !rest.is_empty() => {
                    if let Ok(sub_dir) = self.read_directory(entry.offset) {
                        if sub_dir.name.eq_ignore_ascii_case(part) {
                            if let Some(found) = self.find_file_in_dir(entry.offset, rest) {
                                return Some(found);
                            }
                        }
                    }
                }
                RecordTag::FILE if rest.is_empty() => {
                    if let Ok(file) = self.read_file_record(entry.offset) {
                        if file.name.eq_ignore_ascii_case(part) {
                            return Some(file);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub fn list_files_in_directory(&self, path: &str) -> io::Result<Vec<String>> {
         let parts: Vec<&str> = path.split('/').collect();
         let mut current_offset = self.root_offset;
         

         for part in parts {
             if part.is_empty() { continue; }
             let dir = self.read_directory(current_offset)?;
             let mut found_offset = None;
             for entry in dir.entries {
                  let header = self.read_record_header(entry.offset)?;
                  if header.tag == RecordTag::PDIR {
                      let sub_dir = self.read_directory(entry.offset)?;
                      if sub_dir.name.eq_ignore_ascii_case(part) {
                          found_offset = Some(entry.offset);
                      }
                  }
             }
             if let Some(offset) = found_offset {
                 current_offset = offset;
             } else {
                 return Err(io::Error::new(io::ErrorKind::NotFound, format!("Directory {} not found", part)));
             }
         }
         

         let dir = self.read_directory(current_offset)?;
         let mut entries = Vec::new();
         for entry in dir.entries {
              let header = self.read_record_header(entry.offset)?;
              match header.tag {
                  RecordTag::FILE => {
                      if let Ok(file) = self.read_file_record(entry.offset) {
                          entries.push(format!("FILE:{}", file.name));
                      }
                  },
                  RecordTag::PDIR => {
                      if let Ok(sub_dir) = self.read_directory(entry.offset) {
                          entries.push(format!("DIR:{}", sub_dir.name));
                      }
                  },
                  _ => {}
              }
         }
         Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression test for issue #10: GGPKs patched over time contain stale
    // duplicate records (e.g. two Bundles2/folders/ dirs); path resolution
    // must return the live (last) one or bundle contents no longer match the
    // index and every file inside reads corrupted/truncated.
    //
    // Needs a local PoE2 install configured in the app settings.
    // Run with: cargo test ggpk_resolves_live_bundle_records -- --ignored --nocapture
    #[test]
    #[ignore]
    fn ggpk_resolves_live_bundle_records() {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = GgpkReader::open(&ggpk_path).unwrap();

        let index_rec = reader
            .read_file_by_path("Bundles2/_.index.bin")
            .unwrap()
            .expect("_.index.bin not found");
        let data = reader
            .get_data_slice(index_rec.data_offset, index_rec.data_length)
            .unwrap();
        let mut cursor = std::io::Cursor::new(data);
        let bundle = crate::bundles::bundle::Bundle::read_header(&mut cursor).unwrap();
        let decompressed = bundle.decompress(&mut cursor).unwrap();
        let index = crate::bundles::index::Index::read(&decompressed).unwrap();

        let mut checked = 0;
        let mut mismatches = Vec::new();
        for info in &index.bundles {
            let path = format!("Bundles2/{}.bundle.bin", info.name);
            let Ok(Some(rec)) = reader.read_file_by_path(&path) else {
                continue; // bundle not in GGPK (fetched from Steam/CDN)
            };
            let Ok(data) = reader.get_data_slice(rec.data_offset, rec.data_length) else {
                continue;
            };
            let mut cursor = std::io::Cursor::new(data);
            let Ok(header) = crate::bundles::bundle::Bundle::read_header(&mut cursor) else {
                continue;
            };
            checked += 1;
            if header.uncompressed_size != info.uncompressed_size {
                mismatches.push(format!(
                    "{}: header says {} bytes, index expects {} bytes (stale record resolved)",
                    info.name, header.uncompressed_size, info.uncompressed_size
                ));
            }
        }
        println!("checked {} bundles, {} mismatches", checked, mismatches.len());
        assert!(checked > 0, "no bundles could be resolved from the GGPK");
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    }
}
