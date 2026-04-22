pub mod graphics;
pub mod skeletal;
pub mod text_config;
pub mod types;
pub mod utils;

pub use types::{FileFormat, FileFormatParser, ParsedContent};

use graphics::*;
use skeletal::*;
use text_config::*;

/// Get the appropriate parser for a file format
pub fn get_parser(format: FileFormat) -> Box<dyn FileFormatParser> {
    match format {
        // Text/Config formats
        FileFormat::AMD => Box::new(AMDParser),
        FileFormat::AO => Box::new(AOParser),
        FileFormat::ARM => Box::new(ARMParser),
        FileFormat::MAT => Box::new(MATParser),
        FileFormat::PET => Box::new(PETParser),
        FileFormat::ET => Box::new(ETParser),
        FileFormat::TRL => Box::new(TRLParser),
        FileFormat::TSI => Box::new(TSIParser),
        FileFormat::GFT => Box::new(GFTParser),
        FileFormat::GT => Box::new(GTParser),
        FileFormat::ECF => Box::new(ECFParser),
        FileFormat::TMO => Box::new(TMOParser),

        // Graphics/Binary formats
        FileFormat::FMT => Box::new(FMTParser),
        FileFormat::SMD => Box::new(SMDParser),

        // Placeholder parsers for other formats
        FileFormat::PSG => Box::new(GraphicsParser), // Placeholder
        FileFormat::TST => Box::new(TextConfigParser), // Placeholder
        FileFormat::TOY => Box::new(GraphicsParser), // Placeholder
        FileFormat::DLP => Box::new(GraphicsParser), // Placeholder
        FileFormat::GCF => Box::new(TextConfigParser), // Placeholder
        FileFormat::MTD => Box::new(TextConfigParser), // Placeholder

        FileFormat::Unknown => Box::new(GraphicsParser), // Fallback to generic binary parser
    }
}

/// Parse bytes into structured content based on file format
pub fn parse(format: FileFormat, bytes: &[u8]) -> Result<ParsedContent, String> {
    let parser = get_parser(format);
    parser.parse(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_selection() {
        // UTF-16 LE BOM + "a=b\n"
        let bytes = [0xFF, 0xFE, b'a', 0, b'=', 0, b'b', 0, b'\n', 0];
        let parsed = parse(FileFormat::AMD, &bytes).expect("AMD parser should parse UTF-16 text");
        assert!(matches!(parsed, ParsedContent::Text { .. }));
    }

    #[test]
    fn test_file_format_detection() {
        assert_eq!(FileFormat::from_extension("amd"), FileFormat::AMD);
        assert_eq!(FileFormat::from_extension("AMD"), FileFormat::AMD);
        assert_eq!(FileFormat::from_extension("fmt"), FileFormat::FMT);
        assert_eq!(FileFormat::from_extension("smd"), FileFormat::SMD);
        assert_eq!(FileFormat::from_extension("unknown"), FileFormat::Unknown);
    }
}
