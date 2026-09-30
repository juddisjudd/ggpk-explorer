//! `Skills/*.lua` and `Gems.lua`, ported from PoB's `skills.lua`, with the
//! file lists `skillGemList.lua` helps PoB curate worked out from the game
//! data instead; PoE 1's `PearlSupports.lua` (`skillGemList.lua`); and PoE 2's
//! `Assets.lua` and `Skills/SkillAssets.lua` (`assets.lua`).
//!
//! PoB runs `skills.lua` over directive files it keeps by hand
//! (`Export/Skills/*.txt`). What those files say about the game — which
//! granted effects exist and which stat sets they carry — comes from the game
//! tables here. What they add by hand is left out: `statMap`, `#flags`
//! (`baseFlags`), `#baseMod` (`baseMods`), `#minionList`, `#addSkillTypes`,
//! `#hideFromSideBar` and the Lua blocks written between directives.

use crate::dat::relational::{LoadedTable, Row};
use crate::data_export::Ctx;
use crate::pob_export::statdesc::lua_literal;
use crate::pob_export::text::{escape_ggg_string, sanitise_text};
use crate::pob_export::{game, read_text, write, Table};
use crate::settings::Game;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

pub fn skills(ctx: &Ctx) -> Result<(), String> {
    let db = Db::load(ctx)?;
    let plan = derive_plan(&db)?;
    let mut gems = GemUse::default();
    for (file, entries) in &plan {
        let mut out = Table::new();
        for entry in entries {
            let Some(effect) = db.effects.by_id(&entry.id) else { continue };
            let skill = match db.game {
                Game::Poe2 => poe2_skill(&db, effect, entry, &mut gems),
                Game::Poe1 => poe1_skill(&db, effect, entry, &mut gems),
            };
            out.set(entry.id.as_str(), skill);
        }
        write(ctx, &format!("Skills/{}", file), out)?;
    }
    let gem_table = match db.game {
        Game::Poe2 => poe2_gems(&db, &gems),
        Game::Poe1 => poe1_gems(&db, &gems),
    };
    write(ctx, "Gems", gem_table)?;
    if db.game == Game::Poe1 {
        write(ctx, "PearlSupports", pearl_supports(&db))?;
    }
    Ok(())
}

/// `Skills/SkillAssets.lua` and `Assets.lua` (`assets.lua`, PoE 2): where each
/// skill icon, gem background and monster category picture sits in the
/// texture arrays PoB stacks them into. The arrays themselves are art, not
/// data, and are not written.
pub fn assets(ctx: &Ctx) -> Result<(), String> {
    let ui_images = parse_ui_images(&read_text(ctx, "Art/UIImages1.txt").unwrap_or_default());
    let mut icons = Sheet::new("gem-icons");
    for skill in ctx.table("ActiveSkills")?.rows() {
        let icon = skill.str("Icon_DDSFile");
        if !icon.is_empty() && valid_skill_name(skill.str("DisplayedName")) {
            icons.add(&icon.to_lowercase(), Meta::alias(icon));
        }
    }
    let mut backgrounds = Sheet::new("gem-backgrounds");
    let gems = ctx.table("SkillGems")?;
    let effects_col = gems.require(&["GemEffects", "GemVariants"])?;
    let hover_col = gems.require(&["UI_Image", "HoverImage"])?;
    for gem in gems.rows() {
        let hover = gem.str(hover_col);
        if hover.is_empty() {
            continue;
        }
        let name = ctx
            .rr
            .deref_list(gem, effects_col)
            .last()
            .and_then(|e| ctx.rr.deref_id(e.row(), "GrantedEffect"))
            .unwrap_or_default();
        // Since 0.5.5 the column names a UI image rather than a texture.
        let path = match hover.to_lowercase().ends_with(".dds") {
            true => Some(hover.to_string()),
            false => ui_images.get(&hover.to_lowercase()).map(|i| i.path.clone()),
        };
        if let Some(path) = path {
            backgrounds.add(&path, Meta::alias(&name));
        }
    }
    let mut skill_coords = Table::new();
    for sheet in [&icons, &backgrounds] {
        for (file, coords) in sheet.pack(ctx) {
            skill_coords.set(file, coords);
        }
    }
    write(ctx, "Skills/SkillAssets", Table::new().with("ddsCoords", skill_coords))?;

    let mut categories = Sheet::new("monster-categories");
    for category in ctx.table("MonsterCategories")?.rows() {
        let kind = category.str("Name");
        if kind.starts_with("[DNT") {
            continue;
        }
        let image = category.str(category.table.pick(&["Icon", "HudImage"]).unwrap_or("Icon"));
        if let Some(asset) = ui_images.get(&image.to_lowercase()) {
            let rect = (asset.x, asset.y, asset.width, asset.height);
            categories.add(&asset.path, Meta { alias: Some(kind.to_string()), rect: Some(rect) });
        }
    }
    let mut coords = Table::new();
    for (file, entries) in categories.pack(ctx) {
        coords.set(file, entries);
    }
    write(ctx, "Assets", Table::new().with("ddsCoords", coords))
}

/// `isValidSkillDisplayName`.
fn valid_skill_name(name: &str) -> bool {
    !name.is_empty() && !name.contains("DNT") && !name.contains("UNUSED") && !name.contains("???")
}

/// One line of `Art/UIImages1.txt` as `assetSheets.parseUIImages` reads it:
/// the texture it sits in and four numbers PoB calls x, y, width and height.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct UiImage {
    pub(crate) path: String,
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
    pub(crate) width: Option<f64>,
    pub(crate) height: Option<f64>,
}

/// `assetSheets.parseUIImages`: names and paths lower-cased, fields split on
/// spaces and quotes.
pub(crate) fn parse_ui_images(text: &str) -> HashMap<String, UiImage> {
    let mut out: HashMap<String, UiImage> = HashMap::new();
    for line in text.split(['\r', '\n']).filter(|l| !l.is_empty()) {
        let mut name = String::new();
        let fields = line.split(|c: char| c.is_ascii_whitespace() || c == '"').filter(|f| !f.is_empty());
        for (index, field) in fields.enumerate() {
            if index == 0 {
                name = field.to_lowercase();
                out.insert(name.clone(), UiImage::default());
                continue;
            }
            let Some(image) = out.get_mut(&name) else { continue };
            let number = field.parse::<f64>().ok();
            match index {
                1 => image.path = field.to_lowercase(),
                2 => image.x = number,
                3 => image.y = number,
                4 => image.width = number,
                5 => image.height = number,
                _ => {}
            }
        }
    }
    out
}

/// What `commonMetadata` records for one picture.
#[derive(Clone, Debug, PartialEq)]
struct Meta {
    alias: Option<String>,
    rect: Option<(Option<f64>, Option<f64>, Option<f64>, Option<f64>)>,
}

impl Meta {
    fn alias(alias: &str) -> Self {
        Self { alias: Some(alias.to_string()), rect: None }
    }
}

/// `assetSheets.newSheet`: the pictures one sheet stacks, by texture path.
struct Sheet {
    name: &'static str,
    files: BTreeMap<String, Vec<Meta>>,
}

impl Sheet {
    fn new(name: &'static str) -> Self {
        Self { name, files: BTreeMap::new() }
    }

    /// `assetSheets.addToSheet`: one entry per alias for each texture.
    fn add(&mut self, path: &str, meta: Meta) {
        if path.is_empty() {
            return;
        }
        let list = self.files.entry(path.to_string()).or_default();
        if !list.iter().any(|m| m.alias == meta.alias) {
            list.push(meta);
        }
    }

    /// `assetSheets.calculateDDSPack`: textures grouped by size and format,
    /// each group one stacked file, and every alias's position in its stack.
    fn pack(&self, ctx: &Ctx) -> Vec<(String, Table)> {
        let paths: Vec<String> = self.files.keys().cloned().collect();
        let bytes = ctx.files.fetch_many(&paths);
        let mut stacks: BTreeMap<String, Vec<&String>> = BTreeMap::new();
        for path in &paths {
            let info = bytes
                .get(path)
                .map(|b| dds_payload(ctx, b))
                .and_then(|b| dds_info(&b));
            let Some((width, height, format)) = info else { continue };
            stacks.entry(format!("{}_{}_{}", width, height, format)).or_default().push(path);
        }
        let mut out = Vec::new();
        for (ident, paths) in stacks {
            let mut coords = Table::new();
            for (position, path) in paths.iter().enumerate() {
                for meta in &self.files[*path] {
                    let key = meta.alias.clone().unwrap_or_else(|| (*path).clone());
                    let position = position + 1;
                    match meta.rect {
                        Some((Some(x), Some(y), Some(w), Some(h))) => coords.set(key, Table::list([x, y, w, h, position as f64])),
                        _ => coords.set(key, position),
                    };
                }
            }
            out.push((format!("{}_{}.dds.zst", self.name, ident), coords));
        }
        out
    }
}

/// A texture's bytes, following the `*path` redirect some files hold.
fn dds_payload(ctx: &Ctx, bytes: &[u8]) -> Vec<u8> {
    if let Some(target) = bytes.strip_prefix(b"*") {
        let target = String::from_utf8_lossy(target).trim().to_string();
        if let Some(b) = crate::dat::relational::FileSource::fetch(ctx.files, &target) {
            return b;
        }
    }
    bytes.to_vec()
}

/// Width, height and format name of a DDS texture, as SimpleGraphic's
/// `Texture:Info()` reports them (`BC1`, `BC7`, `RGBA`, …).
fn dds_info(bytes: &[u8]) -> Option<(u32, u32, String)> {
    let at = bytes.windows(4).take(64).position(|w| w == b"DDS ")?;
    let b = &bytes[at..];
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let height = u32_at(12)?;
    let width = u32_at(16)?;
    let flags = u32_at(80)?;
    let four_cc = b.get(84..88)?;
    let bits = u32_at(88)?;
    let format = if flags & 0x4 != 0 {
        match four_cc {
            b"DXT1" => "BC1".to_string(),
            b"DXT2" | b"DXT3" => "BC2".to_string(),
            b"DXT4" | b"DXT5" => "BC3".to_string(),
            b"ATI1" | b"BC4U" => "BC4".to_string(),
            b"ATI2" | b"BC5U" => "BC5".to_string(),
            b"DX10" => match u32_at(128)? {
                70..=72 => "BC1".to_string(),
                73..=75 => "BC2".to_string(),
                76..=78 => "BC3".to_string(),
                79..=81 => "BC4".to_string(),
                82..=84 => "BC5".to_string(),
                94..=96 => "BC6H".to_string(),
                97..=99 => "BC7".to_string(),
                27..=32 | 87..=93 => "RGBA".to_string(),
                other => format!("DXGI{}", other),
            },
            other => String::from_utf8_lossy(other).into_owned(),
        }
    } else if bits == 32 {
        "RGBA".to_string()
    } else {
        "RGB".to_string()
    };
    Some((width, height, format))
}

/// One `#skill` directive: the granted effect, PoB's name for it where the
/// game has none, and the stat sets (`#set`, PoE 2) it lists.
#[derive(Clone, Debug, Default)]
struct Entry {
    id: String,
    name: Option<String>,
    no_gem: bool,
    from: Option<String>,
    sets: Vec<String>,
}

/// The files `skills.lua` writes, in the order it writes them.
fn skill_files(game: Game) -> &'static [&'static str] {
    match game {
        Game::Poe2 => &["act_str", "act_dex", "act_int", "other", "minion", "spectre", "sup_str", "sup_dex", "sup_int"],
        Game::Poe1 => &["act_str", "act_dex", "act_int", "other", "glove", "minion", "spectre", "sup_str", "sup_dex", "sup_int"],
    }
}

/// Collects the granted effects for each skill file, each effect once.
struct Planner<'a, 'c> {
    db: &'a Db<'c>,
    files: Vec<(String, Vec<Entry>)>,
    placed: HashSet<usize>,
}

impl<'a, 'c> Planner<'a, 'c> {
    fn new(db: &'a Db<'c>) -> Self {
        let files = skill_files(db.game).iter().map(|f| (f.to_string(), Vec::new())).collect();
        Self { db, files, placed: HashSet::new() }
    }

    fn place(&mut self, file: &str, effect: usize, from: Option<&str>) {
        if !self.placed.insert(effect) {
            return;
        }
        let Some(row) = self.db.effects.row(effect) else { return };
        let game = self.db.game;
        let id = row.id();
        let overrides: &[(&str, &str)] = match game {
            Game::Poe2 => &POE2_FILE_OVERRIDES,
            Game::Poe1 => &POE1_FILE_OVERRIDES,
        };
        let file = overrides.iter().find(|(e, _)| *e == id).map(|(_, f)| *f).unwrap_or(file);
        let names: &[(&str, &str)] = match game {
            Game::Poe2 => &POE2_NAMES,
            Game::Poe1 => &POE1_NAMES,
        };
        let name = names.iter().find(|(e, _)| *e == id).map(|(_, n)| n.to_string());
        let sets = match self.db.game {
            Game::Poe2 => shown_sets(self.db, row),
            Game::Poe1 => Vec::new(),
        };
        let entry = Entry {
            id: id.to_string(),
            name,
            no_gem: NO_GEM.contains(&id),
            from: from.map(str::to_string),
            sets,
        };
        if let Some((_, list)) = self.files.iter_mut().find(|(f, _)| f == file) {
            list.push(entry);
        }
    }
}

/// The stat sets `skillGemList.lua` lists for an effect: its own, then the
/// additional ones, leaving out those labelled `Hidden`.
fn shown_sets(db: &Db, effect: Row<'_>) -> Vec<String> {
    let ctx = db.ctx;
    let mut sets: Vec<crate::dat::relational::Ref> = ctx.rr.deref(effect, db.col.primary_set).into_iter().collect();
    sets.extend(ctx.rr.deref_list(effect, db.col.extra_sets));
    sets.iter()
        .filter(|s| ctx.rr.deref_id(s.row(), "Label").as_deref() != Some("Hidden"))
        .map(|s| s.id())
        .collect()
}

/// Which file each granted effect goes in, from the game data: gems by
/// colour, then the skills items and passives grant, then player minions'
/// and spectres' skills.
fn derive_plan(db: &Db) -> Result<Vec<(String, Vec<Entry>)>, String> {
    let ctx = db.ctx;
    let mut plan = Planner::new(db);
    let grants = gem_grants(db);
    for gem in db.gems.rows() {
        let base = db.gem_base(gem);
        let base_name = base.as_ref().map(|b| b.row().string("Name")).unwrap_or_default();
        let effects: Vec<Row<'_>> = gem.list_keys(db.col.gem_effects).into_iter().filter_map(|k| db.gem_effects.row(k)).collect();
        for ge in &effects {
            let granted = ge.key("GrantedEffect").and_then(|k| db.effects.row(k));
            let active = granted.and_then(|g| db.active_skill(g));
            let support = db.is_support_gem(gem);
            if unused_gem(db, gem, ge, &effects, &base_name, active) {
                continue;
            }
            let file = if support {
                if db.game == Game::Poe2 && !db.support_gem_by_gem.contains_key(&gem.index) {
                    continue;
                }
                match gem.int("GemColour") {
                    1 => "sup_str",
                    2 => "sup_dex",
                    3 => "sup_int",
                    _ => "other",
                }
            } else {
                match gem_colour(gem) {
                    1 => "act_str",
                    2 => "act_dex",
                    3 => "act_int",
                    _ => "other",
                }
            };
            let from = grants.get(&gem.index).copied();
            let mut list: Vec<usize> = ge.key("GrantedEffect").into_iter().collect();
            match db.game {
                Game::Poe2 => list.extend(ge.list_keys("AdditionalGrantedEffects")),
                Game::Poe1 => list.extend(ge.key("GrantedEffect2")),
            }
            for effect in list {
                plan.place(file, effect, from);
            }
        }
    }
    // PoE 1 grants item and passive skills directly. Enchantment mods grant
    // the glove skills.
    let mut item_granted: Vec<(usize, &str)> = Vec::new();
    let mut tree_granted: Vec<usize> = Vec::new();
    if db.game == Game::Poe1 {
        let per_level = &db.per_level;
        let effect_of = |k: usize| per_level.row(k).and_then(|r| r.key("GrantedEffect"));
        if let Some(mods) = ctx.optional_table("Mods") {
            let col = mods.pick(&["GrantedEffectsPerLevelKeys", "GrantedEffectsPerLevel"]).unwrap_or("GrantedEffectsPerLevelKeys");
            for m in mods.rows() {
                if ctx.rr.enum_label(m, "Domain").as_deref() == Some("MONSTER") {
                    continue;
                }
                let file = match ctx.rr.enum_label(m, "GenerationType").as_deref() {
                    Some("ENCHANTMENT") => "glove",
                    _ => "other",
                };
                item_granted.extend(m.list_keys(col).into_iter().filter_map(effect_of).map(|e| (e, file)));
            }
        }
        if let Some(passives) = ctx.optional_table("PassiveSkills").filter(|t| t.has_col("GrantedEffectsPerLevel")) {
            for p in passives.rows() {
                tree_granted.extend(p.key("GrantedEffectsPerLevel").and_then(effect_of));
            }
        }
        for (effect, file) in &item_granted {
            plan.place(file, *effect, Some("item"));
        }
        for effect in &tree_granted {
            plan.place("other", *effect, Some("tree"));
        }
        // A gem PoB files with the item skills is flagged the same way.
        let item: HashSet<usize> = item_granted.iter().map(|(e, _)| *e).collect();
        let tree: HashSet<usize> = tree_granted.iter().copied().collect();
        for (file, entries) in plan.files.iter_mut() {
            if file != "other" {
                continue;
            }
            for entry in entries.iter_mut().filter(|e| e.from.is_none()) {
                let Some(effect) = db.effects.by_id(&entry.id).map(|r| r.index) else { continue };
                if item.contains(&effect) {
                    entry.from = Some("item".into());
                } else if tree.contains(&effect) {
                    entry.from = Some("tree".into());
                }
            }
        }
    }
    for (effect, minion) in monster_skills(db) {
        let file = if minion { "minion" } else { "spectre" };
        plan.place(file, effect, None);
    }
    Ok(plan.files)
}

