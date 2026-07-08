use serde::{Deserialize, Serialize};

use crate::parsers::utils::utf16_bom_to_string;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FxTextureSource {
    pub filename: String,
    #[serde(default)]
    pub src_mask: Option<String>,
    #[serde(default)]
    pub dest_mask: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FxTexture {
    pub filename: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub sources: Vec<FxTextureSource>,
    #[serde(default)]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FxNode {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub ui_position: Option<[f32; 2]>,
    #[serde(default)]
    pub custom_parameter: Option<String>,
    #[serde(default)]
    pub parameters: Vec<serde_json::Value>,
}

impl FxNode {
    /// Links reference nodes by this (type, index) pair, not array position.
    pub fn key(&self) -> (String, i64) {
        (self.node_type.clone(), self.index)
    }

    pub fn position(&self) -> [f32; 2] {
        self.ui_position.unwrap_or([0.0, 0.0])
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FxLinkEndpoint {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub variable: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub swizzle: Option<String>,
}

impl FxLinkEndpoint {
    pub fn key(&self) -> (String, i64) {
        (self.node_type.clone(), self.index)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FxLink {
    pub src: FxLinkEndpoint,
    pub dst: FxLinkEndpoint,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FxGraph {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub shader_group: Vec<String>,
    #[serde(default)]
    pub overriden_blend_mode: Option<String>,
    #[serde(default)]
    pub textures: Vec<FxTexture>,
    #[serde(default)]
    pub nodes: Vec<FxNode>,
    #[serde(default)]
    pub links: Vec<FxLink>,
}

/// `.fxgraph` files are UTF-16LE JSON (with BOM) describing a shader/particle
/// node graph: `nodes` carry an editor `ui_position`, and `links` wire named
/// ports between nodes identified by `(type, index)` rather than array index.
pub fn parse_fxgraph(bytes: &[u8]) -> Result<FxGraph, String> {
    let text = match utf16_bom_to_string(bytes) {
        Ok(s) => s,
        Err(_) => String::from_utf8_lossy(bytes).to_string(),
    };
    serde_json::from_str::<FxGraph>(&text).map_err(|e| format!("FxGraph JSON parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_fxgraph_minimal() {
        let json = r#"{
            "version": 3,
            "shader_group": ["Material"],
            "nodes": [
                { "type": "One", "index": 0, "ui_position": [0.0, 0.0] },
                { "type": "AlbedoColor", "index": 0, "ui_position": [200.0, 0.0] }
            ],
            "links": [
                { "src": { "type": "One", "index": 0, "variable": "output" },
                  "dst": { "type": "AlbedoColor", "index": 0, "variable": "input" } }
            ]
        }"#;
        let mut bytes = vec![0xFF, 0xFE];
        for u in json.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let graph = parse_fxgraph(&bytes).expect("should parse");
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.links.len(), 1);
        assert_eq!(graph.links[0].src.key(), ("One".to_string(), 0));
    }
}
