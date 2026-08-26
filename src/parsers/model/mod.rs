//! Structured parsers for binary geometry/skeleton formats: `.ast`, `.fmt`, `.tgm`, `.smd`.
//! Layouts follow poe_data_tools' FORMATS.md; that crate needs nightly so the
//! structs are ported rather than depended on.

pub mod ast;
pub mod cursor;
pub mod dolm;
pub mod fmt;
pub mod smd;
pub mod tgm;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ModelFile {
    Ast(ast::AstFile),
    Fmt(fmt::FmtFile),
    Tgm(tgm::TgmFile),
    Smd(smd::SmdFile),
}

pub fn model_kind(path: &str) -> Option<&'static str> {
    let p = path.to_ascii_lowercase();
    if p.ends_with(".ast") {
        Some("ast")
    } else if p.ends_with(".fmt") {
        Some("fmt")
    } else if p.ends_with(".tgm") {
        Some("tgm")
    } else if p.ends_with(".smd") {
        Some("smd")
    } else {
        None
    }
}

pub fn is_model_path(path: &str) -> bool {
    model_kind(path).is_some()
}

pub fn parse_model(path: &str, data: &[u8]) -> Result<ModelFile, String> {
    let kind = model_kind(path).ok_or_else(|| "not a model file".to_string())?;
    let res = match kind {
        "ast" => ast::parse_ast(data).map(ModelFile::Ast),
        "fmt" => fmt::parse_fmt(data).map(ModelFile::Fmt),
        "tgm" => tgm::parse_tgm(data).map(ModelFile::Tgm),
        "smd" => smd::parse_smd(data).map(ModelFile::Smd),
        _ => unreachable!(),
    };
    res.map_err(|e| e.to_string())
}

impl ModelFile {
    pub fn kind(&self) -> &'static str {
        match self {
            ModelFile::Ast(_) => "ast",
            ModelFile::Fmt(_) => "fmt",
            ModelFile::Tgm(_) => "tgm",
            ModelFile::Smd(_) => "smd",
        }
    }

    pub fn version(&self) -> u32 {
        match self {
            ModelFile::Ast(f) => f.header.version as u32,
            ModelFile::Fmt(f) => f.version as u32,
            ModelFile::Tgm(f) => f.version as u32,
            ModelFile::Smd(f) => f.version as u32,
        }
    }

    /// Everything except the vertex/index/keyframe buffers.
    pub fn summary(&self) -> serde_json::Value {
        match self {
            ModelFile::Ast(f) => f.summary(),
            ModelFile::Fmt(f) => f.summary(),
            ModelFile::Tgm(f) => f.summary(),
            ModelFile::Smd(f) => f.summary(),
        }
    }
}

#[cfg(test)]
mod real_data_tests {
    use super::*;
    use std::sync::Arc;

    /// Parses a sample of every model format straight out of the configured GGPK.
    /// Needs the app to have been run once (index cache + settings).
    /// `cargo test --release -- --ignored parse_real_game_models --nocapture`
    #[test]
    #[ignore]
    fn parse_real_game_models() {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(crate::ggpk::reader::GgpkReader::open(&ggpk_path).unwrap());
        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = crate::bundles::index::Index::load_from_cache(&cache_path).expect("run the app once to build the index cache");

        let per_ext: usize = std::env::var("MODEL_SAMPLE").ok().and_then(|s| s.parse().ok()).unwrap_or(400);
        let mut total_fail = 0usize;
        let mut total = 0usize;
        for ext in ["ast", "fmt", "tgm", "smd"] {
            let suffix = format!(".{}", ext);
            let mut files: Vec<_> = index.files.values().filter(|f| f.path.ends_with(&suffix)).collect();
            files.sort_by(|a, b| a.path.cmp(&b.path));
            // Spread the sample across the whole list rather than the first N alphabetically.
            let step = (files.len() / per_ext).max(1);
            let sample: Vec<_> = files.iter().step_by(step).take(per_ext).collect();
            let mut fails = Vec::new();
            let mut versions = std::collections::BTreeMap::new();
            let (mut anims, mut anims_decoded, mut bundles, mut bundles_decoded) = (0usize, 0usize, 0usize, 0usize);
            for fi in &sample {
                let Some(bytes) = crate::ui::content_view::extract_bundle_file_sync(fi, &index, Some(&reader), None) else {
                    continue;
                };
                match parse_model(&fi.path, &bytes) {
                    Ok(m) => {
                        *versions.entry(m.version()).or_insert(0usize) += 1;
                        if let ModelFile::Ast(a) = &m {
                            anims += a.animations.len();
                            anims_decoded += a.animations.iter().filter(|x| x.tracks.is_some()).count();
                            if let Some(b) = &a.bundle {
                                bundles += 1;
                                bundles_decoded += b.decoded as usize;
                            }
                        }
                    }
                    Err(e) => fails.push(format!("{} ({} bytes): {}", fi.path, bytes.len(), e)),
                }
            }
            println!("{}: {} sampled of {}, {} failed, versions {:?}", ext, sample.len(), files.len(), fails.len(), versions);
            if ext == "ast" {
                println!("   animations {} (tracks decoded {}), embedded bundles {} (decoded {})", anims, anims_decoded, bundles, bundles_decoded);
            }
            for f in fails.iter().take(10) {
                println!("   FAIL {}", f);
            }
            total_fail += fails.len();
            total += sample.len();
        }
        assert!(total > 0, "no model files found in index");
        assert!(
            total_fail * 100 <= total,
            "{} of {} sampled model files failed to parse",
            total_fail,
            total
        );
    }
}