/// PoE 2 grants item and passive skills as gems. A gem no uncut gem can
/// become is `from tree` when a passive grants it and `from item` when an item
/// mod, a base item or a weapon's default attack does.
fn gem_grants(db: &Db) -> HashMap<usize, &'static str> {
    let ctx = db.ctx;
    let mut item = HashSet::new();
    let mut tree = HashSet::new();
    if db.game != Game::Poe2 {
        return HashMap::new();
    }
    if let Some(t) = ctx.optional_table("PassiveSkills").filter(|t| t.has_col("GrantedSkill")) {
        tree.extend(t.rows().filter_map(|r| r.key("GrantedSkill")));
    }
    if let Some(t) = ctx.optional_table("ModGrantedSkills") {
        item.extend(t.rows().filter_map(|r| r.key("Skill")));
    }
    for (table, column) in [("ItemInherentSkills", "SkillsGranted"), ("CharacterMeleeSkills", "SkillGems")] {
        if let Some(t) = ctx.optional_table(table) {
            item.extend(t.rows().flat_map(|r| r.list_keys(column)));
        }
    }
    let mut out = HashMap::new();
    for gem in db.gems.rows() {
        if db.is_support_gem(gem) || !gem.list_keys("CraftingTypes").is_empty() {
            continue;
        }
        if tree.contains(&gem.index) {
            out.insert(gem.index, "tree");
        } else if item.contains(&gem.index) {
            out.insert(gem.index, "item");
        }
    }
    out
}

/// `skillGemList.lua`'s test for gems the developers left in unused.
fn unused_gem(db: &Db, gem: Row<'_>, ge: &Row<'_>, all: &[Row<'_>], base_name: &str, active: Option<Row<'_>>) -> bool {
    let id = ge.id();
    let shown = active.map(|a| a.str("DisplayedName")).unwrap_or("");
    match db.game {
        Game::Poe2 => {
            id.contains("Unknown")
                || id.contains("Unusable")
                || id.contains("Playtest")
                || base_name.contains("DNT")
                || shown.contains("DNT")
                || all.iter().any(|v| v.str("SupportText").to_lowercase().contains("dnt-unused"))
                || active.is_some_and(|a| a.str("Description").to_lowercase().contains("dnt-unused"))
        }
        Game::Poe1 => {
            let text = ge.str("SupportText");
            let common = id.contains("Unknown") || id.contains("Playtest") || id.contains("Royale") || text.contains("DNT");
            if db.is_support_gem(gem) {
                common
                    || ["DNT", "UNUSED", "NOT CURRENTLY USED", "WIP", "Unnamed"].iter().any(|w| base_name.contains(w))
            } else {
                let hard_mode = ge.key("GrantedEffect").and_then(|k| db.effects.row(k)).is_some_and(|g| g.id().contains("HardMode"));
                common
                    || shown.is_empty()
                    || ["...", "DNT", "UNUSED", "NOT CURRENTLY USED", "Unnamed"].iter().any(|w| shown.contains(w))
                    || hard_mode
                    || base_name.contains("DNT")
                    || (gem.bool("IsVaalVariant") && ge.int("ItemColor") != 5)
            }
        }
    }
}

/// Monster skills: the effect row and whether a player minion uses it,
/// minions first.
fn monster_skills(db: &Db) -> Vec<(usize, bool)> {
    let ctx = db.ctx;
    let Some(mv) = ctx.optional_table("MonsterVarieties") else { return Vec::new() };
    let Some(ge_col) = mv.pick(&["GrantedEffects", "GrantedEffectsKeys"]) else { return Vec::new() };
    let (minions, spectres) = monster_lists(db);
    let mut out = Vec::new();
    for (ids, minion) in [(minions, true), (spectres, false)] {
        for id in ids {
            let Some(m) = mv.by_id(&id) else { continue };
            out.extend(m.list_keys(ge_col).into_iter().map(|k| (k, minion)));
        }
    }
    out
}

/// The player minions and spectres whose skills the minion and spectre files
/// hold: the same varieties `Minions.lua` and `Spectres.lua` list.
fn monster_lists(db: &Db) -> (Vec<String>, Vec<String>) {
    super::monsters::monster_ids(db.ctx).unwrap_or_default()
}

/// Gem effects the skill files touched, and the names `skills.lua` found for
/// them, which `Gems.lua` reuses.
#[derive(Default)]
struct GemUse {
    used: HashSet<String>,
    true_names: HashMap<String, String>,
}

/// PoB's fix for two gems whose names collide once " Support" is dropped.
const FULL_NAME_GEMS: [&str; 1] = ["Metadata/Items/Gems/SupportGemBarrage"];

/// `weaponClassMap`: item class ids to the weapon type names PoB uses.
fn weapon_type(game: Game, class: &str) -> Option<&'static str> {
    let poe2: &[(&str, &str)] = &[
        ("Claw", "Claw"),
        ("Dagger", "Dagger"),
        ("One Hand Sword", "One Hand Sword"),
        ("Thrusting One Hand Sword", "Thrusting One Hand Sword"),
        ("One Hand Axe", "One Hand Axe"),
        ("One Hand Mace", "One Hand Mace"),
        ("Bow", "Bow"),
        ("Crossbow", "Crossbow"),
        ("Fishing Rod", "Fishing Rod"),
        ("Warstaff", "Staff"),
        ("Two Hand Sword", "Two Hand Sword"),
        ("Two Hand Axe", "Two Hand Axe"),
        ("Two Hand Mace", "Two Hand Mace"),
        ("Unarmed", "None"),
        ("Flail", "Flail"),
        ("Spear", "Spear"),
        ("Talisman", "Talisman"),
    ];
    let poe1: &[(&str, &str)] = &[
        ("Claw", "Claw"),
        ("Dagger", "Dagger"),
        ("Wand", "Wand"),
        ("One Hand Sword", "One Handed Sword"),
        ("Thrusting One Hand Sword", "Thrusting One Handed Sword"),
        ("One Hand Axe", "One Handed Axe"),
        ("One Hand Mace", "One Handed Mace"),
        ("Bow", "Bow"),
        ("FishingRod", "Fishing Rod"),
        ("Staff", "Staff"),
        ("Two Hand Sword", "Two Handed Sword"),
        ("Two Hand Axe", "Two Handed Axe"),
        ("Two Hand Mace", "Two Handed Mace"),
        ("Sceptre", "Sceptre"),
        ("Unarmed", "None"),
    ];
    let map = match game {
        Game::Poe2 => poe2,
        Game::Poe1 => poe1,
    };
    map.iter().find(|(id, _)| *id == class).map(|(_, name)| *name)
}

/// The tables `skills.lua` reads, with the lookups it makes by scanning them.
struct Db<'c> {
    ctx: &'c Ctx<'c>,
    game: Game,
    effects: Rc<LoadedTable>,
    per_level: Rc<LoadedTable>,
    sets: Rc<LoadedTable>,
    set_levels: Rc<LoadedTable>,
    quality: Option<Rc<LoadedTable>>,
    gems: Rc<LoadedTable>,
    gem_effects: Rc<LoadedTable>,
    active_skills: Rc<LoadedTable>,
    exp: Option<Rc<LoadedTable>>,
    support_gems: Option<Rc<LoadedTable>>,
    /// Stat id and whether it is flagged as not granted to minions, by row.
    stats: Vec<(String, bool)>,
    /// `EffectivenessCostConstants` multipliers, by row.
    interpolation_bases: Vec<f64>,
    col: Cols,
    levels_by_effect: HashMap<usize, Vec<usize>>,
    set_levels_by_set: HashMap<usize, Vec<usize>>,
    set_levels_by_effect: HashMap<usize, Vec<usize>>,
    quality_by_effect: HashMap<usize, Vec<usize>>,
    gem_effect_by_primary: HashMap<usize, usize>,
    gem_effect_by_secondary: HashMap<usize, usize>,
    gem_effects_by_additional: HashMap<usize, Vec<usize>>,
    gem_by_effect_id: HashMap<String, usize>,
    exp_by_type: HashMap<usize, Vec<usize>>,
    support_gem_by_gem: HashMap<usize, usize>,
    /// PoE 1: each active skill's own description file.
    stat_scope: HashMap<String, String>,
}

/// The column names this game's schema uses for what PoB's spec calls them.
struct Cols {
    primary_set: &'static str,
    extra_sets: &'static str,
    gem_effects: &'static str,
    gem_base: &'static str,
    set_level_set: &'static str,
    quality_effect: &'static str,
    quality_stats: &'static str,
    exp_type: &'static str,
}

impl<'c> Db<'c> {
    fn load(ctx: &'c Ctx<'c>) -> Result<Self, String> {
        let game = game(ctx);
        let effects = ctx.table("GrantedEffects")?;
        let per_level = ctx.table("GrantedEffectsPerLevel")?;
        let sets = ctx.table("GrantedEffectStatSets")?;
        let set_levels = ctx.table("GrantedEffectStatSetsPerLevel")?;
        let quality = ctx.optional_table("GrantedEffectQualityStats");
        let gems = ctx.table("SkillGems")?;
        let gem_effects = ctx.table("GemEffects")?;
        let active_skills = ctx.table("ActiveSkills")?;
        let exp = ctx.optional_table("ItemExperiencePerLevel");
        let support_gems = match game {
            Game::Poe2 => ctx.optional_table("SupportGems"),
            Game::Poe1 => None,
        };
        let col = Cols {
            primary_set: effects.require(&["StatSet", "StatSet1"])?,
            extra_sets: effects.require(&["AdditionalStatSets", "StatSet2"])?,
            gem_effects: gems.require(&["GemEffects", "GemVariants"])?,
            gem_base: gems.require(&["BaseItemType", "BaseItemTypesKey"])?,
            set_level_set: set_levels.require(&["StatSet"])?,
            quality_effect: quality.as_ref().and_then(|q| q.pick(&["GrantedEffect", "GrantedEffectsKey"])).unwrap_or("GrantedEffect"),
            quality_stats: quality.as_ref().and_then(|q| q.pick(&["Stats", "StatsKeys"])).unwrap_or("Stats"),
            exp_type: "ItemExperienceType",
        };

        let stats_table = ctx.table("Stats")?;
        let no_minion_col = stats_table.column_or_after(&["CannotGrantToMinion"], "Category", 1);
        let stats = stats_table
            .rows()
            .map(|r| (r.id().to_string(), no_minion_col.map(|c| r.bool_at(c)).unwrap_or(false)))
            .collect();
        let interpolation_bases = ctx
            .optional_table("EffectivenessCostConstants")
            .map(|t| {
                let c = t.pick(&["Multiplier", "Value"]).unwrap_or("Multiplier");
                t.rows().map(|r| r.float(c) as f64).collect()
            })
            .unwrap_or_default();

        let group = |table: &LoadedTable, column: &str| -> HashMap<usize, Vec<usize>> {
            let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
            for row in table.rows() {
                if let Some(k) = row.key(column) {
                    out.entry(k).or_default().push(row.index);
                }
            }
            out
        };
        let levels_by_effect = group(&per_level, "GrantedEffect");
        let set_levels_by_set = group(&set_levels, col.set_level_set);
        let mut set_levels_by_effect: HashMap<usize, Vec<usize>> = HashMap::new();
        for row in set_levels.rows() {
            let mut seen = HashSet::new();
            for k in row.list_keys("GrantedEffects") {
                if seen.insert(k) {
                    set_levels_by_effect.entry(k).or_default().push(row.index);
                }
            }
        }
        let quality_by_effect = quality.as_ref().map(|q| group(q, col.quality_effect)).unwrap_or_default();
        let first = |table: &LoadedTable, column: &str| -> HashMap<usize, usize> {
            let mut out = HashMap::new();
            if table.has_col(column) {
                for row in table.rows() {
                    if let Some(k) = row.key(column) {
                        out.entry(k).or_insert(row.index);
                    }
                }
            }
            out
        };
        let gem_effect_by_primary = first(&gem_effects, "GrantedEffect");
        let gem_effect_by_secondary = first(&gem_effects, "GrantedEffect2");
        let mut gem_effects_by_additional: HashMap<usize, Vec<usize>> = HashMap::new();
        if gem_effects.has_col("AdditionalGrantedEffects") {
            for row in gem_effects.rows() {
                let mut seen = HashSet::new();
                for k in row.list_keys("AdditionalGrantedEffects") {
                    if seen.insert(k) {
                        gem_effects_by_additional.entry(k).or_default().push(row.index);
                    }
                }
            }
        }
        let mut gem_by_effect_id: HashMap<String, usize> = HashMap::new();
        for gem in gems.rows() {
            for k in gem.list_keys(col.gem_effects) {
                if let Some(effect) = gem_effects.row(k) {
                    gem_by_effect_id.entry(effect.id().to_string()).or_insert(gem.index);
                }
            }
        }
        let exp_by_type = exp.as_ref().map(|t| group(t, col.exp_type)).unwrap_or_default();
        let support_gem_by_gem = support_gems.as_ref().map(|t| first(t, "SkillGem")).unwrap_or_default();

        let mut stat_scope = HashMap::new();
        if game == Game::Poe1 {
            if let Some(text) = read_text(ctx, "Metadata/StatDescriptions/skillpopup_stat_filters.txt") {
                stat_scope = skill_stat_scopes(&text);
            }
        }

        Ok(Self {
            ctx,
            game,
            effects,
            per_level,
            sets,
            set_levels,
            quality,
            gems,
            gem_effects,
            active_skills,
            exp,
            support_gems,
            stats,
            interpolation_bases,
            col,
            levels_by_effect,
            set_levels_by_set,
            set_levels_by_effect,
            quality_by_effect,
            gem_effect_by_primary,
            gem_effect_by_secondary,
            gem_effects_by_additional,
            gem_by_effect_id,
            exp_by_type,
            support_gem_by_gem,
            stat_scope,
        })
    }

    fn stat_id(&self, row: usize) -> String {
        self.stats.get(row).map(|s| s.0.clone()).unwrap_or_default()
    }

    fn no_minion(&self, row: usize) -> bool {
        self.stats.get(row).map(|s| s.1).unwrap_or(false)
    }

    /// `SkillType.<Id>`: PoB's `Global.lua` numbers the skill types by their
    /// row in `ActiveSkillType`, counting from 1.
    fn skill_types(&self, row: Row<'_>, column: &str) -> Vec<i64> {
        row.list_keys(column).into_iter().map(|k| k as i64 + 1).collect()
    }

