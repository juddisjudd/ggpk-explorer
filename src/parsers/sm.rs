//! `.sm` skinned meshes: the `.smd` they wrap, a `.mat` per run of its shapes, a bounding box (v5+) and bone groups (v6+).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SmFile {
    pub version: u32,
    pub smd_file: String,
    pub materials: Vec<Material>,
    /// `[min x, min y, min z, max x, max y, max z]`.
    pub bbox: Option<[f32; 6]>,
    pub bone_groups: Option<Vec<BoneGroup>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Material {
    /// `None` when the file leaves the material empty.
    pub mat_file: Option<String>,
    /// How many consecutive `.smd` shapes, after the previous material's, use this one.
    pub shape_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoneGroup {
    pub name: String,
    pub bones: Vec<String>,
}

impl SmFile {
    /// The material of each `.smd` shape, in shape order.
    pub fn shape_materials(&self) -> Vec<Option<&str>> {
        self.materials.iter().flat_map(|m| std::iter::repeat_n(m.mat_file.as_deref(), m.shape_count as usize)).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Token<'a> {
    Quoted(&'a str),
    Bare(&'a str),
}

impl<'a> Token<'a> {
    fn quoted(self) -> Option<&'a str> {
        match self {
            Token::Quoted(s) => Some(s),
            Token::Bare(_) => None,
        }
    }

    fn bare(self) -> Option<&'a str> {
        match self {
            Token::Bare(s) => Some(s),
            Token::Quoted(_) => None,
        }
    }
}

fn tokens(line: &str) -> Result<Vec<Token<'_>>, String> {
    let mut out = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        if let Some(s) = rest.strip_prefix('"') {
            let end = s.find('"').ok_or("unterminated quote")?;
            out.push(Token::Quoted(&s[..end]));
            rest = s[end + 1..].trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push(Token::Bare(&rest[..end]));
            rest = rest[end..].trim_start();
        }
    }
    Ok(out)
}

/// The file's non-blank lines with their line numbers, read front to back.
struct Lines<'a> {
    lines: Vec<(usize, &'a str)>,
    pos: usize,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let lines = text.lines().enumerate().map(|(i, l)| (i + 1, l.trim())).filter(|(_, l)| !l.is_empty()).collect();
        Self { lines, pos: 0 }
    }

    fn next(&mut self, what: &str) -> Result<Vec<Token<'a>>, String> {
        let (_, line) = *self.lines.get(self.pos).ok_or_else(|| format!("missing {}", what))?;
        self.pos += 1;
        tokens(line).map_err(|e| self.error(&e))
    }

    /// `msg` placed at the line read last.
    fn error(&self, msg: &str) -> String {
        match self.pos.checked_sub(1).and_then(|i| self.lines.get(i)) {
            Some((n, _)) => format!("line {}: {}", n, msg),
            None => msg.to_string(),
        }
    }

    /// The count after `keyword` on the next line.
    fn counted(&mut self, keyword: &str) -> Result<usize, String> {
        match self.next(keyword)?.as_slice() {
            [Token::Bare(k), Token::Bare(n)] if *k == keyword => n.parse().map_err(|_| self.error(&format!("bad {} count {:?}", keyword, n))),
            _ => Err(self.error(&format!("expected `{} <count>`", keyword))),
        }
    }
}

fn file_with_ext(path: &str, ext: &str) -> bool {
    path.to_ascii_lowercase().ends_with(ext)
}

