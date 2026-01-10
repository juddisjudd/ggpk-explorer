use std::io::{self, Cursor, Read};
use byteorder::{ByteOrder, LittleEndian};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleInfo {
    pub name: String,
    pub uncompressed_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path_hash: u64,
    pub bundle_index: u32,
    pub file_offset: u32,
    pub file_size: u32,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryInfo {
    pub path_hash: u64,
    pub offset: u32,
    pub size: u32,
    pub recursive_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub bundles: Vec<BundleInfo>,
    pub files: HashMap<u64, FileInfo>,
}


#[derive(Debug, Clone, Copy, PartialEq)]
enum HashAlgorithm {
    Murmur64A,
    Fnv1a,
    Unknown,
}

impl Index {
    pub fn read(data: &[u8]) -> io::Result<Self> {
        let mut cursor = Cursor::new(data);
        

        let bundle_count = read_i32(&mut cursor)?;
        let mut bundles = Vec::with_capacity(bundle_count as usize);
        
        for _ in 0..bundle_count {
            let name_len = read_i32(&mut cursor)?;
            let mut name_buf = vec![0u8; name_len as usize];
            cursor.read_exact(&mut name_buf)?;
            let name = String::from_utf8_lossy(&name_buf).to_string();
            
            let uncompressed_size = read_u32(&mut cursor)?;
            bundles.push(BundleInfo { name, uncompressed_size });
        }
        
        let file_count = read_i32(&mut cursor)?;
        println!("Index::read: Found {} files", file_count);
        let mut files_map = HashMap::with_capacity(file_count as usize);
        
        for _ in 0..file_count {
            let path_hash = read_u64(&mut cursor)?;
            let bundle_index = read_u32(&mut cursor)?;
            let file_offset = read_u32(&mut cursor)?;
            let file_size = read_u32(&mut cursor)?;
            
            files_map.insert(path_hash, FileInfo { 
                path_hash, 
                bundle_index, 
                file_offset, 
                file_size,
                path: String::new(),
            });
        }
        
        let directory_count = read_i32(&mut cursor)?;
        let mut directories = Vec::with_capacity(directory_count as usize);
        
        for _ in 0..directory_count {
            let path_hash = read_u64(&mut cursor)?;
            let offset = read_u32(&mut cursor)?;
            let size = read_u32(&mut cursor)?;
            let recursive_size = read_u32(&mut cursor)?;
            
            directories.push(DirectoryInfo { path_hash, offset, size, recursive_size });
        }
        
        let current_pos = cursor.position() as usize;
        let directory_bundle_data = &data[current_pos..];
        

        let hash_algo = if let Some(first_dir) = directories.first() {
             match first_dir.path_hash {
                 0xF42A94E69CFF42FE => {
                     println!("Index::read: Detected Hash Algorithm: Murmur64A");
                     HashAlgorithm::Murmur64A
                 },
                 0x07E47507B4A92E53 => {
                     println!("Index::read: Detected Hash Algorithm: FNV1a");
                     HashAlgorithm::Fnv1a
                 },
                 other => {
                     println!("Index::read: Unknown Hash Algorithm root hash: {:X}. Defaulting to fallback.", other);
                     HashAlgorithm::Unknown
                 },
             }
        } else {
             HashAlgorithm::Unknown
        };

        let mut dir_cursor = Cursor::new(directory_bundle_data);
        if let Ok(bundle) = crate::bundles::bundle::Bundle::read_header(&mut dir_cursor) {
             if let Ok(dir_data) = bundle.decompress(&mut dir_cursor) {
                 Self::parse_paths(&directories, &dir_data, &mut files_map, hash_algo);
             } else {
                 println!("Failed to decompress directory bundle");
             }
        } else {
            println!("Failed to read directory bundle header");
        }

        let populated_count = files_map.values().filter(|f| !f.path.is_empty()).count();
        println!("Index::read: {}/{} files have paths", populated_count, files_map.len());
        
        Ok(Self { bundles, files: files_map })
    }

    pub fn save_to_cache<P: AsRef<std::path::Path>>(&self, path: P) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        bincode::serialize_into(&mut writer, self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    pub fn load_from_cache<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let mut reader = std::io::BufReader::new(file);
        bincode::deserialize_from(&mut reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    fn parse_paths(directories: &[DirectoryInfo], dir_data: &[u8], files: &mut HashMap<u64, FileInfo>, hash_algo: HashAlgorithm) {
        if dir_data.is_empty() { return; }

        for d in directories {
            if d.offset as usize >= dir_data.len() { continue; }
            let start = d.offset as usize;
            let end = start + d.size as usize;
            if end > dir_data.len() { continue; }
            
            let chunk = &dir_data[start..end];
            let mut ptr = 0;
            let mut temp: Vec<Vec<u8>> = Vec::new();
            let mut base = false;

            while ptr + 4 <= chunk.len() {
                let val = LittleEndian::read_u32(&chunk[ptr..ptr+4]);
                ptr += 4;

                if val == 0 {
                    base = !base;
                    if base {
                        temp.clear();
                    }
                    continue;
                }

                let idx = (val - 1) as usize;
                
                // Read String
                let mut str_len = 0;
                while ptr + str_len < chunk.len() && chunk[ptr + str_len] != 0 {
                    str_len += 1;
                }
                
                let s_bytes = if ptr + str_len < chunk.len() {
                    &chunk[ptr..ptr+str_len]
                } else {
                    &[]
                };
                
                // Construct full path content
                let full_path_bytes = if idx < temp.len() {
                    let mut p = temp[idx].clone();
                    p.extend_from_slice(s_bytes);
                    p
                } else {
                    s_bytes.to_vec()
                };
                
                ptr += str_len + 1; // +1 for null

                if base {
                    if idx < temp.len() {
                         temp.push(full_path_bytes);
                    } else {
                         temp.push(full_path_bytes);
                    }
                } else {
                    // File Mode
                    let path_str = String::from_utf8_lossy(&full_path_bytes).to_string();

                    match hash_algo {
                        HashAlgorithm::Murmur64A => {
                             let hash = murmur_hash64a(&full_path_bytes);
                             if let Some(f) = files.get_mut(&hash) { 
                                 f.path = path_str;
                             }
                        },
                        HashAlgorithm::Fnv1a => {
                             // FNV1a usually expects lowercase in old GGPK? 
                             // LibGGPK3 says: "NameHash(utf8Name) ... utf8Name must be lowercased unless it comes from ggpk before patch 3.21.2"
                             // But wait, older GGPK (FNV) used various casing?
                             // Actually LibGGPK3 code:
                             // case 0x07E47507B4A92E53: return FNV1a64Hash(utf8Name);
                             // AND NameHash(name) calls name.ToLowerInvariant() first if directory hash matches recent Murmur magic.
                             // Wait, if it is FNV (else branch), it allocates stackalloc byte[name.Length] ... NO, it uses original casing?
                             
                             // LibGGPK3 logic:
                             // If root_hash == MURMUR_MAGIC: lower case it, then hash.
                             // Else: use original name to hash?
                             
                             // Let's look at `NameHash(scoped ReadOnlySpan<char> name)` in Index.cs again.
                             /*
                                if (_Directories[0].PathHash == 0xF42A94E69CFF42FEul) { // Murmur
                                    name.ToLowerInvariant(span);
                                    return MurmurHash64A(...);
                                } else {
                                    return NameHash(utf8... original); -> which uses FNV1a64Hash
                                }
                             */
                             
                             // So for FNV, we try original casing?
                             // But my previous code tried all 4 combinations.
                             // Let's try Original first, then Lower?
                             // Optimization: Only compute FNV.
                             
                             let hash = fnv1a64(&full_path_bytes);
                             if let Some(f) = files.get_mut(&hash) {
                                 f.path = path_str.clone();
                             } else {
                                 // Fallback to lower?
                                 let lower_bytes = full_path_bytes.to_ascii_lowercase();
                                 let hash_lower = fnv1a64(&lower_bytes);
                                 if let Some(f) = files.get_mut(&hash_lower) {
                                     f.path = path_str;
                                 }
                             }
                        },
                        HashAlgorithm::Unknown => {
                            // Fallback to trying everything (old slow behavior)
                            let lower_bytes = full_path_bytes.to_ascii_lowercase();
                            let hash_murmur = murmur_hash64a(&full_path_bytes);
                            let hash_murmur_lower = murmur_hash64a(&lower_bytes);
                            let hash_fnv = fnv1a64(&full_path_bytes);
                            let hash_fnv_lower = fnv1a64(&lower_bytes);
                            
                            if let Some(f) = files.get_mut(&hash_murmur) { f.path = path_str; }
                            else if let Some(f) = files.get_mut(&hash_murmur_lower) { f.path = path_str; }
                            else if let Some(f) = files.get_mut(&hash_fnv) { f.path = path_str; }
                            else if let Some(f) = files.get_mut(&hash_fnv_lower) { f.path = path_str; }
                        }
                    }
                }
            }
        }
    }
}

pub fn murmur_hash64a(key: &[u8]) -> u64 {
    let seed: u64 = 0x1337B33F;
    let m: u64 = 0xc6a4a7935bd1e995;
    let r: i32 = 47;

    let len = key.len();
    let mut h: u64 = seed ^ ((len as u64).wrapping_mul(m));

    let n_blocks = len / 8;
    let mut data = key;

    for _ in 0..n_blocks {
        let mut k = LittleEndian::read_u64(&data[0..8]);

        k = k.wrapping_mul(m);
        k ^= k >> r;
        k = k.wrapping_mul(m);

        h ^= k;
        h = h.wrapping_mul(m);

        data = &data[8..];
    }

    let remaining = &data;
    if !remaining.is_empty() {
        // C++:
        // switch (len & 7) {
        // case 7: h ^= uint64_t(data2[6]) << 48;
        // case 6: h ^= uint64_t(data2[5]) << 40;
        // ...
        // case 1: h ^= uint64_t(data2[0]);
        //         h *= m;
        // };

        let len_rem = len & 7;
        if len_rem >= 7 { h ^= (remaining[6] as u64) << 48; }
        if len_rem >= 6 { h ^= (remaining[5] as u64) << 40; }
        if len_rem >= 5 { h ^= (remaining[4] as u64) << 32; }
        if len_rem >= 4 { h ^= (remaining[3] as u64) << 24; }
        if len_rem >= 3 { h ^= (remaining[2] as u64) << 16; }
        if len_rem >= 2 { h ^= (remaining[1] as u64) << 8; }
        if len_rem >= 1 { 
            h ^= remaining[0] as u64; 
            h = h.wrapping_mul(m);
        }
    }

    h ^= h >> r;
    h = h.wrapping_mul(m);
    h ^= h >> r;

    h
}

fn fnv1a64(key: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in key {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}


fn read_i32<R: Read>(reader: &mut R) -> io::Result<i32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(LittleEndian::read_i32(&buf))
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(LittleEndian::read_u32(&buf))
}

fn read_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(LittleEndian::read_u64(&buf))
}