    fn active_skill(&self, effect: Row<'_>) -> Option<Row<'_>> {
        effect.key("ActiveSkill").and_then(|k| self.active_skills.row(k))
    }

    fn gem_base(&self, gem: Row<'_>) -> Option<crate::dat::relational::Ref> {
        self.ctx.rr.deref(gem, self.col.gem_base)
    }

    /// `GemEffects:GetRow("GrantedEffect", granted)`, whether it came from
    /// the secondary effect column, and the first gem listing it.
    fn gem_for(&self, effect: Row<'_>) -> (Option<usize>, bool, Option<usize>) {
        let mut gem_effect = self.gem_effect_by_primary.get(&effect.index).copied();
        let mut secondary = false;
        // PoE 2 asks `GetRow` of the `AdditionalGrantedEffects` list, which
        // compares a list with a row and never matches.
        if gem_effect.is_none() && self.game == Game::Poe1 {
            gem_effect = self.gem_effect_by_secondary.get(&effect.index).copied();
            secondary = gem_effect.is_some();
        }
        let gem = gem_effect
            .and_then(|g| self.gem_effects.row(g))
            .and_then(|g| self.gem_by_effect_id.get(g.id()).copied());
        (gem_effect, secondary, gem)
    }

    fn is_support_gem(&self, gem: Row<'_>) -> bool {
        match self.game {
            // `GemType` 1 is a support gem, 2 a spirit gem.
            Game::Poe2 => gem.int("GemType") == 1,
            Game::Poe1 => gem.bool("IsSupport"),
        }
    }

    fn set_level_row(&self, index: usize) -> StatRow {
        let row = self.set_levels.row(index).expect("stat set level row");
        let t = row.table;
        let ints = |c: &str| row.list_int(c);
        StatRow {
            gem_level: row.int("GemLevel"),
            crit_a: row.int("SpellCritChance"),
            crit_b: row.int("AttackCritChance"),
            base_multiplier: row.int("BaseMultiplier"),
            damage_effectiveness: row.int("DamageEffectiveness"),
            player_level_req: t.has_col("PlayerLevelReq").then(|| row.float("PlayerLevelReq") as f64),
            resolved: ints("BaseResolvedValues"),
            additional_values: ints("AdditionalStatsValues"),
            flags: row.list_keys("AdditionalFlags"),
            float_stats: row.list_keys("FloatStats"),
            bases: row
                .list_keys("InterpolationBases")
                .into_iter()
                .map(|k| self.interpolation_bases.get(k).copied().unwrap_or(0.0))
                .collect(),
            additional: row.list_keys("AdditionalStats"),
            interpolations: ints("StatInterpolations").into_iter().map(|v| Some(v as f64)).collect(),
            float_values: row.list_float("FloatStatsValues").into_iter().map(|v| v as f64).collect(),
            actor_level: t.has_col("ActorLevel").then(|| row.float("ActorLevel") as f64),
            base_stats: None,
        }
    }
}

/// PoE 1's `skillpopup_stat_filters.txt`: each skill's description file, and
/// `copy` lines that give one skill another's.
fn skill_stat_scopes(text: &str) -> HashMap<String, String> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = HashMap::new();
    let prefix = " \"Metadata/StatDescriptions/";
    let mut from = 0;
    while let Some(at) = text[from..].find(prefix).map(|i| from + i) {
        let before = &text[..at];
        let name = &before[before.rfind(|c: char| !word(c)).map(|i| i + 1).unwrap_or(0)..];
        let after = &text[at + prefix.len()..];
        let stem = &after[..after.find(|c: char| !word(c)).unwrap_or(after.len())];
        if !name.is_empty() && !stem.is_empty() && after[stem.len()..].starts_with(".txt\"") {
            out.insert(name.to_string(), stem.to_string());
        }
        from = at + prefix.len();
    }
    let mut from = 0;
    while let Some(at) = text[from..].find("copy ").map(|i| from + i) {
        let after = &text[at + 5..];
        let a = &after[..after.find(|c: char| !word(c)).unwrap_or(after.len())];
        let tail = &after[a.len()..];
        if let Some(tail) = tail.strip_prefix(' ') {
            let b = &tail[..tail.find(|c: char| !word(c)).unwrap_or(tail.len())];
            if !a.is_empty() && !b.is_empty() {
                match out.get(b).cloned() {
                    Some(scope) => out.insert(a.to_string(), scope),
                    None => out.remove(a),
                };
            }
        }
        from = at + 5;
    }
    out
}

/// One `GrantedEffectStatSetsPerLevel` row, under PoB's names, as `#set`
/// copies and merges it.
#[derive(Clone, Default)]
struct StatRow {
    gem_level: i64,
    /// PoB's `AttackCritChance` (`SpellCritChance` in dat-schema).
    crit_a: i64,
    /// PoB's `OffhandCritChance` (`AttackCritChance` in dat-schema).
    crit_b: i64,
    base_multiplier: i64,
    damage_effectiveness: i64,
    player_level_req: Option<f64>,
    resolved: Vec<i64>,
    additional_values: Vec<i64>,
    flags: Vec<usize>,
    float_stats: Vec<usize>,
    bases: Vec<f64>,
    additional: Vec<usize>,
    interpolations: Vec<Option<f64>>,
    float_values: Vec<f64>,
    actor_level: Option<f64>,
    base_stats: Option<Vec<usize>>,
}

/// A Lua sequence that may have holes, as `statInterpolation` becomes when PoB
/// assigns past its end.
trait Seq {
    fn border(&self) -> usize;
    fn assign(&mut self, index: usize, value: f64);
    fn insert_at(&mut self, index: usize, value: f64);
}

impl Seq for Vec<Option<f64>> {
    /// `#t`: the first border counting from 1.
    fn border(&self) -> usize {
        self.iter().position(Option::is_none).unwrap_or(self.len())
    }

    /// `t[index] = value`, 1-based.
    fn assign(&mut self, index: usize, value: f64) {
        if index == 0 {
            return;
        }
        if self.len() < index {
            self.resize(index, None);
        }
        self[index - 1] = Some(value);
    }

    /// `table.insert(t, index, value)`: shifts `t[index..#t]` up one.
    fn insert_at(&mut self, index: usize, value: f64) {
        let n = self.border();
        if index == 0 || index > n + 1 {
            return;
        }
        if self.len() == n {
            self.push(None);
        }
        for i in (index..=n).rev() {
            self[i] = self[i - 1];
        }
        self[index - 1] = Some(value);
    }
}

/// One level PoB writes: values, then `extra` fields, `statInterpolation`,
/// `actorLevel` and `cost`.
#[derive(Default)]
struct Level {
    level: i64,
    values: Vec<f64>,
    extra: BTreeMap<&'static str, f64>,
    interpolation: Vec<Option<f64>>,
    actor_level: Option<f64>,
    cost: BTreeMap<String, f64>,
}

impl Level {
    fn to_lua(&self) -> Table {
        let mut t = Table::list(self.values.iter().copied());
        for (k, v) in &self.extra {
            t.set(*k, *v);
        }
        let n = self.interpolation.border();
        if n > 0 {
            t.set("statInterpolation", Table::list(self.interpolation[..n].iter().map(|v| v.unwrap_or(0.0))));
        }
        if let Some(a) = self.actor_level {
            t.set("actorLevel", a);
        }
        if !self.cost.is_empty() {
            let mut cost = Table::new();
            for (k, v) in &self.cost {
                cost.set(k.as_str(), *v);
            }
            t.set("cost", cost);
        }
        t
    }
}

/// `[level] = { … }` for each level; a later row with the same level wins.
fn levels_table(levels: &[Level]) -> Table {
    let mut t = Table::new();
    for level in levels {
        t.set(level.level, level.to_lua());
    }
    t
}

/// `escapeGGGString(text:gsub('"','\\"'):gsub('\r',''):gsub('\n','\\n'))`
/// written between quotes, as Lua reads it back.
fn description(text: &str) -> String {
    let escaped = text.replace('"', "\\\"").replace('\r', "").replace('\n', "\\n");
    lua_literal(&escape_ggg_string(&escaped))
}

/// `cleanAndSplit`: the non-empty trimmed lines of a flavour text.
fn clean_and_split(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|l| l.trim_matches(|c: char| c.is_ascii_whitespace()))
        .filter(|l| !l.is_empty())
        .map(|l| lua_literal(&l.replace('"', "\\\"")))
        .collect()
}

/// `name:gsub(" Support", "")`.
fn strip_support(name: &str) -> String {
    name.replace(" Support", "")
}

fn weapon_types(db: &Db, classes: impl IntoIterator<Item = String>) -> Option<Table> {
    let set: std::collections::BTreeSet<&str> = classes.into_iter().filter_map(|c| weapon_type(db.game, &c)).collect();
    (!set.is_empty()).then(|| Table::set_of(set))
}

/// The item classes an active skill's weapon requirement allows (PoE 2).
fn requirement_classes(db: &Db, active: Row<'_>) -> Vec<String> {
    let Some(req) = db.ctx.rr.deref(active, "WeaponRequirements") else { return Vec::new() };
    db.ctx
        .rr
        .deref_list(req.row(), "WieldableClasses")
        .iter()
        .map(|w| db.ctx.rr.deref_id(w.row(), "ItemClass").unwrap_or_default())
        .collect()
}

/// `mapAST` for `SkillType.Triggered`.
fn triggered_type(db: &Db) -> Option<i64> {
    let table = db.ctx.optional_table("ActiveSkillType")?;
    let row = table.by_id("Triggered")?;
    Some(row.index as i64 + 1)
}

/// `#from item` / `#from tree`: `fromItem = true` or `fromTree = true`.
fn set_from(entry: &Entry, out: &mut Table) {
    if let Some(from) = &entry.from {
        let mut chars = from.chars();
        let key = match chars.next() {
            Some(c) => format!("from{}{}", c.to_ascii_uppercase(), chars.as_str()),
            None => "from".to_string(),
        };
        out.set(key, true);
    }
}

/// The gem colour `skills.lua` works out from the attribute split.
fn gem_colour(gem: Row<'_>) -> i64 {
    let (s, d, i) = (
        gem.int("StrengthRequirementPercent"),
        gem.int("DexterityRequirementPercent"),
        gem.int("IntelligenceRequirementPercent"),
    );
    if s >= 50 {
        1
    } else if i >= 50 {
        3
    } else if d >= 50 {
        2
    } else {
        4
    }
}

/// The support block both games write for a support skill.
fn support_types(db: &Db, effect: Row<'_>, out: &mut Table) -> bool {
    out.set("support", true);
    out.set("requireSkillTypes", Table::list(db.skill_types(effect, "AllowedActiveSkillTypes")));
    let added = db.skill_types(effect, "AddedActiveSkillTypes");
    let is_trigger = triggered_type(db).is_some_and(|t| added.contains(&t));
    out.set("addSkillTypes", Table::list(added));
    out.set("excludeSkillTypes", Table::list(db.skill_types(effect, "ExcludedActiveSkillTypes")));
    is_trigger
}

/// The name, `baseTypeName` or `hidden`, and description lines `#skill`
/// starts with. Returns the name later stat set labels fall back to.
fn skill_header(db: &Db, effect: Row<'_>, entry: &Entry, gems: &mut GemUse, out: &mut Table) -> (String, Option<usize>) {
    let game = db.game;
    let is_support = effect.bool("IsSupport");
    let active = db.active_skill(effect);
    let (gem_effect, secondary, gem) = db.gem_for(effect);
    let gem_effect = gem_effect.and_then(|g| db.gem_effects.row(g));
    if let (Some(ge), Some(_)) = (gem_effect, gem) {
        if !ge.str("Name").is_empty() {
            gems.true_names.insert(ge.id().to_string(), ge.string("Name"));
        }
    }
    let true_name = gem_effect.and_then(|ge| gems.true_names.get(ge.id()).cloned());
    let shown = active.map(|a| a.string("DisplayedName")).unwrap_or_default();
    // PoE 2 sanitises the names it writes; PoE 1 writes them as they are.
    let clean = |s: &str| match game {
        Game::Poe2 => sanitise_text(s, game),
        Game::Poe1 => s.to_string(),
    };
    let label_name;
    match (gem.and_then(|g| db.gems.row(g)), entry.no_gem) {
        (Some(gem), false) => {
            let ge = gem_effect.expect("a gem lists its effect");
            gems.used.insert(ge.id().to_string());
            let base = db.gem_base(gem);
            let base_id = base.as_ref().map(|b| b.id()).unwrap_or_default();
            let base_name = base.as_ref().map(|b| b.row().string("Name")).unwrap_or_default();
            if is_support {
                let name = if FULL_NAME_GEMS.contains(&base_id.as_str()) { base_name } else { strip_support(&base_name) };
                label_name = clean(&name);
                out.set("name", lua_literal(&label_name));
                let text = ge.str("SupportText");
                if !text.is_empty() {
                    out.set("description", description(text));
                }
            } else {
                label_name = match secondary {
                    true => shown.clone(),
                    false => true_name.clone().unwrap_or_else(|| shown.clone()),
                };
                out.set("name", lua_literal(&clean(&label_name)));
                out.set("baseTypeName", lua_literal(&shown));
            }
        }
        _ => {
            let name = match (&entry.name, is_support) {
                (Some(name), _) => name.clone(),
                (None, true) => entry.id.clone(),
                // A monster skill PoB has no name for yet goes by its id.
                (None, false) => Some(true_name.clone().unwrap_or_else(|| shown.clone()))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| entry.id.clone()),
            };
            label_name = sanitise_text(&name, game);
            out.set("name", lua_literal(&name));
            out.set("hidden", true);
        }
    }
    (label_name, gem)
}

