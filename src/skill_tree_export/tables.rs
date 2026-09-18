//! DAT tables the web export needs beyond what the shared skill graph
//! database already resolves. Everything here is optional: a missing table
//! just leaves the matching fields out of `data.json`.

use super::TreeExportSource;
use crate::dat::reader::{DatReader, DatValue};
use crate::dat::schema::Table;
use crate::ui::atlas_node_db::SkillGraphDatabase;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct GrantedSkill {
    pub name: String,
    pub icon: String,
}

#[derive(Debug, Default)]
pub struct ExtraTables {
    /// `PassiveSkillTrees.Id` of the tree whose graph is being exported.
    pub tree_name: String,
    /// `(Characters row, skill graph id, override graph id)`.
    pub class_overrides: Vec<(usize, u32, u32)>,
    /// `(Ascendancy row, skill graph id, override graph id)`.
    pub ascendancy_overrides: Vec<(usize, u32, u32)>,
    pub jewel_slots: Vec<u32>,
    /// `PassiveSkillVariants` graph ids: the attribute choices every generic
    /// attribute node can become (listed first in `skillOverrides`).
    pub variants: Vec<u32>,
    /// `PassiveSkillMasteryGroups` row -> `ActiveEffectImage` path.
    pub mastery_effect_images: HashMap<usize, String>,
    /// Anoint recipes by graph id (distilled emotion names, spaces removed).
    pub recipes: HashMap<u32, Vec<String>>,
    /// `SkillGems` row -> gem name/icon.
    pub granted_skills: HashMap<usize, GrantedSkill>,
    /// Jewel radius ring textures (`PassiveJewelRadiiArt`), deduplicated.
    pub jewel_radius_art: Vec<String>,
    /// Graph ids of the passives a cluster jewel can add, which sit in no
    /// group because the jewel places them (PoE 1).
    pub expansion_pool: Vec<u32>,
    /// Jewel socket graph id -> the cluster jewel it anchors.
    pub expansion_jewels: HashMap<u32, ExpansionJewel>,
    /// The three attributes in the order the `Attributes` enumeration lists them.
    pub character_attributes: Vec<String>,
    /// One sheet per ascendancy panel: the base game's and each bloodline's.
    pub ascendancy_panels: Vec<AscendancyPanel>,
}

/// The art one ascendancy panel draws with, as PoE 1's tree export sheets it.
#[derive(Debug, Clone, Default)]
pub struct AscendancyPanel {
    pub sheet: String,
    /// `(sprite name, art path)`.
    pub images: Vec<(String, String)>,
}

/// What a socket contributes to a cluster jewel: its size and index, the
/// proxy node the jewel's own passives hang off, and the socket it replaces.
#[derive(Debug, Clone, Default)]
pub struct ExpansionJewel {
    pub size: i64,
    pub index: i64,
    pub proxy: Option<u32>,
    pub parent: Option<u32>,
}

/// The sprite names PoE 1's export gives the ascendancy frame columns.
const PANEL_FRAME_ART: [(&str, &str); 10] = [
    ("StartNode", "AscendancyMiddle"),
    ("PassiveFrameNormal", "AscendancyFrameSmallNormal"),
    ("PassiveFrameCanAllocate", "AscendancyFrameSmallCanAllocate"),
    ("PassiveFrameActive", "AscendancyFrameSmallAllocated"),
    ("NotableFrameNormal", "AscendancyFrameLargeNormal"),
    ("NotableFrameCanAllocate", "AscendancyFrameLargeCanAllocate"),
    ("NotableFrameActive", "AscendancyFrameLargeAllocated"),
    ("SocketFrameNormal", "CharmFrameNormal"),
    ("SocketFrameCanAllocate", "CharmFrameCanAllocate"),
    ("SocketFrameActive", "CharmFrameAllocated"),
];

const PANEL_BUTTON_ART: [(&str, &str); 3] = [
    ("AscendancyButtonNormal", "AscendancyButton"),
    ("AscendancyButtonHighlight", "AscendancyButtonHighlight"),
    ("AscendancyButtonPressed", "AscendancyButtonPressed"),
];

/// `DescendancyAzmeri` -> `azmeriBloodline`; the base game's panel is
/// `ascendancy`.
fn panel_sheet_name(id: &str) -> String {
    match id.strip_prefix("Descendancy").filter(|rest| !rest.is_empty()) {
        Some(rest) => {
            let mut chars = rest.chars();
            let first = chars.next().map(|c| c.to_ascii_lowercase()).unwrap_or_default();
            format!("{}{}Bloodline", first, chars.as_str())
        }
        None => "ascendancy".to_string(),
    }
}

