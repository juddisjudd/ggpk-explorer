//! `.smd` skinned mesh data. Layout ported from poe_data_tools (`file_parsers/smd`).

use super::cursor::{Cur, PResult};
use super::dolm::{bbox_json, parse_dolm, read_index_buffer, Dolm, IndexBuffer, Vertex};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct V3Section {
    pub dolm: Dolm,
    pub shape_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeExtents {
    pub name: String,
    pub triangle_index: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct V2Section {
    pub c04_2: Option<u32>,
    pub shape_extents: Vec<ShapeExtents>,
    pub index_buffer: IndexBuffer,
    pub vertex_buffer: Vec<Vertex>,
}

#[derive(Debug, Clone, Serialize)]
pub enum Section {
    V2(V2Section),
    V3(V3Section),
}

#[derive(Debug, Clone, Serialize)]
pub struct Ellipsoid {
    pub floats: Vec<f32>,
    pub unk1: u32,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sphere {
    pub centre: [f32; 3],
    pub radius: f32,
    pub unk1: u32,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SphereConnection {
    pub s0_index: u32,
    pub s1_index: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkinnedVertex {
    pub pos: [f32; 3],
    pub unk1: [u32; 4],
    pub unk2: [f32; 4],
}

#[derive(Debug, Clone, Serialize)]
pub struct Tail {
    pub tail_version: u32,
    pub ellipsoids: Vec<Ellipsoid>,
    pub spheres: Vec<Sphere>,
    pub sphere_connections: Vec<SphereConnection>,
    pub skinned_vertices: Vec<SkinnedVertex>,
    pub num_t3s: u32,
    pub sv_refs1: Vec<u32>,
    pub sv_refs2: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmdFile {
    pub version: u8,
    pub vertex_format: u8,
    pub section: Section,
    pub bbox: [f32; 6],
    pub tail: Tail,
}

fn read_v2_vertex(cur: &mut Cur, vf: u8) -> PResult<Vertex> {
    Ok(Vertex {
        pos: cur.f32s::<3>()?,
        normal: cur.i8s::<4>()?,
        tangent: cur.i8s::<4>()?,
        tex_coord0: Some(cur.f16s::<2>()?),
        skin_bones: Some(cur.arr::<4>()?),
        skin_weights: Some(cur.arr::<4>()?),
        skin_extra: if (vf >> 1) & 1 == 1 { Some(cur.arr::<4>()?) } else { None },
        tex_coord1: if vf & 1 == 1 { Some(cur.f16s::<2>()?) } else { None },
        tail_float: None,
        extra_vformat_6: None,
    })
}

fn read_utf16_len_prefixed(cur: &mut Cur) -> PResult<String> {
    let n = cur.u32()? as usize;
    cur.utf16le(n)
}

fn parse_v2(cur: &mut Cur, version: u8) -> PResult<(u8, Section, [f32; 6])> {
    let num_triangles = cur.u32()?;
    let num_vertices = cur.u32()?;
    let vertex_format = cur.u8()?;
    let num_shapes = cur.u16()?;
    let _bytes_name_section = cur.u32()?;
    let bbox = cur.f32s::<6>()?;
    let c04_2 = if version == 2 { Some(cur.u32()?) } else { None };

    let mut raw = Vec::with_capacity(num_shapes as usize);
    for _ in 0..num_shapes {
        raw.push((cur.u32()?, cur.u32()?));
    }
    let mut shape_extents = Vec::with_capacity(num_shapes as usize);
    for (name_length, triangle_index) in raw {
        shape_extents.push(ShapeExtents { name: cur.utf16le(name_length as usize)?, triangle_index });
    }
    let index_buffer = read_index_buffer(cur, num_vertices, num_triangles)?;
    cur.check_count(num_vertices as usize, 32)?;
    let mut vertex_buffer = Vec::with_capacity(num_vertices as usize);
    for _ in 0..num_vertices {
        vertex_buffer.push(read_v2_vertex(cur, vertex_format)?);
    }
    Ok((vertex_format, Section::V2(V2Section { c04_2, shape_extents, index_buffer, vertex_buffer }), bbox))
}

fn parse_v3(cur: &mut Cur) -> PResult<(u8, Section, [f32; 6])> {
    let vertex_format = cur.u8()?;
    let num_shapes = cur.u16()?;
    let _bytes_name_section = cur.u32()?;
    let bbox = cur.f32s::<6>()?;
    let dolm = parse_dolm(cur)?;
    let lengths: Vec<u32> = (0..num_shapes).map(|_| cur.u32()).collect::<PResult<_>>()?;
    let shape_names = lengths.into_iter().map(|len| cur.utf16le(len as usize)).collect::<PResult<Vec<_>>>()?;
    Ok((vertex_format, Section::V3(V3Section { dolm, shape_names }), bbox))
}

fn read_ellipsoid(cur: &mut Cur, version: u8) -> PResult<Ellipsoid> {
    let floats = (0..15).map(|_| cur.f32()).collect::<PResult<Vec<_>>>()?;
    let unk1 = cur.u32()?;
    let name = if version >= 3 { Some(read_utf16_len_prefixed(cur)?) } else { None };
    Ok(Ellipsoid { floats, unk1, name })
}

fn read_sphere(cur: &mut Cur, version: u8) -> PResult<Sphere> {
    let centre = cur.f32s::<3>()?;
    let radius = cur.f32()?;
    let unk1 = cur.u32()?;
    let name = if version >= 3 { Some(read_utf16_len_prefixed(cur)?) } else { None };
    Ok(Sphere { centre, radius, unk1, name })
}

fn read_skinned_vertex(cur: &mut Cur) -> PResult<SkinnedVertex> {
    Ok(SkinnedVertex { pos: cur.f32s::<3>()?, unk1: cur.u32s::<4>()?, unk2: cur.f32s::<4>()? })
}

fn read_u32s(cur: &mut Cur, n: u32) -> PResult<Vec<u32>> {
    cur.check_count(n as usize, 4)?;
    (0..n).map(|_| cur.u32()).collect()
}

fn parse_tail(cur: &mut Cur, version: u8) -> PResult<Tail> {
    let tail_version = cur.u32()?;
    match tail_version {
        2 => {
            let [num_ellipsoids, num_skinned, num_refs1, num_refs2] = cur.u32s::<4>()?;
            cur.check_count(num_skinned as usize, 44)?;
            let skinned_vertices = (0..num_skinned).map(|_| read_skinned_vertex(cur)).collect::<PResult<_>>()?;
            let sv_refs1 = read_u32s(cur, num_refs1)?;
            let ellipsoids = (0..num_ellipsoids).map(|_| read_ellipsoid(cur, version)).collect::<PResult<_>>()?;
            let sv_refs2 = read_u32s(cur, num_refs2)?;
            Ok(Tail { tail_version, ellipsoids, spheres: vec![], sphere_connections: vec![], skinned_vertices, num_t3s: 0, sv_refs1, sv_refs2 })
        }
        3 => {
            let [num_ellipsoids, num_spheres, num_conn, num_t3s, num_skinned, num_refs1, num_refs2] = cur.u32s::<7>()?;
            let ellipsoids = (0..num_ellipsoids).map(|_| read_ellipsoid(cur, version)).collect::<PResult<_>>()?;
            let spheres = (0..num_spheres).map(|_| read_sphere(cur, version)).collect::<PResult<_>>()?;
            cur.check_count(num_conn as usize, 8)?;
            let sphere_connections = (0..num_conn).map(|_| Ok(SphereConnection { s0_index: cur.u32()?, s1_index: cur.u32()? })).collect::<PResult<_>>()?;
            cur.check_count(num_skinned as usize, 44)?;
            let skinned_vertices = (0..num_skinned).map(|_| read_skinned_vertex(cur)).collect::<PResult<_>>()?;
            let sv_refs1 = read_u32s(cur, num_refs1)?;
            let sv_refs2 = read_u32s(cur, num_refs2)?;
            Ok(Tail { tail_version, ellipsoids, spheres, sphere_connections, skinned_vertices, num_t3s, sv_refs1, sv_refs2 })
        }
        4 => {
            let [num_ellipsoids, num_spheres, num_conn, num_t3s, num_skinned, num_refs1, num_refs2] = cur.u32s::<7>()?;
            cur.check_count(num_skinned as usize, 44)?;
            let skinned_vertices = (0..num_skinned).map(|_| read_skinned_vertex(cur)).collect::<PResult<_>>()?;
            let sv_refs1 = read_u32s(cur, num_refs1)?;
            let ellipsoids = (0..num_ellipsoids).map(|_| read_ellipsoid(cur, version)).collect::<PResult<_>>()?;
            let spheres = (0..num_spheres).map(|_| read_sphere(cur, version)).collect::<PResult<_>>()?;
            cur.check_count(num_conn as usize, 8)?;
            let sphere_connections = (0..num_conn).map(|_| Ok(SphereConnection { s0_index: cur.u32()?, s1_index: cur.u32()? })).collect::<PResult<_>>()?;
            let sv_refs2 = read_u32s(cur, num_refs2)?;
            Ok(Tail { tail_version, ellipsoids, spheres, sphere_connections, skinned_vertices, num_t3s, sv_refs1, sv_refs2 })
        }
        other => Err(cur.err(format!("unsupported SMD tail version {}", other))),
    }
}

pub fn parse_smd(data: &[u8]) -> PResult<SmdFile> {
    let mut cur = Cur::new(data);
    let version = cur.u8()?;
    let (vertex_format, section, bbox) = if version < 3 { parse_v2(&mut cur, version)? } else { parse_v3(&mut cur)? };
    let tail = parse_tail(&mut cur, version)?;
    Ok(SmdFile { version, vertex_format, section, bbox, tail })
}

impl SmdFile {
    pub fn total_vertices(&self) -> usize {
        match &self.section {
            Section::V2(s) => s.vertex_buffer.len(),
            Section::V3(s) => s.dolm.total_vertices(),
        }
    }

    pub fn total_triangles(&self) -> usize {
        match &self.section {
            Section::V2(s) => s.index_buffer.len() / 3,
            Section::V3(s) => s.dolm.total_triangles(),
        }
    }

    pub fn shape_names(&self) -> Vec<String> {
        match &self.section {
            Section::V2(s) => s.shape_extents.iter().map(|s| s.name.clone()).collect(),
            Section::V3(s) => s.shape_names.clone(),
        }
    }

    pub fn summary(&self) -> serde_json::Value {
        let geometry = match &self.section {
            Section::V2(s) => serde_json::json!({
                "format": "v2",
                "triangles": s.index_buffer.len() / 3,
                "vertices": s.vertex_buffer.len(),
                "shapes": s.shape_extents.iter().map(|e| serde_json::json!({ "name": e.name, "triangle_index": e.triangle_index })).collect::<Vec<_>>(),
            }),
            Section::V3(s) => s.dolm.summary(),
        };
        serde_json::json!({
            "kind": "smd",
            "version": self.version,
            "stats": {
                "vertices": self.total_vertices(),
                "triangles": self.total_triangles(),
                "shapes": self.shape_names().len(),
                "colliders": self.tail.ellipsoids.len() + self.tail.spheres.len(),
            },
            "vertex_format": self.vertex_format,
            "bbox": bbox_json(&self.bbox),
            "shapes": self.shape_names(),
            "geometry": geometry,
            "physics": {
                "tail_version": self.tail.tail_version,
                "ellipsoids": self.tail.ellipsoids.iter().map(|e| serde_json::json!({ "name": e.name, "unk1": e.unk1 })).collect::<Vec<_>>(),
                "spheres": self.tail.spheres.iter().map(|s| serde_json::json!({ "name": s.name, "centre": s.centre, "radius": s.radius })).collect::<Vec<_>>(),
                "sphere_connections": self.tail.sphere_connections.len(),
                "skinned_vertices": self.tail.skinned_vertices.len(),
                "refs": [self.tail.sv_refs1.len(), self.tail.sv_refs2.len()],
            },
        })
    }
}