pub fn parse(text: &str) -> Result<SmFile, String> {
    let mut lines = Lines::new(text);

    let version = lines.counted("version")? as u32;

    let smd_file = match lines.next("SkinnedMeshData")?.as_slice() {
        [Token::Bare("SkinnedMeshData"), Token::Quoted(p)] if file_with_ext(p, ".smd") => p.to_string(),
        _ => return Err(lines.error("expected `SkinnedMeshData \"<file>.smd\"`")),
    };

    let n = lines.counted("Materials")?;
    let mut materials = Vec::with_capacity(n);
    for _ in 0..n {
        let material = match lines.next("material")?.as_slice() {
            [Token::Quoted(p), Token::Bare(count)] if p.is_empty() || file_with_ext(p, ".mat") => {
                count.parse().ok().map(|shape_count| Material { mat_file: (!p.is_empty()).then(|| p.to_string()), shape_count })
            }
            _ => None,
        };
        materials.push(material.ok_or_else(|| lines.error("expected `\"<file>.mat\" <shape count>`"))?);
    }

    let bbox = if version >= 5 {
        let values = match lines.next("BoundingBox")?.split_first() {
            Some((Token::Bare("BoundingBox"), rest)) => rest.iter().map(|t| t.bare()?.parse().ok()).collect::<Option<Vec<f32>>>(),
            _ => None,
        };
        Some(values.and_then(|v| <[f32; 6]>::try_from(v).ok()).ok_or_else(|| lines.error("expected `BoundingBox` and six numbers"))?)
    } else {
        None
    };

    let bone_groups = if version >= 6 {
        let n = lines.counted("BoneGroups")?;
        let mut groups = Vec::with_capacity(n);
        for _ in 0..n {
            let group = match lines.next("bone group")?.as_slice() {
                [Token::Quoted(name), Token::Bare(count), bones @ ..] if count.parse() == Ok(bones.len()) => bones
                    .iter()
                    .map(|b| b.quoted().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
                    .map(|bones| BoneGroup { name: name.to_string(), bones }),
                _ => None,
            };
            groups.push(group.ok_or_else(|| lines.error("expected `\"<group>\" <count> \"<bone>\"…`"))?);
        }
        Some(groups)
    } else {
        None
    };

    if lines.pos < lines.lines.len() {
        lines.pos += 1;
        return Err(lines.error("unexpected line after the last section"));
    }
    Ok(SmFile { version, smd_file, materials, bbox, bone_groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_6() {
        let text = "version 6\r\nSkinnedMeshData \"Art/Models/Items/BackAttachments/GraverobberBackAttachment/rig_10c10475.smd\"\r\nMaterials 2\r\n\t\"Art/Models/Items/BackAttachments/GraverobberBackAttachment/Textures/GraverobberBackattach01c.mat\" 3\r\n\t\"\" 1\r\nBoundingBox -24.5229 -2.89996 -57.4775 24.7684 78.6808 84.7975\r\nBoneGroups 2\r\n\t\"child_attach\" 2 \"chest_jntBnd\" \"L_clavicle_jntBnd\"\r\n\t\"empty group\" 0\r\n";
        let sm = parse(text).unwrap();
        assert_eq!(sm.version, 6);
        assert_eq!(sm.smd_file, "Art/Models/Items/BackAttachments/GraverobberBackAttachment/rig_10c10475.smd");
        assert_eq!(sm.materials.len(), 2);
        assert_eq!(sm.materials[0].mat_file.as_deref(), Some("Art/Models/Items/BackAttachments/GraverobberBackAttachment/Textures/GraverobberBackattach01c.mat"));
        assert_eq!(sm.materials[0].shape_count, 3);
        assert_eq!(sm.materials[1].mat_file, None);
        assert_eq!(sm.bbox, Some([-24.5229, -2.89996, -57.4775, 24.7684, 78.6808, 84.7975]));
        let groups = sm.bone_groups.unwrap();
        assert_eq!(groups[0].name, "child_attach");
        assert_eq!(groups[0].bones, vec!["chest_jntBnd", "L_clavicle_jntBnd"]);
        assert_eq!(groups[1].name, "empty group");
        assert!(groups[1].bones.is_empty());
    }

    #[test]
    fn older_versions_stop_after_their_sections() {
        let v4 = parse("version 4\nSkinnedMeshData \"Art/Rays_0a7ad2f9.smd\"\nMaterials 1\n\t\"Art/Rays_Tex/shockwavec.mat\" 2\n").unwrap();
        assert_eq!(v4.materials[0].shape_count, 2);
        assert!(v4.bbox.is_none() && v4.bone_groups.is_none());
        let v5 = parse("version 5\nSkinnedMeshData \"Art/footprints_821de188.smd\"\nMaterials 0\nBoundingBox -16.6733 -18.2365 -22.2192 16.7297 18.338 0\n").unwrap();
        assert_eq!(v5.bbox, Some([-16.6733, -18.2365, -22.2192, 16.7297, 18.338, 0.0]));
        assert!(v5.bone_groups.is_none());
    }

    #[test]
    fn materials_cover_runs_of_shapes() {
        let sm = parse("version 4\nSkinnedMeshData \"A/b.smd\"\nMaterials 3\n\"A/body.mat\" 2\n\"\" 1\n\"A/acc.mat\" 1\n").unwrap();
        assert_eq!(sm.shape_materials(), vec![Some("A/body.mat"), Some("A/body.mat"), None, Some("A/acc.mat")]);
    }

    #[test]
    fn rejects_malformed_files() {
        assert!(parse("").is_err());
        assert!(parse("version 4\nSkinnedMeshData \"A/b.fmt\"\nMaterials 0\n").is_err());
        assert!(parse("version 4\nSkinnedMeshData \"A/b.smd\"\nMaterials 2\n\"A/b.mat\" 1\n").is_err());
        assert!(parse("version 4\nSkinnedMeshData \"A/b.smd\"\nMaterials 1\n\"A/b.mat 1\n").is_err());
        assert!(parse("version 5\nSkinnedMeshData \"A/b.smd\"\nMaterials 0\nBoundingBox 0 0 0 1 1\n").is_err());
        assert!(parse("version 6\nSkinnedMeshData \"A/b.smd\"\nMaterials 0\nBoundingBox 0 0 0 1 1 1\nBoneGroups 1\n\"g\" 2 \"a\"\n").is_err());
        assert_eq!(parse("version 4\nSkinnedMeshData \"A/b.smd\"\nMaterials 0\n\nExtra 1\n").unwrap_err(), "line 5: unexpected line after the last section");
    }

    /// `cargo test --release parse_real_sm_files -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn parse_real_sm_files() {
        let files = crate::parsers::real_files("sm", 400);
        let (reader, index) = crate::parsers::real_source();
        let by_path: std::collections::HashMap<String, &crate::bundles::index::FileInfo> =
            index.files.values().map(|f| (f.path.to_ascii_lowercase(), f)).collect();
        let mut versions = std::collections::BTreeMap::new();
        let mut fails = Vec::new();
        let (mut smd_found, mut smd_parsed, mut covered) = (0usize, 0usize, 0usize);
        for (path, bytes) in &files {
            let sm = match parse(&crate::parsers::utils::decode_text_lossy(bytes)) {
                Ok(sm) => sm,
                Err(e) => {
                    fails.push(format!("{}: {}", path, e));
                    continue;
                }
            };
            *versions.entry(sm.version).or_insert(0usize) += 1;
            let Some(fi) = by_path.get(&sm.smd_file.replace('\\', "/").to_ascii_lowercase()) else { continue };
            smd_found += 1;
            let Some(data) = crate::bundles::extract::extract_bundle_file_sync(fi, &index, Some(&reader), None) else { continue };
            let Ok(crate::parsers::model::ModelFile::Smd(smd)) = crate::parsers::model::parse_model(&fi.path, &data) else { continue };
            smd_parsed += 1;
            covered += (sm.shape_materials().len() == smd.shape_names().len()) as usize;
        }
        let parsed = files.len() - fails.len();
        println!("sm: {} of {} parsed, versions {:?}", parsed, files.len(), versions);
        for f in fails.iter().take(10) {
            println!("   FAIL {}", f);
        }
        println!("smd: {} named, {} in the index, {} parsed, materials cover every shape in {}", parsed, smd_found, smd_parsed, covered);
        assert!(!files.is_empty(), "no .sm files in the index");
        assert!(parsed * 100 >= files.len() * 95, "{} of {} .sm files failed to parse", fails.len(), files.len());
    }
}