/// The panel art each ascendancy and bloodline draws with, keyed the way the
/// official export names it.
fn ascendancy_panels(source: &TreeExportSource, db: &SkillGraphDatabase) -> Vec<AscendancyPanel> {
    let mut panels: Vec<AscendancyPanel> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let panel_of = |id: &str, panels: &mut Vec<AscendancyPanel>, index: &mut HashMap<String, usize>| -> usize {
        *index.entry(id.to_string()).or_insert_with(|| {
            panels.push(AscendancyPanel { sheet: panel_sheet_name(id), images: Vec::new() });
            panels.len() - 1
        })
    };

    let mut art_ids: Vec<String> = Vec::new();
    if let Some(t) = open(source, db, "PassiveSkillTreeUIArtAscendancy") {
        for row in t.rows() {
            let id = t.string(&row, "Id");
            art_ids.push(id.clone());
            let at = panel_of(&id, &mut panels, &mut index);
            for (column, sprite) in PANEL_FRAME_ART {
                let path = t.string(&row, column);
                if !path.is_empty() {
                    panels[at].images.push((sprite.to_string(), path));
                }
            }
        }
    }
    if let Some(t) = open(source, db, "UIArtAscendancy") {
        for row in t.rows() {
            let id = t.string(&row, "Id");
            let at = panel_of(&id, &mut panels, &mut index);
            for (column, sprite) in PANEL_BUTTON_ART {
                let path = t.string(&row, column);
                if !path.is_empty() {
                    panels[at].images.push((sprite.to_string(), path));
                }
            }
        }
    }
    // Every ascendancy's own backdrop hangs off the base panel; a bloodline's
    // hangs off the panel its `UIArt` names.
    if let Some(t) = open(source, db, "Ascendancy") {
        let at = panel_of("Default", &mut panels, &mut index);
        for row in t.rows() {
            let (id, path) = (t.string(&row, "Id"), t.string(&row, "BackgroundImage"));
            if !id.is_empty() && !path.is_empty() {
                panels[at].images.push((format!("Classes{}", id), path));
            }
        }
    }
    if let Some(t) = open(source, db, "Descendancy") {
        for row in t.rows() {
            let (id, path) = (t.string(&row, "Id"), t.string(&row, "BackgroundImage"));
            let art = t.row_ref(&row, "UIArt").and_then(|r| art_ids.get(r)).cloned();
            let Some(art) = art.filter(|_| !id.is_empty() && !path.is_empty()) else { continue };
            let at = panel_of(&art, &mut panels, &mut index);
            panels[at].images.push((format!("Classes{}", id), path));
        }
    }
    panels.retain(|p| !p.images.is_empty());
    panels
}

/// `STRENGTH` -> `Strength`, the spelling the tree export uses.
fn title_case(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

struct Dat<'a> {
    reader: DatReader,
    table: &'a Table,
}

impl<'a> Dat<'a> {
    fn col(&self, name: &str) -> Option<usize> {
        self.table.columns.iter().position(|c| c.name.as_deref() == Some(name))
    }

    fn row(&self, i: usize) -> Option<Vec<DatValue>> {
        if i >= self.reader.row_count as usize {
            return None;
        }
        self.reader.read_row(i as u32, self.table).ok()
    }

    fn string(&self, row: &[DatValue], name: &str) -> String {
        match self.col(name).and_then(|c| row.get(c)) {
            Some(DatValue::String(s)) => s.clone(),
            _ => String::new(),
        }
    }

    /// The first of `names` this table has, since PoE 1 suffixes its keys.
    fn pick<'n>(&self, names: &[&'n str]) -> &'n str {
        names.iter().copied().find(|n| self.col(n).is_some()).unwrap_or("")
    }

    fn int(&self, row: &[DatValue], name: &str) -> i64 {
        match self.col(name).and_then(|c| row.get(c)) {
            Some(DatValue::Int(i)) => *i,
            Some(DatValue::Long(l)) => *l as i64,
            _ => 0,
        }
    }

    fn row_ref(&self, row: &[DatValue], name: &str) -> Option<usize> {
        match self.col(name).and_then(|c| row.get(c)) {
            Some(DatValue::ForeignRow(i)) if *i != usize::MAX => Some(*i),
            Some(DatValue::Int(i)) if *i >= 0 => Some(*i as usize),
            _ => None,
        }
    }

    fn row_refs(&self, row: &[DatValue], name: &str) -> Vec<usize> {
        let Some(c) = self.col(name) else { return Vec::new() };
        match row.get(c) {
            Some(DatValue::List(count, offset)) if *count > 0 => self
                .reader
                .read_list_values(*offset, *count, &self.table.columns[c])
                .unwrap_or_default()
                .iter()
                .filter_map(|v| match v {
                    DatValue::ForeignRow(i) if *i != usize::MAX => Some(*i),
                    DatValue::Int(i) if *i >= 0 => Some(*i as usize),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn rows(&self) -> impl Iterator<Item = Vec<DatValue>> + '_ {
        (0..self.reader.row_count as usize).filter_map(move |i| self.row(i))
    }
}

fn open<'a>(source: &'a TreeExportSource, db: &SkillGraphDatabase, name: &str) -> Option<Dat<'a>> {
    let table = source.schema.find_table(name, db.is_poe2)?;
    // PoE 2 keeps its tables under `data/balance/`, PoE 1 one level up.
    let path = match db.is_poe2 {
        true => format!("data/balance/{}.datc64", name.to_ascii_lowercase()),
        false => format!("data/{}.datc64", name.to_ascii_lowercase()),
    };
    let bytes = source.fetch(&path)?;
    let reader = DatReader::new(bytes, &path).ok()?;
    Some(Dat { reader, table })
}

