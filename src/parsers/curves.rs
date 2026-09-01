//! Keyframed and sampled curves in `.trl` trails and keyword-form `.pet` emitters:
//! `color_r 2 0 1 Linear 1 1 Linear` (keyframes `t v Interp`) and
//! `opacity 0 0.5 0.38 0.26 0` (evenly spaced samples).

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Curve {
    pub key: String,
    pub points: Vec<(f32, f32)>,
    pub keyframed: bool,
    /// Every sample has the same value.
    pub constant: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CurveBlock {
    /// Bare word that opens the block, e.g. `PointEmitter`.
    pub title: String,
    pub props: Vec<(String, String)>,
    pub curves: Vec<Curve>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CurveFile {
    pub version: Option<u32>,
    pub blocks: Vec<CurveBlock>,
}

const INTERP: &[&str] = &["Linear", "Smooth", "Constant", "Bezier", "Step", "Ease", "EaseIn", "EaseOut", "EaseInOut", "Spline", "Cubic"];

fn is_interp(tok: &str) -> bool {
    INTERP.iter().any(|i| i.eq_ignore_ascii_case(tok))
}

/// Interprets the tokens after a key as a curve, or `None` when they are a plain value.
pub fn parse_curve(key: &str, rest: &[&str]) -> Option<Curve> {
    let toks: Vec<&str> = rest.iter().map(|t| t.trim_matches('"')).collect();
    if toks.iter().any(|t| is_interp(t)) {
        let mut points = Vec::new();
        for (i, t) in toks.iter().enumerate() {
            if is_interp(t) && i >= 2 {
                if let (Ok(x), Ok(y)) = (toks[i - 2].parse::<f32>(), toks[i - 1].parse::<f32>()) {
                    points.push((x, y));
                }
            }
        }
        if points.len() >= 2 {
            let constant = points.iter().all(|p| (p.1 - points[0].1).abs() < 1e-6);
            return Some(Curve { key: key.to_string(), points, keyframed: true, constant });
        }
        return None;
    }
    let nums: Vec<f32> = toks.iter().filter_map(|t| t.parse::<f32>().ok()).collect();
    if nums.len() != toks.len() || nums.len() < 3 {
        return None;
    }
    let n = nums.len();
    let points: Vec<(f32, f32)> = nums.iter().enumerate().map(|(i, v)| (i as f32 / (n - 1) as f32, *v)).collect();
    let constant = nums.iter().all(|v| (v - nums[0]).abs() < 1e-6);
    Some(Curve { key: key.to_string(), points, keyframed: false, constant })
}

fn split_tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(stripped) = rest.strip_prefix('"') {
            let end = stripped.find('"').unwrap_or(stripped.len());
            out.push(&rest[..end + 2.min(rest.len())]);
            rest = &stripped[end.min(stripped.len())..];
            rest = rest.strip_prefix('"').unwrap_or(rest);
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push(&rest[..end]);
            rest = &rest[end..];
        }
    }
    out
}

pub fn parse(text: &str) -> CurveFile {
    let mut file = CurveFile::default();
    let mut current: Option<CurveBlock> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line == "{" {
            if let Some(b) = current.take() {
                file.blocks.push(b);
            }
            current = Some(CurveBlock::default());
            continue;
        }
        if line == "}" {
            if let Some(b) = current.take() {
                file.blocks.push(b);
            }
            continue;
        }
        let toks = split_tokens(line);
        let Some(&key) = toks.first() else { continue };
        let rest = &toks[1..];
        let Some(block) = current.as_mut() else {
            if key == "version" {
                file.version = rest.first().and_then(|v| v.parse().ok());
            }
            continue;
        };
        if rest.is_empty() {
            if key.starts_with('"') {
                block.props.push(("material".into(), key.trim_matches('"').to_string()));
            } else if block.title.is_empty() && key.parse::<f64>().is_err() {
                block.title = key.to_string();
            } else {
                block.props.push((key.to_string(), String::new()));
            }
            continue;
        }
        match parse_curve(key, rest) {
            Some(c) => block.curves.push(c),
            None => block.props.push((key.to_string(), rest.iter().map(|t| t.trim_matches('"')).collect::<Vec<_>>().join(" "))),
        }
    }
    if let Some(b) = current.take() {
        file.blocks.push(b);
    }
    file
}

pub fn is_curve_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".trl") || p.ends_with(".pet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_and_samples() {
        let k = parse_curve("color_r", &["2", "0", "1", "Linear", "1", "0.5", "Linear"]).unwrap();
        assert!(k.keyframed);
        assert_eq!(k.points, vec![(0.0, 1.0), (1.0, 0.5)]);
        let quoted = parse_curve("stretch", &["1", "2", "0", "1", "\"Linear\"", "1", "1", "\"Linear\""]).unwrap();
        assert_eq!(quoted.points, vec![(0.0, 1.0), (1.0, 1.0)]);
        assert!(quoted.constant);
        let s = parse_curve("opacity", &["0", "0.5", "1"]).unwrap();
        assert!(!s.keyframed);
        assert_eq!(s.points, vec![(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]);
        assert!(parse_curve("particle_offset", &["0", "0"]).is_none());
        assert!(parse_curve("blend_type", &["AlphaBlend"]).is_none());
    }

    #[test]
    fn parses_trail_blocks() {
        let f = parse("3\nversion 4\n{\n\tmaterial \"Art/p/main_trail.mat\"\n\tmax_segment_length 40\n\tcolor_a 2 0 1 Linear 1 1 Linear\n\topacity 0 0.5 0.3 0\n}\n{\n\t\"Art/p/x.mat\"\n\tPointEmitter\n\tscale 1 1 1\n}\n");
        assert_eq!(f.version, Some(4));
        assert_eq!(f.blocks.len(), 2);
        assert_eq!(f.blocks[0].props[0], ("material".to_string(), "Art/p/main_trail.mat".to_string()));
        assert_eq!(f.blocks[0].props[1].0, "max_segment_length");
        assert_eq!(f.blocks[0].curves.len(), 2);
        assert_eq!(f.blocks[1].props[0].1, "Art/p/x.mat");
        assert_eq!(f.blocks[1].title, "PointEmitter");
        assert!(f.blocks[1].curves[0].constant);
    }
}
