use crate::parsers::types::{FileFormatParser, ParsedContent};
use std::collections::HashMap;

/// SMD Parser - Skeletal Mesh Data
pub struct SMDParser;

impl FileFormatParser for SMDParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        if bytes.is_empty() {
            return Err("SMD file is empty".to_string());
        }

        let mut metadata = HashMap::new();
        let version = bytes[0];
        metadata.insert("version".to_string(), version.to_string());

        if bytes.len() >= 2 {
            let vertex_format = bytes[1];
            metadata.insert("vertex_format".to_string(), vertex_format.to_string());
        }

        // Parse bounding box if present
        if bytes.len() >= 28 {
            metadata.insert(
                "has_bounding_box".to_string(),
                "true".to_string(),
            );
        }

        // Estimate bone count from file size (rough heuristic)
        let estimated_bones = (bytes.len() / 64).min(256);
        metadata.insert("estimated_bones".to_string(), estimated_bones.to_string());

        Ok(ParsedContent::Binary {
            data: bytes.to_vec(),
            metadata,
        })
    }
}

/// TSI Parser (Skeletal variant) - Configuration/Indices
pub struct TSISkelParser;

impl FileFormatParser for TSISkelParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        let mut metadata = HashMap::new();
        metadata.insert("format".to_string(), "TSI Configuration".to_string());
        metadata.insert("size".to_string(), format!("{} bytes", bytes.len()));

        Ok(ParsedContent::Binary {
            data: bytes.to_vec(),
            metadata,
        })
    }
}

/// TMO Parser (Skeletal variant) - Texture/Transform Override
pub struct TMOSkelParser;

impl FileFormatParser for TMOSkelParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        let mut metadata = HashMap::new();
        metadata.insert("format".to_string(), "TMO Transform/Texture Override".to_string());
        metadata.insert("size".to_string(), format!("{} bytes", bytes.len()));

        if bytes.len() >= 4 {
            let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            metadata.insert("entry_count".to_string(), count.to_string());
        }

        Ok(ParsedContent::Binary {
            data: bytes.to_vec(),
            metadata,
        })
    }
}