pub fn load(source: &TreeExportSource, db: &SkillGraphDatabase, psg_path: &str) -> ExtraTables {
    let mut out = ExtraTables { tree_name: "Default".to_string(), ..Default::default() };
    if !db.is_poe2 {
        out.ascendancy_panels = ascendancy_panels(source, db);
    }
    if let Some(attributes) = source.schema.find_enumeration("Attributes", db.is_poe2) {
        out.character_attributes = attributes
            .enumerators
            .iter()
            .flatten()
            .filter(|name| !name.eq_ignore_ascii_case("NONE"))
            .map(|name| title_case(name))
            .collect();
    }
    let gid = |row: usize| db.row_graph_ids.get(row).copied().filter(|g| *g > 0);

    if let Some(trees) = open(source, db, "PassiveSkillTrees") {
        let wanted = psg_path.trim_end_matches(".psg").to_ascii_lowercase();
        for row in trees.rows() {
            let graph = trees.string(&row, "PassiveSkillGraph").to_ascii_lowercase();
            if !graph.is_empty() && graph == wanted {
                out.tree_name = trees.string(&row, "Id");
            }
        }
    }

    if let Some(t) = open(source, db, "ClassPassiveSkillOverrides") {
        for row in t.rows() {
            if let (Some(c), Some(s), Some(o)) = (
                t.row_ref(&row, "CharacterToOverrideFor"),
                t.row_ref(&row, "SkillToOverride").and_then(gid),
                t.row_ref(&row, "Override").and_then(gid),
            ) {
                out.class_overrides.push((c, s, o));
            }
        }
    }
    if let Some(t) = open(source, db, "AscendancyPassiveSkillOverrides") {
        for row in t.rows() {
            if let (Some(a), Some(s), Some(o)) = (
                t.row_ref(&row, "AscendancyToOverrideFor"),
                t.row_ref(&row, "SkillToOverride").and_then(gid),
                t.row_ref(&row, "Override").and_then(gid),
            ) {
                out.ascendancy_overrides.push((a, s, o));
            }
        }
    }

    // Variant types carry two unnamed flags; the first marks the attribute
    // choices a generic attribute node offers (the web export lists those).
    if let (Some(variants), Some(types)) = (open(source, db, "PassiveSkillVariants"), open(source, db, "PassiveSkillVariantTypes")) {
        let flag_col = types.table.columns.iter().position(|c| c.name.is_none() && c.r#type == "bool");
        let is_choice = |row: usize| -> bool {
            types
                .row(row)
                .and_then(|r| flag_col.and_then(|c| r.get(c).cloned()))
                .map(|v| matches!(v, DatValue::Bool(true)))
                .unwrap_or(false)
        };
        for row in variants.rows() {
            if variants.row_ref(&row, "Type").map(is_choice).unwrap_or(false) {
                if let Some(g) = variants.row_ref(&row, "Variant").and_then(gid) {
                    out.variants.push(g);
                }
            }
        }
    }

    if let Some(t) = open(source, db, "PassiveJewelSlots") {
        let slot = t.pick(&["Slot", "Passive"]);
        out.jewel_slots = t.rows().filter_map(|row| t.row_ref(&row, slot).and_then(gid)).collect();
        for row in t.rows() {
            let Some(id) = t.row_ref(&row, slot).and_then(gid) else { continue };
            let size = t.row_ref(&row, "ClusterJewelSize");
            // PoE 1 points at the proxy passive itself, not at its slot row.
            let proxy_col = t.pick(&["ProxySlot", "Proxy"]);
            let proxy = t
                .row_ref(&row, proxy_col)
                .and_then(|r| gid(r).or_else(|| t.row(r).and_then(|r| t.row_ref(&r, slot)).and_then(gid)));
            let parent = t.row_ref(&row, t.pick(&["ReplacesSlot", "Parent"])).and_then(|r| t.row(r)).and_then(|r| t.row_ref(&r, slot)).and_then(gid);
            if size.is_none() && proxy.is_none() && parent.is_none() {
                continue;
            }
            out.expansion_jewels.insert(
                id,
                ExpansionJewel {
                    size: size.unwrap_or(0) as i64,
                    index: t.int(&row, "ClusterIndex"),
                    proxy,
                    parent,
                },
            );
        }
    }

    // A cluster jewel's own passives are named by the expansion tables rather
    // than placed in the graph, and PoE 1's tree export lists them all.
    let mut pool: Vec<u32> = Vec::new();
    let push = |id: Option<u32>, pool: &mut Vec<u32>| {
        if let Some(id) = id {
            if !pool.contains(&id) {
                pool.push(id);
            }
        }
    };
    if let Some(t) = open(source, db, "PassiveTreeExpansionSkills") {
        for row in t.rows() {
            push(t.row_ref(&row, t.pick(&["PassiveSkill", "PassiveSkillsKey"])).and_then(gid), &mut pool);
            push(t.row_ref(&row, t.pick(&["Mastery_PassiveSkill", "Mastery_PassiveSkillsKey"])).and_then(gid), &mut pool);
        }
    }
    if let Some(t) = open(source, db, "PassiveTreeExpansionSpecialSkills") {
        for row in t.rows() {
            push(t.row_ref(&row, t.pick(&["PassiveSkill", "PassiveSkillsKey"])).and_then(gid), &mut pool);
        }
    }
    out.expansion_pool = pool;

    out.mastery_effect_images = db.mastery_effect_images.clone();

    if let (Some(recipes), Some(results), Some(items), Some(bases)) = (
        open(source, db, "BlightCraftingRecipes"),
        open(source, db, "BlightCraftingResults"),
        open(source, db, "BlightCraftingItems"),
        open(source, db, "BaseItemTypes"),
    ) {
        let mut item_names: HashMap<usize, String> = HashMap::new();
        for row in recipes.rows() {
            let Some(target) = recipes
                .row_ref(&row, recipes.pick(&["BlightCraftingResult", "BlightCraftingResultsKey"]))
                .and_then(|r| results.row(r))
                .and_then(|r| results.row_ref(&r, "PassiveSkill"))
                .and_then(gid)
            else {
                continue;
            };
            let mut names = Vec::new();
            for item in recipes.row_refs(&row, recipes.pick(&["BlightCraftingItems", "BlightCraftingItemsKeys"])) {
                let name = item_names.entry(item).or_insert_with(|| {
                    items
                        .row(item)
                        .and_then(|r| items.row_ref(&r, items.pick(&["BaseItemType", "Oil"])))
                        .and_then(|b| bases.row(b))
                        .map(|r| bases.string(&r, "Name").replace(' ', ""))
                        .unwrap_or_default()
                });
                if !name.is_empty() {
                    names.push(name.clone());
                }
            }
            if !names.is_empty() {
                out.recipes.insert(target, names);
            }
        }
    }

    let gem_rows: Vec<usize> = db.nodes.values().filter_map(|n| n.granted_skill).collect();
    if !gem_rows.is_empty() {
        if let (Some(gems), Some(bases)) = (open(source, db, "SkillGems"), open(source, db, "BaseItemTypes")) {
            let visuals = open(source, db, "ItemVisualIdentity");
            for gem in gem_rows {
                let Some(base) = gems.row(gem).and_then(|r| gems.row_ref(&r, gems.pick(&["BaseItemType", "BaseItemTypesKey"]))).and_then(|b| bases.row(b)) else { continue };
                let icon = visuals
                    .as_ref()
                    .and_then(|v| bases.row_ref(&base, bases.pick(&["ItemVisualIdentity", "ItemVisualIdentityKey"])).and_then(|i| v.row(i)).map(|r| v.string(&r, "DDSFile")))
                    .unwrap_or_default();
                out.granted_skills.insert(gem, GrantedSkill { name: bases.string(&base, "Name"), icon });
            }
        }
    }

    if let Some(t) = open(source, db, "PassiveJewelRadiiArt") {
        for row in t.rows() {
            let id = t.string(&row, "Id");
            if id.starts_with("MTX") || id.starts_with("Abyss") {
                continue;
            }
            for col in ["Circle1", "Circle2", "Inverse1", "Inverse2"] {
                let path = t.string(&row, col);
                if !path.is_empty() && !out.jewel_radius_art.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
                    out.jewel_radius_art.push(path);
                }
            }
        }
    }

    out
}