fn poe2_skill(db: &Db, effect: Row<'_>, entry: &Entry, gems: &mut GemUse) -> Table {
    let ctx = db.ctx;
    let mut out = Table::new();
    let is_support = effect.bool("IsSupport");
    let active = db.active_skill(effect);
    let (display_name, gem) = skill_header(db, effect, entry, gems, &mut out);
    let gem = gem.and_then(|g| db.gems.row(g));
    let per_level: Vec<usize> = db.levels_by_effect.get(&effect.index).cloned().unwrap_or_default();
    let gem_levels = match (gem, entry.no_gem, is_support) {
        (Some(_), false, true) => 1,
        _ => per_level.len(),
    };
    if let Some(a) = active {
        out.set("icon", lua_literal(a.str("Icon_DDSFile")));
    }
    set_from(entry, &mut out);
    if let Some(gem) = gem {
        out.set("color", gem_colour(gem));
    }

    let own_set = db.sets.by_id(effect.id()).map(|s| s.index);
    let primary: Vec<usize> = own_set.and_then(|s| db.set_levels_by_set.get(&s)).cloned().unwrap_or_default();
    let stat_rows = match primary.len() >= gem_levels {
        true => primary,
        false => db.set_levels_by_effect.get(&effect.index).cloned().unwrap_or_default(),
    };
    let progression: Vec<usize> = gem
        .and_then(|g| g.key(db.col.exp_type))
        .and_then(|k| db.exp_by_type.get(&k))
        .cloned()
        .unwrap_or_default();
    let cost_types: Vec<String> = ctx.rr.deref_list_ids(effect, "CostTypes");
    let mut levels: Vec<Level> = Vec::new();
    let mut base_rows: Vec<Option<StatRow>> = Vec::new();
    let mut next_req = 0i64;
    for indx in 0..gem_levels {
        let Some(level_row) = per_level.get(indx).and_then(|&i| db.per_level.row(i)) else { continue };
        let stat_row = stat_rows.get(indx).map(|&i| db.set_level_row(i));
        let mut level = Level { level: level_row.int("Level"), ..Default::default() };
        let from_curve = progression
            .get(indx)
            .and_then(|&i| db.exp.as_ref()?.row(i))
            .map(|r| r.int("Level"))
            .unwrap_or(0);
        next_req = from_curve.max(next_req);
        level.extra.insert("levelRequirement", next_req as f64);
        let amounts = level_row.list_int("CostAmounts");
        for (i, kind) in cost_types.iter().enumerate() {
            if let Some(amount) = amounts.get(i) {
                level.cost.insert(kind.clone(), *amount as f64);
            }
        }
        let int = |c: &str| level_row.int(c);
        if int("Reservation") != 0 {
            level.extra.insert("spiritReservationFlat", int("Reservation") as f64);
        }
        let multiplier = level_row.table.pick(&["ReservationMultiplier", "EffectOnPlayer"]).map(int).unwrap_or(100);
        if multiplier != 100 {
            level.extra.insert("reservationMultiplier", (multiplier - 100) as f64);
        }
        if int("CostMultiplier") != 100 {
            level.extra.insert("manaMultiplier", (int("CostMultiplier") - 100) as f64);
        }
        if int("AttackSpeedMultiplier") != 0 {
            level.extra.insert("attackSpeedMultiplier", int("AttackSpeedMultiplier") as f64);
        }
        if int("AttackTime") != 0 {
            level.extra.insert("attackTime", int("AttackTime") as f64);
        }
        if int("Cooldown") != 0 {
            level.extra.insert("cooldown", int("Cooldown") as f64 / 1000.0);
        }
        if int("PvPDamageMultiplier") != 0 {
            level.extra.insert("PvPDamageMultiplier", int("PvPDamageMultiplier") as f64);
        }
        if int("StoredUses") != 0 {
            level.extra.insert("storedUses", int("StoredUses") as f64);
        }
        if let Some(s) = &stat_row {
            if s.crit_a != 0 {
                level.extra.insert("critChance", s.crit_a as f64 / 100.0);
            }
            if s.crit_b != 0 {
                level.extra.insert("critChance", s.crit_b as f64 / 100.0);
            }
            if s.base_multiplier != 0 {
                level.extra.insert("baseMultiplier", s.base_multiplier as f64 / 10000.0 + 1.0);
            }
        }
        if int("VaalSouls") != 0 {
            level.cost.insert("Soul".into(), int("VaalSouls") as f64);
        }
        if int("VaalStoredUses") != 0 {
            level.extra.insert("vaalStoredUses", int("VaalStoredUses") as f64);
        }
        if int("SoulGainPreventionDuration") != 0 {
            level.extra.insert("soulPreventionDuration", int("SoulGainPreventionDuration") as f64 / 1000.0);
        }
        base_rows.push(stat_row);
        levels.push(level);
    }

    if !(gem.is_some() && is_support) {
        let (shown, alternate) = poe2_quality(db, effect);
        out.set("qualityStats", shown);
        out.set("altQualityStats", alternate);
    }

    if is_support {
        let is_trigger = support_types(db, effect, &mut out);
        if let Some(support) = gem.and_then(|g| poe2_support_row(db, g)) {
            let families = ctx.rr.deref_list_ids(support, "Family");
            if !families.is_empty() {
                out.set("gemFamily", Table::list(families));
            }
            if let (true, Some(f)) = (support.bool("IsLineage"), ctx.rr.deref(support, "FlavourText")) {
                out.set("isLineage", true);
                out.set("flavourText", Table::list(clean_and_split(f.row().str("Text"))));
            }
        }
        if is_trigger {
            out.set("isTrigger", true);
        }
        if effect.bool("SupportsGemsOnly") {
            out.set("supportGemsOnly", true);
        }
        if effect.bool("IgnoreMinionTypes") {
            out.set("ignoreMinionTypes", true);
        }
    } else if let Some(active) = active {
        let text = active.str("Description");
        if !text.is_empty() {
            out.set("description", description(text));
        }
        out.set("skillTypes", Table::set_of(db.skill_types(active, "ActiveSkillTypes")));
        let minion_types = db.skill_types(active, "MinionActiveSkillTypes");
        if !minion_types.is_empty() {
            out.set("minionSkillTypes", Table::set_of(minion_types));
        }
        if let Some(t) = weapon_types(db, requirement_classes(db, active)) {
            out.set("weaponTypes", t);
        }
        let totem = active.int("SkillTotemId");
        if totem < 25 {
            out.set("skillTotemId", totem);
        }
        out.set("castTime", effect.int("CastTime") as f64 / 1000.0);
        if effect.bool("CannotBeSupported") {
            out.set("cannotBeSupported", true);
        }
    }
    out.set("levels", levels_table(&levels));

    let mut state = SetState { base_set: None, base_rows, display_name, is_meta: false };
    let mut stat_sets = Table::new();
    for (i, set_id) in entry.sets.iter().enumerate() {
        if let Some(set) = poe2_set(db, effect, set_id, i + 1, &mut state) {
            stat_sets.set(i as i64 + 1, set);
        }
    }
    if !entry.sets.is_empty() {
        out.set("statSets", stat_sets);
    }
    out
}

/// `SupportGems` row for a gem, found the way `skills.lua` finds it: through
/// the first base item with the gem's id and the first gem on that base.
fn poe2_support_row<'a>(db: &'a Db, gem: Row<'_>) -> Option<Row<'a>> {
    let base_id = db.gem_base(gem)?.id();
    let bases = db.ctx.optional_table("BaseItemTypes")?;
    let base = bases.by_id(&base_id)?.index;
    let gem_row = db.gems.rows().find(|g| g.key(db.col.gem_base) == Some(base))?;
    let support = *db.support_gem_by_gem.get(&gem_row.index)?;
    db.support_gems.as_ref()?.row(support)
}

/// `qualityStats` and `altQualityStats` from the effect's first quality row.
fn poe2_quality(db: &Db, effect: Row<'_>) -> (Table, Table) {
    let mut shown = Table::new();
    let mut alternate = Table::new();
    let row = db
        .quality_by_effect
        .get(&effect.index)
        .and_then(|rows| rows.first())
        .and_then(|&i| db.quality.as_ref()?.row(i));
    if let Some(row) = row {
        let add = |out: &mut Table, stats: &str, values: &str, sets: &str| {
            let values = row.list_int(values);
            let sets = row.list_int(sets);
            for (i, stat) in row.list_keys(stats).into_iter().enumerate() {
                let mut t = Table::new().with(1, db.stat_id(stat));
                if let Some(v) = values.get(i) {
                    t.set(2, *v as f64 / 1000.0);
                }
                t.set(3, Table::list(sets.iter().copied()));
                out.push(t);
            }
        };
        add(&mut shown, db.col.quality_stats, "StatsValuesPermille", "ApplyToStatSets");
        add(&mut alternate, "AltStats", "AltStatValuesPermille", "AltApplyToStatSets");
    }
    (shown, alternate)
}

/// What one `#set` carries over to the next within a skill.
struct SetState {
    /// The first set, whose stats later sets start from.
    base_set: Option<SetDef>,
    /// The skill's own stat rows, by level position.
    base_rows: Vec<Option<StatRow>>,
    display_name: String,
    is_meta: bool,
}

/// A `GrantedEffectStatSets` row under PoB's names.
#[derive(Clone, Default)]
struct SetDef {
    implicit: Vec<usize>,
    constant: Vec<usize>,
    constant_values: Vec<i64>,
    base_effectiveness: f64,
    incremental_effectiveness: f64,
    damage_incremental_effectiveness: f64,
}

/// Stat bookkeeping `#set` keeps across a set's levels.
#[derive(Default)]
struct StatList {
    map: HashSet<String>,
    order: Vec<String>,
    stats: Vec<String>,
    no_minion: Vec<String>,
}

impl StatList {
    /// Adds a stat seen on a level, or pads the level with zeroes up to where
    /// the stat sat on the first level.
    fn place(&mut self, id: &str, cannot: bool, first: bool, level: &mut Level, smoi: &mut usize) {
        if !self.map.contains(id) || first {
            self.map.insert(id.to_string());
            self.stats.push(id.to_string());
            if first {
                self.order.push(id.to_string());
                if cannot && !self.no_minion.iter().any(|s| s == id) {
                    self.no_minion.push(id.to_string());
                }
            }
        } else if self.order.get(*smoi - 1).map(String::as_str) != Some(id) {
            while *smoi < self.order.len() && self.order[*smoi - 1] != id {
                level.values.push(0.0);
                if level.interpolation.border() < self.order.len() {
                    level.interpolation.insert_at(*smoi, 0.0);
                }
                *smoi += 1;
            }
        }
    }

    fn add(&mut self, id: &str, cannot: bool) {
        if self.map.insert(id.to_string()) {
            self.stats.push(id.to_string());
            if cannot && !self.no_minion.iter().any(|s| s == id) {
                self.no_minion.push(id.to_string());
            }
        }
    }
}

/// Removes the first `id` from a `removeStats` list, saying whether it was there.
fn take(list: &mut Vec<String>, id: &str) -> bool {
    match list.iter().position(|v| v == id) {
        Some(k) => {
            list.remove(k);
            true
        }
        None => false,
    }
}

fn poe2_set(db: &Db, effect: Row<'_>, set_id: &str, set_index: usize, state: &mut SetState) -> Option<Table> {
    let ctx = db.ctx;
    let game = db.game;
    let set = db.sets.by_id(set_id)?;
    let rows: Vec<usize> = db.set_levels_by_set.get(&set.index).cloned().unwrap_or_default();
    let label = match ctx.rr.deref(set, "Label") {
        Some(l) => l.row().string("Text"),
        None => state.display_name.clone(),
    };
    let label = sanitise_text(&label, game);
    let mut def = SetDef {
        implicit: set.list_keys("ImplicitStats"),
        constant: set.list_keys("ConstantStats"),
        constant_values: set.list_int("ConstantStatsValues"),
        base_effectiveness: set.float("BaseEffectiveness") as f64,
        incremental_effectiveness: set.float("IncrementalEffectiveness") as f64,
        damage_incremental_effectiveness: set.float("DamageIncrementalEffectiveness") as f64,
    };
    let mut remove: Vec<String> = set.list_keys("IgnoredStats").into_iter().map(|k| db.stat_id(k)).collect();
    if set_index == 1 {
        state.base_set = Some(def.clone());
    } else if let Some(base) = &state.base_set {
        def.implicit = [base.implicit.clone(), def.implicit].concat();
        def.constant = [base.constant.clone(), def.constant].concat();
        def.constant_values = [base.constant_values.clone(), def.constant_values].concat();
        if def.base_effectiveness == 1.0 {
            def.base_effectiveness = base.base_effectiveness;
        }
        if def.incremental_effectiveness == 0.0 {
            def.incremental_effectiveness = base.incremental_effectiveness;
        }
        if def.damage_incremental_effectiveness == 0.0 {
            def.damage_incremental_effectiveness = base.damage_incremental_effectiveness;
        }
    }
    let base_def = state.base_set.clone().unwrap_or_default();

    let mut list = StatList::default();
    let mut levels: Vec<Level> = Vec::new();
    // Minion and monster skills have few levels, and resolving them would
    // break them, so their raw values are kept.
    let resolve = rows.len() > 5;
    for (indx, &row_index) in rows.iter().enumerate() {
        let first = indx == 0;
        let mut row = db.set_level_row(row_index);
        let base_row = state.base_rows.get(indx).cloned().flatten().unwrap_or_default();
        let mut level = Level { level: row.gem_level, ..Default::default() };
        if set_index != 1 {
            if row.crit_a != 0 {
                level.extra.insert("critChance", (base_row.crit_a + row.crit_a) as f64 / 100.0);
            }
            if row.crit_b != 0 {
                level.extra.insert("critChance", (base_row.crit_b + row.crit_b) as f64 / 100.0);
            }
            // PoB checks `UseSetAttackMulti` here, but both of its branches end
            // by setting the set's own multiplier.
            if row.base_multiplier != 0 {
                level.extra.insert("baseMultiplier", row.base_multiplier as f64 / 10000.0 + 1.0);
            }
            row.resolved = [base_row.resolved.clone(), row.resolved].concat();
            row.float_stats = [base_row.float_stats.clone(), row.float_stats].concat();
            row.float_values = [base_row.float_values.clone(), row.float_values].concat();
            row.interpolations = [base_row.interpolations.clone(), row.interpolations].concat();
            row.bases = [base_row.bases.clone(), row.bases].concat();
            row.additional = [base_row.additional.clone(), row.additional].concat();
            row.additional_values = [base_row.additional_values.clone(), row.additional_values].concat();
            row.base_stats = Some(
                [base_def.implicit.clone(), base_def.constant.clone(), base_row.float_stats.clone(), base_row.additional.clone()]
                    .concat(),
            );
        }
        level.interpolation = row.interpolations.clone();
        level.actor_level = row.actor_level;

        // A removed stat the first set does not have stays. PoB drops such
        // entries while walking the list, which skips the entry after each.
        let base_stats: Vec<String> = row.base_stats.clone().unwrap_or_default().iter().map(|&k| db.stat_id(k)).collect();
        let mut temp_remove = remove.clone();
        let mut i = 0;
        while i < remove.len() {
            if !base_stats.contains(&remove[i]) {
                temp_remove.remove(i);
                remove.remove(i);
            }
            i += 1;
        }

        let mut smoi = 1usize;
        for (i, &stat) in row.float_stats.iter().enumerate() {
            let id = db.stat_id(stat);
            if take(&mut temp_remove, &id) && i < row.resolved.len() {
                row.resolved[i] = 0;
            }
            list.place(&id, db.no_minion(stat), first, &mut level, &mut smoi);
            if resolve {
                if let Some(v) = row.resolved.get(i) {
                    level.values.push(*v as f64);
                }
                let at = if set_index != 1 { level.values.len() } else { smoi };
                level.interpolation.assign(at, 1.0);
            } else {
                let value = row.float_values.get(i).copied().unwrap_or(0.0);
                let base = row.bases.get(i).copied().unwrap_or(0.0);
                level.values.push(value / base.max(0.00001));
            }
            smoi += 1;
        }
        for (i, &stat) in row.additional.iter().enumerate() {
            let id = db.stat_id(stat);
            if take(&mut temp_remove, &id) && i < row.additional_values.len() {
                row.additional_values[i] = 0;
            }
            list.place(&id, db.no_minion(stat), first, &mut level, &mut smoi);
            if let Some(v) = row.additional_values.get(i) {
                level.values.push(*v as f64);
            }
            level.interpolation.assign(smoi, 1.0);
            smoi += 1;
        }
        for &stat in &row.flags {
            let id = db.stat_id(stat);
            if !take(&mut temp_remove, &id) {
                list.add(&id, db.no_minion(stat));
            }
        }
        levels.push(level);
    }
    for &stat in &def.implicit {
        let id = db.stat_id(stat);
        if !take(&mut remove, &id) {
            list.add(&id, false);
        }
    }
    let mut constant = Table::new();
    for (i, &stat) in def.constant.iter().enumerate() {
        let id = db.stat_id(stat);
        if take(&mut remove, &id) {
            continue;
        }
        let mut pair = Table::new().with(1, id);
        if let Some(v) = def.constant_values.get(i) {
            pair.set(2, *v);
        }
        constant.push(pair);
    }

    let mut out = Table::new();
    out.set("label", lua_literal(&label));
    if def.base_effectiveness != 1.0 {
        out.set("baseEffectiveness", def.base_effectiveness);
    }
    if def.incremental_effectiveness != 0.0 {
        out.set("incrementalEffectiveness", def.incremental_effectiveness);
    }
    if def.damage_incremental_effectiveness != 0.0 {
        out.set("damageIncrementalEffectiveness", def.damage_incremental_effectiveness);
    }
    let scope = if effect.bool("IsSupport") {
        let first = db.gem_effects_by_additional.get(&effect.index).and_then(|v| v.first()).and_then(|&i| db.gem_effects.row(i));
        if first.is_some_and(|ge| ctx.rr.deref_list_ids(ge, "GemTags").iter().any(|t| t == "meta")) {
            state.is_meta = true;
        }
        match state.is_meta {
            true => "meta_gem_stat_descriptions".to_string(),
            false => "gem_stat_descriptions".to_string(),
        }
    } else {
        let path = db.active_skill(effect).map(|a| a.string("StatDescription")).unwrap_or_default();
        stat_description_scope(&path, set_index)
    };
    out.set("statDescriptionScope", lua_literal(&scope));
    if !constant.is_empty() {
        out.set("constantStats", constant);
    }
    out.set("stats", Table::list(list.stats));
    if !list.no_minion.is_empty() {
        out.set("notMinionStat", Table::list(list.no_minion));
    }
    out.set("levels", levels_table(&levels));
    Some(out)
}

