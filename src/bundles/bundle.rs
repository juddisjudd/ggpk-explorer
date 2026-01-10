use std::io::{self, Read, Seek, SeekFrom};
use byteorder::{ByteOrder, LittleEndian};
use crate::ooz::sys::Ooz_Decompress;
use std::ptr;

pub struct Bundle {
    pub uncompressed_size: u32,
    pub total_payload_size: u32,
    pub head_payload_size: u32,
    pub first_file_encode: u32,
    pub uncompressed_size2: u64,
    pub total_payload_size2: u64,
    pub block_count: u32,
    pub chunk_size: u32,
    pub block_sizes: Vec<u32>,
    pub data_offset: u64,
}

impl Bundle {
    pub fn read_header<R: Read + Seek>(mut reader: R) -> io::Result<Self> {
        let mut header = [0u8; 60];
        reader.read_exact(&mut header)?;
        
        let uncompressed_size = LittleEndian::read_u32(&header[0..4]);
        let total_payload_size = LittleEndian::read_u32(&header[4..8]);
        let head_payload_size = LittleEndian::read_u32(&header[8..12]);
        let first_file_encode = LittleEndian::read_u32(&header[12..16]);
        println!("Bundle Header: Compressor Type = {}", first_file_encode);
        // unk10 at 16..20
        let uncompressed_size2 = LittleEndian::read_u64(&header[20..28]);
        let total_payload_size2 = LittleEndian::read_u64(&header[28..36]);
        let block_count = LittleEndian::read_u32(&header[36..40]);
        let chunk_size = LittleEndian::read_u32(&header[40..44]);
        // unk28 at 44..60

        let mut block_sizes = Vec::with_capacity(block_count as usize);
        let mut block_sizes_buf = vec![0u8; (block_count * 4) as usize];
        reader.read_exact(&mut block_sizes_buf)?;
        
        for i in 0..block_count {
            let size = LittleEndian::read_u32(&block_sizes_buf[(i as usize)*4..((i as usize)+1)*4]);
            block_sizes.push(size);
        }
        
        let data_offset = reader.stream_position()?;

        Ok(Self {
            uncompressed_size,
            total_payload_size,
            head_payload_size,
            first_file_encode,
            uncompressed_size2,
            total_payload_size2,
            block_count,
            chunk_size,
            block_sizes,
            data_offset,
        })
    }

    pub fn decompress<R: Read + Seek>(&self, mut reader: R) -> io::Result<Vec<u8>> {
        let mut output = vec![0u8; self.uncompressed_size as usize]; // Using u32 size for now
        let output_ptr = output.as_mut_ptr();
        let mut output_offset = 0;
        
        reader.seek(SeekFrom::Start(self.data_offset))?;
        
        for &block_size in &self.block_sizes {
            let mut compressed_data = vec![0u8; block_size as usize];
            reader.read_exact(&mut compressed_data)?;
            
            // Determine decompressed size for this block
            // Usually 256KB, except last one.
            let remaining = self.uncompressed_size as usize - output_offset;
            let dst_len = std::cmp::min(remaining, self.chunk_size as usize);
            
            let ret = unsafe {
                Ooz_Decompress(
                    compressed_data.as_ptr(),
                    block_size as i32,
                    output_ptr.add(output_offset),
                    dst_len,
                    0, 0, 0,
                    ptr::null_mut(), 0, ptr::null_mut(), ptr::null_mut(),
                    ptr::null_mut(), 0, 0
                )
            };
            
            if ret != dst_len as i32 {
                println!("Ooz_Decompress FAILED: ret={}, dst_len={}", ret, dst_len);
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Ooz_Decompress failed: returned {}, expected {}", ret, dst_len)));
            } else {
                // println!("Ooz_Decompress success: {}", ret);
            }
            
            output_offset += dst_len;
        }
        
        Ok(output)
    }
}

