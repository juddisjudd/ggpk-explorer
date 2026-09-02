use crate::dat::reader::{DatReader, DatValue};
use crate::dat::schema::{Schema, Table};
use std::collections::HashMap;

/// Which `PassiveSkillTreeUIArt` frame-reference column a node's frame
/// texture comes from, chosen by its `SkillGraphNodeInfo` type flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeFrameKind {
    Passive,
    Notable,
    Keystone,
    Jewel,
    AscendancyStart,
    MultipleChoice,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameArt {
    pub id: String,
    pub normal: String,
    pub can_allocate: String,
    pub active: String,
    pub mask: String,
    pub header: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConnectionArt {
    pub normal: String,
    pub intermediate: String,
    pub intermediate2: String,
    pub active: String,
    pub mask: String,
    pub ornament1: String,
    pub ornament2: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct GroupBackground {
    pub small: String,
    pub medium: String,
    pub large: String,
    pub small_blank: String,
    pub medium_blank: String,
    pub large_blank: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct SkillTreeArtSet {
    pub group_background: GroupBackground,
    pub frames: HashMap<NodeFrameKind, FrameArt>,
    pub connection: ConnectionArt,
    pub glow: String,
}

/// The community schema bundles both PoE1 and PoE2 definitions for tables
/// shared by both games (distinguished by `validFor`), PoE1 listed first —
/// this app is PoE2-only, so always prefer `validFor == 2` when present.
/// See the matching helper in `atlas_node_db.rs` for the full story.
fn find_table<'a>(schema: &'a Schema, name: &str) -> Result<&'a Table, String> {
    let matches: Vec<&Table> = schema.tables.iter().filter(|t| t.name.eq_ignore_ascii_case(name)).collect();
    matches
        .iter()
        .find(|t| t.valid_for == Some(2))
        .or_else(|| matches.first())
        .copied()
        .ok_or_else(|| format!("Schema table '{}' not found", name))
}

fn col_index(table: &Table, name: &str) -> Option<usize> {
    table.columns.iter().position(|c| c.name.as_deref() == Some(name))
}

fn as_string(val: &DatValue) -> String {
    match val {
        DatValue::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// Parses `PassiveSkillTreeNodeFrameArt.datc64` (schema-driven — reads
/// cleanly with the app's normal `DatReader`/schema path, unlike
/// `PassiveSkillTreeUIArt` below) into a row-index-ordered `Vec`, since
/// `PassiveSkillTreeUIArt`'s foreignrow fields reference it by row index.
pub fn parse_node_frame_art(bytes: Vec<u8>, schema: &Schema) -> Result<Vec<FrameArt>, String> {
    let table = find_table(schema, "PassiveSkillTreeNodeFrameArt")?;
    let reader = DatReader::new(bytes, "passiveskilltreenodeframeart.datc64").map_err(|e| e.to_string())?;
    let id_col = col_index(table, "Id");
    let normal_col = col_index(table, "Normal");
    let can_allocate_col = col_index(table, "CanAllocate");
    let active_col = col_index(table, "Active");
    let mask_col = col_index(table, "Mask");
    let header_col = col_index(table, "Header");

    let mut out = Vec::with_capacity(reader.row_count as usize);
    for i in 0..reader.row_count {
        let row = reader.read_row(i, table).map_err(|e| e.to_string())?;
        let get = |c: Option<usize>| c.and_then(|c| row.get(c)).map(as_string).unwrap_or_default();
        out.push(FrameArt {
            id: get(id_col),
            normal: get(normal_col),
            can_allocate: get(can_allocate_col),
            active: get(active_col),
            mask: get(mask_col),
            header: get(header_col),
        });
    }
    Ok(out)
}

/// Parses `PassiveSkillTreeConnectionArt.datc64` (also schema-driven, also
/// reads cleanly) into a row-index-ordered `Vec`, for the same reason.
pub fn parse_connection_art(bytes: Vec<u8>, schema: &Schema) -> Result<Vec<ConnectionArt>, String> {
    let table = find_table(schema, "PassiveSkillTreeConnectionArt")?;
    let reader = DatReader::new(bytes, "passiveskilltreeconnectionart.datc64").map_err(|e| e.to_string())?;
    let normal_col = col_index(table, "Normal");
    let intermediate_col = col_index(table, "Intermediate");
    let intermediate2_col = col_index(table, "Intermediate2");
    let active_col = col_index(table, "Active");
    let mask_col = col_index(table, "Mask");
    let ornament1_col = col_index(table, "Ornament1");
    let ornament2_col = col_index(table, "Ornament2");

    let mut out = Vec::with_capacity(reader.row_count as usize);
    for i in 0..reader.row_count {
        let row = reader.read_row(i, table).map_err(|e| e.to_string())?;
        let get = |c: Option<usize>| c.and_then(|c| row.get(c)).map(as_string).unwrap_or_default();
        out.push(ConnectionArt {
            normal: get(normal_col),
            intermediate: get(intermediate_col),
            intermediate2: get(intermediate2_col),
            active: get(active_col),
            mask: get(mask_col),
            ornament1: get(ornament1_col),
            ornament2: get(ornament2_col),
        });
    }
    Ok(out)
}

/// Resolves a 64-bit-dat string pointer (u32 offset, second u32 ignored) the
/// same way `DatReader`'s internal `read_string_at` does. Duplicated here
/// (rather than exposed from `reader.rs`) because `PassiveSkillTreeUIArt`
/// needs a hand-rolled fixed-offset parse — see `parse_ui_art` below.
fn resolve_string(data: &[u8], data_section_offset: u64, raw_lo: u32) -> String {
    if raw_lo == 0 {
        return String::new();
    }
    let abs_offset = if raw_lo >= 8 {
        data_section_offset + (raw_lo as u64 - 8)
    } else {
        data_section_offset
    } as usize;
    if abs_offset >= data.len() {
        return String::new();
    }
    let mut units = Vec::new();
    let mut i = abs_offset;
    while i + 1 < data.len() {
        let u = u16::from_le_bytes([data[i], data[i + 1]]);
        if u == 0 {
            break;
        }
        units.push(u);
        i += 2;
        if units.len() > 500 {
            break;
        }
    }
    String::from_utf16_lossy(&units)
}

fn read_string_field(data: &[u8], data_section_offset: u64, pos: usize) -> String {
    let lo = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
    resolve_string(data, data_section_offset, lo)
}

/// Row index a foreignrow field points to, or `None` if it's the null
/// sentinel (16 bytes of `0xFE` in a 64-bit dat).
fn read_foreignrow_field(data: &[u8], pos: usize) -> Option<usize> {
    let lo = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
    let hi = u64::from_le_bytes(data[pos + 8..pos + 16].try_into().unwrap());
    if lo == 0xfefefefe_fefefefe && hi == lo {
        None
    } else {
        Some(lo as usize)
    }
}

/// Parses `PassiveSkillTreeUIArt.datc64` into `Id -> SkillTreeArtSet` (`Id`
/// is the tree context, e.g. `"Character"`/`"Atlas"`/`"BrequelSkillTree"`).
///
/// The community `schema.min.json` release asset is stale for this specific
/// table (verified byte-for-byte against the real GGPK and the live
/// upstream GraphQL schema source at
/// `poe-tool-dev/dat-schema/dat-schema/poe2/_Core.gql`) — it only gets the
/// first 4 columns right. The real 177-byte row layout, confirmed by
/// cross-referencing every foreignrow against the (correctly-read)
/// `PassiveSkillTreeNodeFrameArt`/`PassiveSkillTreeConnectionArt` tables:
///
/// `Id, GroupBackgroundSmall, GroupBackgroundMedium, GroupBackgroundLarge` (4x8 bytes)
/// `_: bool` (1 byte)
/// `GroupBackgroundSmallBlank, GroupBackgroundMediumBlank, GroupBackgroundLargeBlank` (3x8 bytes)
/// `Connection, PassiveFrame, NotableFrame, KeystoneFrame, JewelFrame, AscendancyStart, MultipleChoiceFrame` (7x16-byte foreignrows)
/// `Glow: string` (8 bytes)
/// = 32 + 1 + 24 + 112 + 8 = 177 bytes/row, matching the file's actual row length exactly.
/// Returns the art sets keyed by `Id` plus the row-ordered list of ids, so
/// foreignrows into this table (`Ascendancy.UIArt`) can be resolved.
pub fn parse_ui_art(
    bytes: Vec<u8>,
    node_frames: &[FrameArt],
    connections: &[ConnectionArt],
) -> Result<(HashMap<String, SkillTreeArtSet>, Vec<String>), String> {
    let reader = DatReader::new(bytes, "passiveskilltreeuiart.datc64").map_err(|e| e.to_string())?;
    let data = reader.get_data();
    let data_section_offset = reader.data_section_offset;
    let row_length = reader.row_length.ok_or("PassiveSkillTreeUIArt: no fixed row length")?;
    if row_length != 177 {
        return Err(format!(
            "PassiveSkillTreeUIArt row length changed ({}, expected 177) — the hand-rolled layout in skill_tree_art.rs needs re-verifying against the live dat-schema source",
            row_length
        ));
    }

    let mut out = HashMap::new();
    let mut ids = Vec::with_capacity(reader.row_count as usize);
    for r in 0..reader.row_count {
        let start = 4 + (r as usize) * row_length;
        let id = read_string_field(data, data_section_offset, start);
        ids.push(id.clone());
        if id.is_empty() {
            continue;
        }

        let group_background = GroupBackground {
            small: read_string_field(data, data_section_offset, start + 8),
            medium: read_string_field(data, data_section_offset, start + 16),
            large: read_string_field(data, data_section_offset, start + 24),
            small_blank: read_string_field(data, data_section_offset, start + 33),
            medium_blank: read_string_field(data, data_section_offset, start + 41),
            large_blank: read_string_field(data, data_section_offset, start + 49),
        };

        let connection_idx = read_foreignrow_field(data, start + 57);
        let passive_idx = read_foreignrow_field(data, start + 73);
        let notable_idx = read_foreignrow_field(data, start + 89);
        let keystone_idx = read_foreignrow_field(data, start + 105);
        let jewel_idx = read_foreignrow_field(data, start + 121);
        let glow = read_string_field(data, data_section_offset, start + 137);
        let ascendancy_start_idx = read_foreignrow_field(data, start + 145);
        let multiple_choice_idx = read_foreignrow_field(data, start + 161);

        let mut frames = HashMap::new();
        let mut set_frame = |kind: NodeFrameKind, idx: Option<usize>| {
            if let Some(f) = idx.and_then(|i| node_frames.get(i)).cloned() {
                frames.insert(kind, f);
            }
        };
        set_frame(NodeFrameKind::Passive, passive_idx);
        set_frame(NodeFrameKind::Notable, notable_idx);
        set_frame(NodeFrameKind::Keystone, keystone_idx);
        set_frame(NodeFrameKind::Jewel, jewel_idx);
        set_frame(NodeFrameKind::AscendancyStart, ascendancy_start_idx);
        set_frame(NodeFrameKind::MultipleChoice, multiple_choice_idx);

        let connection = connection_idx
            .and_then(|i| connections.get(i))
            .cloned()
            .unwrap_or_default();

        out.insert(
            id,
            SkillTreeArtSet {
                group_background,
                frames,
                connection,
                glow,
            },
        );
    }

    Ok((out, ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundles::index::Index as BundleIndex;
    use crate::export::{run_export, ExportStatus};
    use crate::ggpk::reader::GgpkReader;
    use crate::ui::export_window::ExportSettings;
    use std::sync::Arc;

    fn fetch_real(paths: &[&str]) -> std::path::PathBuf {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(GgpkReader::open(&ggpk_path).unwrap());
        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = Arc::new(
            BundleIndex::load_from_cache(&cache_path).expect("run the app once to build the index cache"),
        );
        let hashes: Vec<u64> = index
            .files
            .iter()
            .filter(|(_, f)| paths.iter().any(|p| f.path.eq_ignore_ascii_case(p)))
            .map(|(h, _)| *h)
            .collect();
        assert_eq!(hashes.len(), paths.len(), "not all requested files were found in the bundle index");

        let out_dir = std::env::temp_dir().join("ggpk_skill_tree_art_test");
        std::fs::create_dir_all(&out_dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        run_export(hashes, Some(reader), Some(index), ExportSettings::default(), out_dir.clone(), None, None, None, tx, None);
        while let Ok(status) = rx.try_recv() {
            if let ExportStatus::Complete { errors, .. } = status {
                assert_eq!(errors, 0, "export had errors");
            }
        }
        out_dir
    }

    fn load_schema() -> Schema {
        let path = crate::settings::AppSettings::get_app_data_dir().join("schema.min.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("schema.min.json not found at {}: {} (run the app once first)", path.display(), e));
        serde_json::from_str(&text).expect("failed to parse schema.min.json")
    }

    /// Validates the hand-rolled `PassiveSkillTreeUIArt` layout against the
    /// real GGPK: every foreignrow must resolve to the exact frame/connection
    /// row confirmed during research (Atlas -> AtlasSmall/Notable/Keystone,
    /// BrequelSkillTree -> BrequelSkillTreeMultichoice, etc). If this ever
    /// fails after a game patch, the table's byte layout likely changed and
    /// the hardcoded offsets in `parse_ui_art` need re-deriving.
    #[test]
    #[ignore]
    fn ui_art_matches_real_game_data() {
        let dir = fetch_real(&[
            "data/balance/passiveskilltreeuiart.datc64",
            "data/balance/passiveskilltreenodeframeart.datc64",
            "data/balance/passiveskilltreeconnectionart.datc64",
        ]);
        let schema = load_schema();

        let node_frames = parse_node_frame_art(
            std::fs::read(dir.join("data/balance/passiveskilltreenodeframeart.datc64")).unwrap(),
            &schema,
        )
        .unwrap();
        let connections = parse_connection_art(
            std::fs::read(dir.join("data/balance/passiveskilltreeconnectionart.datc64")).unwrap(),
            &schema,
        )
        .unwrap();
        let (ui_art, _ids) = parse_ui_art(
            std::fs::read(dir.join("data/balance/passiveskilltreeuiart.datc64")).unwrap(),
            &node_frames,
            &connections,
        )
        .unwrap();

        // Cross-reference each resolved foreignrow against the exact row
        // index confirmed during research (see the doc comment on
        // `parse_ui_art`) — the resolved struct must be byte-identical to
        // that row in the independently-parsed (and known-clean)
        // NodeFrameArt/ConnectionArt tables.
        let atlas = ui_art.get("Atlas").expect("Atlas UIArt row missing");
        assert!(atlas.group_background.small.contains("AtlasPassiveSkillScreenGroupBackgroundSmall"));
        assert_eq!(atlas.connection, connections[4]); // "Atlas"
        assert_eq!(*atlas.frames.get(&NodeFrameKind::Passive).unwrap(), node_frames[22]); // "AtlasSmall"
        assert_eq!(*atlas.frames.get(&NodeFrameKind::Notable).unwrap(), node_frames[23]); // "AtlasNotable"
        assert_eq!(*atlas.frames.get(&NodeFrameKind::Keystone).unwrap(), node_frames[24]); // "AtlasKeystone"
        assert!(atlas.frames.get(&NodeFrameKind::Jewel).is_none());

        let brequel = ui_art.get("BrequelSkillTree").expect("BrequelSkillTree UIArt row missing");
        assert_eq!(brequel.connection, connections[9]); // "BrequelSkillTree"
        assert_eq!(*brequel.frames.get(&NodeFrameKind::Passive).unwrap(), node_frames[36]); // "BrequelSkillTreeSmall"
        assert_eq!(*brequel.frames.get(&NodeFrameKind::Notable).unwrap(), node_frames[38]); // "BrequelSkillTreeNotable"
        assert_eq!(*brequel.frames.get(&NodeFrameKind::Keystone).unwrap(), node_frames[39]); // "BrequelSkillTreeKeystone"
        assert_eq!(*brequel.frames.get(&NodeFrameKind::MultipleChoice).unwrap(), node_frames[37]); // "BrequelSkillTreeMultichoice"

        let character = ui_art.get("Character").expect("Character UIArt row missing");
        assert_eq!(character.connection, connections[0]); // "Character"
        assert_eq!(*character.frames.get(&NodeFrameKind::Passive).unwrap(), node_frames[0]); // "CharacterSmall"
        assert_eq!(*character.frames.get(&NodeFrameKind::Notable).unwrap(), node_frames[1]); // "CharacterNotable"
        assert_eq!(*character.frames.get(&NodeFrameKind::Keystone).unwrap(), node_frames[2]); // "CharacterKeystone"
        assert_eq!(*character.frames.get(&NodeFrameKind::Jewel).unwrap(), node_frames[3]); // "CharacterJewel"
    }
}
