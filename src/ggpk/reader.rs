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

    pub fn read_file_by_path(&self, path: &str) -> io::Result<Option<FileRecord>> {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.is_empty() {
            return Ok(None);
        }

        let mut current_offset = self.root_offset;
        
        for (i, part) in parts.iter().enumerate() {
            let dir = self.read_directory(current_offset)?;
            let mut found_offset = None;
            let mut is_file = false;

            for entry in dir.entries {
                let header = self.read_record_header(entry.offset)?;
                match header.tag {
                    RecordTag::PDIR => {
                        let sub_dir = self.read_directory(entry.offset)?;
                        if sub_dir.name.eq_ignore_ascii_case(part) {
                            found_offset = Some(entry.offset);
                            is_file = false;
                        }
                    },
                    RecordTag::FILE => {
                         let file = self.read_file_record(entry.offset)?;
                         if file.name.eq_ignore_ascii_case(part) {
                             found_offset = Some(entry.offset);
                             is_file = true;
                         }
                    },
                    _ => {}
                }
            }
            
            if let Some(offset) = found_offset {
                if i == parts.len() - 1 {
                    if is_file {
                        return Ok(Some(self.read_file_record(offset)?));
                    } else {
                        return Ok(None);
                    }
                } else {
                    if is_file {
                        return Ok(None);
                    }
                    current_offset = offset;
                }
            } else {
                return Ok(None);
            }
        }
        Ok(None)
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
