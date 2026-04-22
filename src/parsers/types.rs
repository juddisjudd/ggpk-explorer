use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Trait for parsing different file formats
pub trait FileFormatParser: Send + Sync {
    /// Parse raw bytes into structured output
    fn parse(&self, bytes: &[u8]) -> Result<ParsedContent, String>;
}

/// Normalized output from all format parsers
#[derive(Debug, Clone)]
pub enum ParsedContent {
    /// Plain text with optional syntax highlighting hint
    Text {
        content: String,
        language: Option<String>, // e.g., "hlsl", "python", etc.
    },
    /// Structured table data
    Table {
        rows: Vec<HashMap<String, String>>,
        columns: Vec<String>,
    },
    /// Hierarchical tree data (JSON-like)
    Tree(JsonValue),
    /// Binary data with metadata
    Binary {
        data: Vec<u8>,
        metadata: HashMap<String, String>,
    },
    /// Metadata-only (for formats without viewable content)
    Metadata(HashMap<String, String>),
}

impl ParsedContent {
    pub fn as_text(&self) -> Option<(&str, Option<&str>)> {
        match self {
            ParsedContent::Text { content, language } => Some((content, language.as_deref())),
            _ => None,
        }
    }

    pub fn as_tree(&self) -> Option<&JsonValue> {
        match self {
            ParsedContent::Tree(json) => Some(json),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<(&[HashMap<String, String>], &[String])> {
        match self {
            ParsedContent::Table { rows, columns } => Some((rows, columns)),
            _ => None,
        }
    }

    pub fn as_binary(&self) -> Option<(&[u8], &HashMap<String, String>)> {
        match self {
            ParsedContent::Binary { data, metadata } => Some((data, metadata)),
            _ => None,
        }
    }

    pub fn as_metadata(&self) -> Option<&HashMap<String, String>> {
        match self {
            ParsedContent::Metadata(meta) => Some(meta),
            _ => None,
        }
    }
}

/// File format types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileFormat {
    // Text/Config formats
    AMD,  // Animation/Mesh definitions
    AO,   // Abstract Object definitions
    ARM,  // Area Map
    ECF,  // Effect Configuration
    ET,   // Effect Template
    GFT,  // Generator File Table
    GT,   // Graphics Template
    MAT,  // Material
    PET,  // Particle Emitter Template
    TRL,  // Trail data
    TSI,  // Text String Index/Configuration
    TMO,  // Texture Override

    // Graphics/Binary formats
    FMT, // Format/Mesh (graphics)
    SMD, // Skeletal Mesh Data

    // Other formats (placeholder for future expansion)
    PSG, // Passive Skill Graph
    TST, // Text/String Table
    TOY, // Toy data
    DLP, // Dolm Primitives
    GCF, // Game Config File
    MTD, // Metadata

    // Fallback
    Unknown,
}

impl FileFormat {
    pub fn from_extension(ext: &str) -> Self {
        let ext_normalized = ext
            .rsplit('.')
            .next()
            .unwrap_or(ext)
            .trim_start_matches('.')
            .to_lowercase();

        match ext_normalized.as_str() {
            "amd" => FileFormat::AMD,
            "ao" => FileFormat::AO,
            "arm" => FileFormat::ARM,
            "ecf" => FileFormat::ECF,
            "et" => FileFormat::ET,
            "gft" => FileFormat::GFT,
            "gt" => FileFormat::GT,
            "mat" => FileFormat::MAT,
            "pet" => FileFormat::PET,
            "trl" => FileFormat::TRL,
            "tsi" => FileFormat::TSI,
            "tmo" => FileFormat::TMO,
            "fmt" => FileFormat::FMT,
            "smd" => FileFormat::SMD,
            "psg" => FileFormat::PSG,
            "tst" => FileFormat::TST,
            "toy" => FileFormat::TOY,
            "dlp" => FileFormat::DLP,
            "gcf" => FileFormat::GCF,
            "mtd" => FileFormat::MTD,
            _ => FileFormat::Unknown,
        }
    }

    pub fn is_text_format(&self) -> bool {
        matches!(
            self,
            FileFormat::AMD
                | FileFormat::AO
                | FileFormat::ARM
                | FileFormat::ECF
                | FileFormat::ET
                | FileFormat::GFT
                | FileFormat::GT
                | FileFormat::MAT
                | FileFormat::PET
                | FileFormat::TRL
                | FileFormat::TSI
                | FileFormat::TMO
        )
    }

    pub fn is_graphics_format(&self) -> bool {
        matches!(self, FileFormat::FMT | FileFormat::SMD)
    }
}
