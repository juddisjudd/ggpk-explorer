use crate::parsers::types::{FileFormatParser, ParsedContent};
use std::collections::HashMap;

/// Parser for graphics/binary formats (FMT, GFT binary, etc.)
pub struct GraphicsParser;

impl FileFormatParser for GraphicsParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        // For now, provide basic metadata about the binary format
        let mut metadata = HashMap::new();
        metadata.insert("format".to_string(), "binary".to_string());
        metadata.insert("size".to_string(), format!("{} bytes", bytes.len()));

        // Try to detect structure from first few bytes
        if bytes.len() >= 4 {
            let first_u32 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            metadata.insert(
                "first_dword".to_string(),
                format!("0x{:08x}", first_u32),
            );
        }

        Ok(ParsedContent::Binary {
            data: bytes.to_vec(),
            metadata,
        })
    }
}

/// FMT Parser - Format/Mesh (graphics)
pub struct FMTParser;

impl FileFormatParser for FMTParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        if bytes.len() < 20 {
            return Err("FMT file too small".to_string());
        }

        let mut metadata = HashMap::new();
        let version = bytes[0];
        metadata.insert("version".to_string(), version.to_string());
        metadata.insert("format".to_string(), "FMT Mesh".to_string());

        // Parse bounding box (6 f32 values)
        if bytes.len() >= 28 {
            let bbox_start = 1;
            metadata.insert(
                "bbox_start".to_string(),
                format!(
                    "({:.2}, {:.2}, {:.2})",
                    f32::from_le_bytes([
                        bytes[bbox_start],
                        bytes[bbox_start + 1],
                        bytes[bbox_start + 2],
                        bytes[bbox_start + 3]
                    ]),
                    f32::from_le_bytes([
                        bytes[bbox_start + 4],
                        bytes[bbox_start + 5],
                        bytes[bbox_start + 6],
                        bytes[bbox_start + 7]
                    ]),
                    f32::from_le_bytes([
                        bytes[bbox_start + 8],
                        bytes[bbox_start + 9],
                        bytes[bbox_start + 10],
                        bytes[bbox_start + 11]
                    ])
                ),
            );
        }

        Ok(ParsedContent::Binary {
            data: bytes.to_vec(),
            metadata,
        })
    }
}

/// GT Parser (Binary variant) - Graphics Template
pub struct GTBinaryParser;

impl FileFormatParser for GTBinaryParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        let mut metadata = HashMap::new();
        metadata.insert("format".to_string(), "GT Graphics Template".to_string());
        metadata.insert("size".to_string(), format!("{} bytes", bytes.len()));

        if bytes.len() >= 4 {
            let header = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            metadata.insert("header".to_string(), format!("0x{:08x}", header));
        }

        Ok(ParsedContent::Binary {
            data: bytes.to_vec(),
            metadata,
        })
    }
}

/// ECF Parser (Binary variant) - Effect Configuration
pub struct ECFBinaryParser;

impl FileFormatParser for ECFBinaryParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        let mut metadata = HashMap::new();
        metadata.insert("format".to_string(), "ECF Effect Configuration".to_string());
        metadata.insert("size".to_string(), format!("{} bytes", bytes.len()));

        Ok(ParsedContent::Binary {
            data: bytes.to_vec(),
            metadata,
        })
    }
}
