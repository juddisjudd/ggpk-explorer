//! `Bases/*.lua`, `FlavourText.lua` and PoE 2's `InventorySlots.lua`, ported
//! from PoB's `bases.lua`, `flavourText.lua` and `buildplanner.lua`.
//!
//! PoB chooses the bases for each file with hand-written directives
//! (`Export/Bases/*.txt`). Here the file, `type` and `subType` come from the
//! base's item class and tags, so a base GGG adds to a known class shows up
//! without a list to edit. What PoB decides by hand —
//! the bases it hides, the extra names it gives a few, and the old rows it
//! leaves out — is kept in small per-game tables.

use crate::dat::relational::{LoadedTable, Row};
use crate::data_export::Ctx;
use crate::pob_export::lua::tostring;
use crate::pob_export::modules::mods::{describe_mod, ModReader};
use crate::pob_export::statdesc::{decode_utf16, lua_literal, Descriptors, Stats};
use crate::pob_export::text::{round, sanitise_text};
use crate::pob_export::{describer, game, write, Table};
use crate::settings::Game;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

/// What picks a base for a rule.
enum On {
    Class(&'static str),
    /// A tag on the base's own row, for bases whose class says nothing (PoE 1
    /// keeps its grafts under `RemovedItem`).
    Tag(&'static str),
}

/// How a base's `subType` is found.
enum Sub {
    None,
    Fixed(&'static str),
    /// The first of these tags the base carries names its sub-type.
    Tags(&'static [(&'static str, &'static str)]),
}

/// The attribute tag each armour piece carries, and PoB's name for its
/// defences.
const ARMOUR: &[(&str, &str)] = &[
    ("str_armour", "Armour"),
    ("dex_armour", "Evasion"),
    ("int_armour", "Energy Shield"),
    ("str_dex_armour", "Armour/Evasion"),
    ("str_int_armour", "Armour/Energy Shield"),
    ("dex_int_armour", "Evasion/Energy Shield"),
    ("str_dex_int_armour", "Armour/Evasion/Energy Shield"),
    ("ward_armour", "Ward"),
];

/// One PoB base file's share of the game's bases, and PoB's name for them.
struct Rule {
    on: On,
    file: &'static str,
    kind: &'static str,
    sub: Sub,
}

const fn class(id: &'static str, file: &'static str, kind: &'static str, sub: Sub) -> Rule {
    Rule { on: On::Class(id), file, kind, sub }
}

const POE2_RULES: [Rule; 36] = [
    class("One Hand Axe", "axe", "One Hand Axe", Sub::None),
    class("Two Hand Axe", "axe", "Two Hand Axe", Sub::None),
    class("Bow", "bow", "Bow", Sub::None),
    class("Claw", "claw", "Claw", Sub::None),
    class("Crossbow", "crossbow", "Crossbow", Sub::None),
    class("Dagger", "dagger", "Dagger", Sub::None),
    class("FishingRod", "fishing", "Fishing Rod", Sub::None),
    class("Flail", "flail", "Flail", Sub::None),
    class("One Hand Mace", "mace", "One Hand Mace", Sub::None),
    class("Two Hand Mace", "mace", "Two Hand Mace", Sub::None),
    class("Sceptre", "sceptre", "Sceptre", Sub::None),
    class("Spear", "spear", "Spear", Sub::None),
    class("Staff", "staff", "Staff", Sub::None),
    class("Warstaff", "staff", "Staff", Sub::Fixed("Warstaff")),
    class("One Hand Sword", "sword", "One Hand Sword", Sub::None),
    class("Two Hand Sword", "sword", "Two Hand Sword", Sub::None),
    class("Wand", "wand", "Wand", Sub::None),
    class("Helmet", "helmet", "Helmet", Sub::Tags(ARMOUR)),
    class("Body Armour", "body", "Body Armour", Sub::Tags(ARMOUR)),
    class("Focus", "focus", "Focus", Sub::None),
    class("Gloves", "gloves", "Gloves", Sub::Tags(ARMOUR)),
    class("Boots", "boots", "Boots", Sub::Tags(ARMOUR)),
    class("Shield", "shield", "Shield", Sub::Tags(ARMOUR)),
    class("Buckler", "shield", "Shield", Sub::Tags(ARMOUR)),
    class("Quiver", "quiver", "Quiver", Sub::None),
    class("TrapTool", "traptool", "TrapTool", Sub::None),
    class("Amulet", "amulet", "Amulet", Sub::None),
    class("Ring", "ring", "Ring", Sub::None),
    class("Belt", "belt", "Belt", Sub::None),
    class("Jewel", "jewel", "Jewel", Sub::Tags(&[("radius_jewel", "Radius")])),
    class("UtilityFlask", "flask", "Charm", Sub::None),
    class("LifeFlask", "flask", "Flask", Sub::Fixed("Life")),
    class("ManaFlask", "flask", "Flask", Sub::Fixed("Mana")),
    class("Talisman", "talisman", "Talisman", Sub::None),
    class("IncursionArm", "incursionlimb", "Transcendent Limb", Sub::Fixed("Transcendent Arm")),
    class("IncursionLeg", "incursionlimb", "Transcendent Limb", Sub::Fixed("Transcendent Leg")),
];

const CLUSTER: &[(&str, &str)] =
    &[("expansion_jewel_large", "Cluster"), ("expansion_jewel_medium", "Cluster"), ("expansion_jewel_small", "Cluster")];

const POE1_RULES: [Rule; 34] = [
    class("One Hand Axe", "axe", "One Handed Axe", Sub::None),
    class("Two Hand Axe", "axe", "Two Handed Axe", Sub::None),
    class("Bow", "bow", "Bow", Sub::None),
    class("Claw", "claw", "Claw", Sub::None),
    class("Dagger", "dagger", "Dagger", Sub::None),
    class("Rune Dagger", "dagger", "Dagger", Sub::Fixed("Rune Dagger")),
    class("FishingRod", "fishing", "Fishing Rod", Sub::None),
    class("One Hand Mace", "mace", "One Handed Mace", Sub::None),
    class("Sceptre", "mace", "Sceptre", Sub::None),
    class("Two Hand Mace", "mace", "Two Handed Mace", Sub::None),
    class("Staff", "staff", "Staff", Sub::None),
    class("Warstaff", "staff", "Staff", Sub::Fixed("Warstaff")),
    class("One Hand Sword", "sword", "One Handed Sword", Sub::None),
    class("Thrusting One Hand Sword", "sword", "One Handed Sword", Sub::Fixed("Thrusting")),
    class("Two Hand Sword", "sword", "Two Handed Sword", Sub::None),
    class("Wand", "wand", "Wand", Sub::None),
    class("Helmet", "helmet", "Helmet", Sub::Tags(ARMOUR)),
    class("Body Armour", "body", "Body Armour", Sub::Tags(ARMOUR)),
    class("Gloves", "gloves", "Gloves", Sub::Tags(ARMOUR)),
    class("Boots", "boots", "Boots", Sub::Tags(ARMOUR)),
    class("Shield", "shield", "Shield", Sub::Tags(ARMOUR)),
    class("Quiver", "quiver", "Quiver", Sub::None),
    class("Amulet", "amulet", "Amulet", Sub::Tags(&[("talisman", "Talisman")])),
    class("Ring", "ring", "Ring", Sub::None),
    class("Belt", "belt", "Belt", Sub::None),
    class("Jewel", "jewel", "Jewel", Sub::Tags(CLUSTER)),
    class("AbyssJewel", "jewel", "Jewel", Sub::Fixed("Abyss")),
    class("AnimalCharm", "jewel", "Jewel", Sub::Fixed("Charm")),
    class("LifeFlask", "flask", "Flask", Sub::Fixed("Life")),
    class("ManaFlask", "flask", "Flask", Sub::Fixed("Mana")),
    class("HybridFlask", "flask", "Flask", Sub::Fixed("Hybrid")),
    class("UtilityFlask", "flask", "Flask", Sub::Fixed("Utility")),
    class("Tincture", "tincture", "Tincture", Sub::None),
    Rule { on: On::Tag("graft"), file: "graft", kind: "Graft", sub: Sub::None },
];

/// PoB's own choices about single bases, which no game column records.
#[derive(PartialEq)]
enum Tweak {
    /// A base PoB leaves out of its files.
    Skip,
    /// `#forceHide`: in the file, but not offered in the item crafter.
    Hide,
    /// Written a second time under PoB's name for it, shown.
    Also(&'static str),
    /// Written under PoB's name instead of the game's.
    Rename(&'static str),
    SubType(Option<&'static str>),
}

type Tweaks = &'static [(&'static str, &'static [Tweak])];

const POE2_TWEAKS: Tweaks = &[
    ("Metadata/Items/Weapons/OneHandWeapons/Sceptres/FourSceptre6a", &[Tweak::Hide, Tweak::Also("Shrine Sceptre (Purity of Fire)")]),
    ("Metadata/Items/Weapons/OneHandWeapons/Sceptres/FourSceptre6b", &[Tweak::Hide, Tweak::Also("Shrine Sceptre (Purity of Cold)")]),
    ("Metadata/Items/Weapons/OneHandWeapons/Sceptres/FourSceptre6c", &[Tweak::Hide, Tweak::Also("Shrine Sceptre (Purity of Lighting)")]),
    ("Metadata/Items/Weapons/OneHandWeapons/Sceptres/FourSceptreUnique1", &[Tweak::Skip]),
    ("Metadata/Items/Armours/Gloves/FourGlovesDexIntAscendancy", &[Tweak::Hide]),
    ("Metadata/Items/Armours/Gloves/FourGlovesDexIntAscendancyVerisium", &[Tweak::Hide]),
    ("Metadata/Items/Armours/Helmets/FourHelmetDemigod", &[Tweak::Skip]),
    ("Metadata/Items/Jewels/JewelTimeless", &[Tweak::Hide, Tweak::SubType(Some("Timeless"))]),
    ("Metadata/Items/Weapons/OneHandWeapons/OneHandSwords/StormBladeOneHand", &[Tweak::Hide]),
    ("Metadata/Items/Weapons/TwoHandWeapons/TwoHandSwords/StormBladeTwoHand", &[Tweak::Hide]),
    ("Metadata/Items/Weapons/TwoHandWeapons/TwoHandSwords/TwoHandSwordDev", &[Tweak::Hide]),
];

const POE1_TWEAKS: Tweaks = &[
    ("Metadata/Items/Amulets/Talismans/Talisman2_6_1", &[Tweak::Also("Avian Twins Talisman (Fire-To-Cold)")]),
    ("Metadata/Items/Amulets/Talismans/Talisman2_6_2", &[Tweak::Also("Avian Twins Talisman (Fire-To-Lightning)")]),
    ("Metadata/Items/Amulets/Talismans/Talisman2_6_3", &[Tweak::Also("Avian Twins Talisman (Cold-To-Lightning)")]),
    ("Metadata/Items/Amulets/Talismans/Talisman2_6_4", &[Tweak::Also("Avian Twins Talisman (Cold-To-Fire)")]),
    ("Metadata/Items/Amulets/Talismans/Talisman2_6_5", &[Tweak::Also("Avian Twins Talisman (Lightning-To-Cold)")]),
    ("Metadata/Items/Amulets/Talismans/Talisman3_6_1", &[Tweak::Also("Monkey Paw Talisman (Power)")]),
    ("Metadata/Items/Amulets/Talismans/Talisman3_6_2", &[Tweak::Also("Monkey Paw Talisman (Frenzy)")]),
    ("Metadata/Items/Rings/Ring12", &[Tweak::Also("Two-Stone Ring (Fire/Lightning)")]),
    ("Metadata/Items/Rings/Ring13", &[Tweak::Also("Two-Stone Ring (Cold/Lightning)")]),
    ("Metadata/Items/Rings/Ring14", &[Tweak::Also("Two-Stone Ring (Fire/Cold)")]),
    ("Metadata/Items/Armours/Boots/BootsAtlas1", &[Tweak::Rename("Two-Toned Boots (Evasion/Energy Shield)")]),
    ("Metadata/Items/Armours/Boots/BootsAtlas2", &[Tweak::Rename("Two-Toned Boots (Armour/Evasion)")]),
    ("Metadata/Items/Armours/Boots/BootsAtlas3", &[Tweak::Rename("Two-Toned Boots (Armour/Energy Shield)")]),
    ("Metadata/Items/Armours/BodyArmours/BodyDemigods1", &[Tweak::Hide]),
    ("Metadata/Items/Jewels/JewelTimeless", &[Tweak::SubType(Some("Timeless"))]),
    ("Metadata/Items/Weapons/OneHandWeapons/OneHandSwords/StormBladeOneHand", &[Tweak::Hide]),
    ("Metadata/Items/Weapons/TwoHandWeapons/TwoHandSwords/StormBladeTwoHand", &[Tweak::Hide]),
    // The quivers the `QuiverNew` bases replaced.
    ("Metadata/Items/Quivers/Quiver1", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver2", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver3", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver4", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver5", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver6", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver7", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver8", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver9", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver10", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver11", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver12", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/Quiver13", &[Tweak::Hide]),
    ("Metadata/Items/Quivers/QuiverDescent", &[Tweak::Skip]),
    ("Metadata/Items/Armours/Boots/BootsDescent1", &[Tweak::Skip]),
    ("Metadata/Items/Armours/BodyArmours/BodyStrTemp", &[Tweak::Skip]),
    ("Metadata/Items/Armours/Boots/BootsStrTemp", &[Tweak::Skip]),
    ("Metadata/Items/Armours/Helmets/HelmetDemigods1", &[Tweak::Skip]),
    ("Metadata/items/Weapons/OneHandWeapons/OneHandSwords/OneHandSwordDemigods1", &[Tweak::Skip]),
    ("Metadata/Items/Chayula/TutorialGraft", &[Tweak::Skip]),
    ("Metadata/Items/Tinctures/TinctureStun", &[Tweak::Skip]),
    ("Metadata/Items/Tinctures/TinctureIgnite", &[Tweak::Skip]),
    ("Metadata/Items/Tinctures/TinctureShock", &[Tweak::Skip]),
    ("Metadata/Items/Tinctures/TincturePoison", &[Tweak::Skip]),
    ("Metadata/Items/Tinctures/TinctureCriticalStrike", &[Tweak::Skip]),
    ("Metadata/Items/Tinctures/TinctureCullingStrike", &[Tweak::Skip]),
    ("Metadata/Items/Tinctures/TinctureFreeze", &[Tweak::Skip]),
];

/// PoE 1's influence suffixes, in the order `bases.lua` writes them. The
/// first six are the item class's influence tags; the Eldritch two follow
/// the same naming.
const INFLUENCES: [&str; 8] = ["shaper", "elder", "adjudicator", "basilisk", "crusader", "eyrie", "cleansing", "tangle"];

/// A number as Lua reads it back after PoB wrote it with `tostring`.
fn written(value: f64) -> f64 {
    tostring(value).parse().unwrap_or(value)
}

/// Lua's `s:match("^%s*(.-)%s*$")`.
fn lua_trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'))
}

/// Lua's `line:match(prefix .. '(.+)"')`: what sits between the first
/// `prefix` and the last quote after it.
fn capture_quoted<'l>(line: &'l str, prefix: &str) -> Option<&'l str> {
    let start = line.find(prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest.rfind('"')?;
    (end > 0).then(|| &rest[..end])
}

/// Lua's `table.sort(t, lt)`: Lua 5.1's quicksort, which LuaJIT keeps. Its
/// order for elements `lt` calls equal is part of PoB's output.
fn lua_sort<T>(items: &mut [T], lt: impl Fn(&T, &T) -> bool) {
    if items.len() > 1 {
        auxsort(items, 1, items.len() as isize, &lt);
    }
}

fn auxsort<T>(a: &mut [T], mut l: isize, mut u: isize, lt: &impl Fn(&T, &T) -> bool) {
    let at = |i: isize| (i - 1) as usize;
    while l < u {
        if lt(&a[at(u)], &a[at(l)]) {
            a.swap(at(l), at(u));
        }
        if u - l == 1 {
            break;
        }
        let mut i = (l + u) / 2;
        if lt(&a[at(i)], &a[at(l)]) {
            a.swap(at(i), at(l));
        } else if lt(&a[at(u)], &a[at(i)]) {
            a.swap(at(i), at(u));
        }
        if u - l == 2 {
            break;
        }
        a.swap(at(i), at(u - 1));
        let pivot = at(u - 1);
        i = l;
        let mut j = u - 1;
        loop {
            i += 1;
            while i < u && lt(&a[at(i)], &a[pivot]) {
                i += 1;
            }
            j -= 1;
            while j > l && lt(&a[pivot], &a[at(j)]) {
                j -= 1;
            }
            if j < i {
                break;
            }
            a.swap(at(i), at(j));
        }
        a.swap(at(u - 1), at(i));
        if i - l < u - i {
            let (lo, hi) = (l, i - 1);
            l = i + 1;
            auxsort(a, lo, hi, lt);
        } else {
            let (lo, hi) = (i + 1, u);
            u = i - 1;
            auxsort(a, lo, hi, lt);
        }
    }
}

/// `socket_info` lists `sockets:item level:weight` for each count; a level of
/// 9999 rules the count out.
fn socket_limit(info: &str) -> Option<i64> {
    info.split_whitespace()
        .filter_map(|entry| {
            let mut parts = entry.split(':');
            let count: i64 = parts.next()?.trim().parse().ok()?;
            let level: i64 = parts.next()?.trim().parse().ok()?;
            (level < 9999).then_some(count)
        })
        .max()
}

/// `getMaximumQuality`'s three outcomes: `0` at the end of the chain, nil
/// for a missing file, or the text after `max_quality = `.
#[derive(Clone)]
enum Quality {
    Zero,
    Nil,
    Text(String),
}

/// The `.it` files `bases.lua` reads, split into lines as PoB splits them,
/// and what it works out from each inheritance chain.
#[derive(Default)]
struct ItFiles {
    lines: HashMap<String, Option<Rc<Vec<String>>>>,
    tags: RefCell<HashMap<String, Option<Vec<String>>>>,
}

impl ItFiles {
    /// Reads every file the chains of `paths` reach, a generation at a time,
    /// in bundle order.
    fn load(&mut self, ctx: &Ctx, paths: impl IntoIterator<Item = String>) {
        let mut pending: Vec<String> = paths.into_iter().collect();
        while !pending.is_empty() {
            pending.retain(|p| p != "nothing" && !self.lines.contains_key(&p.to_ascii_lowercase()));
            pending.sort();
            pending.dedup();
            let names: Vec<String> = pending.iter().map(|p| format!("{}.it", p)).collect();
            let found = ctx.files.fetch_many(&names);
            let mut next = Vec::new();
            for (path, name) in pending.iter().zip(&names) {
                let lines = found.get(name).map(|bytes| {
                    let text = decode_utf16(bytes);
                    Rc::new(text.split(['\r', '\n']).filter(|l| !l.is_empty()).map(str::to_string).collect::<Vec<_>>())
                });
                if let Some(lines) = &lines {
                    next.extend(lines.iter().filter_map(|l| capture_quoted(l, "extends \"")).map(str::to_string));
                }
                self.lines.insert(path.to_ascii_lowercase(), lines);
            }
            pending = next;
        }
    }

    fn file(&self, path: &str) -> Option<Rc<Vec<String>>> {
        self.lines.get(&path.to_ascii_lowercase()).cloned().flatten()
    }

    /// `getBaseItemTags`: each file's `tag`s after its parent's. A
    /// `remove_tag` drops the first match, or the last tag when nothing
    /// matches (`table.remove(t, nil)`).
    fn tags(&self, path: &str, depth: usize) -> Option<Vec<String>> {
        if path == "nothing" {
            return Some(Vec::new());
        }
        if let Some(hit) = self.tags.borrow().get(path) {
            return hit.clone();
        }
        let lines = self.file(path)?;
        let mut tags: Vec<String> = Vec::new();
        for line in lines.iter() {
            if let Some(parent) = capture_quoted(line, "extends \"") {
                if depth < 32 {
                    tags.extend(self.tags(parent, depth + 1).unwrap_or_default());
                }
            } else if line.contains("remove_tag") {
                let at = capture_quoted(line, "remove_tag = \"").and_then(|name| tags.iter().position(|t| t == name));
                match at {
                    Some(i) => {
                        tags.remove(i);
                    }
                    None => {
                        tags.pop();
                    }
                }
            } else if line.contains("tag") {
                if let Some(tag) = capture_quoted(line, "tag = \"") {
                    tags.push(tag.to_string());
                }
            }
        }
        self.tags.borrow_mut().insert(path.to_string(), Some(tags.clone()));
        Some(tags)
    }

    /// `getMaximumQuality`: the nearest file in the chain that states one.
    fn quality(&self, path: &str, depth: usize) -> Quality {
        if path == "nothing" {
            return Quality::Zero;
        }
        let Some(lines) = self.file(path) else { return Quality::Nil };
        let mut inherited = Quality::Nil;
        for line in lines.iter() {
            if let Some(parent) = capture_quoted(line, "extends \"") {
                if depth < 32 {
                    inherited = self.quality(parent, depth + 1);
                }
            } else if line.contains("max_quality") {
                return match line.find("max_quality = ").map(|at| &line[at + "max_quality = ".len()..]) {
                    Some(rest) if !rest.is_empty() => Quality::Text(rest.to_string()),
                    _ => Quality::Nil,
                };
            }
        }
        inherited
    }

    /// The most sockets a base can roll, from the nearest file in the chain
    /// whose `Sockets` component allows any.
    fn sockets(&self, path: &str, depth: usize) -> Option<i64> {
        if path == "nothing" || depth > 32 {
            return None;
        }
        let lines = self.file(path)?;
        let mut parent = None;
        for line in lines.iter() {
            if let Some(p) = capture_quoted(line, "extends \"") {
                parent = Some(p);
            } else if let Some(limit) = capture_quoted(line, "socket_info = \"").and_then(socket_limit) {
                return Some(limit);
            }
        }
        parent.and_then(|p| self.sockets(p, depth + 1))
    }
}

/// What PoB reads from one implicit mod.
struct Implicit {
    id: String,
    level: i64,
    lines: Vec<String>,
    first_order: f64,
    tags: Vec<String>,
    unscalable: bool,
    stats: Vec<(String, f64, f64, u32)>,
}

/// A side table keyed by the base item each row describes.
struct Side {
    table: Option<Rc<LoadedTable>>,
    rows: HashMap<usize, usize>,
}

impl Side {
    fn new(ctx: &Ctx, bases: &LoadedTable, names: &[&str]) -> Self {
        let Some(table) = names.iter().find(|n| ctx.rr.table(n).is_some()).and_then(|n| ctx.optional_table(n)) else {
            return Side { table: None, rows: HashMap::new() };
        };
        let mut rows = HashMap::new();
        if let Some(column) = table.pick(&["BaseItemType", "BaseItemTypesKey", "BaseItem", "BaseType"]) {
            for row in table.rows() {
                let base = row.key(column).or_else(|| {
                    let id = row.str(column);
                    (!id.is_empty()).then(|| bases.by_id(id).map(|b| b.index)).flatten()
                });
                if let Some(base) = base {
                    rows.entry(base).or_insert(row.index);
                }
            }
        }
        Side { table: Some(table), rows }
    }

    fn row(&self, base: usize) -> Option<Row<'_>> {
        self.table.as_ref()?.row(*self.rows.get(&base)?)
    }
}

/// Everything `bases.lua` looks at, loaded once.
struct Bases<'c, 'a> {
    ctx: &'c Ctx<'a>,
    game: Game,
    descriptors: Rc<Descriptors>,
    mods: Rc<LoadedTable>,
    reader: ModReader,
    implicits: RefCell<HashMap<usize, Rc<Implicit>>>,
    it: ItFiles,
    weapon: Side,
    armour: Side,
    shield: Side,
    flask: Side,
    charges: Side,
    requirements: Side,
    spirit: Side,
    inherent: Side,
    tincture: Side,
    /// PoE 1: the influence tag prefix of each item class, from `InfluenceTags`.
    influence: HashMap<usize, String>,
}

/// How one base is written: its rule, and what PoB's tweaks change.
struct Pick<'r> {
    rule: &'r Rule,
    name: Option<&'static str>,
    hide: bool,
    sub: Option<Option<&'static str>>,
}

impl<'c, 'a> Bases<'c, 'a> {
    fn new(ctx: &'c Ctx<'a>, bases: &LoadedTable) -> Result<Self, String> {
        let game = game(ctx);
        let descriptors = match game {
            Game::Poe2 => describer(ctx, &["stat_descriptions.csd"]),
            Game::Poe1 => describer(ctx, &["tincture_stat_descriptions.txt"]),
        };
        let mods = ctx.table("Mods")?;
        let reader = ModReader::new(&mods);
        let mut influence = HashMap::new();
        if game == Game::Poe1 {
            if let Some(table) = ctx.optional_table("InfluenceTags") {
                let tag_col = table.pick(&["Tag", "TagsKey", "Tags"]).unwrap_or("Tag");
                for row in table.rows() {
                    let (Some(class), Some(tag)) = (row.key("ItemClass"), ctx.rr.deref_id(row, tag_col)) else { continue };
                    if let Some(prefix) = tag.strip_suffix("_shaper") {
                        influence.insert(class, prefix.to_string());
                    }
                }
            }
        }
        Ok(Bases {
            ctx,
            game,
            descriptors,
            mods,
            reader,
            implicits: RefCell::new(HashMap::new()),
            it: ItFiles::default(),
            weapon: Side::new(ctx, bases, &["WeaponTypes"]),
            armour: Side::new(ctx, bases, &["ArmourTypes"]),
            shield: Side::new(ctx, bases, &["ShieldTypes"]),
            flask: Side::new(ctx, bases, &["Flasks"]),
            charges: Side::new(ctx, bases, &["ComponentCharges"]),
            requirements: Side::new(ctx, bases, &["AttributeRequirements", "ComponentAttributeRequirements"]),
            spirit: Side::new(ctx, bases, &["ItemSpirit"]),
            inherent: Side::new(ctx, bases, &["ItemInherentSkills"]),
            tincture: Side::new(ctx, bases, &["Tinctures"]),
            influence,
        })
    }

    /// `describeMod` for one mod, kept for the bases that share it.
    fn implicit(&self, index: usize) -> Option<Rc<Implicit>> {
        if let Some(hit) = self.implicits.borrow().get(&index) {
            return Some(Rc::clone(hit));
        }
        let row = self.mods.row(index)?;
        let m = self.reader.read(self.ctx, row);
        let described = describe_mod(self.game, &self.descriptors, &m);
        let implicit = Rc::new(Implicit {
            id: m.id.clone(),
            level: m.level,
            first_order: described.orders.first().copied().unwrap_or(0.0),
            lines: described.lines,
            tags: m.implicit_tags.clone(),
            unscalable: m.unscalable,
            stats: m.stats.clone(),
        });
        self.implicits.borrow_mut().insert(index, Rc::clone(&implicit));
        Some(implicit)
    }

    /// One `itemBases[name] = { … }`, or `None` for a base PoB skips.
    fn entry(&self, row: Row<'_>, pick: &Pick, tags: &BTreeSet<String>) -> Option<(String, Table)> {
        let game = self.game;
        let id = row.id();
        let rule = pick.rule;
        let mut name = pick.name.map(str::to_string).unwrap_or_else(|| row.string("Name")).replace('\u{f6}', "o");
        name = lua_trim(&name).to_string();
        if name == "Energy Blade" {
            name = match rule.kind {
                "One Hand Sword" | "One Handed Sword" => "Energy Blade One Handed".into(),
                _ => "Energy Blade Two Handed".into(),
            };
        }
        if game == Game::Poe2 && name.contains("DNT") {
            return None;
        }
        let hidden = match game {
            Game::Poe2 => pick.hide || id.contains("Unique") || name.contains("Runemastered"),
            Game::Poe1 => pick.hide,
        };
        let inherits = row.str("InheritsFrom");

        let mut out = Table::new();
        out.set("type", rule.kind);
        let sub = match pick.sub {
            Some(sub) => sub.map(str::to_string),
            None => match rule.sub {
                Sub::None => None,
                Sub::Fixed(s) => Some(s.to_string()),
                Sub::Tags(map) => map.iter().find(|(tag, _)| tags.contains(*tag)).map(|(_, s)| s.to_string()),
            },
        };
        if let Some(sub) = sub.filter(|s| !s.is_empty()) {
            out.set("subType", sub);
        }
        if game == Game::Poe2 {
            if let Quality::Text(text) = self.it.quality(inherits, 0) {
                if let Ok(q) = lua_trim(&text).parse::<f64>() {
                    out.set("quality", q);
                }
            }
            if rule.kind == "Belt" {
                out.set("charmLimit", 0);
            }
            if let Some(spirit) = self.spirit.row(row.index) {
                out.set("spirit", spirit.int(spirit.table.pick(&["SpiritGranted", "Value"]).unwrap_or("Value")));
            }
        }
        if hidden {
            out.set("hidden", true);
        }
        if let Some(limit) = self.socket_limit(inherits) {
            out.set("socketLimit", limit);
        }
        out.set("tags", Table::set_of(tags));
        if game == Game::Poe1 {
            let class = row.key(row.table.pick(&["ItemClassesKey", "ItemClass"]).unwrap_or("ItemClassesKey"));
            if let Some(prefix) = class.and_then(|c| self.influence.get(&c)) {
                let mut t = Table::new();
                for suffix in INFLUENCES {
                    t.set(suffix, lua_literal(&format!("{}_{}", prefix, suffix)));
                }
                out.set("influenceTags", t);
            }
        }

        let implicit_col = row.table.pick(&["Implicit_Mods", "Implicit_ModsKeys"]).unwrap_or("Implicit_Mods");
        let implicit_rows: Vec<usize> = row.list_keys(implicit_col);
        let mut lines: Vec<String> = Vec::new();
        let mut mod_types = Table::new();
        match game {
            Game::Poe2 => self.poe2_implicits(&implicit_rows, &mut lines, &mut mod_types),
            Game::Poe1 => {
                let mut ids = Vec::new();
                for &m in &implicit_rows {
                    let Some(m) = self.implicit(m) else { continue };
                    for line in &m.lines {
                        lines.push(line.clone());
                        mod_types.push(strings(&m.tags));
                    }
                    if !m.lines.is_empty() {
                        ids.push(m.id.clone());
                    }
                }
                if !ids.is_empty() {
                    out.set("implicitIds", Table::list(ids));
                }
            }
        }
        let weapon = self.weapon.row(row.index);
        let armour = self.armour.row(row.index);
        let inherent = if game == Game::Poe2 { self.inherent.row(row.index) } else { None };
        if let Some(skills) = inherent {
            self.inherent_skills(skills, &mut out, &mut lines);
        }
        if !lines.is_empty() {
            out.set("implicit", lua_literal(&lines.join("\\n")));
        }
        out.set("implicitModTypes", mod_types);
        if game == Game::Poe1 {
            self.enchants(row, &mut out);
        }
        if let Some(w) = weapon {
            out.set(
                "weapon",
                match game {
                    Game::Poe2 => self.poe2_weapon(w, &implicit_rows),
                    Game::Poe1 => poe1_weapon(w),
                },
            );
        }
        if let Some(a) = armour {
            out.set("armour", self.armour_table(row, a));
        }
        let flask = self.flask.row(row.index);
        match game {
            Game::Poe2 if rule.kind == "Flask" || rule.kind == "Charm" => {
                if let Some(f) = flask {
                    let key = if rule.kind == "Charm" { "charm" } else { "flask" };
                    out.set(key, self.flask_table(row, f));
                }
            }
            Game::Poe1 => {
                if let Some(f) = flask {
                    out.set("flask", self.flask_table(row, f));
                }
                if let Some(t) = self.tincture.row(row.index) {
                    let col = |names: &[&str]| t.int(t.table.pick(names).unwrap_or(names[0])) as f64;
                    out.set(
                        "tincture",
                        Table::new()
                            .with("manaBurn", written(col(&["DebuffInterval", "ManaBurn"]) / 1000.0))
                            .with("cooldown", written(col(&["Cooldown", "CoolDown"]) / 1000.0)),
                    );
                }
            }
            _ => {}
        }

        let requirements = self.requirements.row(row.index);
        let drop_level = row.int("DropLevel");
        let mut level = 1;
        let equipment = match game {
            Game::Poe2 => weapon.is_some() || armour.is_some() || inherent.is_some() || requirements.is_some(),
            Game::Poe1 => weapon.is_some() || armour.is_some(),
        };
        if equipment && drop_level > 4 {
            level = drop_level;
        }
        let flask_like = match game {
            Game::Poe2 => rule.kind == "Flask" || rule.kind == "Charm",
            Game::Poe1 => flask.is_some(),
        };
        if flask_like && drop_level > 2 {
            level = drop_level;
        }
        for &m in &implicit_rows {
            if let Some(m) = self.implicit(m) {
                level = level.max((m.level as f64 * 0.8).floor() as i64);
            }
        }
        let mut req = Table::new();
        if level > 1 {
            req.set("level", level);
        }
        if let Some(r) = requirements {
            for (key, cols) in [("str", ["ReqStr", "Str"]), ("dex", ["ReqDex", "Dex"]), ("int", ["ReqInt", "Int"])] {
                let value = r.int(r.table.pick(&cols).unwrap_or(cols[0]));
                if value > 0 {
                    req.set(key, value);
                }
            }
        }
        out.set("req", req);

        if game == Game::Poe1 {
            let text = base_flavour(self.ctx, row);
            let cleaned = text.map(|t| clean_poe1_flavour(&t)).unwrap_or_default();
            if !cleaned.is_empty() {
                out.set("flavourText", Table::list(cleaned.iter().map(|l| lua_literal(l))));
            }
        }
        Some((lua_literal(&name), out))
    }

    /// The most sockets the base's `.it` chain lets it roll. PoB adds two to
    /// this for PoE 2 bases; the game files do not, so neither does the export.
    fn socket_limit(&self, inherits: &str) -> Option<i64> {
        self.it.sockets(inherits, 0).filter(|&n| n > 0)
    }

    /// PoE 2's implicit lines: the mods sorted by their first stat order.
    fn poe2_implicits(&self, rows: &[usize], lines: &mut Vec<String>, mod_types: &mut Table) {
        let mut mods: Vec<Rc<Implicit>> = rows.iter().filter_map(|&m| self.implicit(m)).collect();
        lua_sort(&mut mods, |a, b| a.first_order < b.first_order);
        for m in &mods {
            for line in &m.lines {
                lines.push(format!("{}{}", if m.unscalable { "{unscalable}" } else { "" }, line));
                mod_types.push(strings(&m.tags));
            }
            if m.id == "SpearImplicitDisplaySpearThrow1" {
                lines.push("Grants Skill: Spear Throw".into());
            }
        }
    }

    /// The skills a PoE 2 base grants just by being worn.
    fn inherent_skills(&self, row: Row<'_>, out: &mut Table, lines: &mut Vec<String>) {
        let rr = self.ctx.rr;
        if row.bool(row.table.pick(&["NoReservation", "IsWeapon"]).unwrap_or("IsWeapon")) {
            out.set("grantedSkillsHaveNoReservation", true);
        }
        let skills = rr.deref_list(row, row.table.pick(&["SkillsGranted", "Skill"]).unwrap_or("SkillsGranted"));
        let has_variants = skills.len() > 1;
        let mut variants = Vec::new();
        for (index, skill) in skills.iter().enumerate() {
            let gem_row = skill.row();
            let base_col = gem_row.table.pick(&["BaseItemType", "BaseItemTypesKey"]).unwrap_or("BaseItemType");
            let base = gem_row.key(base_col);
            let gem = gem_row.table.rows().find(|r| r.key(base_col) == base).unwrap_or(gem_row);
            let effects_col = gem.table.pick(&["GemEffects", "GemVariants"]).unwrap_or("GemEffects");
            let name = rr
                .deref_list(gem, effects_col)
                .first()
                .and_then(|e| rr.deref(e.row(), "GrantedEffect"))
                .and_then(|g| rr.deref(g.row(), "ActiveSkill"))
                .map(|a| a.row().string(a.table.pick(&["DisplayedName", "DisplayName"]).unwrap_or("DisplayedName")));
            let Some(name) = name else { continue };
            let support = match gem.table.pick(&["IsSupport", "GemType"]) {
                Some("GemType") => gem.int("GemType") & 0xff == 1,
                Some(col) => gem.bool(col),
                None => false,
            };
            let mut max_level = 1;
            if !support {
                let col = gem.table.pick(&["ItemExperienceType", "GemLevelProgression"]).unwrap_or("ItemExperienceType");
                max_level = self.experience_levels(gem.key(col)).max(1);
            }
            let mut line = format!(
                "Grants Skill: {}{}",
                if max_level == 1 { String::new() } else { format!("Level (1-{}) ", max_level) },
                name
            );
            if has_variants {
                variants.push(name.clone());
                line = format!("{{variant:{}}}{}", index + 1, line);
            }
            lines.push(line);
        }
        if !variants.is_empty() {
            out.set("variantList", Table::list(variants));
        }
    }

    /// How many `ItemExperiencePerLevel` rows a gem's level progression has.
    fn experience_levels(&self, progression: Option<usize>) -> i64 {
        let Some(table) = self.ctx.optional_table("ItemExperiencePerLevel") else { return 0 };
        let col = table.pick(&["ItemExperienceType", "ItemExperienceTypesKey"]).unwrap_or("ItemExperienceType");
        table.rows().filter(|r| r.key(col) == progression).count() as i64
    }

    fn poe2_weapon(&self, w: Row<'_>, implicit_rows: &[usize]) -> Table {
        const TYPES: [&str; 5] = ["Physical", "Fire", "Cold", "Lightning", "Chaos"];
        let element = |stat: &str, prefix: &str, suffix: &str| -> Option<usize> {
            let rest = stat.strip_prefix(prefix)?.strip_suffix(suffix)?;
            TYPES[1..].iter().position(|t| t.eq_ignore_ascii_case(rest)).map(|i| i + 1)
        };
        let mut conversion = [100.0, 0.0, 0.0, 0.0, 0.0];
        let mut added_min = [0.0; 5];
        let mut added_max = [0.0; 5];
        let mut total = 0.0;
        for &m in implicit_rows {
            let Some(m) = self.implicit(m) else { continue };
            for (stat, min, _, _) in &m.stats {
                if let Some(t) = element(stat, "local_weapon_implicit_hidden_%_base_damage_is_", "") {
                    conversion[t] += min;
                    total += min;
                }
                if let Some(t) = element(stat, "local_weapon_implicit_hidden_added_minimum_", "_damage") {
                    added_min[t] += min;
                }
                if let Some(t) = element(stat, "local_weapon_implicit_hidden_added_maximum_", "_damage") {
                    added_max[t] += min;
                }
            }
        }
        let factor = if total > 100.0 { 100.0 / total } else { 1.0 };
        let damage_min = w.int("DamageMin") as f64;
        let damage_max = w.int("DamageMax") as f64;
        let mut out = Table::new();
        for (i, kind) in TYPES.iter().enumerate() {
            conversion[i] = if i == 0 { 1.0 - (total / 100.0f64).min(1.0) } else { conversion[i] * factor / 100.0 };
            if conversion[i] != 0.0 {
                out.set(format!("{}Min", kind), written((damage_min * conversion[i]).floor()));
                out.set(format!("{}Max", kind), written((damage_max * conversion[i]).floor()));
            }
            if added_min[i] != 0.0 {
                out.set(format!("{}Min", kind), written(added_min[i]));
            }
            if added_max[i] != 0.0 {
                out.set(format!("{}Max", kind), written(added_max[i]));
            }
        }
        weapon_common(w, &mut out);
        let reload = w.int("ReloadTime");
        if reload > 0 {
            out.set("ReloadTimeBase", written(round(reload as f64 / 1000.0, Some(2))));
        }
        out
    }

    fn armour_table(&self, row: Row<'_>, a: Row<'_>) -> Table {
        let mut out = Table::new();
        if let Some(shield) = self.shield.row(row.index) {
            out.set("BlockChance", shield.int("Block"));
        }
        let movement = a.int(a.table.pick(&["IncreasedMovementSpeed", "MovementPenalty"]).unwrap_or("IncreasedMovementSpeed"));
        match self.game {
            Game::Poe2 => {
                for col in ["Armour", "Evasion", "EnergyShield", "Ward"] {
                    let value = a.int(col);
                    if value > 0 {
                        out.set(col, value);
                    }
                }
                if movement != 0 {
                    out.set("MovementPenalty", written(-movement as f64 / 10000.0));
                }
            }
            Game::Poe1 => {
                if movement != 0 {
                    out.set("MovementPenalty", -movement);
                }
                for kind in ["Armour", "Evasion", "EnergyShield", "Ward"] {
                    let min = a.int(&format!("{}Min", kind));
                    if min > 0 {
                        out.set(format!("{}BaseMin", kind), min);
                        out.set(format!("{}BaseMax", kind), a.int(&format!("{}Max", kind)));
                    }
                }
            }
        }
        out
    }

    fn flask_table(&self, row: Row<'_>, f: Row<'_>) -> Table {
        let rr = self.ctx.rr;
        let mut out = Table::new();
        for (col, key) in [("LifePerUse", "life"), ("ManaPerUse", "mana")] {
            let value = f.int(col);
            if value > 0 {
                out.set(key, value);
            }
        }
        out.set("duration", written(f.int("RecoveryTime") as f64 / 10.0));
        if let Some(c) = self.charges.row(row.index) {
            out.set("chargesUsed", c.int(c.table.pick(&["PerCharge", "PerUse"]).unwrap_or("PerCharge")));
            out.set("chargesMax", c.int(c.table.pick(&["MaxCharges", "Max"]).unwrap_or("MaxCharges")));
        }
        let mut stats = Stats::new();
        let mut has_buff = false;
        match self.game {
            Game::Poe2 => {
                for buff in rr.deref_list(f, f.table.pick(&["UtilityBuff", "UtilityBuffs"]).unwrap_or("UtilityBuff")) {
                    has_buff = true;
                    let b = buff.row();
                    let values = b.list_int("StatValues");
                    let def_col = b.table.pick(&["BuffDefinition", "BuffDefinitionsKey"]).unwrap_or("BuffDefinition");
                    let Some(def) = rr.deref(b, def_col) else { continue };
                    for (i, stat) in rr.deref_list_ids(def.row(), "GrantedStats").iter().enumerate() {
                        let v = values.get(i).copied().unwrap_or(0) as f64;
                        stats.set(stat, v, v);
                    }
                    for flag in rr.deref_list_ids(def.row(), "GrantedFlags") {
                        stats.set(&flag, 1.0, 1.0);
                    }
                }
            }
            Game::Poe1 => {
                if let Some(def) = rr.deref(f, f.table.pick(&["BuffDefinitionsKey", "Buff"]).unwrap_or("BuffDefinitionsKey")) {
                    has_buff = true;
                    let values = f.list_int(f.table.pick(&["BuffStatValues", "BuffMagnitudes"]).unwrap_or("BuffStatValues"));
                    let stat_col = def.table.pick(&["StatsKeys", "Stats"]).unwrap_or("StatsKeys");
                    for (i, stat) in rr.deref_list_ids(def.row(), stat_col).iter().enumerate() {
                        let v = values.get(i).copied().unwrap_or(0) as f64;
                        stats.set(stat, v, v);
                    }
                    for flag in rr.deref_list_ids(def.row(), "GrantedFlags") {
                        stats.set(&flag, 1.0, 1.0);
                    }
                }
            }
        }
        if has_buff {
            let lines = self.descriptors.describe_stats(&mut stats).lines;
            let buff = if lines.is_empty() {
                Table::list([""])
            } else {
                Table::list(lines.iter().map(|l| lua_literal(l)))
            };
            out.set("buff", buff);
        }
        out
    }

    /// PoE 1's talisman enchantments, which anointing cannot replace.
    fn enchants(&self, row: Row<'_>, out: &mut Table) {
        let Some(col) = row.table.pick(&["TalismanEnchants", "EnchantMods"]) else { return };
        let rows = row.list_keys(col);
        let mut lines = Vec::new();
        let mut ids = Vec::new();
        let mut types = Table::new();
        for &m in &rows {
            let Some(m) = self.implicit(m) else { continue };
            for line in &m.lines {
                lines.push(line.clone());
                types.push(strings(&m.tags));
            }
            if !m.lines.is_empty() {
                ids.push(m.id.clone());
            }
        }
        if !lines.is_empty() {
            out.set("enchant", lua_literal(&lines.join("\\n")));
            if !ids.is_empty() {
                out.set("enchantIds", Table::list(ids));
            }
            out.set("enchantModTypes", types);
        }
        if !rows.is_empty() {
            out.set("cannotBeAnointed", true);
        }
    }
}

/// A PoE 1 base's own flavour text.
fn base_flavour(ctx: &Ctx, row: Row<'_>) -> Option<String> {
    let col = row.table.pick(&["FlavourTextKey", "FlavourText"])?;
    let text = ctx.rr.deref(row, col)?;
    Some(text.row().string("Text"))
}

fn weapon_common(w: Row<'_>, out: &mut Table) {
    let crit = w.int(w.table.pick(&["CritChance", "Critical"]).unwrap_or("CritChance")) as f64;
    out.set("CritChanceBase", written(crit / 100.0));
    let speed = w.int("Speed") as f64;
    if speed != 0.0 {
        out.set("AttackRateBase", written(round(1000.0 / speed, Some(2))));
    }
    out.set("Range", w.int(w.table.pick(&["RangeMax", "Range"]).unwrap_or("RangeMax")));
}

fn poe1_weapon(w: Row<'_>) -> Table {
    let mut out = Table::new();
    out.set("PhysicalMin", w.int("DamageMin"));
    out.set("PhysicalMax", w.int("DamageMax"));
    weapon_common(w, &mut out);
    out
}

/// `{ "a", "b" }` as PoB writes a list of tag ids.
fn strings(items: &[String]) -> Table {
    Table::list(items.iter().map(|s| lua_literal(s)))
}

/// `Bases/*.lua`: every base of the item classes PoB covers, one file per
/// PoB item type. A name two bases share is written by the later row, as
/// PoB's directives do.
pub fn bases(ctx: &Ctx) -> Result<(), String> {
    let game = game(ctx);
    let table = ctx.table("BaseItemTypes")?;
    let mut b = Bases::new(ctx, &table)?;
    let (rules, tweaks): (&[Rule], Tweaks) = match game {
        Game::Poe2 => (&POE2_RULES, POE2_TWEAKS),
        Game::Poe1 => (&POE1_RULES, POE1_TWEAKS),
    };
    let class_col = table.require(&["ItemClass", "ItemClassesKey"])?;
    let tags_col = table.require(&["Tags", "TagsKeys"])?;
    b.it.load(ctx, table.rows().map(|r| r.string("InheritsFrom")).filter(|p| !p.is_empty()));

    let mut files: Vec<(&str, Table)> = Vec::new();
    for rule in rules {
        if !files.iter().any(|(f, _)| *f == rule.file) {
            files.push((rule.file, Table::new()));
        }
    }
    for row in table.rows() {
        let id = row.id();
        if id.is_empty() {
            continue;
        }
        let class = ctx.rr.deref_id(row, class_col).unwrap_or_default();
        let own_tags = ctx.rr.deref_list_ids(row, tags_col);
        let rule = rules.iter().find(|r| matches!(r.on, On::Class(c) if c == class)).or_else(|| {
            rules.iter().find(|r| matches!(r.on, On::Tag(t) if own_tags.iter().any(|x| x == t)))
        });
        let Some(rule) = rule else { continue };
        let mine: &[Tweak] = tweaks.iter().find(|(i, _)| *i == id).map_or(&[], |(_, t)| *t);
        if mine.contains(&Tweak::Skip) || (game == Game::Poe1 && id.contains("Royale")) {
            continue;
        }
        let inherited = b.it.tags(row.str("InheritsFrom"), 0).unwrap_or_default();
        let tags: BTreeSet<String> = inherited.into_iter().chain(own_tags).collect();
        let sub = mine.iter().find_map(|t| match t {
            Tweak::SubType(s) => Some(*s),
            _ => None,
        });
        let rename = mine.iter().find_map(|t| match t {
            Tweak::Rename(n) => Some(*n),
            _ => None,
        });
        let file = &mut files.iter_mut().find(|(f, _)| *f == rule.file).expect("every rule's file is listed").1;
        let plain = Pick { rule, name: rename, hide: mine.contains(&Tweak::Hide), sub };
        if let Some((name, entry)) = b.entry(row, &plain, &tags) {
            file.set(name, entry);
        }
        for extra in mine.iter().filter_map(|t| match t {
            Tweak::Also(n) => Some(*n),
            _ => None,
        }) {
            let pick = Pick { rule, name: Some(extra), hide: false, sub };
            if let Some((name, entry)) = b.entry(row, &pick, &tags) {
                file.set(name, entry);
            }
        }
    }
    for (file, out) in files {
        write(ctx, &format!("Bases/{}", file), out)?;
    }
    Ok(())
}

/// PoE 2 names a few flavour texts after uniques the stash layout does not
/// list under that art.
const POE2_FLAVOUR_NAMES: [(&str, &str); 5] = [
    ("FourUniqueSceptre6", "Guiding Palm"),
    ("FourUniqueSceptre6a", "Guiding Palm of the Heart"),
    ("FourUniqueSceptre6b", "Guiding Palm of the Eye"),
    ("FourUniqueSceptre6c", "Guiding Palm of the Mind"),
    ("VaalLimbReplacements", "Transcendent Limb"),
];

/// PoE 1's flavour texts PoB names by hand, written first and in this order.
/// Several are uniques renamed since, or sharing art with another.
const POE1_FLAVOUR_NAMES: [(&str, &str); 107] = [
    ("UniqueOneHandAxe5", "Jack, the Axe"),
    ("UniqueBootsDIY", "Doryani's Delusion"),
    ("UniqueGlovesStrDex9", "Tombfist"),
    ("UniqueAmulet45", "Impresence"),
    ("UniqueAmuletVictor1", "Talisman of the Victor"),
    ("UniqueStaff8", "Agnerod South"),
    ("UniqueStaff8", "Agnerod North"),
    ("UniqueStaff8", "Agnerod West"),
    ("Ring11", "Gifts from Above"),
    ("Ring12", "Death Rush"),
    ("Ring13", "Shavronne's Revelation"),
    ("Belt5", "Auxium"),
    ("Amulet13", "Daresso's Salute"),
    ("Amulet14", "Voll's Devotion"),
    ("Amulet15", "Victario's Acuity"),
    ("UniqueAmulet27", "Star of Wraeclast"),
    ("UniqueAmulet29x", "Replica Winterheart"),
    ("UniqueJewel82x", "Replica Primordial Might"),
    ("UniqueBootsDex10", "Abberath's Hooves"),
    ("UniqueRing54", "Precursor's Emblem"),
    ("UniqueHelmetStrInt11", "Lightpoacher"),
    ("UniqueBootsDexInt6", "Bubonic Trail"),
    ("UniqueBodyDexInt9", "Shroud of the Lightless"),
    ("UniqueGlovesStrInt6", "Volkuur's Guidance"),
    ("UniqueBootsAtlas1", "Beacon of Madness"),
    ("UniqueBootsAtlas1", "Demigod's Eye"),
    ("UniqueShieldDemigods", "Demigod's Beacon"),
    ("UniqueBootsDemigods1", "Demigod's Stride"),
    ("UniqueBeltDemigods1", "Demigod's Bounty"),
    ("UniqueBodyDemigods", "Demigod's Dominance"),
    ("UniqueHelmetDemigods1", "Demigod's Immortality"),
    ("UniqueQuiver1", "Blackgleam"),
    ("FatedUnique8", "The Signal Fire"),
    ("UniqueBootsStr1", "Windscream"),
    ("FatedUnique35", "Windshriek"),
    ("UniqueBodyDex7", "Briskwrap"),
    ("FatedUnique31", "Wildwrap"),
    ("UniqueShieldInt2", "Matua Tupuna"),
    ("FatedUnique61", "Whakatutuki o Matua"),
    ("UniqueBow8", "Storm Cloud"),
    ("FatedUnique21", "The Tempest"),
    ("UniqueBelt2", "The Magnate"),
    ("FatedUnique46", "The Tactician"),
    ("FatedUnique47", "The Nomad"),
    ("UniqueStaff14", "The Stormheart"),
    ("FatedUnique29", "The Stormwall"),
    ("UniqueTwoHandAxe3", "Limbsplit"),
    ("FatedUnique18", "The Cauteriser"),
    ("UniqueTwoHandSword4", "Queen's Decree"),
    ("FatedUnique25", "Queen's Escape"),
    ("UniqueHelmetDexInt1", "Malachai's Simula"),
    ("FatedUnique43", "Malachai's Awakening"),
    ("UniqueRing2", "Kaom's Sign"),
    ("FatedUnique1", "Kaom's Way"),
    ("UniqueQuiver6", "Hyrri's Bite"),
    ("FatedUnique49", "Hyrri's Demise"),
    ("UniqueGlovesDex1", "Hrimsorrow"),
    ("FatedUnique10", "Hrimburn"),
    ("UniqueTwoHandMace2", "Geofri's Baptism"),
    ("FatedUnique52", "Geofri's Devotion"),
    ("UniqueDexHelmet2", "Heatshiver"),
    ("FatedUnique36", "Frostferno"),
    ("UniqueBootsStrDex3", "Dusktoe"),
    ("FatedUnique26", "Duskblight"),
    ("UniqueOneHandSword1", "Redbeak"),
    ("FatedUnique54", "Dreadbeak"),
    ("UniqueBow11", "Doomfletch"),
    ("FatedUnique19", "Doomfletch's Prism"),
    ("UniqueGlovesInt2", "Doedre's Tenure"),
    ("FatedUnique28", "Doedre's Malevolence"),
    ("UniqueBow3", "Death's Harp"),
    ("FatedUnique5", "Death's Opus"),
    ("UniqueTwoHandMace5", "Chober Chaber"),
    ("FatedUnique58", "Chaber Cairn"),
    ("UniqueOneHandMace4", "Cameria's Maul"),
    ("FatedUnique53", "Cameria's Avarice"),
    ("UniqueIntHelmet2", "Asenath's Mark"),
    ("FatedUnique41", "Asenath's Chant"),
    ("UniqueWand8", "Reverberation Rod"),
    ("FatedUnique23", "Amplification Rod"),
    ("UniqueOneHandAxe7", "Dreadarc"),
    ("FatedUnique50", "Dreadsurge"),
    ("UniqueJewel16", "Apparitions"),
    ("UniqueDescentOneHandSword1", "Blood of Summer"),
    ("UniqueDescentOneHandAxe1", "Rust of Winter"),
    ("UniqueDescentOneHandMace1", "Ashes of the Sun"),
    ("UniqueDescentWand1", "Splinter of the Moon"),
    ("UniqueDescentTwoHandSword1", "Thunder of the Dawn"),
    ("UniqueDescentStaff1", "Vestige of Divinity"),
    ("UniqueDescentDagger1", "Fragment of Eternity"),
    ("UniqueDescentBow1", "Relic of the Cycle"),
    ("UniqueDescentClaw1", "Scar of Fate"),
    ("UniqueDescentHelmet1", "Tears of Entropy"),
    ("UniqueDescentShield1", "Remnant of Empires"),
    ("UniqueDescentBelt1", "Chains of Time"),
    ("UniqueDescentQuiver1", "Slivers of Providence"),
    ("FatedUnique57", "The Iron Fortress"),
    ("UniqueBodyStr9", "Iron Heart"),
    ("UniqueOneHandSword10", "The Goddess Unleashed"),
    ("UniqueRapier1", "The Goddess Bound"),
    ("FatedUnique48", "Winterweave"),
    ("UniqueRing28", "Bloodboil"),
    ("FatedUnique27", "Atziri's Reflection"),
    ("UniqueShieldDex3", "Atziri's Mirror"),
    ("Ring11x", "Replica Gifts from Above"),
    ("UniqueOneHandSword36", "Dread Captain's Cutlass"),
    ("UniqueHelmetStrInt26", "Subsume the Source"),
];

/// PoE 2's `normalizeId`: the id up to its first underscore, so every art
/// variant of a unique (`FourUniqueRing33_a`) names the one flavour text.
fn poe2_flavour_key(id: &str) -> Option<&str> {
    let key = id.split('_').next().unwrap_or("");
    (!key.is_empty()).then_some(key)
}

/// PoE 2's `cleanAndSplit`: the non-blank lines, trimmed.
fn clean_poe2_flavour(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n").split('\n').map(lua_trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// `utils.stringify` without multi-line strings: line breaks become spaces.
fn single_line(s: &str) -> String {
    s.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// Lua's `s:gsub(open .. "(.-)" .. close, keep and "%1" or "")`.
fn strip_delimited(s: &str, open: &str, close: &str, keep: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(open) {
            if let Some(end) = after.find(close) {
                if keep {
                    out.push_str(&after[..end]);
                }
                rest = &after[end + close.len()..];
                continue;
            }
        }
        let c = rest.chars().next().expect("rest is not empty");
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// PoE 1's `cleanAndSplit`: `<default>` starts a new `^8` paragraph after a
/// blank line, braces are unwrapped, `<<…>>` markup dropped and quotes
/// escaped the way PoB writes them.
fn clean_poe1_flavour(text: &str) -> Vec<String> {
    let text = text.replace("\r\n", "\n").replace("<default>", "\n^8");
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let line = lua_trim(line);
        if line.is_empty() {
            continue;
        }
        let line = strip_delimited(line, "{", "}", true);
        let line = strip_delimited(&line, "<<", ">>", false);
        let line = lua_trim(&line).replace('"', "\\\"");
        if line.starts_with("^8") && lines.last().is_none_or(|l| !l.is_empty()) {
            lines.push(String::new());
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// `FlavourText.lua`: each unique's flavour text under its name, found by
/// matching the flavour id to the unique's art id.
pub fn flavour_text(ctx: &Ctx) -> Result<(), String> {
    let layout = ctx.table("UniqueStashLayout")?;
    let flavour = ctx.table("FlavourText")?;
    let word_col = layout.require(&["WordsKey", "Words"])?;
    let art_col = layout.require(&["ItemVisualIdentityKey", "ItemVisualIdentity"])?;
    let art_of = |row: Row<'_>| ctx.rr.deref(row, art_col).map(|v| v.row().string("Id"));
    let name_of = |row: Row<'_>| ctx.rr.deref(row, word_col).map(|w| w.row().string("Text2"));
    let mut out = Table::new();
    match game(ctx) {
        Game::Poe2 => {
            let origins: Vec<(String, String)> = ctx
                .optional_table("UniqueOrigins")
                .map(|t| {
                    t.rows()
                        .filter_map(|r| {
                            let name = ctx.rr.deref(r, "Unique").map(|w| sanitise_text(w.row().str("Text2"), Game::Poe2))?;
                            Some((name, ctx.rr.deref_id(r, "Origin").unwrap_or_default()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut names: HashMap<String, String> = HashMap::new();
            let mut origin: HashMap<String, String> = HashMap::new();
            for row in layout.rows() {
                let (Some(name), Some(art)) = (name_of(row), art_of(row)) else { continue };
                let name = sanitise_text(&name, Game::Poe2);
                let Some(key) = poe2_flavour_key(&art) else { continue };
                if let Some((_, o)) = origins.iter().find(|(n, _)| *n == name) {
                    origin.insert(key.to_string(), o.clone());
                }
                names.insert(key.to_string(), name);
            }
            for row in flavour.rows() {
                let Some(key) = poe2_flavour_key(row.id()) else { continue };
                let forced = POE2_FLAVOUR_NAMES.iter().find(|(id, _)| *id == key).map(|(_, n)| n.to_string());
                let Some(name) = forced.or_else(|| names.get(key).cloned()) else { continue };
                let text = clean_poe2_flavour(row.str("Text"));
                out.push(
                    Table::new()
                        .with("id", single_line(row.id()))
                        .with("name", single_line(&name))
                        .with_opt("origin", origin.get(key).map(|o| single_line(o)))
                        .with("text", Table::list(text.iter().map(|l| single_line(l)))),
                );
            }
        }
        Game::Poe1 => {
            let mut names: BTreeMap<String, String> = BTreeMap::new();
            for row in layout.rows() {
                let (Some(name), Some(art)) = (name_of(row), art_of(row)) else { continue };
                let key = art.trim_end_matches('_');
                if ["Map", "AlternateArt", "AtlasUpgrade", "HeistQuest"].iter().any(|s| key.contains(s)) {
                    continue;
                }
                names.insert(key.to_string(), name);
            }
            let mut texts: HashMap<String, Vec<String>> = HashMap::new();
            for row in flavour.rows() {
                texts.insert(row.id().trim_end_matches('_').to_string(), clean_poe1_flavour(row.str("Text")));
            }
            let forced = POE1_FLAVOUR_NAMES.iter().map(|(id, n)| (id.to_string(), n.to_string()));
            for (id, name) in forced.chain(names) {
                let Some(lines) = texts.get(&id) else { continue };
                out.push(
                    Table::new()
                        .with("id", lua_literal(&id))
                        .with("name", lua_literal(&name))
                        .with("text", Table::list(lines.iter().map(|l| lua_literal(l)))),
                );
            }
        }
    }
    write(ctx, "FlavourText", out)
}

/// PoB's names for the build planner's inventories, by inventory id. The
/// flask inventory holds both flasks and the three charms, one column each.
const INVENTORY_SLOTS: [(&str, &[&str]); 17] = [
    ("Weapon1", &["Weapon 1"]),
    ("Offhand1", &["Weapon 2"]),
    ("Helm1", &["Helmet"]),
    ("Amulet1", &["Amulet"]),
    ("Ring1", &["Ring 1"]),
    ("Ring2", &["Ring 2"]),
    ("Gloves1", &["Gloves"]),
    ("Boots1", &["Boots"]),
    ("Belt1", &["Belt"]),
    ("Flask1", &["Flask 1", "Flask 2", "Charm 1", "Charm 2", "Charm 3"]),
    ("Weapon2", &["Weapon 1 Swap"]),
    ("Offhand2", &["Weapon 2 Swap"]),
    ("Trinket1", &["Trinket"]),
    ("Ring3", &["Ring 3"]),
    ("Weapon3", &["Weapon3"]),
    ("Offhand3", &["Offhand3"]),
    ("BodyArmour1", &["Body Armour"]),
];

/// `InventorySlots.lua`: each build planner slot's inventory and column. An
/// inventory PoB has no name for keeps its id.
pub fn inventory_slots(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("BuildPlannerInventories")?;
    let col = table.require(&["Inventory", "InventoriesKey"])?;
    let mut out = Table::new();
    for row in table.rows() {
        let Some(id) = ctx.rr.deref_id(row, col) else { continue };
        let names: Vec<String> = match INVENTORY_SLOTS.iter().find(|(inv, _)| *inv == id) {
            Some((_, names)) => names.iter().map(|n| n.to_string()).collect(),
            None => vec![id.clone()],
        };
        for (x, name) in names.iter().enumerate() {
            out.set(name.as_str(), Table::new().with("id", lua_literal(&id)).with("slot_x", x));
        }
    }
    write(ctx, "InventorySlots", out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_like_lua_patterns() {
        assert_eq!(capture_quoted("extends \"Metadata/Items/Item\"", "extends \""), Some("Metadata/Items/Item"));
        assert_eq!(capture_quoted("\ttag = \"default\"", "tag = \""), Some("default"));
        assert_eq!(capture_quoted("\tremove_tag = \"weapon\"", "remove_tag = \""), Some("weapon"));
        assert_eq!(capture_quoted("extends \"\"", "extends \""), None);
        assert_eq!(lua_trim("\t Leather Belt \r"), "Leather Belt");
    }

    #[test]
    fn sorts_ties_the_way_luajit_does() {
        // Each key's 1-based position after `table.sort(t, function(a, b)
        // return a.k < b.k end)`, as LuaJIT 2.1 printed it.
        let cases: [(&[i32], &[usize]); 5] = [
            (&[2, 1, 2, 1, 0], &[5, 2, 4, 3, 1]),
            (&[3, 3, 3, 3, 3, 3, 3], &[1, 5, 6, 4, 2, 3, 7]),
            (&[1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0], &[12, 8, 10, 4, 6, 2, 11, 5, 9, 7, 3, 1]),
            (
                &[5, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7, 9, 3, 2, 3, 8, 4, 6, 2, 6, 4, 3, 3, 8, 3, 2, 7, 9, 5],
                &[
                    4, 2, 29, 7, 17, 22, 10, 25, 26, 28, 18, 16, 20, 24, 3, 1, 32, 5, 9, 11, 21, 23, 8, 14, 30, 12, 19, 27,
                    31, 6, 13, 15,
                ],
            ),
            (&[0, 0, 1, 0, 0, 2, 0, 0, 1, 1, 0, 2, 2, 0, 1], &[1, 7, 14, 11, 5, 8, 4, 2, 9, 3, 15, 10, 6, 13, 12]),
        ];
        for (keys, expected) in cases {
            let mut items: Vec<(i32, usize)> = keys.iter().enumerate().map(|(i, &k)| (k, i + 1)).collect();
            lua_sort(&mut items, |a, b| a.0 < b.0);
            assert_eq!(items.iter().map(|p| p.1).collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn reads_it_chains_the_way_bases_lua_does() {
        let mut it = ItFiles::default();
        let file = |lines: &[&str]| Some(Rc::new(lines.iter().map(|l| l.to_string()).collect::<Vec<_>>()));
        it.lines.insert(
            "parent".into(),
            file(&[
                "extends \"nothing\"",
                "\ttag = \"a\"",
                "\ttag = \"b\"",
                "\ttag = \"x\"",
                "\tmax_quality = 20",
                "\tsocket_info = \"1:5:100 2:5:100\"",
            ]),
        );
        it.lines.insert(
            "child".into(),
            file(&["extends \"parent\"", "\tremove_tag = \"a\"", "\tremove_tag = \"zzz\"", "\ttag = \"c\""]),
        );
        // A tag the file does not have takes the last one instead.
        assert_eq!(it.tags("child", 0), Some(vec!["b".to_string(), "c".to_string()]));
        assert!(matches!(it.quality("child", 0), Quality::Text(q) if q == "20"));
        assert!(matches!(it.quality("missing", 0), Quality::Nil));
        assert_eq!(it.sockets("child", 0), Some(2));
    }

    #[test]
    fn socket_counts_no_item_level_reaches_do_not_count() {
        assert_eq!(socket_limit("1:5:100 2:5:100"), Some(2));
        assert_eq!(socket_limit("1:1:100 2:1:90 3:2:80 4:25:30 5:9999:20 6:9999:5"), Some(4));
        assert_eq!(socket_limit(""), None);
    }

    #[test]
    fn flavour_text_cleans_like_pob() {
        assert_eq!(poe2_flavour_key("FourUniqueRing33_a"), Some("FourUniqueRing33"));
        assert_eq!(poe2_flavour_key("_x"), None);
        assert_eq!(clean_poe2_flavour(" One\r\n\r\n  Two  "), ["One", "Two"]);
        assert_eq!(
            clean_poe1_flavour("\"Hello {there}\"<<b>>\r\n<default>- Someone"),
            ["\\\"Hello there\\\"", "", "^8- Someone"]
        );
        assert_eq!(strip_delimited("a {b c", "{", "}", true), "a {b c");
    }
}