/// The description file PoE 2 names for an active skill, as `skills.lua`
/// rewrites the path into a `StatDescriptions` file name.
fn stat_description_scope(path: &str, set_index: usize) -> String {
    let mut s = path.strip_prefix("Data/StatDescriptions/").unwrap_or(path).to_string();
    s = s.replace("specific_skill_stat_descriptions/", "");
    s = s.replace("statset_0", &format!("statset_{}", set_index - 1));
    if s.ends_with('/') {
        s.pop();
    }
    s = s.replace('/', "_");
    // `gsub(".csd", "")`: the dot matches any character.
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 4 <= bytes.len() && &bytes[i + 1..i + 4] == b"csd" {
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn poe2_gems(db: &Db, gems: &GemUse) -> Table {
    let ctx = db.ctx;
    let game = db.game;
    let mut out = Table::new();
    let tags_table = ctx.optional_table("GemTags");
    for gem in db.gems.rows() {
        for ge_index in gem.list_keys(db.col.gem_effects) {
            let Some(ge) = db.gem_effects.row(ge_index) else { continue };
            if !gems.used.contains(ge.id()) {
                continue;
            }
            let base = db.gem_base(gem);
            let base_id = base.as_ref().map(|b| b.id()).unwrap_or_default();
            let base_name = base.as_ref().map(|b| b.row().string("Name")).unwrap_or_default();
            let granted = ge.key("GrantedEffect").and_then(|k| db.effects.row(k));
            let mut t = Table::new();
            let name = if FULL_NAME_GEMS.contains(&base_id.as_str()) {
                base_name.clone()
            } else if let Some(n) = gems.true_names.get(ge.id()) {
                n.clone()
            } else {
                strip_support(&base_name)
            };
            t.set("name", lua_literal(&sanitise_text(&name, game)));
            let is_support = db.is_support_gem(gem);
            let active = granted.and_then(|g| db.active_skill(g));
            if !is_support {
                t.set("baseTypeName", lua_literal(&active.map(|a| a.string("DisplayedName")).unwrap_or_default()));
            }
            t.set("gameId", lua_literal(&base_id));
            t.set("variantId", lua_literal(ge.id()));
            t.set("grantedEffectId", granted.map(|g| g.id().to_string()).unwrap_or_default());
            if let Some(g) = granted {
                for (n, id) in ctx.rr.deref_list_ids(g, db.col.extra_sets).into_iter().enumerate() {
                    t.set(format!("additionalStatSet{}", n + 1), id);
                }
            }
            for (n, id) in ctx.rr.deref_list_ids(ge, "AdditionalGrantedEffects").into_iter().enumerate() {
                t.set(format!("additionalGrantedEffectId{}", n + 1), id);
            }
            if let Some(c) = ge.table.column_or_after(&["GrantedEffectDisplayOrder"], "AdditionalGrantedEffects", 3) {
                let order = ge.list_int_at(c);
                if !order.is_empty() {
                    t.set("grantedEffectDisplayOrder", Table::list(order));
                }
            }
            let secondary = ge.str("SupportName");
            if !secondary.is_empty() {
                t.set("secondaryEffectName", lua_literal(secondary));
            }
            if gem.bool("IsVaalVariant") {
                t.set("vaalGem", true);
            }
            let mut gem_type: Option<String> = None;
            let mut tag_names: Vec<String> = Vec::new();
            let mut tags = Table::new();
            for tag in ge.list_keys("GemTags") {
                let Some(tag) = tags_table.as_ref().and_then(|t| t.row(tag)) else { continue };
                tags.set(tag.id(), true);
                if !tag.str("Name").is_empty() {
                    let name = escape_ggg_string(tag.str("Name"));
                    match gem_type {
                        None => gem_type = Some(name),
                        Some(_) => tag_names.push(name),
                    }
                }
            }
            t.set("tags", tags);
            let mut requirement: Vec<String> = Vec::new();
            if let Some(req) = active.and_then(|a| ctx.rr.deref(a, "WeaponRequirements")) {
                match ctx.rr.deref(req.row(), "String") {
                    Some(s) => requirement.push(escape_ggg_string(s.row().str("Text"))),
                    None => {
                        for w in ctx.rr.deref_list(req.row(), "WieldableClasses") {
                            let Some(class) = ctx.rr.deref(w.row(), "ItemClass") else { continue };
                            if weapon_type(game, class.row().id()).is_some() {
                                let category = ctx
                                    .rr
                                    .deref(class.row(), "ItemClassCategory")
                                    .map(|c| c.row().string("Text"))
                                    .unwrap_or_default();
                                requirement.push(escape_ggg_string(&category));
                            }
                        }
                    }
                }
            }
            t.set("gemType", lua_literal(&gem_type.unwrap_or_default()));
            if is_support {
                if let Some(support) = poe2_support_row(db, gem) {
                    let names: Vec<String> =
                        ctx.rr.deref_list(support, "Family").iter().map(|f| f.row().string("Text")).collect();
                    if !names.is_empty() {
                        t.set("gemFamily", lua_literal(&names.join(", ")));
                    }
                }
            }
            t.set("tagString", lua_literal(&tag_names.join(", ")));
            if !requirement.is_empty() {
                t.set("weaponRequirements", lua_literal(&requirement.join(", ")));
            }
            t.set("reqStr", gem.int("StrengthRequirementPercent"));
            t.set("reqDex", gem.int("DexterityRequirementPercent"));
            t.set("reqInt", gem.int("IntelligenceRequirementPercent"));
            // PoB's `Tier` is the column dat-schema calls `CraftingLevel`.
            t.set("Tier", gem.int("CraftingLevel"));
            let max_level = match is_support {
                true => 1,
                false => gem.key(db.col.exp_type).and_then(|k| db.exp_by_type.get(&k)).map(Vec::len).unwrap_or(0),
            };
            t.set("naturalMaxLevel", max_level.max(1));
            out.set(format!("Metadata/Items/Gems/SkillGem{}", ge.id()), t);
        }
    }
    out
}

fn poe1_skill(db: &Db, effect: Row<'_>, entry: &Entry, gems: &mut GemUse) -> Table {
    let ctx = db.ctx;
    let mut out = Table::new();
    let is_support = effect.bool("IsSupport");
    let active = db.active_skill(effect);
    let (_, gem) = skill_header(db, effect, entry, gems, &mut out);
    set_from(entry, &mut out);
    if let Some(gem) = gem.and_then(|g| db.gems.row(g)) {
        let flavour = db.gem_base(gem).and_then(|b| ctx.rr.deref(b.row(), "FlavourTextKey"));
        if let Some(f) = flavour {
            out.set("flavourText", Table::list(clean_and_split(f.row().str("Text"))));
        }
    }
    out.set("color", effect.int("Attribute"));
    let primary_set = effect.key(db.col.primary_set).and_then(|k| db.sets.row(k));
    if let Some(set) = primary_set {
        let base = set.float("BaseEffectiveness") as f64;
        if base != 1.0 {
            out.set("baseEffectiveness", base);
        }
        let incremental = set.float("IncrementalEffectiveness") as f64;
        if incremental != 0.0 {
            out.set("incrementalEffectiveness", incremental);
        }
    }
    if is_support {
        let is_trigger = support_types(db, effect, &mut out);
        if is_trigger {
            out.set("isTrigger", true);
        }
        if effect.bool("SupportsGemsOnly") {
            out.set("supportGemsOnly", true);
        }
        if effect.bool("IgnoreMinionTypes") {
            out.set("ignoreMinionTypes", true);
        }
        if let Some(plus) = ctx.rr.deref_id(effect, "RegularVariant") {
            out.set("plusVersionOf", plus);
        }
        if let Some(t) = weapon_types(db, ctx.rr.deref_list_ids(effect, "SupportWeaponRestrictions")) {
            out.set("weaponTypes", t);
        }
        out.set("statDescriptionScope", "gem_stat_descriptions");
    } else if let Some(active) = active {
        let text = active.str("Description");
        if !text.is_empty() {
            out.set("description", description(text));
        }
        out.set("skillTypes", Table::set_of(db.skill_types(active, "ActiveSkillTypes")));
        let minion_types = db.skill_types(active, "MinionActiveSkillTypes");
        if !minion_types.is_empty() {
            out.set("minionSkillTypes", Table::set_of(minion_types));
        }
        if let Some(t) = weapon_types(db, ctx.rr.deref_list_ids(active, "WeaponRestriction_ItemClassesKeys")) {
            out.set("weaponTypes", t);
        }
        let scope = db.stat_scope.get(active.id()).cloned().unwrap_or_else(|| "skill_stat_descriptions".into());
        out.set("statDescriptionScope", scope);
        let totem = active.int("SkillTotemId");
        if totem <= 21 {
            out.set("skillTotemId", totem);
        }
        out.set("castTime", effect.int("CastTime") as f64 / 1000.0);
        if effect.bool("CannotBeSupported") {
            out.set("cannotBeSupported", true);
        }
    }

    let stat_rows: Vec<usize> = primary_set.and_then(|s| db.set_levels_by_set.get(&s.index)).cloned().unwrap_or_default();
    let per_level: Vec<usize> = db.levels_by_effect.get(&effect.index).cloned().unwrap_or_default();
    let mut list = StatList::default();
    let mut levels: Vec<Level> = Vec::new();
    for indx in 0..per_level.len().max(stat_rows.len()) {
        let Some(level_row) = per_level.get(indx).or(per_level.first()).and_then(|&i| db.per_level.row(i)) else { continue };
        let Some(&stat_index) = stat_rows.get(indx).or(stat_rows.first()) else { continue };
        let row = db.set_level_row(stat_index);
        let single = per_level.len() == 1;
        let mut level = Level { level: if single { row.gem_level } else { level_row.int("Level") }, ..Default::default() };
        let req = match single {
            true => row.player_level_req.unwrap_or(0.0),
            false => level_row.float("PlayerLevelReq") as f64,
        };
        level.extra.insert("levelRequirement", req);
        let amounts = level_row.list_int("CostAmounts");
        for (i, kind) in ctx.rr.deref_list_ids(level_row, "CostTypes").into_iter().enumerate() {
            if let Some(amount) = amounts.get(i) {
                level.cost.insert(kind, *amount as f64);
            }
        }
        let extras: [(&'static str, &str, f64, f64); 12] = [
            ("manaReservationFlat", "ManaReservationFlat", 0.0, 1.0),
            ("manaReservationPercent", "ManaReservationPercent", 0.0, 100.0),
            ("lifeReservationFlat", "LifeReservationFlat", 0.0, 1.0),
            ("lifeReservationPercent", "LifeReservationPercent", 0.0, 100.0),
            ("manaMultiplier", "CostMultiplier", 100.0, 1.0),
            ("attackSpeedMultiplier", "AttackSpeedMultiplier", 0.0, 1.0),
            ("attackTime", "AttackTime", 0.0, 1.0),
            ("cooldown", "Cooldown", 0.0, 1000.0),
            ("PvPDamageMultiplier", "PvPDamageMultiplier", 0.0, 1.0),
            ("storedUses", "StoredUses", 0.0, 1.0),
            ("vaalStoredUses", "VaalStoredUses", 0.0, 1.0),
            ("soulPreventionDuration", "SoulGainPreventionDuration", 0.0, 1000.0),
        ];
        for (key, column, neutral, divisor) in extras {
            let value = level_row.int(column) as f64;
            if value != neutral {
                level.extra.insert(key, (value - neutral) / divisor);
            }
        }
        if level_row.int("VaalSouls") != 0 {
            level.cost.insert("Soul".into(), level_row.int("VaalSouls") as f64);
        }
        if row.damage_effectiveness != 0 {
            level.extra.insert("damageEffectiveness", row.damage_effectiveness as f64 / 10000.0 + 1.0);
        }
        if row.crit_a != 0 {
            level.extra.insert("critChance", row.crit_a as f64 / 100.0);
        }
        if row.crit_b != 0 {
            level.extra.insert("critChance", row.crit_b as f64 / 100.0);
        }
        if row.base_multiplier != 0 {
            level.extra.insert("baseMultiplier", row.base_multiplier as f64 / 10000.0 + 1.0);
        }
        level.interpolation = row.interpolations.clone();
        let first = indx == 0;
        let mut smoi = 1usize;
        for (i, &stat) in row.float_stats.iter().enumerate() {
            let id = db.stat_id(stat);
            list.place_poe1(&id, db.no_minion(stat), first, true, &mut level, &mut smoi);
            smoi += 1;
            let value = row.float_values.get(i).copied().unwrap_or(0.0);
            let base = row.bases.get(i).copied().unwrap_or(0.0);
            level.values.push(value / base.max(0.00001));
        }
        for (i, &stat) in row.additional.iter().enumerate() {
            let id = db.stat_id(stat);
            list.place_poe1(&id, db.no_minion(stat), first, false, &mut level, &mut smoi);
            smoi += 1;
            if let Some(v) = row.additional_values.get(i) {
                level.values.push(*v as f64);
            }
        }
        for &stat in &row.flags {
            let id = db.stat_id(stat);
            if list.map.insert(id.clone()) {
                list.stats.push(id.clone());
                if db.no_minion(stat) {
                    list.no_minion.push(id);
                }
            }
        }
        levels.push(level);
    }
    if let Some(set) = primary_set {
        for stat in set.list_keys("ImplicitStats") {
            let id = db.stat_id(stat);
            if list.map.insert(id.clone()) {
                list.stats.push(id);
            }
        }
    }
    let mut constant = Table::new();
    if let Some(set) = primary_set {
        let values = set.list_int("ConstantStatsValues");
        for (i, stat) in set.list_keys("ConstantStats").into_iter().enumerate() {
            let mut pair = Table::new().with(1, db.stat_id(stat));
            if let Some(v) = values.get(i) {
                pair.set(2, *v);
            }
            constant.push(pair);
        }
    }
    let mut quality = Table::new();
    for &q in db.quality_by_effect.get(&effect.index).map(Vec::as_slice).unwrap_or_default() {
        let Some(row) = db.quality.as_ref().and_then(|t| t.row(q)) else { continue };
        let values = row.list_int("StatsValuesPermille");
        for (j, stat) in row.list_keys(db.col.quality_stats).into_iter().enumerate() {
            let id = db.stat_id(stat);
            if id != "dummy_stat_display_nothing" {
                let mut pair = Table::new().with(1, id);
                if let Some(v) = values.get(j) {
                    pair.set(2, *v as f64 / 1000.0);
                }
                quality.push(pair);
            }
        }
    }
    if !quality.is_empty() {
        out.set("qualityStats", quality);
    }
    if !constant.is_empty() {
        out.set("constantStats", constant);
    }
    out.set("stats", Table::list(list.stats));
    if !list.no_minion.is_empty() {
        out.set("notMinionStat", Table::list(list.no_minion));
    }
    out.set("levels", levels_table(&levels));
    out
}

impl StatList {
    /// PoE 1's version of [`place`](Self::place): float stats are re-added on
    /// the first level, additional ones only once, and `notMinionStat` takes
    /// repeats.
    fn place_poe1(&mut self, id: &str, cannot: bool, first: bool, float: bool, level: &mut Level, smoi: &mut usize) {
        if !self.map.contains(id) || (float && first) {
            self.map.insert(id.to_string());
            self.stats.push(id.to_string());
            if first {
                self.order.push(id.to_string());
                if cannot {
                    self.no_minion.push(id.to_string());
                }
            }
        } else if self.order.get(*smoi - 1).map(String::as_str) != Some(id) {
            while *smoi < self.order.len() && self.order[*smoi - 1] != id {
                level.values.push(0.0);
                if level.interpolation.border() < self.order.len() {
                    level.interpolation.insert_at(*smoi, 0.0);
                }
                *smoi += 1;
            }
        }
    }
}

fn poe1_gems(db: &Db, gems: &GemUse) -> Table {
    let ctx = db.ctx;
    let mut out = Table::new();
    let mut used = gems.used.clone();
    let tags_table = ctx.optional_table("GemTags");
    for gem in db.gems.rows() {
        for ge_index in gem.list_keys(db.col.gem_effects) {
            let Some(ge) = db.gem_effects.row(ge_index) else { continue };
            if !used.remove(ge.id()) {
                continue;
            }
            let base = db.gem_base(gem);
            let base_id = base.as_ref().map(|b| b.id()).unwrap_or_default();
            let base_name = base.as_ref().map(|b| b.row().string("Name")).unwrap_or_default();
            let granted = ge.key("GrantedEffect").and_then(|k| db.effects.row(k));
            let mut t = Table::new();
            let name = if FULL_NAME_GEMS.contains(&base_id.as_str()) {
                base_name.clone()
            } else if let Some(n) = gems.true_names.get(ge.id()) {
                n.clone()
            } else {
                strip_support(&base_name)
            };
            t.set("name", lua_literal(&name));
            if !db.is_support_gem(gem) {
                let shown = granted.and_then(|g| db.active_skill(g)).map(|a| a.string("DisplayedName")).unwrap_or_default();
                t.set("baseTypeName", lua_literal(&shown));
            }
            t.set("gameId", lua_literal(&base_id));
            t.set("variantId", lua_literal(ge.id()));
            t.set("grantedEffectId", granted.map(|g| g.id().to_string()).unwrap_or_default());
            if let Some(id) = ctx.rr.deref_id(ge, "GrantedEffect2") {
                t.set("secondaryGrantedEffectId", id);
            }
            let secondary = ge.str("SupportName");
            if !secondary.is_empty() {
                t.set("secondaryEffectName", lua_literal(secondary));
            }
            if gem.bool("IsVaalVariant") {
                t.set("vaalGem", true);
            }
            let mut tags = Table::new();
            let mut names: Vec<String> = Vec::new();
            for tag in ge.list_keys("GemTags") {
                let Some(tag) = tags_table.as_ref().and_then(|t| t.row(tag)) else { continue };
                tags.set(tag.id(), true);
                let name = escape_ggg_string(tag.str(tag.table.pick(&["Name", "Tag"]).unwrap_or("Tag")));
                if !name.is_empty() {
                    names.push(name);
                }
            }
            t.set("tags", tags);
            t.set("tagString", lua_literal(&names.join(", ")));
            t.set("reqStr", gem.int("StrengthRequirementPercent"));
            t.set("reqDex", gem.int("DexterityRequirementPercent"));
            t.set("reqInt", gem.int("IntelligenceRequirementPercent"));
            let max_level = gem.key(db.col.exp_type).and_then(|k| db.exp_by_type.get(&k)).map(Vec::len).unwrap_or(0);
            t.set("naturalMaxLevel", max_level.max(1));
            out.set(format!("Metadata/Items/Gems/SkillGem{}", ge.id()), t);
        }
    }
    out
}

/// `PearlSupports.lua`: the supports Pearl of Tsoatha can grant.
fn pearl_supports(db: &Db) -> Table {
    let ctx = db.ctx;
    let mut out = Table::new();
    let Some(table) = ctx.optional_table("IndexableNonActiveSupportGems") else { return out };
    for row in table.rows() {
        let Some(gem) = row.key("SupportGem").and_then(|k| db.gems.row(k)) else { continue };
        let name = db.gem_base(gem).map(|b| b.row().string("Name")).unwrap_or_default();
        let granted = gem
            .list_keys(db.col.gem_effects)
            .first()
            .and_then(|&k| db.gem_effects.row(k))
            .and_then(|ge| ctx.rr.deref_id(ge, "GrantedEffect"))
            .unwrap_or_default();
        out.push(Table::new().with("baseItemName", name).with("grantedEffectName", granted));
    }
    out
}

/// Where PoB files a skill the game data would put elsewhere: gems with no
/// colour that PoB sorts under one, and skills it keeps with another file's.
const POE2_FILE_OVERRIDES: [(&str, &str); 18] = [
    ("BlackPowderBlitzPlayer", "act_str"),
    ("BlackPowderBlitzReservationPlayer", "act_str"),
    ("CracklingPalmPlayer", "act_int"),
    ("CrossbowRequiemAmmoPlayer", "act_str"),
    ("CrossbowRequiemPlayer", "act_str"),
    ("DestructiveLinkSkeletonBombadierMinion", "minion"),
    ("EnervatingNovaPlayer", "sup_int"),
    ("GeminiSurgePlayer", "act_str"),
    ("HyenaCacklePlayer", "act_str"),
    ("MeleeAtAnimationSpeedComboTEMP", "spectre"),
    ("MetaCastLightningSpellOnHitPlayer", "act_str"),
    ("MetaCastOnBlockPlayer", "act_str"),
    ("MoltenCrashPlayer", "act_str"),
    ("PhantasmalArrowPlayer", "act_dex"),
    ("ShatteringSpitePlayer", "act_dex"),
    ("SupportMetaCastLightningSpellOnHitPlayer", "act_str"),
    ("SupportMetaCastOnBlockPlayer", "act_str"),
    ("ValakosChargePlayer", "act_str"),
];

const POE1_FILE_OVERRIDES: [(&str, &str); 9] = [
    ("BloodOffering", "other"),
    ("BoneArmour", "other"),
    ("CallOfSteel", "act_dex"),
    ("DoryanisTouch", "other"),
    ("Envy", "other"),
    ("Melee", "other"),
    ("SupportDivineBlessing", "other"),
    ("SupportEarthbreaker", "other"),
    ("SupportElementalPenetration", "other"),
];

/// Skills PoB writes as hidden although a gem carries them (`#noGem`): gems
/// no player can get, whose skill comes from an item.
const NO_GEM: [&str; 7] = ["MeleeUnarmedPlayer", "BloodOffering", "BoneArmour", "DeathAura", "DoryanisTouch", "Envy", "GluttonyOfElements"];

/// PoB's names for skills the game leaves unnamed or names differently,
/// mostly monster skills (`#skill <id> <name>`).
const POE2_NAMES: [(&str, &str); 240] = [
    ("ABTTProcessionBannerDrain", "Banner"),
    ("ArmourExplosionPlayer", "Armour Explosion"),
    ("ArtilleryBallistaProjectilePlayer", "Ballista Bolt"),
    ("BlackStriderMassMortar", "Mass Mortar"),
    ("BlackStriderWebProjectile", "Web Projectile"),
    ("BoneCultistZealotFirestorm", "Firestorm"),
    ("BoneCultistZealotLightningstorm", "Lightning Storm"),
    ("BurdenedWretchSlam", "Slam"),
    ("BurdenedWretchSlamUnique", "Slam"),
    ("CGEAbyssCocoon3FlameGeyser", "Large Ball Flame Geyser"),
    ("CGEArenaBeastBossSulpurGas", "Sulphur Gas"),
    ("CGEBloodPriestBoilingBlood", "Boiling Blood"),
    ("CGEMudBurrowerVomit", "Vomit Ground"),
    ("CGEQuillCrabCausticGround", "Caustic Ground"),
    ("CGEQuillCrabFireGround", "Burning Ground"),
    ("CGESanctifiedMonstrosityPusGround", "Pus Ground"),
    ("CGESanctumBlackStriderWeb", "Web Ground"),
    ("CGEStarFishSpitCausticGround", "Vomit Ground"),
    ("CoffinWretchBabySoulrend1", "Soulrend"),
    ("CompanionBearLeapImpact", "Leap Slam"),
    ("CultistBeastSunder", "Sunder"),
    ("DTTAnimateWeaponSpearDashStabImpact", "Spear Dash"),
    ("DTTMantisRatLeap", "Leap"),
    ("DTTParasiteSwarmLeap", "Leap"),
    ("DTTParasiteSwarmLeapAttach", "Leap Attach"),
    ("DeathKnightSlamEAA", "Slam"),
    ("EDSAbyssMorayClanFlamethrower", "Flamethrower"),
    ("EDSShellMonsterFlamethrower", "Flamethrower"),
    ("EDSShellMonsterPoisonSpray", "Poison Spray"),
    ("ExpeditionGroundLaser", "Ground Laser"),
    ("FarudinWarlockBugRend", "Rend"),
    ("FungalArtilleryMortar", "Mortar"),
    ("GAAncestralJadeHulkLeapImpact", "Leap Slam"),
    ("GAAnimateWeaponMaceSlam", "Mace Slam"),
    ("GAAnimateWeaponQuarterstaffSweep", "Quarterstaff Sweep"),
    ("GAArenaBeastBossBigSlam", "Big Slam"),
    ("GAArenaBeastBossFissureDamage", "Fissure"),
    ("GAArenaBeastBossFissureExplosion", "Fissure Explosion"),
    ("GAArenaBeastBossPunchLeft", "Punch"),
    ("GAArenaBeastBossPunchLeftEmpowered", "Empowered Punch"),
    ("GAArenaBeastBossPunchRight", "Punch"),
    ("GAArenaBeastBossPunchRightEmpowered", "Empowered Punch"),
    ("GAArenaBeastBossShockwave", "Shockwave"),
    ("GAArenaBeastLeapSlam", "Leap Slam"),
    ("GAArenaBeastLeapSlamEnraged", "Enraged Leap Slam"),
    ("GAArenaBeastLeapSlamEnragedKick", "Enraged Kick"),
    ("GAArenaBeastSlam", "Slam"),
    ("GAArenaBeastSlamEmpowered", "Empowered Slam"),
    ("GABlackStriderWebMortarImpact", "Web Impact"),
    ("GACenobiteBloaterSlam", "Slam"),
    ("GADeathKnightOverheadslamforward", "Overhead Slam"),
    ("GADrownedCrawlerSwipe", "Swipe"),
    ("GAExpeditionShakariMonsterSlam", "Slam"),
    ("GAFigureheadSlamGhostFlame", "Slam"),
    ("GAFirebreatherFireSlam", "Slam"),
    ("GAGoblinArenaBeastGroundSlash", "Ground Slash"),
    ("GAGoblinArenaBeastGroundSlashLightning", "Lightning Ground Slash"),
    ("GAGoblinArenaBeastHeadbutt", "Headbutt"),
    ("GAGoblinArenaBeastHeadbuttEmpowered", "Empowered Headbutt"),
    ("GAGullGoliathSlam", "Slam"),
    ("GAHellscapeFleshLeapImpact", "Leap Slam"),
    ("GAHellscapePaleEliteSkyStab", "Stab Attack"),
    ("GAIcyQuadrillaBossRectSlam", "Pillar Slam"),
    ("GAKaruiSpiritTurtleSlam", "Slam"),
    ("GAKaruiTuataraTailSlam", "Tail Slam"),
    ("GAMantisRatDualStrike", "Dual Strike"),
    ("GAMediumBeetleChargedSunder", "Charged Sunder"),
    ("GAMediumBeetleSunder", "Sunder"),
    ("GAMudBurrowerBloodProj", "Blood Spit Impact"),
    ("GAMudBurrowerDivePush", "Dive"),
    ("GAMudBurrowerGoopSmallImpact", "Goop Ball Impact"),
    ("GAMudBurrowerHeadSlam", "Head Slam"),
    ("GAMudBurrowerSpraySmallImpact", "Blood Spray Impact"),
    ("GAMutewindWomanSpearStab1", "Spear Stab"),
    ("GAQuadrillaBossRectSlam", "Pillar Slam"),
    ("GARathbreakerEnrageSlam", "Slam"),
    ("GATwilightOfficerSmite", "Smite"),
    ("GATwilightOrderSoldierChargeImpact", "Charge Impact"),
    ("GATwilightSoldierStab", "Stab"),
    ("GATwoHeadedTitanSlam", "Slam"),
    ("GATwoHeadedTitanStomp", "Stomp"),
    ("GPAPorcupineAntSpikeNova", "Spike Nova"),
    ("GPAPorcupineAntSpikeNovaSanctum", "Spike Nova"),
    ("GPSCaveDwellerSuperProjectileSanctum", "Sonic Projectile"),
    ("GPSPaleWalkerWave", "Wave"),
    ("GSAbyssCarrionWingBeamImpact", "Beam Impact"),
    ("GSAbyssCocoon3BallSpitImpact", "Large Ball Impact"),
    ("GSAbyssCocoon3BallSpitSmallImpact", "Small Ball Impact"),
    ("GSAbyssPaleEliteBeam", "Beam"),
    ("GSAbyssPaleEliteSnowBallImpact", "Snowball Impact"),
    ("GSAbyssPitArtilleryMortarImpact", "Artillery Impact"),
    ("GSAbyssPrimordialMonsterScreech", "Screech"),
    ("GSArmourCasterVolatileExplode", "Volatile Mote"),
    ("GSBeetleLightningNova", "Lightning Nova"),
    ("GSCaveDwellerSonicPulse", "Sonic Pulse"),
    ("GSCaveDwellerSuperProjectile", "Sonic Projectile"),
    ("GSCenobiteBloaterOnDeath", "Death Explosion"),
    ("GSDesertBatZap", "Zap"),
    ("GSExcavatorOrbDonutExplosion", "Orb Donut Explosion"),
    ("GSExcavatorOrbExplosion", "Orb Explosion"),
    ("GSExcavatorRaptorTriangleSlam", "Slam"),
    ("GSExpeditionBoneCultistEggExplosion", "Pustule"),
    ("GSHellscapeDemonEliteBeamNuke", "Beam"),
    ("GSHellscapePaleEliteBoltImpact", "Bolt Impact"),
    ("GSHellscapePaleEliteOmegaBeam", "Omega Beam"),
    ("GSMercurialCasterBlast", "Rune Blast"),
    ("GSPlagueNymphLaser", "Laser"),
    ("GSProwlingShadeIceBeam", "Ice Beam"),
    ("GSRaptorDefenderRailShot", "Rail Shot"),
    ("GSSerpentClanAcidSpit", "Acid Spit"),
    ("GSShrikeScreech", "Screech"),
    ("GSStarFishSpitImpact", "Vomit Impact"),
    ("GSTwilightOrderPlagueNymphLaser", "Laser"),
    ("GSVaalConstructSkitterbotGrenadeExplode", "Grenade Explosion"),
    ("GSVaalZealotLightningBlast", "Lightning Blast"),
    ("GSWardboundMinionBlast", "Cold Spell"),
    ("GSWarlockRaiseBugs", "Raise Bugs"),
    ("GoreChargerCharge", "Charge"),
    ("GraveyardGhostDashToTarget", "Dash"),
    ("GraveyardSpookyGhostExplode", "Sword Barrage"),
    ("HealSkeletonClericMinion", "Heal Buff"),
    ("HellscapeDemonFodderFaceLaser", "Laser"),
    ("HuhuGrubLarvaeMortar", "Mortar"),
    ("HyenaCentaurMeleeSwipe", "Swipe"),
    ("HyenaCentaurSpearThrow", "Spear Throw"),
    ("HyenaCentaurSpearThrowCliff", "Spear Throw"),
    ("JellyfishNettlerArc", "Arc"),
    ("LivingLightningZap", "Zap"),
    ("MAASExpedition2ShakariTailSwipe", "Tail Swipe"),
    ("MMAPorcupineAntSpikeball", "Spike Mortar"),
    ("MMAPorcupineAntSpikeballSanctum", "Spike Mortar"),
    ("MMSBaneSapling", "Basic Spell"),
    ("MMSBoneRabbleMortar", "Mortar"),
    ("MMSHellscapeDemonEliteTripleMortar", "Triple Mortar"),
    ("MPAMudBurrowerBloodProj", "Blood Spit"),
    ("MPAMudBurrowerGoopBigBall", "Large Goop Ball"),
    ("MPAMudBurrowerGoopSmallBall", "Goop Ball"),
    ("MPAMudBurrowerSprayProj", "Blood Spray"),
    ("MPAMudBurrowerVomitProj", "Vomit"),
    ("MPAVaalHumanoidCannon", "Cannon"),
    ("MPSAbyssCocoon3BallSpit", "Large Ball Spit"),
    ("MPSAbyssCocoon3BallSpitSmall", "Small Ball Spit"),
    ("MPSAbyssPaleEliteFireball", "Fireball"),
    ("MPSAbyssPaleEliteSnowBall", "Snowball"),
    ("MPSAbyssPaleWalker2Fireball", "Fireball"),
    ("MPSAbyssPitArtillery", "Artillery"),
    ("MPSAncestralTotemSpiritSoulCasterProjectile", "Projectile Spell"),
    ("MPSArmourCasterBasic", "Fireball"),
    ("MPSAzmeriPictStaffProj", "Chaos Bolt"),
    ("MPSAzmeriPictStaffProj2", "Chaos Bolt"),
    ("MPSBloodMageBloodProjectile", "Blood Projectile"),
    ("MPSBoneCultistNecromancerLightning", "Basic Spell (Lightning)"),
    ("MPSBoneCultistZealotFire", "Basic Spell (Fire)"),
    ("MPSBoneCultistZealotLightning", "Basic Spell (Lightning)"),
    ("MPSBoneRabbleBurningArrow", "Burning Arrow"),
    ("MPSBreachEliteBoneProjectile", "Basic Spell (Cold)"),
    ("MPSBrineMaidenIceProjectile", "Ice Projectile"),
    ("MPSChaosGodTriHeadLizardBasicProjectile", "Basic Spell (Chaos)"),
    ("MPSCrawGullSpit", "Spit"),
    ("MPSElectricStingRayProjectile", "Basic Spell (Lightning)"),
    ("MPSExpeditionBoneCultistProjectiles", "Basic Spell (Cold)"),
    ("MPSGoblinMinerRockThrow", "Rock Throw"),
    ("MPSGoblinShamanBasicProj", "Basic Spell"),
    ("MPSHellscapeDemonFodderProj", "Fireball"),
    ("MPSHellscapeFleshEliteBasicProj", "Basic Spell (Physical)"),
    ("MPSHellscapePaleHammerhead", "Basic Spell (Physical)"),
    ("MPSKaruiCasterProjectile", "Basic Spell (Cold)"),
    ("MPSMercurialCasterEnrage", "Basic Spell"),
    ("MPSPlagueNymphRailGun", "Rail Gun"),
    ("MPSRaptorDefenderExplosiveShot", "Explosive Shot"),
    ("MPSRedSkeletonCaster", "Basic Spell (Cold)"),
    ("MPSSkeletonMancerBasicProj", "Basic Spell"),
    ("MPSSpearfisherSpearThrow", "Spear Throw"),
    ("MPSStarfishVomit", "Vomit"),
    ("MPSTwilightClericProjectile", "Basic Spell"),
    ("MPSTwilightOrderPlagueNymphRailGun", "Rail Gun"),
    ("MPSTwilightSorcerorFireball", "Fireball"),
    ("MPSVaalBloodPriestProj", "Blood Projectile"),
    ("MPSVaalConstructCannon", "Cannon"),
    ("MPSVaalHumanoidCannonNapalmMiniBlob", "Napalm Cannon"),
    ("MPSVaalSunApparitionBasicProj", "Basic Spell"),
    ("MPWAzmeriPitifulFabricationSkullThrow", "Skull Throw"),
    ("MPWBlackStriderWebProjectile", "Web Projectile"),
    ("MPWBlackStriderWebProjectileSanctum", "Web Projectile"),
    ("MPWCleansedMonstrosityRailgun", "Railgun"),
    ("MPWDrudgeExplosiveGrenade", "Explosive Grenade"),
    ("MPWExpeditionArbalestProjectile", "Basic Attack"),
    ("MPWExpeditionArbalestSnipe", "Snipe"),
    ("MPWFarudinSpearThrow", "Spear Throw"),
    ("MPWGoblinSpearThrow", "Spear Throw"),
    ("MPWKelpDregPuncture", "Puncture"),
    ("MPWKelpDregPunctureIce", "Puncture"),
    ("MPWVaalSavageBlowDart", "Blow Dart"),
    ("MeleeAtAnimationSpeedCold", "Basic Attack (Cold)"),
    ("MeleeAtAnimationSpeedFire", "Basic Attack (Fire)"),
    ("MeleeAtAnimationSpeedFireCombo35", "Basic Attack (Fire)"),
    ("MeleeAtAnimationSpeedLightning", "Basic Attack (Lightning)"),
    ("MeleeMudBurrowerBite", "Bite"),
    ("MeleeMudBurrowerLeftCleave", "Left Cleave"),
    ("MeleeMudBurrowerRightCleave", "Right Cleave"),
    ("MudBurrowerGoopExplode", "Goop Explosion"),
    ("MudBurrowerMaggotSummon", "Summon Maggots"),
    ("MutewindBanditWomanLeap", "Leap Slam"),
    ("QuillCrabSpikeBurst", "Spike Burst"),
    ("QuillCrabSpikeBurstPoison", "Spike Burst"),
    ("QuillCrabSpikeBurstTropical", "Spike Burst"),
    ("QuillCrabSpikeShrapnel", "Spike Shrapnel"),
    ("QuillCrabSpikeShrapnelPoison", "Spike Shrapnel"),
    ("RavenousSwarmAttack", "Attack"),
    ("RisenArbalestBasicProjectile", "Basic Attack"),
    ("RisenArbalestSnipe", "Snipe"),
    ("SSMJellyfishNettlerMinions", "Summon Minions"),
    ("SerpentClanTailWhip", "Tail Whip"),
    ("ShellMonsterDeathMortar", "Death Mortar"),
    ("ShellMonsterDeathMortarPoison", "Death Mortar"),
    ("ShellMonsterFirehose", "Firehose"),
    ("ShellMonsterSprayMortar", "Mortar"),
    ("ShellMonsterSprayMortarPoison", "Mortar"),
    ("SiegeBallistaProjectilePlayer", "Artillery"),
    ("SpookyGhostLightningBounce", "Basic Spell"),
    ("SpookyWraithProjectileExplosionCold", "Basic Spell"),
    ("TBAbyssCarrionWingBeam", "Beam"),
    ("TBBreachElitePaleLightningBoltSpammableLeft", "Lightning Bolt"),
    ("TBExcavatorSceptreErraticBeam", "Beam"),
    ("TBHellscapePaleLightningBoltSpammableLeft", "Lightning Bolt"),
    ("TBVaalPyramidBeam", "Pyramid Beam"),
    ("TCAncestralLeagueKaruiHulk", "Shield Charge"),
    ("TCExcavatorOrbCharge", "Orb Charge"),
    ("TCHellscapePaleElite2Charge", "Charge"),
    ("TCRathbreakerDrivebyCharge", "Charge"),
    ("TCTwilghtOrderSoldierCharge", "Charge"),
    ("UrchinSlingProjectile", "Sling Rock"),
    ("VaalBloodPriestDetonateDead", "Detonate Dead"),
    ("VaalBloodPriestSoulrend", "Soulrend"),
    ("VaalHumanoidShockRifle", "Shock Rifle"),
    ("VaalZealotLightningSpark", "Spark"),
    ("VaalZealotLightningSparkNova", "Spark Nova"),
    ("VoidIllusionSpawnPlayer", "Void Illusion"),
    ("WolfPackleaderDashAttack", "Dash"),
    ("WolfPackleaderLungeBite", "Bite"),
];

const POE1_NAMES: [(&str, &str); 301] = [
    ("ABTTAzmeriBasaliskShroud", "Poison DoT"),
    ("ABTTAzmeriGoddessAura", "Skeleton Buff"),
    ("ABTTAzmeriShepherdSpellDamage", "Damage Buff"),
    ("ABTTAzmeriSpiderLeaderAura", "Spider Buff"),
    ("ABTTAzmeriTurtleInvulnerability", "Damage Immunity Buff"),
    ("AbsolutionMinionEmpowered", "Empowered Absolution"),
    ("AbsolutionMinionVaalCascade", "Absolution Cascade"),
    ("AfflictionMinionPhysSlamCircleBig", "Big Circle"),
    ("AfflictionMinionPhysSlamCircleRectangle", "Rectangle"),
    ("AfflictionMinionPhysSlamCircleSmall", "Small Circle"),
    ("AtlasCruasderJudgeFadingNova", "Nova Spell"),
    ("AtlasCrusaderMageguardBeam", "Beam"),
    ("AtlasCrusaderSisterMortarSpectre", "Mortar"),
    ("AtlasExileCrusaderMageguardBombExplodeSpectre", "Bombs"),
    ("AtlasExilesCrusaderMageguardProjectile", "Projectile Spell"),
    ("AtlasEyrieArcherCrystalImpact", "Crystal Impact"),
    ("AtlasEyrieArcherMortar", "Mortar"),
    ("AtlasEyrieArcherSnipe", "Snipe"),
    ("AtlasEyrieBirdBreath", "Chilling Breath"),
    ("AtlasEyrieKiwethMortarShards", "Mortar Shards"),
    ("AtlasEyrieKiwethMortarSpectre", "Mortar"),
    ("AzmeriAdmiralDashMortars", "Dash Mortars"),
    ("AzmeriAdmiralDashThrustTriggered", "Dash thrust"),
    ("AzmeriAdmiralDoubleStrikeTriggered", "Double Strike"),
    ("AzmeriAdmiralGeyserDamage", "Geyser Damage"),
    ("AzmeriAdmiralTidalWave", "Tidal Wave"),
    ("AzmeriBarrageDemonSpineProjectile", "Spine Projectile"),
    ("AzmeriBasiliskComboSlam", "Combo Slam"),
    ("AzmeriBasiliskComboThrust", "Combo Thrust"),
    ("AzmeriBasiliskDecapThrust", "Decapitate Thrust"),
    ("AzmeriBasiliskDecapitateRightToLeft", "Decapitate"),
    ("AzmeriBasiliskDualProjectile", "Dual Projectile"),
    ("AzmeriBasiliskDualProjectileImpact", "Projectile Impact"),
    ("AzmeriBasiliskShoulderMortar", "Mortar"),
    ("AzmeriBasiliskShoulderMortar2", "Mortar 2"),
    ("AzmeriBasiliskWyvernFlight", "Wyvern Flight"),
    ("AzmeriBasiliskWyvernGroundCollide", "Ground Slam"),
    ("AzmeriBirdBeam", "Beam"),
    ("AzmeriBirdScreechExposure", "Screech"),
    ("AzmeriBossShockRifleSingle", "Lightning Beam"),
    ("AzmeriCasterDemonProjectile", "Default Attack"),
    ("AzmeriCycloneDemonCleave", "Cleave"),
    ("AzmeriDemonTeethShot", "Projectile"),
    ("AzmeriDualStrikeDemonFireEnrage", "Enrage"),
    ("AzmeriFirefuryFireResistAura", "Purity of Fire"),
    ("AzmeriGeofriSlam", "Slam"),
    ("AzmeriGoddessOfferingOfJudgement", "Fire Pillar"),
    ("AzmeriGoddessOfferingOfJudgementChaos", "Chaos Pillar"),
    ("AzmeriGoddessSpiritMortar", "Mortar"),
    ("AzmeriGolemBossWhipLeft", "Turn Attack"),
    ("AzmeriGolemRotateZap", "Spinning Zap"),
    ("AzmeriGolemVTurretProjectile", "Turret Projectile"),
    ("AzmeriGuardian4BeamGun", "Spinning Beam"),
    ("AzmeriGuardian4Slam", "Unpowered Slam"),
    ("AzmeriHailrakeColdResistAura", "Purity of Ice"),
    ("AzmeriHydraBarrage", "Barrage"),
    ("AzmeriHydraDoomArrow", "Doom Arrow"),
    ("AzmeriHydraForkArrow", "Fork Arrow"),
    ("AzmeriMegaSkeletonCleave", "Cleave"),
    ("AzmeriMegaSkeletonHeavyMelee", "Heavy Melee"),
    ("AzmeriOversoulExplosionIgnite", "Slam"),
    ("AzmeriOversoulLaserMaxShock", "Laser"),
    ("AzmeriOversoulRocksTriggered", "Rain of Boulders"),
    ("AzmeriPhantasmExplode", "Explode"),
    ("AzmeriPhantasmExplodeSap", "Explode"),
    ("AzmeriSpiderLeaderMortar", "Mortar"),
    ("AzmeriStampedeTiger", "Stampede"),
    ("AzmeriSwordStormCascade", "Sword Cascade"),
    ("AzmeriTentacleMinionLightningResistAura", "Purity of Lightning"),
    ("AzmeriTigerGeometryAttackStrafe", "Strafe Attack"),
    ("AzmeriTigerSpiritFangs", "Bite"),
    ("AzmeriTigerSpiritLacerate", "lacerate"),
    ("AzmeriTigerSpiritTeleportSlam", "Teleport Slam"),
    ("AzmeriVikingUpheaval", "Sunder"),
    ("AzmeriZombieCausticGroundWhenHit", "Caustic Ground"),
    ("BetrayalSecretPoliceCurveDagger1", "Secret Police Daggers"),
    ("BirdmanBloodProjectileMortar", "Blood Projectile"),
    ("BirdmanConsumeCorpse", "Consume Corpse"),
    ("BlinkMirrorArrowMelee", "Projectile Attack"),
    ("BoneGolemCascade", "Cascade"),
    ("BoneGolemCascadeEmpowered", "Empowered Cascade"),
    ("BoneGolemMultiAttack", "Combo Attack"),
    ("BreachArc", "Breach Arc"),
    ("BreachBlizzardSpectre", "Snow Cloak"),
    ("BreachLightningOrbsCommander", "Breach Lightning Orbs Commander"),
    ("BreachLightningWhip", "Breach Lightning Whip"),
    ("BreachTeamWarp", "Breach Team Warp"),
    ("BullCharge", "Charge"),
    ("CGEFaridunWarlockSwarmGround", "Caustic Ground"),
    ("CageSpiderSandSpark", "Sandstorm"),
    ("ChaosDegenAura", "Chaos Aura"),
    ("ChaosElementalCascadeSummoned", "Cascade"),
    ("CrucibleIceStormTrap", "Ice Storm"),
    ("DTTHellscapeStabWeb", "Thunder Web"),
    ("DeathExplodeIceElementalSummoned", "Explode"),
    ("DeceleratingProjectileAzmeriCasterDemon", "Projectile"),
    ("DeceleratingProjectileAzmeriCasterDemonExplode", "Explode"),
    ("DelayedBlastSpectre", "Delayed Blast"),
    ("DelveMelee", "Default Attack (Cold DoT)"),
    ("DelveMeleeCold", "Default Attack (Cold Hit)"),
    ("DelveProtovaalWhirlingCharge", "Whirling Charge"),
    ("DelveSpiderFlickerStrike", "Flicker Strike (Cold Hit)"),
    ("DelveWraithScreechChaos", "Chaos Screech"),
    ("DemonFemaleRangedProjectile2", "Ranged Attack"),
    ("EDSHarvestMinerHammerSmoke", "Hammer Smoke"),
    ("EDSHeistScienceUnarmedGas", "Gas"),
    ("EGBoneGolemConsumeCorpse", "Consume Corpse"),
    ("ElderTentacleMinionProjectile", "Projectile"),
    ("ElderTentacleMinionProjectileDeepcaller", "Projectile Spell"),
    ("ElderTentacleMinionProjectileEpic", "Projectile Large"),
    ("ElementalHitSkeletonKnight", "Elemental Hit Fire"),
    ("EmptyActionAttackAzmeriGolemVSlam", "Slam"),
    ("EmptyActionAttackSecretPoliceDaggers", "Dagger Trigger Attack"),
    ("EmptyActionSpellWarlordGrandmaster", "Arena Master's Presence"),
    ("FireElementalConeSummoned", "Flame Wave"),
    ("FireElementalFlameRedSummoned", "Immolate"),
    ("FireElementalMeteorSummoned", "Meteor"),
    ("FireElementalMortarSummoned", "Magma Ball"),
    ("FireMonsterWhirlingBlades", "Fire Roll"),
    ("FireballIncursionChaos", "Chaos Ball"),
    ("FireballIncusionLightning", "Lightning Ball"),
    ("FlamebearerFlameBlue", "Blue Flame"),
    ("GAAzmeriDemonLeapSlamDamage", "Leap Slam"),
    ("GAAzmeriDemonMeleeMiniSlam1", "Claw Slam"),
    ("GAAzmeriReaperComboRightSlash", "Slash"),
    ("GAAzmeriReaperComboWhirl", "Whirl"),
    ("GAAzmeriReaperLacerate", "Lacerate"),
    ("GAAzmeriRobotArgusSlam", "Slam"),
    ("GABeastCleave", "Cleave"),
    ("GAExpeditionDeathKnightSlam", "Slam"),
    ("GAHarvestCrabDashSlam", "Dash Slam"),
    ("GAHarvestMinerHammerSlam", "Hammer Slam"),
    ("GAHarvestRhexDashSlash", "Dash Slash"),
    ("GAHeistCultistUnarmedLeapImpact", "Whirling Blades Impact"),
    ("GAHeistRobotHoundStomp", "Stomp"),
    ("GAHeistThugRangedArrowShotgun", "Arrow Shotgun"),
    ("GAHeistThugRangedShotgun", "Ranged Shotgun"),
    ("GAHellscapeDemonElite1DashSlash", "Dash Slash"),
    ("GAHellscapePaleEliteSkyStab", "Stab Attack"),
    ("GAHellscapeStabbyCleave1", "Cleave"),
    ("GASummonReaperComboLeftSlash", "Combo Slash"),
    ("GASummonReaperComboWhirl", "Whirl"),
    ("GASummonReaperUltimateLeftSlash", "Ultimate Slash"),
    ("GAVaalDominationLargeSlam", "AoE Slam"),
    ("GAZombieCorpseGroundImpact", "Falling Slam"),
    ("GPSHellscapeFleshEliteSpikeBarrage", "Spike Barrage"),
    ("GSAncestralDruidFlaskExplode", "Poisonous Concoction"),
    ("GSAzmeriAdmiralCannonball", "Cannonball"),
    ("GSAzmeriBirdDashZap", "Zap"),
    ("GSAzmeriDemonBossCorruptExplode", "Corrupted Blood Explode"),
    ("GSAzmeriHailrakeIceNova", "Ice Nova"),
    ("GSAzmeriShepherdBeamNuke", "Beam Nuke"),
    ("GSAzmeriTentacleMonsterBeam", "Beam"),
    ("GSAzmeriTentacleMonsterShockExplode", "Shock Explode"),
    ("GSExpeditionDeathKnightNova", "Nova Spell"),
    ("GSHarvestRhexScreech", "Screech"),
    ("GSHeistLightningVolatileExplode", "Volatile"),
    ("GSHeistLightningWaterfallHit", "Waterfall"),
    ("GSHeistRobotPyreBeamBlast", "Beam Blast"),
    ("GSHeistRobotPyreBeamSweepBeam", "Beam Sweep"),
    ("GSHeistRobotPyreNukeBeam", "Nuke Beam"),
    ("GSHeistRobotPyreNukeBeamChannelled", "Nuke Beam Channelled"),
    ("GSHeistScienceLightningDashImpact", "Dash"),
    ("GSHellscapeDemonElite1Screech", "Screech"),
    ("GSHellscapeDemonEliteBeamNuke", "Beam Nuke"),
    ("GSHellscapeFleshEliteBloodOrbExplosion", "Blood Orb Explosion"),
    ("GSHellscapePaleEliteBoltImpact", "Bolt Impact"),
    ("GSHellscapePaleEliteOmegaBeam", "Omega Beam"),
    ("GSRoboHoundBellyDamage", "Slam"),
    ("GoatmanFireMagmaOrb", "Magma Orb"),
    ("GoatmanMonsterSlam", "Slam"),
    ("GroundEffectsSlamDockworkerChampion", "Slam"),
    ("HarvestCrabAbyssSlam", "Slam Attack"),
    ("HarvestNessaCrabScreech", "Screech"),
    ("HeistCultistLightningBolt", "Lightning Bolt"),
    ("HeistThugRangedExplosiveArrow", "Explosive Arrow (20 Fuses)"),
    ("HellionRallyingCry", "Rallying Cry"),
    ("HellscapeFleshFodderArc", "Scourge Arc"),
    ("HeraldOfAgonyMinionMortar", "Mortar"),
    ("HeraldOfAgonyMinionTailSpike", "Tail Spike"),
    ("HeraldOfLightMinionSlam", "Slam"),
    ("IceElementalSpearSummoned", "Ice Spear"),
    ("IceElementalSpearSummonedDeathNova", "Death Nova"),
    ("IguanaProjectile", "Barrage"),
    ("IguanaProjectileChrome", "Barrage"),
    ("IncaMinionProjectile", "Chaos Projectile"),
    ("IncursionMeteorUpheaval", "Chaos Spikes"),
    ("InsectSpawnerSpit", "Spit"),
    ("KaomFireBeamTotemSpectre", "Scorching Ray Totem"),
    ("KaomWarriorGroundSlam", "Ground Slam"),
    ("KitavaDemonXMortar", "Mortar"),
    ("LegionKaruiArcherSnipe", "Snipe"),
    ("LegionKaruiMeleeCombo2", "Combo Attack"),
    ("LegionMonsterProximityShield", "Proximity Shield"),
    ("LightningGolemArcSummoned", "Storm Orb"),
    ("LightningGolemWrath", "Lightning Golem Wrath"),
    ("MMSAzmeriDemonBloodVomitLarge", "Large Vomit"),
    ("MMSAzmeriDemonBloodVomitMedium", "Medium Vomit"),
    ("MMSAzmeriDemonBloodVomitSmall", "Small Vomit"),
    ("MMSAzmeriShepherdTripleMortar", "Mortar"),
    ("MMSAzmeriShepherdVomitMortar", "Vomit Mortar"),
    ("MMSHeistRobotClockworkGolemMortarSpectre", "Frost Mortar"),
    ("MMSHellscapeDemonEliteTripleMortar", "Triple Mortal"),
    ("MMSHellscapeDemonEliteVomitMortar", "Vomit Mortar"),
    ("MMSPyromaniacIceMortar", "Ice Mortar"),
    ("MPSFaridunWarlockBloodSpray", "Blood Spray"),
    ("MPSHeistCultistStaffProjectileGreen", "Green Projectile"),
    ("MPSHeistRobotClockworkGolemBasicProjectile", "Frost Projectile"),
    ("MPSHellscapeFleshEliteBasicProj", "Projectile"),
    ("MPSPhantasmBasicBlood", "Projectile Spell"),
    ("MPWExpeditionSummonedArbalestProjectile", "Projectile Attack"),
    ("MPWHeistThugRangedBurningArrow", "Burning Arrow"),
    ("MPWVaalGuardBarrage", "Barrage"),
    ("MassFrenzy", "Mass Frenzy"),
    ("MassPower", "Mass Power"),
    ("MeleeEyrieBird", "Knockback Attack"),
    ("MeleeFire", "Basic Attack"),
    ("MeleeKaruiArcher", "Cold Arrow"),
    ("MinerThrowFireSpectre", "Throw Fire"),
    ("MonsterCausticBomb", "Caustic Bomb"),
    ("MonsterFireBomb", "Fire Bomb"),
    ("MonsterFlameRedCannibal", "Incinerate"),
    ("MonsterLesserMultiFireballSpectre", "Lesser Multi Fireball"),
    ("MonsterLesserMultiIceSpear", "Lesser Multi Ice Spear"),
    ("MonsterLightningThorns", "Lightning Thorns"),
    ("MonsterMultiFireballSpectre", "Multi Fireball"),
    ("MonsterMultiIceSpear", "Multi Ice Spear"),
    ("MonsterProjectileSpellLightningGolemSummoned", "Lightning Projectile"),
    ("MonsterProjectileWeakness", "Projectile Weakness"),
    ("MonsterProximityShield", "Proximity Shield"),
    ("MonsterRighteousFireWhileSpectred", "Unrighteous Fire"),
    ("MonsterSplitFireballSpectre", "Split Fireball"),
    ("MonsterSplitIceSpear", "Split Ice Spear"),
    ("PyroChaosFireball", "Chaos Fireball"),
    ("PyroSuicideExplosion", "Suicide Explosion"),
    ("RainOfArrowsCloneShot", "Rain of Arrow"),
    ("ReaperConsumeMinionForBuff", "Consume"),
    ("RelicTriggeredNova", "Nova"),
    ("RevenantBossSpellProjectile", "Lightning Projectile"),
    ("RevenantSpellProjectileSpectre", "Lightning Projectile"),
    ("RockGolemMinionWhirlingBlades", "Roll"),
    ("RockGolemSlam", "Slam"),
    ("SandLeaperDodgeLeft", "Sand Leaper Dodge Left"),
    ("SandLeaperDodgeRight", "Sand Leaper Dodge Right"),
    ("SandstormChaosElementalSummoned", "Chaos Aura"),
    ("SandstormChaosElementalSummonedEmpowered", "Empowered Chaos Aura"),
    ("SeaWitchScreech", "Screech"),
    ("SecretDesecrateMonsterMultiSlash", "Multi Slash"),
    ("SentinelHolySlam", "Crusade Slam"),
    ("SkeletonCannonBoneNova", "Bone Nova"),
    ("SkeletonCannonMortar", "Mortar"),
    ("SkeletonMassBowProjectile", "Puncture"),
    ("SkeletonMinionProjectileCold", "Cold Projectile"),
    ("SkitterbotWait", "Skitterbot Wait"),
    ("SlavedriverFlameWhip", "Lightning Surge"),
    ("SnakeSpineProjectile", "Spine Attack"),
    ("SolarisChampionFlameVortex", "Flame Vortex"),
    ("SpecialBeamCannon", "Beam"),
    ("SpectralSkullShieldCharge", "Charge"),
    ("SummonPhantasmFadingProjectile", "Physical Projectile"),
    ("SummonedReaperUltimate", "Ultimate"),
    ("SummonedSnakeProjectile", "Chaos Projectile"),
    ("SumonRagingSpiritMelee", "Melee"),
    ("SupportBloodMagicUniquePrismGuardian", "Blood Magic"),
    ("SupportCastOnLifeSpent", "Foulborn Kitava's Thirst"),
    ("SupportCastOnManaSpent", "Kitava's Thirst"),
    ("SupportCrabTotem", "Crab Totem"),
    ("SupportCurseOnTrapTriggered", "Hex on Trap"),
    ("SupportCursePillarTriggerCurses", "Doedre's Effigy"),
    ("SupportTriggerBowSkillOnBowAttack", "Maloney's Mechanism"),
    ("SupportTriggerElementalSpellOnBlock", "Svalinn Cast on Block"),
    ("SupportTriggerSpellFromHelmet", "Focus"),
    ("SupportTriggerSpellOnAttack", "Poet's Pen"),
    ("SupportTriggerSpellOnBowAttack", "Asenath's Chant"),
    ("SupportTriggerSpellOnBowAttackFreezeHit", "Wing of the Wyvern"),
    ("SupportTriggerSpellOnKill", "Squirming Terror"),
    ("SupportTriggerSpellOnSkillUse", "Trigger Craft"),
    ("SupportTriggerSpellOnUnarmedMeleeCriticalHit", "Seven Teachings"),
    ("SupportUniqueCastCurseOnCurse", "Vixen's Entrapment"),
    ("SupportUniqueCosprisMaliceColdSpellsCastOnMeleeCriticalStrike", "Cospri's Malice"),
    ("SupportUniqueMjolnerLightningSpellsCastOnHit", "Mjolner"),
    ("SupportUniqueVirulenceSpellsCastOnBlock", "Festering Resentment Cast on Block"),
    ("SynthesisPhysicalTripleMortar", "Triple Mortar"),
    ("SynthesisPhysicalVolatileSlam", "Volatile Slam"),
    ("SynthesisSoulstealerBolt", "Lightning Bolt"),
    ("SynthesisSoulstealerLaser", "Lightning Laser"),
    ("SynthesisSoulstealerProjectileLightning", "Lightning Projectile"),
    ("SynthesisSoulstealerProjectilePhysical", "Projectile"),
    ("SynthesisSoulstealerQuicksand", "Quicksand"),
    ("TBHellscapePaleLightningBoltSpammableLeft", "Lightning Bolt"),
    ("TarMortarTaster", "Tar Projectile"),
    ("TriggeredSummonGhostOnKill", "Triggered Summon Phantasm"),
    ("UltimatumGuardConeArrowCold", "Cone Arrow"),
    ("UltimatumGuardMeleeCold", "Cold Arrow"),
    ("VaalAbsolutionDelayedBlast", "AoE Blast"),
    ("VaalDominationSunder", "Sunder"),
    ("VaalIncursionSpecialBeamCannonBlood", "Physical Beam"),
    ("VaalincursionMortar", "Physical Mortar"),
    ("VolatileAnomaly", "Summon Volatile Anomaly"),
    ("WalkingDoubleSlash", "Double Slash"),
    ("ZombieSlam", "Slam"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_interpolation_behaves_like_a_lua_sequence() {
        let mut t: Vec<Option<f64>> = vec![Some(3.0), Some(3.0)];
        t.insert_at(1, 0.0);
        assert_eq!(t, vec![Some(0.0), Some(3.0), Some(3.0)]);
        t.assign(5, 1.0);
        assert_eq!(t.border(), 3, "a hole stops `#t` and `ipairs`");
        t.insert_at(2, 0.0);
        assert_eq!(&t[..4], &[Some(0.0), Some(0.0), Some(3.0), Some(3.0)]);
        let mut empty: Vec<Option<f64>> = Vec::new();
        empty.insert_at(3, 0.0);
        assert!(empty.is_empty(), "a position past the end is ignored");
    }

    #[test]
    fn description_scope_follows_the_stat_set() {
        let path = "Data/StatDescriptions/specific_skill_stat_descriptions/ancestral_cry/statset_0.csd";
        assert_eq!(stat_description_scope(path, 1), "ancestral_cry_statset_0");
        assert_eq!(stat_description_scope(path, 2), "ancestral_cry_statset_1");
        assert_eq!(stat_description_scope("Data/StatDescriptions/specific_skill_stat_descriptions/boneshatter.csd", 2), "boneshatter");
        // `gsub(".csd", "")` takes any character before `csd` with it.
        assert_eq!(stat_description_scope("a_bcsd_c.csd", 1), "a__c");
    }

    #[test]
    fn popup_filters_name_each_skills_file() {
        let text = "skill_a \"Metadata/StatDescriptions/aura_skill_stat_descriptions.txt\"\n\
                    skill_b \"Metadata/StatDescriptions/minion_skill_stat_descriptions.txt\"\n\
                    copy skill_c skill_a\ncopy skill_b skill_missing\n";
        let scopes = skill_stat_scopes(text);
        assert_eq!(scopes.get("skill_a").map(String::as_str), Some("aura_skill_stat_descriptions"));
        assert_eq!(scopes.get("skill_c").map(String::as_str), Some("aura_skill_stat_descriptions"));
        assert_eq!(scopes.get("skill_b"), None, "copying from a skill with no entry clears it");
    }

    #[test]
    fn text_is_read_back_as_lua_sees_it() {
        assert_eq!(description("Say \"hi\"\r\nto [Fire|Fire Damage]"), "Say \"hi\"\nto Fire Damage");
        assert_eq!(clean_and_split("  One \"line\"\r\n\r\n  Two  "), vec!["One \"line\"", "Two"]);
        assert_eq!(strip_support("Support Support Gem Support"), "Support Gem");
    }

    #[test]
    fn ui_images_are_read_like_pob_reads_them() {
        let images = parse_ui_images("\"Art/2DArt/UIImages/Cat\" \"Art/Textures/Cat.dds\" 0 0 32 32\r\nbroken\n");
        let cat = images.get("art/2dart/uiimages/cat").expect("parsed");
        assert_eq!(cat.path, "art/textures/cat.dds");
        assert_eq!((cat.x, cat.y, cat.width, cat.height), (Some(0.0), Some(0.0), Some(32.0), Some(32.0)));
        assert_eq!(images.get("broken"), Some(&UiImage::default()));
    }

    fn dds_header(width: u32, height: u32, four_cc: &[u8; 4], dxgi: Option<u32>) -> Vec<u8> {
        let mut b = vec![0u8; 148];
        b[..4].copy_from_slice(b"DDS ");
        b[12..16].copy_from_slice(&height.to_le_bytes());
        b[16..20].copy_from_slice(&width.to_le_bytes());
        b[80..84].copy_from_slice(&4u32.to_le_bytes());
        b[84..88].copy_from_slice(four_cc);
        if let Some(format) = dxgi {
            b[128..132].copy_from_slice(&format.to_le_bytes());
        }
        b
    }

    #[test]
    fn texture_formats_are_named_like_simplegraphic() {
        assert_eq!(dds_info(&dds_header(64, 64, b"DXT1", None)), Some((64, 64, "BC1".to_string())));
        assert_eq!(dds_info(&dds_header(708, 380, b"DX10", Some(98))), Some((708, 380, "BC7".to_string())));
        let mut plain = dds_header(108, 108, b"\0\0\0\0", None);
        plain[80..84].copy_from_slice(&0x41u32.to_le_bytes());
        plain[88..92].copy_from_slice(&32u32.to_le_bytes());
        assert_eq!(dds_info(&plain), Some((108, 108, "RGBA".to_string())));
        assert_eq!(dds_info(b"not a texture"), None);
    }

    #[test]
    fn sheets_keep_one_entry_per_alias() {
        let mut sheet = Sheet::new("gem-icons");
        sheet.add("a.dds", Meta::alias("A"));
        sheet.add("a.dds", Meta::alias("A"));
        sheet.add("a.dds", Meta::alias("B"));
        sheet.add("", Meta::alias("C"));
        assert_eq!(sheet.files.len(), 1);
        assert_eq!(sheet.files["a.dds"].len(), 2);
    }

    #[test]
    fn curated_tables_name_known_files() {
        for (_, file) in POE2_FILE_OVERRIDES {
            assert!(skill_files(Game::Poe2).contains(&file), "{}", file);
        }
        for (_, file) in POE1_FILE_OVERRIDES {
            assert!(skill_files(Game::Poe1).contains(&file), "{}", file);
        }
        for names in [&POE2_NAMES[..], &POE1_NAMES[..]] {
            let mut ids: Vec<&str> = names.iter().map(|(id, _)| *id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), names.len(), "each skill is named once");
        }
    }
}
