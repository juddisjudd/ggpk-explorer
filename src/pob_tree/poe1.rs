//! PoE 1's tree the way Path of Building gets it. PoB builds `3_X/` from
//! GGG's web-export `data.json`: `fix_ascendancy_positions.py` moves each
//! ascendancy to a fixed spot, adds seven retired nodes back and splits the
//! sprite sheets out, then `jsonToLua` turns Python's `json.dump` of each
//! half into `tree.lua` and `sprites.lua`.
//!
//! The `data.json` here comes from the game files through the tree viewer's
//! exporter, so it carries its own sheet packing and stat wording rather than
//! GGG's. Ruthless reads the same graph with each node's Ruthless name, stats
//! and points where the game gives it some. The positions, retired nodes and
//! class illustrations are PoB's own choices, kept as tables below.

use crate::data_export::Ctx;
use crate::pob_export::text::escape_ggg_string;
use crate::skill_tree_export::json::J;
use crate::skill_tree_export::{export_tree, TreeExportOptions, TreeExportSource};
use std::path::Path;
use std::sync::Arc;

/// Where PoB puts each ascendancy's start group (`NODE_GROUPS`).
const NODE_GROUPS: [(&str, i64, i64); 37] = [
    ("Juggernaut", -10400, 5200),
    ("Berserker", -10400, 3700),
    ("Chieftain", -10400, 2200),
    ("Raider", 10200, 5200),
    ("Deadeye", 10200, 2200),
    ("Pathfinder", 10200, 3700),
    ("Occultist", -1500, -9850),
    ("Elementalist", 0, -9850),
    ("Necromancer", 1500, -9850),
    ("Slayer", 1500, 9800),
    ("Gladiator", -1500, 9800),
    ("Champion", 0, 9800),
    ("Inquisitor", -10400, -2200),
    ("Hierophant", -10400, -3700),
    ("Guardian", -10400, -5200),
    ("Assassin", 10200, -5200),
    ("Trickster", 10200, -3700),
    ("Saboteur", 10200, -2200),
    ("Ascendant", -7800, 7200),
    ("Reliquarian", -7800, 8900),
    ("Luminary", -7800, 10600),
    ("Warden", 8250, 8350),
    ("Primalist", 7200, 9400),
    ("Warlock", 9300, 7300),
    ("Aul", -6750, 12000),
    ("Breachlord", -5250, 12000),
    ("Catarina", -3750, 12000),
    ("Trialmaster", -2250, 12000),
    ("Delirious", -750, 12000),
    ("Farrul", 750, 12000),
    ("Lycia", 2250, 12000),
    ("KingInTheMists", 3750, 12000),
    ("Olroth", 5250, 12000),
    ("Oshabi", 6750, 12000),
    ("Necromantic", 9750, 12000),
    ("Abyssal", -750, 13600),
    ("Brinerot", 750, 13600),
];

/// A node the game retired that PoB still lets old builds use
/// (`EXTRA_NODES`, `EXTRA_NODE_IDS`, `EXTRA_NODES_STATS`).
struct Retired {
    ascendancy: &'static str,
    name: &'static str,
    icon: &'static str,
    notable: bool,
    skill: i64,
    offset: (i64, i64),
    group: i64,
    stats: &'static [&'static str],
    reminders: &'static [&'static str],
}

