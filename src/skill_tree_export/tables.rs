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

fn open<'a>(source: &'a TreeExportSource, name: &str) -> Option<Dat<'a>> {
    let table = source.schema.find_table(name, true)?;
    let path = format!("data/balance/{}.datc64", name.to_ascii_lowercase());
    let bytes = source.fetch(&path)?;
    let reader = DatReader::new(bytes, &path).ok()?;
    Some(Dat { reader, table })
}

pub fn load(source: &TreeExportSource, db: &SkillGraphDatabase, psg_path: &str) -> ExtraTables {
    let mut out = ExtraTables { tree_name: "Default".to_string(), ..Default::default() };
    let gid = |row: usize| db.row_graph_ids.get(row).copied().filter(|g| *g > 0);

    if let Some(trees) = open(source, "PassiveSkillTrees") {
        let wanted = psg_path.trim_end_matches(".psg").to_ascii_lowercase();
        for row in trees.rows() {
            let graph = trees.string(&row, "PassiveSkillGraph").to_ascii_lowercase();
            if !graph.is_empty() && graph == wanted {
                out.tree_name = trees.string(&row, "Id");
            }
        }
    }

    if let Some(t) = open(source, "ClassPassiveSkillOverrides") {
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
    if let Some(t) = open(source, "AscendancyPassiveSkillOverrides") {
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
    if let (Some(variants), Some(types)) = (open(source, "PassiveSkillVariants"), open(source, "PassiveSkillVariantTypes")) {
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

    if let Some(t) = open(source, "PassiveJewelSlots") {
        out.jewel_slots = t.rows().filter_map(|row| t.row_ref(&row, "Slot").and_then(gid)).collect();
    }

    if let (Some(groups), Some(art)) = (open(source, "PassiveSkillMasteryGroups"), open(source, "PassiveSkillTreeMasteryArt")) {
        for (i, row) in groups.rows().enumerate() {
            if let Some(image) = groups
                .row_ref(&row, "Art")
                .and_then(|a| art.row(a))
                .map(|r| art.string(&r, "ActiveEffectImage"))
                .filter(|s| !s.is_empty())
            {
                out.mastery_effect_images.insert(i, image);
            }
        }
    }

    if let (Some(recipes), Some(results), Some(items), Some(bases)) = (
        open(source, "BlightCraftingRecipes"),
        open(source, "BlightCraftingResults"),
        open(source, "BlightCraftingItems"),
        open(source, "BaseItemTypes"),
    ) {
        let mut item_names: HashMap<usize, String> = HashMap::new();
        for row in recipes.rows() {
            let Some(target) = recipes
                .row_ref(&row, "BlightCraftingResult")
                .and_then(|r| results.row(r))
                .and_then(|r| results.row_ref(&r, "PassiveSkill"))
                .and_then(gid)
            else {
                continue;
            };
            let mut names = Vec::new();
            for item in recipes.row_refs(&row, "BlightCraftingItems") {
                let name = item_names.entry(item).or_insert_with(|| {
                    items
                        .row(item)
                        .and_then(|r| items.row_ref(&r, "BaseItemType"))
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
        if let (Some(gems), Some(bases)) = (open(source, "SkillGems"), open(source, "BaseItemTypes")) {
            let visuals = open(source, "ItemVisualIdentity");
            for gem in gem_rows {
                let Some(base) = gems.row(gem).and_then(|r| gems.row_ref(&r, "BaseItemType")).and_then(|b| bases.row(b)) else { continue };
                let icon = visuals
                    .as_ref()
                    .and_then(|v| bases.row_ref(&base, "ItemVisualIdentity").and_then(|i| v.row(i)).map(|r| v.string(&r, "DDSFile")))
                    .unwrap_or_default();
                out.granted_skills.insert(gem, GrantedSkill { name: bases.string(&base, "Name"), icon });
            }
        }
    }

    if let Some(t) = open(source, "PassiveJewelRadiiArt") {
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
