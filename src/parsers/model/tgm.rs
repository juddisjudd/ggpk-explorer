//! `.tgm` tile geometry files (main mesh + ground mesh).
//! Layout ported from poe_data_tools (`file_parsers/tgm`).

use super::cursor::{Cur, PResult};
use super::dolm::{bbox_json, parse_dolm, read_index_buffer, Dolm, IndexBuffer, Vertex};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ShapeExtentsV8 {
    pub ordinal: u16,
    pub bbox: [f32; 6],
    pub index_base: u32,
    pub index_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeshV8 {
    pub shape_extents: Vec<ShapeExtentsV8>,
    pub indices: IndexBuffer,
    pub vertices: Vec<Vertex>,
}

#[derive(Debug, Clone, Serialize)]
pub struct V8Section {
    pub vertex_format: u8,
    pub extra_header: Option<u32>,
    /// `[main, ground]`
    pub meshes: Vec<MeshV8>,
    pub tail_entries: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TailEntry {
    pub uint1: u32,
    pub floats: Vec<f32>,
    pub uint2: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeExtentsV9 {
    pub ordinal: u32,
    pub v12_u16: Option<u16>,
    pub bbox: [f32; 6],
}

#[derive(Debug, Clone, Serialize)]
pub struct Geometry {
    pub dolm: Dolm,
    pub shape_extents: Vec<ShapeExtentsV9>,
}

#[derive(Debug, Clone, Serialize)]
pub struct V9Section {
    pub num_shapes: u16,
    pub extra_u16: u16,
    pub extra_u8: u8,
    /// `[main, ground]`
    pub geometries: Vec<Geometry>,
    pub tail: Vec<TailEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub enum Section {
    V8(V8Section),
    V9(V9Section),
}

#[derive(Debug, Clone, Serialize)]
pub struct TgmFile {
    pub version: u8,
    pub bbox: [f32; 6],
    pub section: Section,
}

#[derive(Clone, Copy)]
struct GeomInfo {
    num_shapes: u16,
    num_vertices: u32,
    num_triangles: u32,
}

fn read_vertex_v8(cur: &mut Cur, version: u8, vf: u8, is_main: bool) -> PResult<Vertex> {
    let pos = cur.f32s::<3>()?;
    let normal = cur.i8s::<4>()?;
    let tangent = cur.i8s::<4>()?;
    let (tex_coord0, tail_float, tex_coord1) = if is_main {
        (Some(cur.f16s::<2>()?), None, None)
    } else {
        let tc0 = if version >= 4 && vf == 3 { Some(cur.f16s::<2>()?) } else { None };
        let tail = if version >= 4 && vf == 2 { Some(cur.f16s::<2>()?) } else { None };
        let tc1 = if version >= 4 && vf == 3 { Some(cur.f16s::<2>()?) } else { None };
        (tc0, tail, tc1)
    };
    Ok(Vertex { pos, normal, tangent, tex_coord0, tail_float, skin_bones: None, skin_weights: None, skin_extra: None, tex_coord1, extra_vformat_6: None })
}

fn read_mesh_v8(cur: &mut Cur, version: u8, vf: u8, gi: GeomInfo, is_main: bool) -> PResult<MeshV8> {
    cur.check_count(gi.num_shapes as usize, 34)?;
    let mut shape_extents = Vec::with_capacity(gi.num_shapes as usize);
    for _ in 0..gi.num_shapes {
        shape_extents.push(ShapeExtentsV8 { ordinal: cur.u16()?, bbox: cur.f32s::<6>()?, index_base: cur.u32()?, index_count: cur.u32()? });
    }
    cur.check_count(gi.num_vertices as usize, 20)?;
    let mut vertices = Vec::with_capacity(gi.num_vertices as usize);
    for _ in 0..gi.num_vertices {
        vertices.push(read_vertex_v8(cur, version, vf, is_main)?);
    }
    let indices = read_index_buffer(cur, gi.num_vertices, gi.num_triangles)?;
    Ok(MeshV8 { shape_extents, indices, vertices })
}

fn parse_v8(cur: &mut Cur, version: u8) -> PResult<V8Section> {
    let mut infos = [GeomInfo { num_shapes: 0, num_vertices: 0, num_triangles: 0 }; 2];
    for gi in infos.iter_mut() {
        *gi = GeomInfo { num_shapes: cur.u16()?, num_vertices: cur.u32()?, num_triangles: cur.u32()? };
    }
    let vertex_format = cur.u8()?;
    let num_tail_entries = cur.u8()?;
    let extra_header = if version == 8 { Some(cur.u32()?) } else { None };
    let mut meshes = Vec::with_capacity(2);
    for (gi, is_main) in infos.iter().zip([true, false]) {
        meshes.push(read_mesh_v8(cur, version, vertex_format, *gi, is_main)?);
    }
    let tail_width = match version {
        0..=2 => 70,
        3 => 74,
        4 => 78,
        5 | 6 => 83,
        _ => 87,
    };
    let mut tail_entries = Vec::with_capacity(num_tail_entries as usize);
    for _ in 0..num_tail_entries {
        tail_entries.push(cur.bytes(tail_width)?);
    }
    Ok(V8Section { vertex_format, extra_header, meshes, tail_entries })
}

fn parse_v9(cur: &mut Cur, version: u8) -> PResult<V9Section> {
    let num_shapes = cur.u16()?;
    let extra_u16 = cur.u16()?;
    let extra_u8 = cur.u8()?;
    let tail_count = cur.u8()?;

    let mut geometries = Vec::with_capacity(2);
    for is_main in [true, false] {
        let dolm = parse_dolm(cur)?;
        let n = dolm.lods.first().map(|m| m.shape_extents.len()).unwrap_or(0);
        let mut shape_extents = Vec::with_capacity(n);
        for _ in 0..n {
            let ordinal = if version == 9 || !is_main { cur.u16()? as u32 } else { cur.u32()? };
            let v12_u16 = if version >= 12 && is_main { Some(cur.u16()?) } else { None };
            let bbox = cur.f32s::<6>()?;
            shape_extents.push(ShapeExtentsV9 { ordinal, v12_u16, bbox });
        }
        geometries.push(Geometry { dolm, shape_extents });
    }

    let mut tail = Vec::with_capacity(tail_count as usize);
    for _ in 0..tail_count {
        let uint1 = cur.u32()?;
        let floats = (0..12).map(|_| cur.f32()).collect::<PResult<Vec<_>>>()?;
        let uint2 = cur.u32()?;
        let bytes = cur.bytes(31)?;
        tail.push(TailEntry { uint1, floats, uint2, bytes });
    }
    Ok(V9Section { num_shapes, extra_u16, extra_u8, geometries, tail })
}

pub fn parse_tgm(data: &[u8]) -> PResult<TgmFile> {
    let mut cur = Cur::new(data);
    let version = cur.u8()?;
    let bbox = cur.f32s::<6>()?;
    let section = if version < 9 { Section::V8(parse_v8(&mut cur, version)?) } else { Section::V9(parse_v9(&mut cur, version)?) };
    if !cur.at_end() {
        return Err(cur.err(format!("{} trailing bytes", cur.remaining())));
    }
    Ok(TgmFile { version, bbox, section })
}

impl TgmFile {
    pub fn total_vertices(&self) -> usize {
        match &self.section {
            Section::V8(s) => s.meshes.iter().map(|m| m.vertices.len()).sum(),
            Section::V9(s) => s.geometries.iter().map(|g| g.dolm.total_vertices()).sum(),
        }
    }

    pub fn total_triangles(&self) -> usize {
        match &self.section {
            Section::V8(s) => s.meshes.iter().map(|m| m.indices.len() / 3).sum(),
            Section::V9(s) => s.geometries.iter().map(|g| g.dolm.total_triangles()).sum(),
        }
    }

    pub fn summary(&self) -> serde_json::Value {
        let names = ["main", "ground"];
        let geometry = match &self.section {
            Section::V8(s) => serde_json::json!({
                "format": "v8",
                "vertex_format": s.vertex_format,
                "meshes": s.meshes.iter().enumerate().map(|(i, m)| serde_json::json!({
                    "role": names[i.min(1)],
                    "vertices": m.vertices.len(),
                    "triangles": m.indices.len() / 3,
                    "shapes": m.shape_extents.iter().map(|s| serde_json::json!({
                        "ordinal": s.ordinal, "bbox": bbox_json(&s.bbox), "index_base": s.index_base, "index_count": s.index_count
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "tail_entries": s.tail_entries.len(),
            }),
            Section::V9(s) => serde_json::json!({
                "format": "DOLm",
                "num_shapes": s.num_shapes,
                "geometries": s.geometries.iter().enumerate().map(|(i, g)| {
                    let mut v = g.dolm.summary();
                    v["role"] = serde_json::Value::from(names[i.min(1)]);
                    v["shape_bounds"] = serde_json::Value::Array(g.shape_extents.iter().map(|s| serde_json::json!({
                        "ordinal": s.ordinal, "bbox": bbox_json(&s.bbox)
                    })).collect());
                    v
                }).collect::<Vec<_>>(),
                "tail": s.tail.iter().map(|t| serde_json::json!({ "uint1": t.uint1, "floats": t.floats, "uint2": t.uint2 })).collect::<Vec<_>>(),
            }),
        };
        serde_json::json!({
            "kind": "tgm",
            "version": self.version,
            "stats": {
                "vertices": self.total_vertices(),
                "triangles": self.total_triangles(),
            },
            "bbox": bbox_json(&self.bbox),
            "geometry": geometry,
        })
    }
}
