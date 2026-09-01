//! `.dgr` dungeon graphs: a `Size W H` grid of room nodes joined by edges that name
//! the `.et` room set each connection draws from.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DgrNode {
    pub x: f32,
    pub y: f32,
    pub tile_refs: Vec<i32>,
    pub name: String,
    pub rotation: String,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DgrEdge {
    pub from: usize,
    pub to: usize,
    pub path: String,
    pub raw: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DgrGraph {
    pub version: Option<u32>,
    pub width: u32,
    pub height: u32,
    pub master: String,
    pub nodes: Vec<DgrNode>,
    pub edges: Vec<DgrEdge>,
}

/// Node coordinates sit at `12 + 23·i`; this is that cell pitch.
pub const CELL_PITCH: f32 = 23.0;

fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        if let Some(s) = rest.strip_prefix('"') {
            let end = s.find('"').unwrap_or(s.len());
            out.push(format!("\"{}\"", &s[..end]));
            rest = s[end..].strip_prefix('"').unwrap_or(&s[end..]).trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push(rest[..end].to_string());
            rest = rest[end..].trim_start();
        }
    }
    out
}

fn quoted(t: &str) -> Option<&str> {
    t.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
}

pub fn parse_dgr(text: &str) -> Result<DgrGraph, String> {
    let mut g = DgrGraph::default();
    let all: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let (mut n_nodes, mut n_edges) = (0usize, 0usize);
    let mut pos = 0;
    while pos < all.len() {
        let line = all[pos];
        pos += 1;
        if let Some(v) = line.strip_prefix("version ") {
            g.version = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("Size:") {
            let mut it = v.split_whitespace();
            g.width = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            g.height = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("MasterFile:") {
            g.master = v.trim().trim_matches('"').to_string();
        } else if let Some(v) = line.strip_prefix("Nodes:") {
            n_nodes = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Edges:") {
            n_edges = v.trim().parse().unwrap_or(0);
        } else if line.starts_with("Default%") {
            // Version 25 puts one more counter line here; version 21 starts the nodes directly.
            if all.get(pos).map(|l| l.parse::<i64>().is_ok()).unwrap_or(false) {
                pos += 1;
            }
            break;
        }
    }
    if g.width == 0 || n_nodes == 0 {
        return Err("no Size/Nodes header".into());
    }
    let mut lines = all[pos.min(all.len())..].iter().copied();
    for line in lines.by_ref().take(n_nodes) {
        let t = tokens(line);
        let num = |i: usize| t.get(i).and_then(|s| s.parse::<f32>().ok());
        let (Some(x), Some(y)) = (num(0), num(1)) else { continue };
        let count = t.get(2).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
        let tile_refs: Vec<i32> = (0..count).filter_map(|k| t.get(3 + k).and_then(|s| s.parse().ok())).collect();
        let name_at = t.iter().position(|s| s.starts_with('"'));
        let name = name_at.and_then(|i| quoted(&t[i])).unwrap_or("").to_string();
        let rotation = name_at.and_then(|i| t.get(i + 1)).cloned().unwrap_or_default();
        g.nodes.push(DgrNode { x, y, tile_refs, name, rotation, raw: line.to_string() });
    }
    for line in lines.take(n_edges) {
        let t = tokens(line);
        let (Some(from), Some(to)) = (t.first().and_then(|s| s.parse().ok()), t.get(1).and_then(|s| s.parse().ok())) else { continue };
        let path = t.iter().filter_map(|s| quoted(s)).find(|q| q.contains('/')).unwrap_or("").to_string();
        g.edges.push(DgrEdge { from, to, path, raw: line.to_string() });
    }
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nodes_and_edges() {
        let text = "version 25\nSize: 5 5\nMasterFile: \"Metadata/T/master.tsi\"\nNodes: 2\nEdges: 1\n\"\"\n\"\"\n\"\"\nDefault%: 0 0 0\n0\n81 12 1 3 \"entrance\" FR270 2 \"default\" \"entrance1\" 100 0 0 0 0 0 0 0 0 N 0\n58 58 3 3 2 0 \"\" (any) 0 100 0 0 0 0 0 0 0 0 N 1\n0 1 0 100 0 0 \"Metadata/Terrain/Dungeon/rooms.et\" 0 25 0 \"\" 100 \"\" 0 0 P N 1\n";
        let g = parse_dgr(text).unwrap();
        assert_eq!((g.width, g.height), (5, 5));
        assert_eq!(g.master, "Metadata/T/master.tsi");
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].name, "entrance");
        assert_eq!(g.nodes[0].rotation, "FR270");
        assert_eq!(g.nodes[1].tile_refs, vec![3, 2, 0]);
        assert_eq!(g.nodes[1].rotation, "(any)");
        assert_eq!(g.edges[0].to, 1);
        assert_eq!(g.edges[0].path, "Metadata/Terrain/Dungeon/rooms.et");
        assert!(parse_dgr("nothing").is_err());
    }

    #[test]
    fn parses_version_21_without_counter_line() {
        let text = "version 21\nSize: 9 9\nMasterFile: \"Metadata/T/master.tsi\"\nNodes: 2\nEdges: 0\n\"\"\n\"\"\n\"\"\nDefault%: 0 0 0\n8 9 0 \"hideout\" I 1 \"telepad1\" 100 0 N\n101 101 0 \"swamp\" I 1 \"telepad2\" 100 0 N\n";
        let g = parse_dgr(text).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[1].name, "swamp");
        assert!(g.nodes[0].tile_refs.is_empty());
    }
}
