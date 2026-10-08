pub mod fmod_bank;
pub mod fxgraph;
pub mod model;
pub mod object_dsl;
pub mod curves;
pub mod level;
pub mod arm;
pub mod sm;
pub mod graphics;
pub mod skeletal;
pub mod text_config;
pub mod text_formats;
pub mod translate;
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

/// The configured install's GGPK and bundle index, for `#[ignore]`d real-data tests.
#[cfg(test)]
pub fn real_source() -> (crate::ggpk::reader::GgpkReader, crate::bundles::index::Index) {
    let settings = crate::settings::AppSettings::load();
    let reader = crate::ggpk::reader::GgpkReader::open(settings.ggpk_path.expect("no ggpk_path configured")).unwrap();
    let cache = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
    // A CLI run on a new patch deletes the cache, and only the GUI writes it back.
    let index = crate::bundles::index::Index::load_from_cache(&cache).unwrap_or_else(|_| {
        let record = reader.read_file_by_path("Bundles2/_.index.bin").unwrap().expect("the GGPK has no bundle index");
        crate::cli::read_index_bundle(reader.get_data_slice(record.data_offset, record.data_length).unwrap()).unwrap()
    });
    (reader, index)
}

/// Up to `n` files with extension `ext`, spread across the configured install's index, for `#[ignore]`d real-data tests.
#[cfg(test)]
pub fn real_files(ext: &str, n: usize) -> Vec<(String, Vec<u8>)> {
    let (reader, index) = real_source();
    let suffix = format!(".{}", ext.to_ascii_lowercase());
    let mut files: Vec<_> = index.files.values().filter(|f| f.path.to_ascii_lowercase().ends_with(&suffix)).collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let step = (files.len() / n.max(1)).max(1);
    files
        .iter()
        .step_by(step)
        .take(n)
        .filter_map(|f| {
            crate::bundles::extract::extract_bundle_file_sync(f, &index, Some(&reader), None).map(|b| (f.path.clone(), b))
        })
        .collect()
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
