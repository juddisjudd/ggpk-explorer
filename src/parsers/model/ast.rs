//! `.ast` skeleton files: bone hierarchy, lights and animation tracks.
//! Layout ported from poe_data_tools (`file_parsers/ast`).

use super::cursor::{Cur, PResult};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Header {
    pub version: u8,
    pub num_bones: u8,
    pub unk1: u8,
    pub num_animations: u16,
    pub unk3: u8,
    pub unk4: u8,
    pub num_lights: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bone {
    pub sibling: Option<u8>,
    pub child: Option<u8>,
    pub transform: [[f32; 4]; 4],
    pub unk1: Option<u8>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Light {
    pub unk_bytes1: Vec<u8>,
    pub unk_bytes2: Option<[u8; 4]>,
    pub unk_bytes3: Option<[u8; 4]>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataLocation {
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackHeader {
    pub unk1: u8,
    pub bone_index: u32,
    pub num_scales: u32,
    pub num_rotations: u32,
    pub num_positions: u32,
    pub num_unk2: u32,
    pub num_unk3: u32,
    pub num_unk4: u32,
    pub unk5: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Track {
    pub header: TrackHeader,
    pub scales: Vec<[f32; 4]>,
    pub rotations: Vec<[f32; 5]>,
    pub positions: Vec<[f32; 4]>,
    pub unk2s: Vec<[f32; 4]>,
    pub unk3s: Vec<[f32; 5]>,
    pub unk4s: Vec<[f32; 4]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Animation {
    pub num_tracks: u8,
    pub unk1: u8,
    pub framerate: u8,
    pub unk2: u8,
    pub unk3: Option<u8>,
    pub data_location: Option<DataLocation>,
    pub name: String,
    pub parent_name: Option<String>,
    /// Tracks: inline for v<8, otherwise decoded from the embedded bundle when possible.
    pub tracks: Option<Vec<Track>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BundleInfo {
    pub compressed_size: usize,
    pub uncompressed_size: Option<usize>,
    pub decoded: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AstFile {
    pub header: Header,
    pub bones: Vec<Bone>,
    pub lights: Vec<Light>,
    pub animations: Vec<Animation>,
    pub bundle: Option<BundleInfo>,
}

fn read_bone(cur: &mut Cur, version: u8) -> PResult<Bone> {
    let sibling = cur.u8().map(|i| (i < 255).then_some(i))?;
    let child = cur.u8().map(|i| (i < 255).then_some(i))?;
    let mut transform = [[0f32; 4]; 4];
    for row in transform.iter_mut() {
        *row = cur.f32s::<4>()?;
    }
    let name_length = cur.u8()? as usize;
    let unk1 = if version >= 8 { Some(cur.u8()?) } else { None };
    let name = cur.utf8(name_length)?;
    Ok(Bone { sibling, child, transform, unk1, name })
}

fn read_light(cur: &mut Cur, version: u8) -> PResult<Light> {
    let name_length = cur.u8()? as usize;
    let unk_bytes1 = cur.bytes(51)?;
    let unk_bytes2 = if version >= 7 { Some(cur.arr::<4>()?) } else { None };
    let unk_bytes3 = if version >= 9 { Some(cur.arr::<4>()?) } else { None };
    let name = cur.utf8(name_length)?;
    Ok(Light { unk_bytes1, unk_bytes2, unk_bytes3, name })
}

fn read_track(cur: &mut Cur, version: u8) -> PResult<Track> {
    let header = TrackHeader {
        unk1: cur.u8()?,
        bone_index: cur.u32()?,
        num_scales: cur.u32()?,
        num_rotations: cur.u32()?,
        num_positions: cur.u32()?,
        num_unk2: cur.u32()?,
        num_unk3: cur.u32()?,
        num_unk4: cur.u32()?,
        unk5: if version >= 10 { Some(cur.u32()?) } else { None },
    };
    fn vec4(cur: &mut Cur, n: u32) -> PResult<Vec<[f32; 4]>> {
        cur.check_count(n as usize, 16)?;
        (0..n).map(|_| cur.f32s::<4>()).collect()
    }
    fn vec5(cur: &mut Cur, n: u32) -> PResult<Vec<[f32; 5]>> {
        cur.check_count(n as usize, 20)?;
        (0..n).map(|_| cur.f32s::<5>()).collect()
    }
    let scales = vec4(cur, header.num_scales)?;
    let rotations = vec5(cur, header.num_rotations)?;
    let positions = vec4(cur, header.num_positions)?;
    let unk2s = vec4(cur, header.num_unk2)?;
    let unk3s = vec5(cur, header.num_unk3)?;
    let unk4s = vec4(cur, header.num_unk4)?;
    Ok(Track { header, scales, rotations, positions, unk2s, unk3s, unk4s })
}

fn read_animation(cur: &mut Cur, version: u8) -> PResult<Animation> {
    let num_tracks = cur.u8()?;
    let unk1 = cur.u8()?;
    let framerate = cur.u8()?;
    let unk2 = cur.u8()?;
    let unk3 = if version >= 10 { Some(cur.u8()?) } else { None };
    let name_length = cur.u8()? as usize;
    let parent_name_length = if version >= 11 { Some(cur.u8()? as usize) } else { None };
    let data_location = if version >= 8 { Some(DataLocation { offset: cur.u32()?, length: cur.u32()? }) } else { None };
    let name = cur.utf8(name_length)?;
    let parent_name = match parent_name_length {
        Some(n) => Some(cur.utf8(n)?),
        None => None,
    };
    let tracks = if version < 8 {
        let mut v = Vec::with_capacity(num_tracks as usize);
        for _ in 0..num_tracks {
            v.push(read_track(cur, version)?);
        }
        Some(v)
    } else {
        None
    };
    Ok(Animation { num_tracks, unk1, framerate, unk2, unk3, data_location, name, parent_name, tracks })
}

/// v8+ files end with an Oodle bundle holding the animation keyframes.
fn decode_bundle(rest: &[u8]) -> (BundleInfo, Option<Vec<u8>>) {
    let mut info = BundleInfo { compressed_size: rest.len(), uncompressed_size: None, decoded: false };
    let mut cursor = std::io::Cursor::new(rest);
    let Ok(bundle) = crate::bundles::bundle::Bundle::read_header(&mut cursor) else {
        return (info, None);
    };
    match bundle.decompress(&mut cursor) {
        Ok(data) => {
            info.uncompressed_size = Some(data.len());
            info.decoded = true;
            (info, Some(data))
        }
        Err(_) => (info, None),
    }
}

pub fn parse_ast(data: &[u8]) -> PResult<AstFile> {
    let mut cur = Cur::new(data);
    let header = Header {
        version: cur.u8()?,
        num_bones: cur.u8()?,
        unk1: cur.u8()?,
        num_animations: cur.u16()?,
        unk3: cur.u8()?,
        unk4: cur.u8()?,
        num_lights: cur.u8()?,
    };
    let version = header.version;

    let mut bones = Vec::with_capacity(header.num_bones as usize);
    for _ in 0..header.num_bones {
        bones.push(read_bone(&mut cur, version)?);
    }
    let mut lights = Vec::with_capacity(header.num_lights as usize);
    for _ in 0..header.num_lights {
        lights.push(read_light(&mut cur, version)?);
    }
    let mut animations = Vec::with_capacity(header.num_animations as usize);
    for _ in 0..header.num_animations {
        animations.push(read_animation(&mut cur, version)?);
    }

    let bundle = if version >= 8 {
        let (info, decoded) = decode_bundle(cur.rest());
        if let Some(payload) = decoded {
            for anim in animations.iter_mut() {
                let Some(loc) = &anim.data_location else { continue };
                let start = loc.offset as usize;
                let end = start.saturating_add(loc.length as usize);
                if end > payload.len() || start > end {
                    continue;
                }
                let mut tc = Cur::new(&payload[start..end]);
                let mut tracks = Vec::with_capacity(anim.num_tracks as usize);
                let mut ok = true;
                for _ in 0..anim.num_tracks {
                    match read_track(&mut tc, version) {
                        Ok(t) => tracks.push(t),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    anim.tracks = Some(tracks);
                }
            }
        }
        Some(info)
    } else {
        if !cur.at_end() {
            return Err(cur.err(format!("{} trailing bytes after animations", cur.remaining())));
        }
        None
    };

    Ok(AstFile { header, bones, lights, animations, bundle })
}

impl AstFile {
    /// Nested bone tree built from the first-child / next-sibling links.
    fn hierarchy(&self) -> Vec<serde_json::Value> {
        fn node(bones: &[Bone], idx: usize, depth: usize) -> serde_json::Value {
            let mut children = Vec::new();
            if depth < 64 {
                let mut c = bones.get(idx).and_then(|b| b.child).map(|c| c as usize);
                let mut guard = 0;
                while let Some(ci) = c {
                    if ci >= bones.len() || guard > 256 {
                        break;
                    }
                    children.push(node(bones, ci, depth + 1));
                    c = bones[ci].sibling.map(|s| s as usize);
                    guard += 1;
                }
            }
            let b = &bones[idx];
            let mut v = serde_json::json!({ "index": idx, "name": b.name });
            if !children.is_empty() {
                v["children"] = serde_json::Value::Array(children);
            }
            v
        }
        let mut is_child = vec![false; self.bones.len()];
        for b in &self.bones {
            if let Some(c) = b.child {
                let mut c = Some(c as usize);
                let mut guard = 0;
                while let Some(ci) = c {
                    if ci >= self.bones.len() || guard > 256 {
                        break;
                    }
                    is_child[ci] = true;
                    c = self.bones[ci].sibling.map(|s| s as usize);
                    guard += 1;
                }
            }
        }
        (0..self.bones.len()).filter(|&i| !is_child[i]).map(|i| node(&self.bones, i, 0)).collect()
    }

    pub fn summary(&self) -> serde_json::Value {
        let animations: Vec<serde_json::Value> = self
            .animations
            .iter()
            .map(|a| {
                let mut v = serde_json::json!({
                    "name": a.name,
                    "framerate": a.framerate,
                    "tracks": a.num_tracks,
                });
                if let Some(p) = &a.parent_name {
                    v["parent"] = serde_json::Value::from(p.as_str());
                }
                if let Some(tracks) = &a.tracks {
                    let keys = |f: fn(&Track) -> usize| tracks.iter().map(f).max().unwrap_or(0);
                    v["keyframes"] = serde_json::json!({
                        "positions": keys(|t| t.positions.len()),
                        "rotations": keys(|t| t.rotations.len()),
                        "scales": keys(|t| t.scales.len()),
                    });
                    v["bones"] = serde_json::Value::Array(
                        tracks
                            .iter()
                            .map(|t| {
                                self.bones
                                    .get(t.header.bone_index as usize)
                                    .map(|b| serde_json::Value::from(b.name.as_str()))
                                    .unwrap_or(serde_json::Value::from(t.header.bone_index))
                            })
                            .collect(),
                    );
                } else if let Some(loc) = &a.data_location {
                    v["data"] = serde_json::json!({ "offset": loc.offset, "length": loc.length });
                }
                v
            })
            .collect();

        serde_json::json!({
            "kind": "ast",
            "version": self.header.version,
            "stats": {
                "bones": self.bones.len(),
                "animations": self.animations.len(),
                "lights": self.lights.len(),
            },
            "hierarchy": self.hierarchy(),
            "animations": animations,
            "lights": self.lights.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
            "bundle": self.bundle,
        })
    }
}