const RETIRED: [Retired; 7] = [
    Retired {
        ascendancy: "Necromancer",
        name: "Nine Lives",
        icon: "Art/2DArt/SkillIcons/passives/Ascendants/Int.png",
        notable: true,
        skill: 27602,
        offset: (-1500, -1000),
        group: 44472,
        stats: &["25% of Damage taken Recouped as Life, Mana and Energy Shield", "Recoup Effects instead occur over 3 seconds"],
        reminders: &["(Only Damage from Hits can be Recouped, over 4 seconds following the Hit)"],
    },
    Retired {
        ascendancy: "Guardian",
        name: "Searing Purity",
        icon: "Art/2DArt/SkillIcons/passives/Ascendants/StrInt.png",
        notable: true,
        skill: 57568,
        offset: (-1000, 1500),
        group: 50933,
        stats: &["45% of Chaos Damage taken as Fire Damage", "45% of Chaos Damage taken as Lightning Damage"],
        reminders: &[],
    },
    Retired {
        ascendancy: "Berserker",
        name: "Indomitable Resolve",
        icon: "Art/2DArt/SkillIcons/passives/Ascendants/Str.png",
        notable: true,
        skill: 52435,
        offset: (-1000, 0),
        group: 25519,
        stats: &["Deal 10% less Damage", "Take 25% less Damage"],
        reminders: &[],
    },
    Retired {
        ascendancy: "Ascendant",
        name: "Unleashed Potential",
        icon: "Art/2DArt/SkillIcons/passives/Ascendants/SkillPoint.png",
        notable: false,
        skill: 19355,
        offset: (-1000, 1000),
        group: 60495,
        stats: &[
            "400% increased Endurance, Frenzy and Power Charge Duration",
            "25% chance to gain a Power, Frenzy or Endurance Charge on Kill",
            "+1 to Maximum Endurance Charges",
            "+1 to Maximum Frenzy Charges",
            "+1 to Maximum Power Charges",
        ],
        reminders: &[],
    },
    Retired {
        ascendancy: "Champion",
        name: "Fatal Flourish",
        icon: "Art/2DArt/SkillIcons/passives/Ascendants/StrDex.png",
        notable: true,
        skill: 42469,
        offset: (0, 1000),
        group: 63033,
        stats: &["Final Repeat of Attack Skills deals 60% more Damage", "Non-Travel Attack Skills Repeat an additional Time"],
        reminders: &[],
    },
    Retired {
        ascendancy: "Raider",
        name: "Fury of Nature",
        icon: "Art/2DArt/SkillIcons/passives/Ascendants/Dex.png",
        notable: true,
        skill: 18054,
        offset: (1000, -1500),
        group: 56600,
        stats: &[
            "Non-Damaging Elemental Ailments you inflict spread to nearby enemies within 2 metres",
            "Non-Damaging Elemental Ailments you inflict have 100% more Effect",
        ],
        reminders: &["(Elemental Ailments are Ignited, Scorched, Chilled, Frozen, Brittled, Shocked, and Sapped)"],
    },
    Retired {
        ascendancy: "Saboteur",
        name: "Harness the Void",
        icon: "Art/2DArt/SkillIcons/passives/Ascendants/DexInt.png",
        notable: true,
        skill: 57331,
        offset: (1000, -1500),
        group: 37841,
        stats: &[
            "27% chance to gain 25% of Non-Chaos Damage with Hits as Extra Chaos Damage",
            "13% chance to gain 50% of Non-Chaos Damage with Hits as Extra Chaos Damage",
            "7% chance to gain 100% of Non-Chaos Damage with Hits as Extra Chaos Damage",
        ],
        reminders: &[],
    },
];

/// GGG's `extraImages`: where the six base-class illustrations sit. PoB loads
/// them and draws nothing from them.
const EXTRA_IMAGES: [(f64, f64, &str); 6] = [
    (-4560.69, 292.425, "Art/2DArt/BaseClassIllustrations/Str.png"),
    (1994.17, 387.525, "Art/2DArt/BaseClassIllustrations/Dex.png"),
    (-1535.86, -3848.79, "Art/2DArt/BaseClassIllustrations/Int.png"),
    (-1956.43, 1756.39, "Art/2DArt/BaseClassIllustrations/StrDex.png"),
    (-3755.23, -3210.21, "Art/2DArt/BaseClassIllustrations/StrInt.png"),
    (1704.07, -3799.71, "Art/2DArt/BaseClassIllustrations/DexInt.png"),
];

