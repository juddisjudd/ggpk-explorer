//! Game files that have a parser, written out as JSON: what the raw export's `--parsed json` does to each file.

use super::utils::{decode_text_lossy, split_json_body};

enum Kind {
    Model,
    FxGraph,
    Object,
    Dgr,
    Trl,
    Pet,
    JsonBody,
    Arm,
    Sm,
    TextFormat,
}

fn kind(path: &str) -> Option<Kind> {
    let lower = path.to_ascii_lowercase();
    if super::model::is_model_path(&lower) {
        return Some(Kind::Model);
    }
    if super::object_dsl::is_object_path(&lower) {
        return Some(Kind::Object);
    }
    match lower.rsplit_once('.')?.1 {
        ext if super::text_formats::EXTENSIONS.contains(&ext) => Some(Kind::TextFormat),
        "fxgraph" => Some(Kind::FxGraph),
        "dgr" => Some(Kind::Dgr),
        "trl" => Some(Kind::Trl),
        "pet" => Some(Kind::Pet),
        "mat" | "atl" | "env" => Some(Kind::JsonBody),
        "arm" => Some(Kind::Arm),
        "sm" => Some(Kind::Sm),
        _ => None,
    }
}

pub fn handles(path: &str) -> bool {
    kind(path).is_some()
}

/// The file as JSON text, or `None` when no parser reads this kind of file.
pub fn to_json(path: &str, bytes: &[u8]) -> Option<Result<String, String>> {
    let text = || decode_text_lossy(bytes);
    Some(match kind(path)? {
        Kind::Model => super::model::parse_model(path, bytes).and_then(|m| pretty(&m)),
        Kind::FxGraph => super::fxgraph::parse_fxgraph(bytes).and_then(|g| pretty(&g)),
        Kind::Object => pretty(&super::object_dsl::parse(&text())),
        Kind::Dgr => super::level::parse_dgr(&text()).and_then(|g| pretty(&g)),
        Kind::Trl => pretty(&super::curves::parse(&text())),
        Kind::Pet => {
            let text = text();
            json_document(&text).unwrap_or_else(|| pretty(&super::curves::parse(&text)))
        }
        Kind::JsonBody => json_document(&text()).unwrap_or_else(|| Err("no JSON document after the header".to_string())),
        Kind::Arm => super::arm::parse(&text()).and_then(|f| pretty(&f)),
        Kind::Sm => super::sm::parse(&text()).and_then(|f| pretty(&f)),
        Kind::TextFormat => {
            let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
            super::text_formats::to_json(ext, &text()).unwrap_or_else(|| Err(format!("no parser for .{}", ext)))
        }
    })
}

fn pretty<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| e.to_string())
}

/// A JSON-bodied file kept verbatim so its key order survives; a header goes beside it as `header`.
fn json_document(text: &str) -> Option<Result<String, String>> {
    let (header, body) = split_json_body(text)?;
    serde_json::from_str::<serde::de::IgnoredAny>(body).ok()?;
    let body = body.trim();
    Some(Ok(match header.is_empty() {
        true => body.to_string(),
        false => format!("{{\"header\": {}, \"document\": {}}}", serde_json::to_string(&header).unwrap_or_default(), body),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        [0xFF, 0xFE].into_iter().chain(s.encode_utf16().flat_map(|u| u.to_le_bytes())).collect()
    }

    #[test]
    fn json_bodies_keep_their_key_order_and_header() {
        let out = to_json("Metadata/Effects/a.pet", &utf16("version 5\r\n{\"z\": 1, \"a\": 2}")).unwrap().unwrap();
        assert_eq!(out, r#"{"header": ["version 5"], "document": {"z": 1, "a": 2}}"#);
        let mat = to_json("Art/a.mat", &utf16("{\"b\": [1]}\r\n")).unwrap().unwrap();
        assert_eq!(mat, r#"{"b": [1]}"#);
    }

    /// `cargo test --release translate_real_files -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn translate_real_files() {
        let mut worst = 1.0f32;
        for ext in ["ao", "ot", "it", "epk", "fmt", "tgm", "smd", "ast", "dgr", "trl", "pet", "mat", "atl", "env", "fxgraph", "arm", "sm", "tgt", "et", "tst", "amd"] {
            let files = crate::parsers::real_files(ext, 100);
            let failed: Vec<String> = files
                .iter()
                .filter_map(|(path, bytes)| match to_json(path, bytes)? {
                    Ok(_) => None,
                    Err(e) => Some(format!("{}: {}", path, e)),
                })
                .collect();
            println!("{:<8} {:>3}/{:<3} {}", ext, files.len() - failed.len(), files.len(), failed.iter().take(2).cloned().collect::<Vec<_>>().join(" | "));
            if !files.is_empty() {
                worst = worst.min(1.0 - failed.len() as f32 / files.len() as f32);
            }
        }
        assert!(worst >= 0.7, "an extension translated under 70% of its sample");
    }

    #[test]
    fn only_formats_with_a_parser_translate() {
        assert!(to_json("Art/a.dds", b"DDS ").is_none());
        assert!(handles("Metadata/Monsters/a.ao"));
        let ao = to_json("Metadata/a.ao", &utf16("version 2\r\nextends \"Metadata/Parent\"\r\n")).unwrap().unwrap();
        assert!(ao.contains("Metadata/Parent"), "{}", ao);
    }
}
