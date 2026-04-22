use crate::parsers::{parse, FileFormat, ParsedContent};

/// Main adapter for parsing files
pub struct FileAdapter;

impl FileAdapter {
    /// Detect format from file extension and parse
    pub fn parse_file(extension: &str, bytes: &[u8]) -> Result<ParsedContent, String> {
        let format = FileFormat::from_extension(extension);
        parse(format, bytes)
    }

    /// Parse with explicit format
    pub fn parse_with_format(format: FileFormat, bytes: &[u8]) -> Result<ParsedContent, String> {
        parse(format, bytes)
    }

    /// Detect format from extension
    pub fn detect_format(extension: &str) -> FileFormat {
        FileFormat::from_extension(extension)
    }

    /// Check if format is text-based
    pub fn is_text_format(format: FileFormat) -> bool {
        format.is_text_format()
    }

    /// Check if format is graphics/binary
    pub fn is_graphics_format(format: FileFormat) -> bool {
        format.is_graphics_format()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_detection() {
        assert_eq!(FileAdapter::detect_format("amd"), FileFormat::AMD);
        assert_eq!(FileAdapter::detect_format("fmt"), FileFormat::FMT);
        assert_eq!(FileAdapter::detect_format("smd"), FileFormat::SMD);
    }

    #[test]
    fn test_format_classification() {
        assert!(FileAdapter::is_text_format(FileFormat::AMD));
        assert!(FileAdapter::is_graphics_format(FileFormat::FMT));
        assert!(!FileAdapter::is_graphics_format(FileFormat::AMD));
    }
}