/// Every table the web export reads. It reads them without the fit check, so
/// they are checked here first: a patch that moved one stops the run rather
/// than letting it read the wrong bytes.
const TABLES: [&str; 34] = [
    "Ascendancy",
    "AscendancyPassiveSkillOverrides",
    "AtlasPassiveSkillSubTrees",
    "BaseItemTypes",
    "BlightCraftingItems",
    "BlightCraftingRecipes",
    "BlightCraftingResults",
    "BuffDefinitions",
    "BuffTemplates",
    "Characters",
    "ClassPassiveSkillOverrides",
    "Descendancy",
    "ItemVisualIdentity",
    "PassiveJewelRadiiArt",
    "PassiveJewelSlots",
    "PassiveSkillMasteryEffects",
    "PassiveSkillMasteryGroups",
    "PassiveSkills",
    "PassiveSkillTreeConnectionArt",
    "PassiveSkillTreeGroupBackgroundArt",
    "PassiveSkillTreeMasteryArt",
    "PassiveSkillTreeNodeFrameArt",
    "PassiveSkillTreeUIArt",
    "PassiveSkillTreeUIArtAscendancy",
    "PassiveSkillTrees",
    "PassiveSkillVariantTypes",
    "PassiveSkillVariants",
    "PassiveTreeDecorators",
    "PassiveTreeExpansionSkills",
    "PassiveTreeExpansionSpecialSkills",
    "ReminderText",
    "SkillGems",
    "Stats",
    "UIArtAscendancy",
];

/// `tree.lua`'s top-level keys in the order GGG's `data.json` has them.
const KEY_ORDER: [&str; 13] = [
    "tree",
    "ruthless",
    "classes",
    "alternate_ascendancies",
    "groups",
    "nodes",
    "jewelSlots",
    "min_x",
    "min_y",
    "max_x",
    "max_y",
    "constants",
    "points",
];

/// A JSON value as Python's `json` module holds it: integers and floats kept
/// apart, objects in insertion order.
#[derive(Clone, Debug, PartialEq)]
enum Py {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Py>),
    Dict(Vec<(String, Py)>),
}

impl Py {
    /// `json.load` of what the exporter printed: an integral number is
    /// written without a fraction, so Python reads it as an int.
    fn from_j(j: &J) -> Py {
        match j {
            J::Null => Py::Null,
            J::Bool(b) => Py::Bool(*b),
            J::Int(i) => Py::Int(*i),
            J::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => Py::Int(*n as i64),
            J::Num(n) => Py::Float(*n),
            J::Str(s) => Py::Str(s.clone()),
            J::Arr(items) => Py::List(items.iter().map(Py::from_j).collect()),
            J::Obj(fields) => Py::Dict(fields.iter().map(|(k, v)| (k.clone(), Py::from_j(v))).collect()),
        }
    }

