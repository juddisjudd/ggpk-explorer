use crate::dat::csd::{self, CsdFile};
use crate::dat::reader::{DatReader, DatValue};
use crate::dat::schema::{Schema, Table};
use crate::dat::stat_translation::TranslationLookup;
use std::collections::HashMap;

/// `(UI_Background, IllustrationX, IllustrationY)` and `UI_Image` of an atlas subtree row.
type AtlasSubtreeArt = (Option<(String, f32, f32)>, Option<String>);

/// Resolved display info for one node, keyed by `PassiveSkillGraphId` (the
/// same id a `.psg` file's `skill_id` refers to). Pre-rendered at build time
/// so hovering a node is just a hashmap lookup, not a translation pass.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct SkillGraphNodeInfo {
    pub name: String,
    pub icon: Option<String>,
    pub is_keystone: bool,
    pub is_notable: bool,
    pub is_jewel_socket: bool,
    pub is_mastery: bool,
    pub is_ascendancy_start: bool,
    pub is_multiple_choice: bool,
    pub flavour_text: Option<String>,
    pub stat_lines: Vec<String>,
    /// Set only on an ascendancy's starting node: the plate illustration.
    /// Placement is computed by `skill_tree_layout` (outer ring slots).
    pub ascendancy_illustration: Option<String>,
    /// Set only on an atlas tree's root nodes (`PassiveSkills.AtlasSubTree`
    /// -> `AtlasPassiveSkillSubTrees`): the subtree's themed background DDS
    /// path plus its `(IllustrationX, IllustrationY)` world-unit offset from
    /// this root's own position. Subtree *membership* for the rest of the
    /// nodes isn't stored anywhere in the DAT data — it's a flood-fill from
    /// each root through the `.psg`'s own connection graph, computed by the
    /// renderer (see `psg_viewer.rs`), matching how community atlas planners
    /// derive it.
    pub atlas_subtree_background: Option<(String, f32, f32)>,
    /// `AtlasPassiveSkillSubTrees.UI_Image`: the themed icon drawn on an atlas root.
    pub atlas_subtree_icon: Option<String>,
    /// `Ascendancy` row index for every node that belongs to an ascendancy.
    pub ascendancy: Option<usize>,
    pub is_attribute: bool,
    /// `PassiveSkills.NodeFrameArt` override (row index into `SkillGraphDatabase::node_frames`).
    pub node_frame_art: Option<usize>,
    /// `Characters` row indices for class-start nodes (empty otherwise).
    pub characters: Vec<usize>,
    /// `PassiveSkills.Id`.
    pub id: String,
    pub is_multiple_choice_option: bool,
    /// `IsAnointmentOnly`: Delirium-anoint-only notables ("blighted" in the web export).
    pub is_anointment_only: bool,
    pub is_free: bool,
    /// Unnamed flag after `IsFree`; set on the Smith of Kitava armour nodes
    /// whose connector the client does not draw.
    pub hide_connection: bool,
    pub skill_points_granted: i32,
    pub weapon_points_granted: i32,
    /// `UnlockedBy`, resolved to `PassiveSkillGraphId`s.
    pub unlocked_by: Vec<u32>,
    /// `VisibleForAscendancy` row index.
    pub visible_for_ascendancy: Option<usize>,
    /// `MasteryGroup` row index.
    pub mastery_group: Option<usize>,
    /// `GrantedSkill` (`SkillGems` row index).
    pub granted_skill: Option<usize>,
    pub stat_ids: Vec<String>,
    pub stat_values: Vec<i32>,
    /// One entry per rendered stat description (may contain newlines);
    /// `stat_lines` is the same text split into lines.
    pub stat_texts: Vec<String>,
    /// `PassiveSkills` row index.
    pub row: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct AscendancyInfo {
    pub id: String,
    pub name: String,
    pub character: Option<usize>,
    pub class_no: i32,
    pub illustration: Option<String>,
    pub tree_region_angle: i32,
    pub disabled: bool,
    pub base_ascendancy: Option<usize>,
    /// Row index into `SkillGraphDatabase::ui_art_ids`.
    pub ui_art: Option<usize>,
    pub flavour_text: String,
    /// `RGBFlavourTextColour` as "r,g,b".
    pub flavour_text_colour: String,
    /// `CoordinateRect` as "x,y,w,h".
    pub coordinate_rect: String,
    pub tree_region_vector: i32,
    /// Unnamed column after `BackgroundImage` (135 for every row).
    pub flavour_text_size: i32,
}

