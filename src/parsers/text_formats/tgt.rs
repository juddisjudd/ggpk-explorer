//! `.tgt` tile graphics: the `.tgm` mesh, ground mask and per-subtile materials of a tile.

use super::Cursor;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TgtFile {
    pub version: u32,
    pub section: Section,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Section {
    V1(V1Section),
    V3(V3Section),
}

#[derive(Debug, Clone, Serialize)]
pub struct V1Section {
    pub tile_mesh: String,
    pub ground_mask: Option<String>,
    pub normal_materials: Vec<V1NormalMaterial>,
}

#[derive(Debug, Clone, Serialize)]
pub struct V1NormalMaterial {
    pub mat_file: String,
    pub uint: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct V3Section {
    pub source_scene: Option<String>,
    pub size: [u32; 2],
    pub tile_mesh_root: String,
    pub ground_mask: Option<String>,
    pub normal_materials: Vec<String>,
    pub material_slots: Option<Vec<String>>,
    /// Rows of `size[1]`, each `size[0]` subtiles.
    pub subtile_material_indices: Option<Vec<Vec<Vec<Index>>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Index {
    pub uint1: u32,
    pub uint2: u32,
    pub uint3: Option<u32>,
}

fn keyed<'a>(c: &mut Cursor<'a>, key: &str) -> Option<&'a str> {
    c.lit(key)?.sp()?.quoted()
}

fn counted<'a, T>(c: &mut Cursor<'a>, key: &str, mut f: impl FnMut(&mut Cursor<'a>) -> Option<T>) -> Option<Vec<T>> {
    let n = c.lit(key)?.sp()?.uint()?;
    (0..n).map(|_| f(c.sp()?)).collect()
}

fn subtile(c: &mut Cursor) -> Option<Vec<Index>> {
    let n = c.uint()? as usize;
    let values = c.many(|c| c.lit(" ")?.uint());
    let width = match values.len() {
        len if len == n * 3 => 3,
        len if len == n * 2 => 2,
        _ => return None,
    };
    Some(
        values
            .chunks(width)
            .take(n)
            .map(|v| Index { uint1: v[0], uint2: v[1], uint3: v.get(2).copied() })
            .collect(),
    )
}

fn quoted(c: &mut Cursor) -> Option<String> {
    c.quoted().map(String::from)
}

fn v1(c: &mut Cursor) -> Result<V1Section, String> {
    let tile_mesh = c.expect("`TileMesh \"…\"`", |c| keyed(c, "TileMesh"))?.to_string();
    let ground_mask = c.opt(|c| keyed(c.skip_space(), "GroundMask")).map(String::from);
    let normal_materials = c.expect("`NormalMaterials N` and N `\"…\" N` pairs", |c| {
        counted(c, "NormalMaterials", |c| Some(V1NormalMaterial { mat_file: quoted(c)?, uint: c.sp()?.uint()? }))
    })?;
    Ok(V1Section { tile_mesh, ground_mask, normal_materials })
}

fn subtiles(c: &mut Cursor, [w, h]: [u32; 2]) -> Result<Vec<Vec<Vec<Index>>>, String> {
    c.expect("`SubTileMaterialIndices`", |c| c.lit("SubTileMaterialIndices").map(|_| ()))?;
    (0..h).map(|_| (0..w).map(|_| c.expect("subtile material indices", subtile)).collect()).collect()
}

fn v3(c: &mut Cursor) -> Result<V3Section, String> {
    let source_scene = c.opt(|c| keyed(c.skip_space(), "SourceScene")).map(String::from);
    let size = c.expect("`Size W H`", |c| {
        let c = c.lit("Size")?.sp()?;
        Some([c.uint()?, c.sp()?.uint()?])
    })?;
    let tile_mesh_root = c.expect("`TileMeshRoot \"…\"`", |c| keyed(c, "TileMeshRoot"))?.to_string();
    let ground_mask = c.opt(|c| keyed(c.skip_space(), "GroundMask")).map(String::from);
    let normal_materials = c.expect("`NormalMaterials N` and N paths", |c| counted(c, "NormalMaterials", quoted))?;
    let material_slots = c.opt(|c| counted(c.skip_space(), "MaterialSlots", quoted));
    let subtile_material_indices = if normal_materials.is_empty() { None } else { Some(subtiles(c, size)?) };
    Ok(V3Section { source_scene, size, tile_mesh_root, ground_mask, normal_materials, material_slots, subtile_material_indices })
}

pub fn parse(text: &str) -> Result<TgtFile, String> {
    let mut c = Cursor::new(text);
    let version = c.expect("`version N`", super::version)?;
    let section = match version {
        ..3 => Section::V1(v1(&mut c)?),
        _ => Section::V3(v3(&mut c)?),
    };
    c.expect("the end of the file", |c| c.done().then_some(()))?;
    Ok(TgtFile { version, section })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_3_subtiles() {
        let text = "version 3\r\nSize 2 1\r\nTileMeshRoot \"Art/Models/Terrain/tgms/Cc_thinTri_01\"\r\nGroundMask \"Art/Textures/masks/mask.dds\"\r\nNormalMaterials 2\r\n\t\"Art/a.mat\"\r\n\t\"Art/b.mat\"\r\nSubTileMaterialIndices\r\n\t2 0 2 1 3\r\n\t1 0 1 2\r\n";
        let f = parse(text).unwrap();
        let Section::V3(s) = f.section else { panic!("expected a version 3 section") };
        assert_eq!(s.size, [2, 1]);
        assert_eq!(s.normal_materials.len(), 2);
        let rows = s.subtile_material_indices.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0].len(), 2);
        assert_eq!(rows[0][0][1].uint2, 3);
        assert_eq!(rows[0][1][0].uint3, Some(2));
    }

    #[test]
    fn parses_version_1_and_skips_indices_without_materials() {
        let f = parse("version 1\nTileMesh \"Art/a.tgm\"\nNormalMaterials 1\n\"Art/a.mat\" 3\n").unwrap();
        assert!(matches!(f.section, Section::V1(ref s) if s.normal_materials[0].uint == 3));
        let f = parse("version 3\nSize 1 1\nTileMeshRoot \"Art/a\"\nNormalMaterials 0\n").unwrap();
        assert!(matches!(f.section, Section::V3(ref s) if s.subtile_material_indices.is_none()));
    }
}