    fn get(&self, key: &str) -> Option<&Py> {
        match self {
            Py::Dict(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Py> {
        match self {
            Py::Dict(fields) => fields.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `d[key] = value`: replaced where it is, or added at the end.
    fn set(&mut self, key: &str, value: Py) {
        if let Py::Dict(fields) = self {
            match fields.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value,
                None => fields.push((key.to_string(), value)),
            }
        }
    }

    fn remove(&mut self, key: &str) -> Option<Py> {
        match self {
            Py::Dict(fields) => fields.iter().position(|(k, _)| k == key).map(|i| fields.remove(i).1),
            _ => None,
        }
    }

    fn strs(items: &[&str]) -> Py {
        Py::List(items.iter().map(|s| Py::Str(s.to_string())).collect())
    }

    /// Python's `a + b` on two numbers.
    fn add(&self, other: &Py) -> Option<Py> {
        Some(match (self, other) {
            (Py::Int(a), Py::Int(b)) => Py::Int(a + b),
            (a, b) => Py::Float(a.as_f64()? + b.as_f64()?),
        })
    }

    fn sub(&self, other: &Py) -> Option<Py> {
        Some(match (self, other) {
            (Py::Int(a), Py::Int(b)) => Py::Int(a - b),
            (a, b) => Py::Float(a.as_f64()? - b.as_f64()?),
        })
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Py::Int(i) => Some(*i as f64),
            Py::Float(f) => Some(*f),
            _ => None,
        }
    }
}

/// Writes `<ctx.out>/<version>/`: `tree.lua`, `sprites.lua` and the sheets.
pub fn write(ctx: &Ctx, version: &str, ruthless: bool) -> Result<(), String> {
    let dir = ctx.out.join(version);
    let staging = ctx.out.join(format!(".{}-web-export", version));
    let _ = std::fs::remove_dir_all(&staging);
    let result = build(ctx, &dir, &staging, ruthless);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn build(ctx: &Ctx, dir: &Path, staging: &Path, ruthless: bool) -> Result<(), String> {
    for name in TABLES {
        if ctx.rr.table(name).is_some() {
            ctx.table(name)?;
        }
    }
    let files = ctx.files;
    let source = TreeExportSource::with_cdn(
        files.reader.clone(),
        Arc::clone(&files.index),
        files.steam.clone(),
        files.cdn.clone(),
        ctx.rr.schema.clone(),
    );
    let trees = ctx.table("PassiveSkillTrees")?;
    let graph = trees.by_id("Default").ok_or("PassiveSkillTrees has no Default tree")?.string("PassiveSkillGraph");
    let psg_path = format!("{}.psg", graph);
    let psg_bytes = source.fetch(&psg_path).ok_or_else(|| format!("{} is not in this install", psg_path))?;
    let psg = crate::dat::psg::parse_psg(&psg_bytes)?;
    let db = crate::ui::content_view::build_skill_graph_db_from(&|p| source.fetch(p), &source.schema, false, ruthless)?;
    let (tx, _rx) = std::sync::mpsc::channel();
    let options = TreeExportOptions { viewer: false, ..Default::default() };
    let (_, data) = export_tree(&source, &psg_path, &psg, Some(Arc::new(db)), &options, staging, &tx)?;

    let mut data = Py::from_j(&data);
    strip_viewer_fields(&mut data);
    fix_ascendancy_positions(&mut data)?;
    let mut sprites = data.remove("sprites").ok_or("the export has no sprite sheets")?;
    data.remove("extraImages");
    data.remove("imageZoomLevels");
    if ruthless {
        data.set("ruthless", Py::Bool(true));
    }
    let data = reorder(data);
    let images = flatten_sprites(&mut sprites)?;

    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    for image in &images {
        let from = staging.join("assets").join(image);
        std::fs::copy(&from, dir.join(image)).map_err(|e| format!("{}: {}", from.display(), e))?;
    }
    let mut extra = Vec::new();
    for (i, (x, y, image)) in EXTRA_IMAGES.iter().enumerate() {
        let entry = vec![
            ("x".to_string(), Py::Float(*x)),
            ("y".to_string(), Py::Float(*y)),
            ("image".to_string(), Py::Str(image.to_string())),
        ];
        extra.push(((i + 1).to_string(), Py::Dict(entry)));
    }
    let sprite_file = Py::Dict(vec![("extraImages".to_string(), Py::Dict(extra)), ("sprites".to_string(), sprites)]);
    write_lua(&dir.join("tree.lua"), &data)?;
    write_lua(&dir.join("sprites.lua"), &sprite_file)
}

/// The script's edits to `data.json`, in its order: GGG's link markup taken
/// out of stat and reminder lines, each ascendancy moved so its start group
/// sits where PoB wants it, and the retired nodes added in their own groups.
fn fix_ascendancy_positions(data: &mut Py) -> Result<(), String> {
    if let Some(Py::Dict(nodes)) = data.get_mut("nodes") {
        for (_, node) in nodes.iter_mut() {
            for field in ["stats", "reminderText"] {
                if let Some(Py::List(lines)) = node.get_mut(field) {
                    for line in lines.iter_mut() {
                        if let Py::Str(s) = line {
                            *s = escape_ggg_string(s);
                        }
                    }
                }
            }
        }
    }

    let node = |data: &Py, id: &str| data.get("nodes").and_then(|n| n.get(id)).cloned();
    let group_ids: Vec<String> = match data.get("groups") {
        Some(Py::Dict(groups)) => groups.iter().map(|(k, _)| k.clone()).collect(),
        _ => return Err("data.json has no groups".into()),
    };
    let mut ascendancy_groups: Vec<(String, String)> = Vec::new();
    let mut starts: Vec<(String, (Py, Py))> = Vec::new();
    for gid in &group_ids {
        let group = data.get("groups").and_then(|g| g.get(gid)).cloned().unwrap_or(Py::Null);
        let Some(Py::List(members)) = group.get("nodes") else { continue };
        let Some(Py::Str(first)) = members.first() else { continue };
        let Some(Py::Str(ascendancy)) = node(data, first).and_then(|n| n.get("ascendancyName").cloned()) else { continue };
        ascendancy_groups.push((ascendancy.clone(), gid.clone()));
        for member in members {
            let Py::Str(member) = member else { continue };
            if node(data, member).is_some_and(|n| n.get("isAscendancyStart").is_some()) {
                let x = group.get("x").cloned().unwrap_or(Py::Int(0));
                let y = group.get("y").cloned().unwrap_or(Py::Int(0));
                starts.retain(|(a, _)| *a != ascendancy);
                starts.push((ascendancy.clone(), (x, y)));
            }
        }
    }
    for (ascendancy, gid) in &ascendancy_groups {
        let Some(&(_, tx, ty)) = NODE_GROUPS.iter().find(|(name, _, _)| name == ascendancy) else {
            eprintln!("pob tree: PoB has no position for ascendancy {}; left where the game puts it", ascendancy);
            continue;
        };
        let Some((_, (sx, sy))) = starts.iter().find(|(a, _)| a == ascendancy) else {
            eprintln!("pob tree: ascendancy {} has no start node; left where the game puts it", ascendancy);
            continue;
        };
        let (ox, oy) = (Py::Int(tx).sub(sx).ok_or("bad group x")?, Py::Int(ty).sub(sy).ok_or("bad group y")?);
        let group = data.get_mut("groups").and_then(|g| g.get_mut(gid)).ok_or("group vanished")?;
        let x = group.get("x").and_then(|x| x.add(&ox)).ok_or("bad group x")?;
        let y = group.get("y").and_then(|y| y.add(&oy)).ok_or("bad group y")?;
        group.set("x", x);
        group.set("y", y);
    }

    for retired in &RETIRED {
        let key = retired.group.to_string();
        if data.get("groups").and_then(|g| g.get(&key)).is_some() {
            eprintln!("pob tree: group {} is taken; {} left out", key, retired.name);
            continue;
        }
        let Some(&(_, tx, ty)) = NODE_GROUPS.iter().find(|(name, _, _)| *name == retired.ascendancy) else { continue };
        let group = Py::Dict(vec![
            ("x".into(), Py::Int(tx + retired.offset.0)),
            ("y".into(), Py::Int(ty + retired.offset.1)),
            ("orbits".into(), Py::List(vec![Py::Int(0)])),
            ("nodes".into(), Py::List(vec![Py::Str(retired.skill.to_string())])),
        ]);
        data.get_mut("groups").ok_or("data.json has no groups")?.set(&key, group);
        let mut fields = vec![("name".to_string(), Py::Str(retired.name.into())), ("icon".to_string(), Py::Str(retired.icon.into()))];
        if retired.notable {
            fields.push(("isNotable".into(), Py::Bool(true)));
        }
        fields.extend([
            ("skill".into(), Py::Int(retired.skill)),
            ("group".into(), Py::Int(retired.group)),
            ("ascendancyName".into(), Py::Str(retired.ascendancy.into())),
            ("orbit".into(), Py::Int(0)),
            ("orbitIndex".into(), Py::Int(0)),
            ("out".into(), Py::List(Vec::new())),
            ("in".into(), Py::List(Vec::new())),
            ("stats".into(), Py::strs(retired.stats)),
            ("reminderText".into(), Py::strs(retired.reminders)),
        ]);
        data.get_mut("nodes").ok_or("data.json has no nodes")?.set(&retired.skill.to_string(), Py::Dict(fields));
    }
    Ok(())
}

/// Class and ascendancy fields the viewer draws with and GGG's export, as PoB
/// has it, does not carry.
fn strip_viewer_fields(data: &mut Py) {
    let Some(Py::List(classes)) = data.get_mut("classes") else { return };
    for class in classes.iter_mut() {
        for key in ["image", "image_offset_x", "image_offset_y", "overridePairs"] {
            class.remove(key);
        }
        if let Some(Py::List(ascendancies)) = class.get_mut("ascendancies") {
            for ascendancy in ascendancies.iter_mut() {
                for key in ["image", "offsetX", "offsetY", "flavourTextSize", "overridePairs"] {
                    ascendancy.remove(key);
                }
            }
        }
    }
}

/// Puts the top-level keys in the order GGG's export has them; anything else
/// follows in the order it came.
fn reorder(data: Py) -> Py {
    let Py::Dict(mut fields) = data else { return data };
    let mut out = Vec::new();
    for key in KEY_ORDER {
        if let Some(i) = fields.iter().position(|(k, _)| k == key) {
            out.push(fields.remove(i));
        }
    }
    out.extend(fields);
    Py::Dict(out)
}

/// Keeps each sheet at the one zoom PoB draws from (`"1"`, else `"0.3835"`),
/// named by its file alone. Returns the image files the sheets name. The
/// class illustration sheets the viewer uses are not in GGG's export and
/// are left out.
fn flatten_sprites(sprites: &mut Py) -> Result<Vec<String>, String> {
    let Py::Dict(entries) = sprites else { return Err("sprites is not an object".into()) };
    entries.retain(|(name, _)| !name.starts_with("class"));
    let mut images = Vec::new();
    for (_, zooms) in entries.iter_mut() {
        let picked = zooms.get("1").or_else(|| zooms.get("0.3835")).cloned();
        let Some(mut sheet) = picked else { continue };
        if let Some(Py::Str(file)) = sheet.get("filename").cloned() {
            let name = file.rsplit('/').next().unwrap_or(&file).to_string();
            if !images.contains(&name) {
                images.push(name.clone());
            }
            sheet.set("filename", Py::Str(name));
        }
        *zooms = sheet;
    }
    Ok(images)
}

fn write_lua(path: &Path, value: &Py) -> Result<(), String> {
    let mut json = String::new();
    dump(value, 0, &mut json);
    let lua = format!("return {}", json_to_lua(&json));
    std::fs::write(path, lua).map_err(|e| format!("{}: {}", path.display(), e))
}

/// `json.dump(value, f, indent=4)`.
fn dump(value: &Py, level: usize, out: &mut String) {
    let indent = |out: &mut String, level: usize| out.extend(std::iter::repeat_n(' ', level * 4));
    match value {
        Py::Null => out.push_str("null"),
        Py::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Py::Int(i) => out.push_str(&i.to_string()),
        Py::Float(f) => out.push_str(&float_repr(*f)),
        Py::Str(s) => quote(s, out),
        Py::List(items) if items.is_empty() => out.push_str("[]"),
        Py::Dict(fields) if fields.is_empty() => out.push_str("{}"),
        Py::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                indent(out, level + 1);
                dump(item, level + 1, out);
            }
            out.push('\n');
            indent(out, level);
            out.push(']');
        }
        Py::Dict(fields) => {
            out.push('{');
            for (i, (key, item)) in fields.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                indent(out, level + 1);
                quote(key, out);
                out.push_str(": ");
                dump(item, level + 1, out);
            }
            out.push('\n');
            indent(out, level);
            out.push('}');
        }
    }
}

/// Python's `repr` of a float: the shortest digits that read back the same,
/// with `.0` on whole numbers and an exponent outside 1e-4 to 1e16.
fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-Infinity".into() } else { "Infinity".into() };
    }
    let sci = format!("{:e}", f);
    let (mantissa, exp) = sci.split_once('e').expect("LowerExp has an exponent");
    let exp: i32 = exp.parse().expect("exponent is an integer");
    let (sign, mantissa) = mantissa.strip_prefix('-').map(|m| ("-", m)).unwrap_or(("", mantissa));
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let point = exp + 1;
    if (-3..=16).contains(&point) {
        let body = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!("{}{}.0", digits, "0".repeat(point as usize - digits.len()))
        } else {
            format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
        };
        format!("{}{}", sign, body)
    } else {
        let mantissa = match digits.len() {
            1 => digits.clone(),
            _ => format!("{}.{}", &digits[..1], &digits[1..]),
        };
        format!("{}{}e{}{:02}", sign, mantissa, if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

/// `json.dumps` string quoting with `ensure_ascii`.
fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || !c.is_ascii() => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// PoB's `jsonToLua` (`Modules/Common.lua`): text substitutions, applied to
/// strings as much as to the structure, which is why the stat lines lose
/// their link brackets first.
fn json_to_lua(json: &str) -> String {
    use regex::{Captures, Regex};
    let s = json.replace('[', "{").replace(']', "}");
    let s = Regex::new(r#""(\d[\d.]*)":"#).expect("valid pattern").replace_all(&s, "[$1]=").into_owned();
    let s = Regex::new(r#""([^"]+)":"#).expect("valid pattern").replace_all(&s, "[\"$1\"]=").into_owned();
    let s = s.replace("\\/", "/");
    let s = Regex::new(r"\{([A-Za-z0-9]+)\}").expect("valid pattern").replace_all(&s, "{[0]=$1}").into_owned();
    Regex::new(r"\\u([0-9A-Fa-f]{4})")
        .expect("valid pattern")
        .replace_all(&s, |c: &Captures| {
            let code = u32::from_str_radix(&c[1], 16).unwrap_or(0xFFFD);
            match char::from_u32(code) {
                Some(ch) if !(0xD800..=0xDFFF).contains(&code) => ch.to_string(),
                _ => "?".to_string(),
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_like_python_repr() {
        let cases: [(f64, &str); 7] = [
            (-750.0, "-750.0"),
            (-1096.9000000000015, "-1096.9000000000015"),
            (13399.92, "13399.92"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e16, "1e+16"),
            (123456789012345680.0, "1.2345678901234568e+17"),
        ];
        for (value, expected) in cases {
            assert_eq!(float_repr(value), expected);
        }
    }

    #[test]
    fn json_to_lua_matches_pob() {
        let mut out = String::new();
        dump(
            &Py::Dict(vec![
                ("tree".into(), Py::Str("Default".into())),
                ("28609".into(), Py::Dict(vec![("in".into(), Py::List(vec![Py::Str("1".into())]))])),
                ("stats".into(), Py::List(Vec::new())),
            ]),
            0,
            &mut out,
        );
        assert_eq!(
            format!("return {}", json_to_lua(&out)),
            "return {\n    [\"tree\"]= \"Default\",\n    [28609]= {\n        [\"in\"]= {\n            \"1\"\n        }\n    },\n    [\"stats\"]= {}\n}"
        );
    }
}