impl AscendancyInfo {
    /// Rows for classes not yet in PoE 2 are kept in the table as
    /// `[DNT-UNUSED] …` placeholders; the game never lays them out.
    pub fn is_enabled(&self) -> bool {
        !self.disabled && !self.name.starts_with("[DNT")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct CharacterInfo {
    pub id: String,
    pub name: String,
    pub illustration: Option<String>,
    /// Attribute folder under `UIImages/InGame/Classes/` (`Str`, `DexInt`, …).
    pub attr_dir: String,
    pub integer_id: i32,
    pub base_strength: i32,
    pub base_dexterity: i32,
    pub base_intelligence: i32,
    /// Unnamed float pair before `SkillTreeBackground`: where the class
    /// illustration sits relative to the tree centre.
    pub image_offset: (f32, f32),
}

/// `PassiveTreeDecorators` row: art anchored to a node (atlas blockers).
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct Decorator {
    pub node: u32,
    pub background: String,
    pub blocked: String,
    pub x: f32,
    pub y: f32,
    pub rotation_deg: f32,
    pub scale: f32,
    pub tree: Option<usize>,
}

#[derive(Debug, Default)]
pub struct SkillGraphDatabase {
    pub nodes: HashMap<u32, SkillGraphNodeInfo>,
    /// Art (icons/frames/connectors/backgrounds) per tree context — e.g.
    /// `"Character"`/`"Atlas"`/`"BrequelSkillTree"`. Populated by the caller
    /// (see `content_view.rs::build_skill_graph_db`) after `build()` returns,
    /// since it comes from a different set of DAT tables
    /// ([crate::ui::skill_tree_art]) than node/stat resolution.
    pub art_sets: HashMap<String, crate::ui::skill_tree_art::SkillTreeArtSet>,
    /// `PassiveSkillTreeUIArt` ids in row order (foreignrow targets).
    pub ui_art_ids: Vec<String>,
    /// `PassiveSkillTreeNodeFrameArt` rows in row order (foreignrow targets).
    pub node_frames: Vec<crate::ui::skill_tree_art::FrameArt>,
    pub ascendancies: Vec<AscendancyInfo>,
    pub characters: Vec<CharacterInfo>,
    pub decorators: Vec<Decorator>,
    /// `PassiveSkillGraphId` of every `PassiveSkills` row (0 when unset), so
    /// tables that reference passives by row can be resolved to graph ids.
    pub row_graph_ids: Vec<u32>,
}

impl SkillGraphDatabase {
    /// Characters that have at least one enabled ascendancy — the playable PoE 2 classes.
    pub fn playable_characters(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self
            .ascendancies
            .iter()
            .filter(|a| a.is_enabled())
            .filter_map(|a| a.character)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn ascendancies_of(&self, character: usize) -> Vec<usize> {
        (0..self.ascendancies.len())
            .filter(|&i| self.ascendancies[i].is_enabled() && self.ascendancies[i].character == Some(character))
            .collect()
    }

    pub fn ui_art_for_ascendancy(&self, asc: usize) -> Option<&crate::ui::skill_tree_art::SkillTreeArtSet> {
        let id = self.ascendancies.get(asc)?.ui_art.and_then(|i| self.ui_art_ids.get(i))?;
        self.art_sets.get(id).or_else(|| self.art_sets.get("Ascendancy"))
    }
}

/// Maps a `.psg` `graph_type` to the `PassiveSkillTreeUIArt` row `Id` that
/// applies to it. Hardcoded rather than DAT-driven because the DAT-level
/// join for this (`PassiveSkillTrees.PassiveSkillGraph`/`.UIArt`) was found
/// to be stale/unreliable in the same way `PassiveSkillTreeUIArt`'s own
/// columns were (see `skill_tree_art.rs`'s doc comment).
pub fn tree_context_for_graph_type(graph_type: u8) -> &'static str {
    match graph_type {
        1 => "Atlas",
        2 => "BrequelSkillTree",
        _ => "Character",
    }
}

/// The atlas tree's single full-canvas backdrop. Unlike the per-subtree
/// (Ritual/Breach/etc.) backgrounds, this isn't referenced by any DAT
/// row/column anywhere — `AtlasPassiveSkillSubTrees` only has entries for
/// the league-mechanic subtrees, and `AtlasGenericStart`'s own
/// `AtlasSubTree` foreignrow is unset. Confirmed present in the real bundle
/// index at this exact path, so it's hardcoded the same way
/// `tree_context_for_graph_type` already hardcodes the graph_type -> UIArt
/// context mapping (also not reliably DAT-joinable).
pub const ATLAS_MAIN_TREE_BG_PATH: &str =
    "art/textures/interface/2d/2dart/uiimages/ingame/atlasscreen/precursortheme/atlasmaintreebg.dds";

/// The community schema bundles both PoE1 and PoE2 definitions for tables
/// that exist in both games (distinguished by `validFor`: 1 or 2), with the
/// PoE1 one listed first — e.g. `PassiveSkills`/`Stats`/`Ascendancy`/
/// `Characters` all have two defs. Blindly taking the first match silently
/// picks the PoE1 shape, which happens to share column *order* with PoE2 for
/// early/common fields (so those still read correctly) but is simply
/// missing PoE2-only columns added later (like `PassiveSkills.AtlasSubTree`)
/// — this app is PoE2-only, so always prefer `validFor == 2` when present.
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

/// Index of the column `offset` places after `anchor`, if it is unnamed and
/// of type `ty` — the community schema leaves a few columns this code needs
/// unnamed, so they are located relative to a named neighbour.
fn unnamed_col_near(table: &Table, anchor: &str, offset: isize, ty: &str) -> Option<usize> {
    let base = col_index(table, anchor)? as isize + offset;
    let col = table.columns.get(usize::try_from(base).ok()?)?;
    (col.name.is_none() && col.r#type == ty && !col.array).then_some(base as usize)
}

fn as_row_index(val: &DatValue) -> Option<usize> {
    match val {
        DatValue::ForeignRow(idx) if *idx != usize::MAX => Some(*idx),
        DatValue::Int(i) if *i >= 0 => Some(*i as usize),
        DatValue::Long(l) => Some(*l as usize),
        _ => None,
    }
}

fn as_string(val: &DatValue) -> Option<String> {
    match val {
        DatValue::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn as_bool(val: &DatValue) -> bool {
    matches!(val, DatValue::Bool(true))
}

fn as_int(val: &DatValue) -> i32 {
    match val {
        DatValue::Int(i) => *i as i32,
        DatValue::Long(l) => *l as i32,
        _ => 0,
    }
}

fn as_foreign_row(val: &DatValue) -> Option<usize> {
    match val {
        DatValue::ForeignRow(idx) if *idx != usize::MAX => Some(*idx),
        _ => None,
    }
}

/// One named CSD source file (path used only for error messages) parsed
/// ahead of time by the caller so `build` can merge whichever set of
/// stat-description files applies to the graph_type being resolved.
pub struct StatCsdSource {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Parses `PassiveSkills`/`Stats`/`Ascendancy`/`AtlasPassiveSkillSubTrees`
/// DAT tables plus a caller-chosen set of stat-description CSD files into a
/// `PassiveSkillGraphId -> SkillGraphNodeInfo` map. All byte buffers are
/// read from the loaded GGPK/Bundles2 install by the caller.
/// `ascendancy_bytes`/`atlas_subtrees_bytes` are optional — the
/// corresponding illustrations just won't resolve if unavailable.
fn as_float(val: &DatValue) -> f32 {
    match val {
        DatValue::Float(f) => *f,
        DatValue::Int(i) => *i as f32,
        _ => 0.0,
    }
}

fn parse_characters(bytes: Vec<u8>, schema: &Schema) -> Result<Vec<CharacterInfo>, String> {
    let table = find_table(schema, "Characters")?;
    let reader = DatReader::new(bytes, "characters.datc64").map_err(|e| e.to_string())?;
    let id_col = col_index(table, "Id").ok_or("Characters missing Id")?;
    let name_col = col_index(table, "Name");
    let img_col = col_index(table, "PassiveTreeImage");
    let int_id_col = col_index(table, "IntegerId");
    let str_col = col_index(table, "BaseStrength");
    let dex_col = col_index(table, "BaseDexterity");
    let int_col = col_index(table, "BaseIntelligence");
    let off_x_col = unnamed_col_near(table, "SkillTreeBackground", -2, "f32");
    let off_y_col = unnamed_col_near(table, "SkillTreeBackground", -1, "f32");
    let mut out = Vec::with_capacity(reader.row_count as usize);
    for i in 0..reader.row_count {
        let row = reader.read_row(i, table).map_err(|e| e.to_string())?;
        let id = row.get(id_col).and_then(as_string).unwrap_or_default();
        // "Metadata/Characters/StrDex/StrDexFourb" -> "StrDex"
        let attr_dir = id.split('/').nth(2).unwrap_or("").to_string();
        let int = |c: Option<usize>| c.and_then(|c| row.get(c)).map(as_int).unwrap_or(0);
        let float = |c: Option<usize>| c.and_then(|c| row.get(c)).map(as_float).unwrap_or(0.0);
        out.push(CharacterInfo {
            name: name_col.and_then(|c| row.get(c)).and_then(as_string).unwrap_or_else(|| id.clone()),
            illustration: img_col.and_then(|c| row.get(c)).and_then(as_string),
            attr_dir,
            integer_id: int(int_id_col),
            base_strength: int(str_col),
            base_dexterity: int(dex_col),
            base_intelligence: int(int_col),
            image_offset: (float(off_x_col), float(off_y_col)),
            id,
        });
    }
    Ok(out)
}

fn parse_ascendancies(bytes: Vec<u8>, schema: &Schema) -> Result<Vec<AscendancyInfo>, String> {
    let table = find_table(schema, "Ascendancy")?;
    let reader = DatReader::new(bytes, "ascendancy.datc64").map_err(|e| e.to_string())?;
    let id_col = col_index(table, "Id").ok_or("Ascendancy missing Id")?;
    let get = |name: &str| col_index(table, name);
    let (name_col, char_col, class_col, img_col, angle_col, disabled_col, base_col, ui_col) = (
        get("Name"), get("Character"), get("ClassNo"), get("PassiveTreeImage"),
        get("TreeRegionAngle"), get("Disabled"), get("BaseAscendancy"), get("UIArt"),
    );
    let (flavour_col, colour_col, rect_col, vector_col) =
        (get("FlavourText"), get("RGBFlavourTextColour"), get("CoordinateRect"), get("TreeRegionVector"));
    let size_col = unnamed_col_near(table, "BackgroundImage", 1, "i32");
    let mut out = Vec::with_capacity(reader.row_count as usize);
    for i in 0..reader.row_count {
        let row = reader.read_row(i, table).map_err(|e| e.to_string())?;
        let text = |c: Option<usize>| c.and_then(|c| row.get(c)).and_then(as_string).unwrap_or_default();
        out.push(AscendancyInfo {
            id: row.get(id_col).and_then(as_string).unwrap_or_default(),
            name: name_col.and_then(|c| row.get(c)).and_then(as_string).unwrap_or_default(),
            character: char_col.and_then(|c| row.get(c)).and_then(as_foreign_row),
            class_no: class_col.and_then(|c| row.get(c)).map(as_int).unwrap_or(0),
            illustration: img_col.and_then(|c| row.get(c)).and_then(as_string),
            tree_region_angle: angle_col.and_then(|c| row.get(c)).map(as_int).unwrap_or(0),
            disabled: disabled_col.and_then(|c| row.get(c)).map(as_bool).unwrap_or(false),
            base_ascendancy: base_col.and_then(|c| row.get(c)).and_then(as_foreign_row),
            ui_art: ui_col.and_then(|c| row.get(c)).and_then(as_foreign_row),
            flavour_text: text(flavour_col),
            flavour_text_colour: text(colour_col),
            coordinate_rect: text(rect_col),
            tree_region_vector: vector_col.and_then(|c| row.get(c)).map(as_int).unwrap_or(0),
            flavour_text_size: size_col.and_then(|c| row.get(c)).map(as_int).unwrap_or(0),
        });
    }
    Ok(out)
}

/// `PassiveTreeDecorators`: the schema names only the art columns; the
/// unnamed ones are (node id, x, y, rotation, tree, scale) — verified against
/// the atlas blockers, which sit at 0/90° and scale 0.5 next to their node.
fn parse_decorators(bytes: Vec<u8>, schema: &Schema) -> Result<Vec<Decorator>, String> {
    let table = find_table(schema, "PassiveTreeDecorators")?;
    let reader = DatReader::new(bytes, "passivetreedecorators.datc64").map_err(|e| e.to_string())?;
    let bg_col = col_index(table, "BackgroundArt");
    let blocked_col = col_index(table, "BlockedArt");
    let tree_col = col_index(table, "SkillTree");
    let unnamed: Vec<usize> = table.columns.iter().enumerate().filter(|(_, c)| c.name.is_none()).map(|(i, _)| i).collect();
    let mut out = Vec::new();
    for i in 0..reader.row_count {
        let row = reader.read_row(i, table).map_err(|e| e.to_string())?;
        let u = |k: usize| unnamed.get(k).and_then(|&c| row.get(c));
        let node = u(1).map(as_int).unwrap_or(0);
        if node <= 0 {
            continue;
        }
        out.push(Decorator {
            node: node as u32,
            background: bg_col.and_then(|c| row.get(c)).and_then(as_string).unwrap_or_default(),
            blocked: blocked_col.and_then(|c| row.get(c)).and_then(as_string).unwrap_or_default(),
            x: u(2).map(as_float).unwrap_or(0.0),
            y: u(3).map(as_float).unwrap_or(0.0),
            rotation_deg: u(4).map(as_float).unwrap_or(0.0),
            scale: u(5).map(as_float).filter(|s| *s > 0.0).unwrap_or(1.0),
            tree: tree_col.and_then(|c| row.get(c)).and_then(as_foreign_row),
        });
    }
    Ok(out)
}

pub struct ExtraTables {
    pub ascendancy: Option<Vec<u8>>,
    pub atlas_subtrees: Option<Vec<u8>>,
    pub characters: Option<Vec<u8>>,
    pub decorators: Option<Vec<u8>>,
}

pub fn build(
    passiveskills_bytes: Vec<u8>,
    stats_bytes: Vec<u8>,
    stat_csd_sources: &[StatCsdSource],
    extra: ExtraTables,
    schema: &Schema,
) -> Result<SkillGraphDatabase, String> {
    let ExtraTables { ascendancy: ascendancy_bytes, atlas_subtrees: atlas_subtrees_bytes, characters: characters_bytes, decorators: decorators_bytes } = extra;
    let stats_table = find_table(schema, "Stats")?;
    let stats_reader = DatReader::new(stats_bytes, "stats.datc64").map_err(|e| e.to_string())?;
    let stats_id_col = col_index(stats_table, "Id").ok_or("Stats table missing Id column")?;

    let mut stat_ids: Vec<String> = Vec::with_capacity(stats_reader.row_count as usize);
    for i in 0..stats_reader.row_count {
        let row = stats_reader.read_row(i, stats_table).map_err(|e| e.to_string())?;
        stat_ids.push(row.get(stats_id_col).and_then(as_string).unwrap_or_default());
    }

    let csd_files: Vec<CsdFile> = stat_csd_sources
        .iter()
        .map(|s| csd::parse_csd(&s.bytes, &s.path))
        .collect::<Result<Vec<_>, _>>()?;
    let csd_refs: Vec<&CsdFile> = csd_files.iter().collect();
    let lookup = TranslationLookup::build(&csd_refs);

    // Ascendancy row index -> PassiveTreeImage, needed only for the (few)
    // nodes that are an ascendancy's starting node. Positioning is resolved
    // by the renderer from the node's own `.psg` group (see doc comment on
    // `SkillGraphNodeInfo::ascendancy_illustration`), not from any field here.
    let ascendancies = match ascendancy_bytes {
        Some(bytes) => parse_ascendancies(bytes, schema)?,
        None => Vec::new(),
    };
    let characters = match characters_bytes {
        Some(bytes) => parse_characters(bytes, schema)?,
        None => Vec::new(),
    };
    let decorators = match decorators_bytes {
        Some(bytes) => parse_decorators(bytes, schema).unwrap_or_default(),
        None => Vec::new(),
    };

    // AtlasPassiveSkillSubTrees row index -> (UI_Background, IllustrationX, IllustrationY), UI_Image.
    let atlas_subtrees: Vec<AtlasSubtreeArt> = if let Some(bytes) = atlas_subtrees_bytes {
        let table = find_table(schema, "AtlasPassiveSkillSubTrees")?;
        let reader = DatReader::new(bytes, "atlaspassiveskillsubtrees.datc64").map_err(|e| e.to_string())?;
        let bg_col = col_index(table, "UI_Background");
        let icon_col = col_index(table, "UI_Image");
        let ix_col = col_index(table, "IllustrationX");
        let iy_col = col_index(table, "IllustrationY");
        (0..reader.row_count)
            .map(|i| {
                let Ok(row) = reader.read_row(i, table) else { return (None, None) };
                let bg = bg_col.and_then(|c| row.get(c)).and_then(as_string).map(|bg| {
                    let ix = ix_col.and_then(|c| row.get(c)).map(as_int).unwrap_or(0) as f32;
                    let iy = iy_col.and_then(|c| row.get(c)).map(as_int).unwrap_or(0) as f32;
                    (bg, ix, iy)
                });
                (bg, icon_col.and_then(|c| row.get(c)).and_then(as_string))
            })
            .collect()
    } else {
        Vec::new()
    };

    let ps_table = find_table(schema, "PassiveSkills")?;
    let ps_reader = DatReader::new(passiveskills_bytes, "passiveskills.datc64").map_err(|e| e.to_string())?;

    let graph_id_col = col_index(ps_table, "PassiveSkillGraphId")
        .ok_or("PassiveSkills table missing PassiveSkillGraphId column")?;
    let name_col = col_index(ps_table, "Name").ok_or("PassiveSkills table missing Name column")?;
    let icon_col = col_index(ps_table, "Icon_DDSFile");
    let is_keystone_col = col_index(ps_table, "IsKeystone");
    let is_notable_col = col_index(ps_table, "IsNotable");
    let is_jewel_col = col_index(ps_table, "IsJewelSocket");
    let is_mastery_col = col_index(ps_table, "MasteryGroup");
    let is_just_icon_col = col_index(ps_table, "IsJustIcon");
    let is_ascendancy_start_col = col_index(ps_table, "IsAscendancyStartingNode");
    let is_multiple_choice_col = col_index(ps_table, "IsMultipleChoice");
    let ascendancy_key_col = col_index(ps_table, "Ascendancy");
    let atlas_subtree_col = col_index(ps_table, "AtlasSubTree");
    let is_attribute_col = col_index(ps_table, "IsAttribute");
    let node_frame_art_col = col_index(ps_table, "NodeFrameArt");
    let characters_col = col_index(ps_table, "Characters");
    let flavour_col = col_index(ps_table, "FlavourText");
    let stats_col = col_index(ps_table, "Stats");
    let stat_value_cols: Vec<Option<usize>> =
        (1..=7).map(|n| col_index(ps_table, &format!("Stat{}Value", n))).collect();
    let id_col = col_index(ps_table, "Id");
    let is_option_col = col_index(ps_table, "IsMultipleChoiceOption");
    let anoint_col = col_index(ps_table, "IsAnointmentOnly");
    let is_free_col = col_index(ps_table, "IsFree");
    let hide_connection_col = unnamed_col_near(ps_table, "IsFree", 1, "bool");
    let points_col = col_index(ps_table, "SkillPointsGranted");
    let weapon_points_col = col_index(ps_table, "WeaponPointsGranted");
    let unlocked_by_col = col_index(ps_table, "UnlockedBy");
    let visible_for_col = col_index(ps_table, "VisibleForAscendancy");
    let granted_skill_col = col_index(ps_table, "GrantedSkill");

    // `UnlockedBy` references rows of this same table, so graph ids are
    // needed for every row before any node is built.
    let mut row_graph_ids: Vec<u32> = Vec::with_capacity(ps_reader.row_count as usize);
    for i in 0..ps_reader.row_count {
        let gid = ps_reader
            .read_row(i, ps_table)
            .ok()
            .and_then(|row| row.get(graph_id_col).map(as_int))
            .unwrap_or(0);
        row_graph_ids.push(gid.max(0) as u32);
    }

    let mut nodes = HashMap::new();
    for i in 0..ps_reader.row_count {
        let row = match ps_reader.read_row(i, ps_table) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let graph_id = row.get(graph_id_col).map(as_int).unwrap_or(0);
        if graph_id <= 0 {
            continue;
        }

        let name = row.get(name_col).and_then(as_string).unwrap_or_default();
        let read_row_refs = |col: Option<usize>| -> Vec<usize> {
            match col.and_then(|c| row.get(c).map(|v| (c, v))) {
                Some((c, DatValue::List(count, offset))) if *count > 0 => ps_reader
                    .read_list_values(*offset, *count, &ps_table.columns[c])
                    .unwrap_or_default()
                    .iter()
                    .filter_map(as_row_index)
                    .collect(),
                _ => Vec::new(),
            }
        };

        let stat_id_list: Vec<String> = if let Some(sc) = stats_col {
            match row.get(sc) {
                Some(DatValue::List(count, offset)) => {
                    let list_col = &ps_table.columns[sc];
                    ps_reader
                        .read_list_values(*offset, *count, list_col)
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|v| match v {
                            DatValue::ForeignRow(idx) if idx != usize::MAX => stat_ids.get(idx).cloned(),
                            _ => None,
                        })
                        .collect()
                }
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let stat_values: Vec<i32> = stat_value_cols
            .iter()
            .filter_map(|c| c.and_then(|idx| row.get(idx)).map(as_int))
            .collect();
        // The client skips stats whose value is zero.
        let (stat_id_list, stat_values): (Vec<String>, Vec<i32>) = stat_id_list
            .into_iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let v = stat_values.get(i).copied().unwrap_or(0);
                (v != 0).then_some((id, v))
            })
            .unzip();

        let stat_texts = if stat_id_list.is_empty() { Vec::new() } else { lookup.translate_grouped(&stat_id_list, &stat_values) };
        let stat_lines: Vec<String> = stat_texts.iter().flat_map(|t| t.lines().map(|l| l.to_string())).collect();

        let is_ascendancy_start = is_ascendancy_start_col.and_then(|c| row.get(c)).map(as_bool).unwrap_or(false);
        let ascendancy = ascendancy_key_col.and_then(|c| row.get(c)).and_then(as_foreign_row);
        let ascendancy_illustration = if is_ascendancy_start {
            ascendancy.and_then(|idx| ascendancies.get(idx)).and_then(|a| a.illustration.clone())
        } else {
            None
        };

        let subtree = atlas_subtree_col
            .and_then(|c| row.get(c))
            .and_then(as_foreign_row)
            .and_then(|idx| atlas_subtrees.get(idx));
        let atlas_subtree_background = subtree.and_then(|s| s.0.clone());
        let atlas_subtree_icon = subtree.and_then(|s| s.1.clone());

        let characters: Vec<usize> = read_row_refs(characters_col);
        let unlocked_by: Vec<u32> = read_row_refs(unlocked_by_col)
            .into_iter()
            .filter_map(|r| row_graph_ids.get(r).copied().filter(|g| *g > 0))
            .collect();
        let flag = |c: Option<usize>| c.and_then(|c| row.get(c)).map(as_bool).unwrap_or(false);

        let info = SkillGraphNodeInfo {
            name,
            icon: icon_col.and_then(|c| row.get(c)).and_then(as_string),
            is_keystone: is_keystone_col.and_then(|c| row.get(c)).map(as_bool).unwrap_or(false),
            is_notable: is_notable_col.and_then(|c| row.get(c)).map(as_bool).unwrap_or(false),
            is_jewel_socket: is_jewel_col.and_then(|c| row.get(c)).map(as_bool).unwrap_or(false),
            // Notables can belong to a mastery group too; the mastery node itself is the icon-only one.
            is_mastery: flag(is_just_icon_col) && is_mastery_col.and_then(|c| row.get(c)).and_then(as_foreign_row).is_some(),
            is_ascendancy_start,
            is_multiple_choice: is_multiple_choice_col.and_then(|c| row.get(c)).map(as_bool).unwrap_or(false),
            flavour_text: flavour_col.and_then(|c| row.get(c)).and_then(as_string),
            stat_lines,
            ascendancy_illustration,
            atlas_subtree_background,
            atlas_subtree_icon,
            ascendancy,
            is_attribute: is_attribute_col.and_then(|c| row.get(c)).map(as_bool).unwrap_or(false),
            node_frame_art: node_frame_art_col.and_then(|c| row.get(c)).and_then(as_foreign_row),
            characters,
            id: id_col.and_then(|c| row.get(c)).and_then(as_string).unwrap_or_default(),
            is_multiple_choice_option: flag(is_option_col),
            is_anointment_only: flag(anoint_col),
            is_free: flag(is_free_col),
            hide_connection: flag(hide_connection_col),
            skill_points_granted: points_col.and_then(|c| row.get(c)).map(as_int).unwrap_or(0),
            weapon_points_granted: weapon_points_col.and_then(|c| row.get(c)).map(as_int).unwrap_or(0),
            unlocked_by,
            visible_for_ascendancy: visible_for_col.and_then(|c| row.get(c)).and_then(as_foreign_row),
            mastery_group: is_mastery_col.and_then(|c| row.get(c)).and_then(as_foreign_row),
            granted_skill: granted_skill_col.and_then(|c| row.get(c)).and_then(as_foreign_row),
            stat_ids: stat_id_list,
            stat_values,
            stat_texts,
            row: i as usize,
        };

        nodes.insert(graph_id as u32, info);
    }

    Ok(SkillGraphDatabase {
        nodes,
        art_sets: HashMap::new(),
        ui_art_ids: Vec::new(),
        node_frames: Vec::new(),
        ascendancies,
        characters,
        decorators,
        row_graph_ids,
    })
}
