//! `.fmt` static mesh files. Layout ported from poe_data_tools (`file_parsers/fmt`).

use super::cursor::{Cur, PResult};
use super::dolm::{bbox_json, parse_dolm, read_index_buffer, Dolm, IndexBuffer};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Subcomponent {
    pub unk1: u8,
    pub d1s: Vec<Vec<u8>>,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct V8Vertex {
    pub pos: [f32; 3],
    pub unk: [u8; 8],
    pub uv: [f32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uv2: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct V8Section {
    pub vertex_format: Option<u32>,
    pub index_buffer: IndexBuffer,
    pub vertex_buffer: Vec<V8Vertex>,
}

#[derive(Debug, Clone, Serialize)]
pub enum Section {
    V8(V8Section),
    V9(Dolm),
}

#[derive(Debug, Clone, Serialize)]
pub struct Shape {
    pub name: String,
    pub material: String,
    pub triangle_start: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FmtFile {
    pub version: u8,
    pub bbox: [f32; 6],
    pub section: Section,
    pub shapes: Vec<Shape>,
    pub subcomponents: Vec<Subcomponent>,
    pub d3s: Vec<Vec<u8>>,
    pub string_table: String,
}

struct UnresolvedShape {
    name: u32,
    material: u32,
    triangle_index: u32,
}

struct UnresolvedSub {
    unk1: u8,
    num_d1s: u8,
    tag: u32,
}

struct Header {
    num_t_v: Option<(u32, u32)>,
    num_shapes: u16,
    num_subcomponents: u8,
    num_d3s: u8,
}

fn read_string_table(cur: &mut Cur) -> PResult<Vec<u16>> {
    let n = cur.u32()? as usize;
    cur.check_count(n, 2)?;
    (0..n).map(|_| cur.u16()).collect()
}

fn table_string(table: &[u16], offset: usize) -> PResult<String> {
    if offset >= table.len() {
        return Err(super::cursor::ParseError { msg: format!("string table index {} out of bounds ({})", offset, table.len()), offset: 0 });
    }
    let end = table[offset..].iter().position(|&c| c == 0).map(|p| offset + p).unwrap_or(table.len());
    Ok(String::from_utf16_lossy(&table[offset..end]))
}

fn read_subs(cur: &mut Cur, n: usize) -> PResult<Vec<UnresolvedSub>> {
    (0..n).map(|_| Ok(UnresolvedSub { unk1: cur.u8()?, num_d1s: cur.u8()?, tag: cur.u32()? })).collect()
}

pub fn parse_fmt(data: &[u8]) -> PResult<FmtFile> {
    let mut cur = Cur::new(data);
    let version = cur.u8()?;
    let header = Header {
        num_t_v: if version < 9 { Some((cur.u32()?, cur.u32()?)) } else { None },
        num_shapes: cur.u16()?,
        num_subcomponents: cur.u8()?,
        num_d3s: {
            let _num_d1s = cur.u16()?;
            cur.u8()?
        },
    };
    let bbox = cur.f32s::<6>()?;

    let (section, shapes, subs) = if version < 9 {
        let (num_triangles, num_vertices) = header.num_t_v.unwrap();
        let vertex_format = if version >= 8 { Some(cur.u32()?) } else { None };
        let mut shapes = Vec::with_capacity(header.num_shapes as usize);
        for _ in 0..header.num_shapes {
            let [name, material, triangle_index] = cur.u32s::<3>()?;
            shapes.push(UnresolvedShape { name, material, triangle_index });
        }
        let subs = read_subs(&mut cur, header.num_subcomponents as usize)?;
        let index_buffer = read_index_buffer(&mut cur, num_vertices, num_triangles)?;
        cur.check_count(num_vertices as usize, 24)?;
        let mut vertex_buffer = Vec::with_capacity(num_vertices as usize);
        for _ in 0..num_vertices {
            vertex_buffer.push(V8Vertex {
                pos: cur.f32s::<3>()?,
                unk: cur.arr::<8>()?,
                uv: cur.f16s::<2>()?,
                uv2: if vertex_format == Some(1) { Some(cur.f16s::<2>()?) } else { None },
            });
        }
        (Section::V8(V8Section { vertex_format, index_buffer, vertex_buffer }), shapes, subs)
    } else {
        let dolm = parse_dolm(&mut cur)?;
        let mut shapes = Vec::with_capacity(header.num_shapes as usize);
        for _ in 0..header.num_shapes {
            let [name, material] = cur.u32s::<2>()?;
            shapes.push(UnresolvedShape { name, material, triangle_index: 0 });
        }
        let subs = read_subs(&mut cur, header.num_subcomponents as usize)?;
        (Section::V9(dolm), shapes, subs)
    };

    let mut d1s = Vec::with_capacity(subs.len());
    for s in &subs {
        let mut v = Vec::with_capacity(s.num_d1s as usize);
        for _ in 0..s.num_d1s {
            v.push(cur.bytes(12)?);
        }
        d1s.push(v);
    }

    let d3_width = match version {
        0..=2 => 45,
        3 => 70,
        4 | 5 => 78,
        6 => 83,
        _ => 87,
    };
    let mut d3s = Vec::with_capacity(header.num_d3s as usize);
    for _ in 0..header.num_d3s {
        d3s.push(cur.bytes(d3_width)?);
    }
    let table = read_string_table(&mut cur)?;
    if !cur.at_end() {
        return Err(cur.err(format!("{} trailing bytes after string table", cur.remaining())));
    }

    let shapes = shapes
        .into_iter()
        .map(|s| {
            Ok(Shape {
                name: table_string(&table, s.name as usize)?,
                material: table_string(&table, s.material as usize)?,
                triangle_start: s.triangle_index,
            })
        })
        .collect::<PResult<Vec<_>>>()?;
    let subcomponents = subs
        .into_iter()
        .zip(d1s)
        .map(|(s, d1s)| Ok(Subcomponent { unk1: s.unk1, d1s, tag: table_string(&table, s.tag as usize)? }))
        .collect::<PResult<Vec<_>>>()?;

    Ok(FmtFile { version, bbox, section, shapes, subcomponents, d3s, string_table: String::from_utf16_lossy(&table).replace('\0', "\n") })
}

impl FmtFile {
    pub fn total_vertices(&self) -> usize {
        match &self.section {
            Section::V8(s) => s.vertex_buffer.len(),
            Section::V9(d) => d.total_vertices(),
        }
    }

    pub fn total_triangles(&self) -> usize {
        match &self.section {
            Section::V8(s) => s.index_buffer.len() / 3,
            Section::V9(d) => d.total_triangles(),
        }
    }

    pub fn summary(&self) -> serde_json::Value {
        let geometry = match &self.section {
            Section::V8(s) => serde_json::json!({
                "format": "v8",
                "vertex_format": s.vertex_format,
                "triangles": s.index_buffer.len() / 3,
                "vertices": s.vertex_buffer.len(),
            }),
            Section::V9(d) => d.summary(),
        };
        serde_json::json!({
            "kind": "fmt",
            "version": self.version,
            "stats": {
                "vertices": self.total_vertices(),
                "triangles": self.total_triangles(),
                "shapes": self.shapes.len(),
                "materials": self.shapes.iter().map(|s| s.material.as_str()).collect::<std::collections::BTreeSet<_>>().len(),
            },
            "bbox": bbox_json(&self.bbox),
            "shapes": self.shapes,
            "subcomponents": self.subcomponents.iter().map(|s| serde_json::json!({ "tag": s.tag, "unk1": s.unk1, "entries": s.d1s.len() })).collect::<Vec<_>>(),
            "geometry": geometry,
            "d3_entries": self.d3s.len(),
        })
    }
}
