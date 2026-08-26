//! `DOLm` geometry block shared by FMT (v9+), TGM (v9+) and SMD (v3+).

use super::cursor::{Cur, PResult};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum IndexBuffer {
    U16(Vec<u16>),
    U32(Vec<u32>),
}

impl IndexBuffer {
    pub fn len(&self) -> usize {
        match self {
            IndexBuffer::U16(v) => v.len(),
            IndexBuffer::U32(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [i8; 4],
    pub tangent: [i8; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tex_coord0: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_float: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skin_bones: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skin_weights: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skin_extra: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tex_coord1: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_vformat_6: Option<[u8; 4]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeExtents {
    pub start_index: u32,
    pub count_index: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Mesh {
    pub shape_extents: Vec<ShapeExtents>,
    pub indices: IndexBuffer,
    pub vertices: Vec<Vertex>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dolm {
    pub c0h: u16,
    pub vertex_format: u32,
    /// `[num_triangles, num_vertices]` per LOD.
    pub lod_extents: Vec<[u32; 2]>,
    pub lods: Vec<Mesh>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_vformat_6: Option<Vec<Vec<u8>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_vformat_6_c0h_2: Option<Vec<[u8; 4]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_c0h_4: Option<[u8; 4]>,
}

/// Index buffers use u16 indices while the vertex count fits, u32 otherwise.
pub fn read_index_buffer(cur: &mut Cur, num_vertices: u32, num_triangles: u32) -> PResult<IndexBuffer> {
    let n = num_triangles as usize * 3;
    if num_vertices < 0x10000 {
        cur.check_count(n, 2)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(cur.u16()?);
        }
        Ok(IndexBuffer::U16(v))
    } else {
        cur.check_count(n, 4)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(cur.u32()?);
        }
        Ok(IndexBuffer::U32(v))
    }
}

fn read_vertex(cur: &mut Cur, vf: u32) -> PResult<Vertex> {
    let pos = cur.f32s::<3>()?;
    let normal = cur.i8s::<4>()?;
    let tangent = cur.i8s::<4>()?;
    let tex_coord0 = if (vf >> 3) & 1 == 1 { Some(cur.f16s::<2>()?) } else { None };
    let skinned = (vf >> 2) & 1 == 1;
    let skin_bones = if skinned { Some(cur.arr::<4>()?) } else { None };
    let skin_weights = if skinned { Some(cur.arr::<4>()?) } else { None };
    let skin_extra = if (vf >> 1) & 1 == 1 { Some(cur.arr::<4>()?) } else { None };
    let tex_coord1 = if vf & 1 == 1 { Some(cur.f16s::<2>()?) } else { None };
    let extra_vformat_6 = if (vf >> 6) & 1 == 1 { Some(cur.arr::<4>()?) } else { None };
    Ok(Vertex { pos, normal, tangent, tex_coord0, tail_float: None, skin_bones, skin_weights, skin_extra, tex_coord1, extra_vformat_6 })
}

fn read_mesh(cur: &mut Cur, num_shapes: usize, num_triangles: u32, num_vertices: u32, vf: u32) -> PResult<Mesh> {
    cur.check_count(num_shapes, 8)?;
    let mut shape_extents = Vec::with_capacity(num_shapes);
    for _ in 0..num_shapes {
        shape_extents.push(ShapeExtents { start_index: cur.u32()?, count_index: cur.u32()? });
    }
    let indices = read_index_buffer(cur, num_vertices, num_triangles)?;
    cur.check_count(num_vertices as usize, 20)?;
    let mut vertices = Vec::with_capacity(num_vertices as usize);
    for _ in 0..num_vertices {
        vertices.push(read_vertex(cur, vf)?);
    }
    Ok(Mesh { shape_extents, indices, vertices })
}

pub fn parse_dolm(cur: &mut Cur) -> PResult<Dolm> {
    cur.expect(b"DOLm", "DOLm magic")?;
    let c0h = cur.u16()?;
    let num_lods = cur.u8()?;
    let num_shapes = cur.u16()?;
    let vertex_format = cur.u32()?;

    let mut lod_extents = Vec::with_capacity(num_lods as usize);
    for _ in 0..num_lods {
        lod_extents.push(cur.u32s::<2>()?);
    }
    let mut lods = Vec::with_capacity(num_lods as usize);
    for [num_triangles, num_vertices] in &lod_extents {
        lods.push(read_mesh(cur, num_shapes as usize, *num_triangles, *num_vertices, vertex_format)?);
    }

    let extra_vformat_6 = if (vertex_format >> 6) & 1 == 1 {
        let mut v = Vec::with_capacity(num_shapes as usize);
        for _ in 0..num_shapes {
            v.push(cur.bytes(36)?);
        }
        Some(v)
    } else {
        None
    };
    let extra_vformat_6_c0h_2 = if c0h == 2 && (vertex_format >> 6) & 1 == 1 {
        let mut v = Vec::with_capacity(num_shapes as usize);
        for _ in 0..num_shapes {
            v.push(cur.arr::<4>()?);
        }
        Some(v)
    } else {
        None
    };
    let extra_c0h_4 = if c0h == 4 && num_lods > 0 { Some(cur.arr::<4>()?) } else { None };

    Ok(Dolm { c0h, vertex_format, lod_extents, lods, extra_vformat_6, extra_vformat_6_c0h_2, extra_c0h_4 })
}

impl Dolm {
    pub fn total_vertices(&self) -> usize {
        self.lods.iter().map(|m| m.vertices.len()).sum()
    }

    pub fn total_triangles(&self) -> usize {
        self.lods.iter().map(|m| m.indices.len() / 3).sum()
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "format": "DOLm",
            "c0h": self.c0h,
            "vertex_format": format!("0x{:x}", self.vertex_format),
            "lods": self.lods.iter().map(|m| serde_json::json!({
                "triangles": m.indices.len() / 3,
                "vertices": m.vertices.len(),
                "shapes": m.shape_extents.iter().map(|s| serde_json::json!([s.start_index, s.count_index])).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

pub fn bbox_json(b: &[f32; 6]) -> serde_json::Value {
    serde_json::json!({ "min": [b[0], b[1], b[2]], "max": [b[3], b[4], b[5]] })
}
