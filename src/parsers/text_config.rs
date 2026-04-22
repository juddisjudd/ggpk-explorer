use crate::parsers::types::{FileFormatParser, ParsedContent};
use crate::parsers::utils::*;
use serde_json::{Map, Value};

/// Parser for text-based config formats (AMD, AO, ARM, etc.)
pub struct TextConfigParser;

impl FileFormatParser for TextConfigParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        // Try UTF-16 with BOM first (most common for PoE formats)
        match utf16_bom_to_string(bytes) {
            Ok(content) => {
                let content = remove_trailing_commas(&content);
                Ok(ParsedContent::Text {
                    content,
                    language: Some("text".to_string()),
                })
            }
            Err(_) => {
                // Fallback to UTF-8
                match String::from_utf8(bytes.to_vec()) {
                    Ok(content) => Ok(ParsedContent::Text {
                        content,
                        language: Some("text".to_string()),
                    }),
                    Err(e) => Err(format!("Failed to decode text: {}", e)),
                }
            }
        }
    }
}

/// AMD Parser - Animation/Mesh definitions
pub struct AMDParser;

impl FileFormatParser for AMDParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content_str) => {
                // Extract metadata (version and group count) for future use
                let _first_line = content_str.lines().next();
                let _group_count = content_str
                    .lines()
                    .filter(|line: &&str| line.trim().starts_with('"'))
                    .count();

                Ok(ParsedContent::Text {
                    content: content_str,
                    language: Some("text".to_string()),
                })
            }
            Err(e) => Err(format!("AMD parse error: {}", e)),
        }
    }
}

/// AO Parser - Abstract Object definitions
pub struct AOParser;

impl FileFormatParser for AOParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content_str) => {
                let mut tree = Map::new();

                // Simple parsing: extract key-value pairs
                for line in content_str.lines() {
                    let line_trimmed = line.trim();
                    if line_trimmed.is_empty() || line_trimmed.starts_with("//") {
                        continue;
                    }

                    if let Some(eq_pos) = line_trimmed.find('=') {
                        let key = line_trimmed[..eq_pos].trim();
                        let value = line_trimmed[eq_pos + 1..].trim();
                        tree.insert(key.to_string(), Value::String(value.to_string()));
                    }
                }

                Ok(ParsedContent::Tree(Value::Object(tree)))
            }
            Err(e) => Err(format!("AO parse error: {}", e)),
        }
    }
}

/// ARM Parser - Area Map
pub struct ARMParser;

impl FileFormatParser for ARMParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content) => Ok(ParsedContent::Text {
                content,
                language: Some("text".to_string()),
            }),
            Err(e) => Err(format!("ARM parse error: {}", e)),
        }
    }
}

/// MAT Parser - Material definitions
pub struct MATParser;

impl FileFormatParser for MATParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content) => Ok(ParsedContent::Text {
                content,
                language: Some("text".to_string()),
            }),
            Err(e) => Err(format!("MAT parse error: {}", e)),
        }
    }
}

/// PET Parser - Particle Emitter Template
pub struct PETParser;

impl FileFormatParser for PETParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content) => Ok(ParsedContent::Text {
                content,
                language: Some("text".to_string()),
            }),
            Err(e) => Err(format!("PET parse error: {}", e)),
        }
    }
}

/// ET Parser - Effect Template
pub struct ETParser;

impl FileFormatParser for ETParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content) => Ok(ParsedContent::Text {
                content,
                language: Some("text".to_string()),
            }),
            Err(e) => Err(format!("ET parse error: {}", e)),
        }
    }
}

/// TRL Parser - Trail data
pub struct TRLParser;

impl FileFormatParser for TRLParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content) => Ok(ParsedContent::Text {
                content,
                language: Some("text".to_string()),
            }),
            Err(e) => Err(format!("TRL parse error: {}", e)),
        }
    }
}

/// TSI Parser - Text String Index / Configuration
pub struct TSIParser;

impl FileFormatParser for TSIParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content_str) => {
                let mut tree = Map::new();

                for line in content_str.lines() {
                    let line_trimmed = line.trim();
                    if line_trimmed.is_empty() || line_trimmed.starts_with("//") {
                        continue;
                    }

                    if let Some(eq_pos) = line_trimmed.find('=') {
                        let key = line_trimmed[..eq_pos].trim();
                        let value = line_trimmed[eq_pos + 1..].trim();
                        tree.insert(key.to_string(), Value::String(value.to_string()));
                    }
                }

                Ok(ParsedContent::Tree(Value::Object(tree)))
            }
            Err(e) => Err(format!("TSI parse error: {}", e)),
        }
    }
}

/// GFT Parser - Generator File Table
pub struct GFTParser;

impl FileFormatParser for GFTParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content_str) => {
                let mut sections = Vec::new();
                let mut current_section: Option<Map<String, Value>> = None;

                for line in content_str.lines() {
                    let line_trimmed = line.trim();
                    if line_trimmed.is_empty() || line_trimmed.starts_with("//") {
                        continue;
                    }

                    if line_trimmed.starts_with("Version") {
                        if let Some(section) = current_section.take() {
                            sections.push(Value::Object(section));
                        }
                        current_section = Some(Map::new());
                        if let Some(ref mut section) = current_section {
                            section.insert("type".to_string(), Value::String("Version".to_string()));
                            section.insert(
                                "content".to_string(),
                                Value::String(line_trimmed.to_string()),
                            );
                        }
                    } else {
                        if current_section.is_none() {
                            current_section = Some(Map::new());
                        }
                        if let Some(ref mut section) = current_section {
                            section.insert(
                                "line".to_string(),
                                Value::String(line_trimmed.to_string()),
                            );
                        }
                    }
                }

                if let Some(section) = current_section {
                    sections.push(Value::Object(section));
                }

                Ok(ParsedContent::Tree(Value::Array(sections)))
            }
            Err(e) => Err(format!("GFT parse error: {}", e)),
        }
    }
}

/// GT Parser - Graphics Template
pub struct GTParser;

impl FileFormatParser for GTParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content) => Ok(ParsedContent::Text {
                content,
                language: Some("text".to_string()),
            }),
            Err(e) => Err(format!("GT parse error: {}", e)),
        }
    }
}

/// ECF Parser - Effect Configuration
pub struct ECFParser;

impl FileFormatParser for ECFParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content) => Ok(ParsedContent::Text {
                content,
                language: Some("text".to_string()),
            }),
            Err(e) => Err(format!("ECF parse error: {}", e)),
        }
    }
}

/// TMO Parser - Texture Override
pub struct TMOParser;

impl FileFormatParser for TMOParser {
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String> {
        match utf16_bom_to_string(bytes) {
            Ok(content) => Ok(ParsedContent::Text {
                content,
                language: Some("text".to_string()),
            }),
            Err(e) => Err(format!("TMO parse error: {}", e)),
        }
    }
}
