//! PoB-PoE2's `src/Export/Scripts/passivetree.lua`, ported: the character
//! tree as `tree.lua` and `tree.json`, its art stacked into `.dds.zst` array
//! textures, and the orbit arcs cut out as PNGs.
//!
//! The script's quirks are kept, since the point is output PoB can drop in:
//! nodes are dropped or kept by the same rules, a group's `orbits` come out
//! in LuaJIT's `pairs` order, `min_x`/`min_y` start from 0, and ascendancy
//! clusters are moved onto a ring around the tree by its arithmetic.

use super::{dds, format, luajit, orbits};
use crate::dat::relational::{LoadedTable, Ref, Row};
use crate::data_export::Ctx;
use crate::pob_export::lua::{Lua, Table};
use crate::pob_export::modules::skills::{parse_ui_images, UiImage};
use crate::pob_export::statdesc::{Descriptors, Stats};
use crate::pob_export::text::escape_ggg_string;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::rc::Rc;

/// Nodes named `[DNT…]` are left out, except these.
const KEPT_DNT: [&str; 5] = ["[DNT] Kite Fisher", "[DNT] Troller", "[DNT] Spearfisher", "[DNT] Angler", "[DNT] Whaler"];

/// The attribute choices every attribute node offers, in the order LuaJIT's
/// `pairs` returns the script's `{ [26297], [14927], [57022] }`.
const ATTRIBUTES: [i64; 3] = [26297, 14927, 57022];

const ORBIT_RADII: [f64; 10] = [0.0, 82.0, 162.0, 335.0, 493.0, 662.0, 846.0, 251.0, 1080.0, 1322.0];

/// Degrees of arc the ascendancies around one class start are spread over,
/// by how many there are.
const ARC_ANGLE: [f64; 10] = [0.0, 0.0, 12.0, 24.0, 36.0, 48.0, 60.0, 72.0, 84.0, 96.0];

const LEGION_ART: [&str; 13] = [
    "Art/2DArt/UIImages/InGame/Abyss/AbyssPassiveSkillScreenJewelCircle1",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenEternalEmpireJewelCircle1",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenEternalEmpireJewelCircle2",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenKalguuranJewelCircle1",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenKalguuranJewelCircle2",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenKaruiJewelCircle1",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenKaruiJewelCircle2",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenMarakethJewelCircle1",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenMarakethJewelCircle2",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenTemplarJewelCircle1",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenTemplarJewelCircle2",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenVaalJewelCircle1",
    "Art/2DArt/UIImages/InGame/PassiveSkillScreenVaalJewelCircle2",
];

/// The connection art cut into orbit PNGs, and the three states of each.
const ORBIT_ART: [&str; 3] = ["Character", "CharacterAscendancy", "CharacterPlanned"];
const ORBIT_STATES: [(&str, &str, &str); 3] =
    [("Normal", "normal", "Normal"), ("Intermediate", "intermediate", "Intermediate"), ("Active", "intermediateactive", "Active")];

fn is_dnt(name: &str) -> bool {
    name.starts_with("[DNT")
}

/// `assetSheets.newSheet`: the pictures one sheet stacks, by texture path.
struct Sheet {
    name: &'static str,
    files: BTreeMap<String, Vec<(&'static str, Option<String>)>>,
}

impl Sheet {
    fn new(name: &'static str) -> Self {
        Self { name, files: BTreeMap::new() }
    }

    /// `addToSheet`: once per section and alias.
    fn add(&mut self, icon: &str, section: &'static str, alias: Option<&str>) {
        if icon.is_empty() {
            return;
        }
        let list = self.files.entry(icon.to_string()).or_default();
        if !list.iter().any(|(s, a)| *s == section && a.as_deref() == alias) {
            list.push((section, alias.map(str::to_string)));
        }
    }
}

const SKILLS: usize = 0;
const SKILLS_DISABLED: usize = 1;
const GROUP_BACKGROUND: usize = 2;
const MASTERY_ACTIVE_EFFECT: usize = 3;
const ASCENDANCY_BACKGROUND: usize = 4;
const OILS: usize = 5;
const JEWEL_SOCKETS: usize = 7;
const LEGION: usize = 8;

/// Column names, taken from whichever name dat-schema uses this patch.
struct Cols {
    id: &'static str,
    icon: &'static str,
    stats: &'static str,
    stat_values: Vec<&'static str>,
    graph_id: &'static str,
    name: &'static str,
    class_start: &'static str,
    keystone: &'static str,
    notable: &'static str,
    flavour: &'static str,
    only_image: &'static str,
    jewel_socket: &'static str,
    ascendancy: &'static str,
    ascendancy_start: &'static str,
    points: &'static str,
    multiple_choice: &'static str,
    multiple_choice_option: &'static str,
    anoint_only: &'static str,
    mastery_group: &'static str,
    attribute: &'static str,
    granted_skill: &'static str,
    weapon_points: &'static str,
    free: Option<&'static str>,
    apply_to_armour: Option<usize>,
    constraint: Option<&'static str>,
    ascendancy_unlock: Option<&'static str>,
    frame_art: Option<&'static str>,
}

impl Cols {
    fn new(t: &LoadedTable) -> Result<Self, String> {
        let stat_values = (1..=7)
            .filter_map(|k| {
                let names: [&'static str; 2] = match k {
                    1 => ["Stat1Value", "Stat1"],
                    2 => ["Stat2Value", "Stat2"],
                    3 => ["Stat3Value", "Stat3"],
                    4 => ["Stat4Value", "Stat4"],
                    5 => ["Stat5Value", "Stat5"],
                    6 => ["Stat6Value", "Stat6"],
                    _ => ["Stat7Value", "Stat7"],
                };
                t.pick(&names)
            })
            .collect();
        Ok(Self {
            id: t.require(&["Id"])?,
            icon: t.require(&["Icon_DDSFile", "Icon"])?,
            stats: t.require(&["Stats"])?,
            stat_values,
            graph_id: t.require(&["PassiveSkillGraphId", "PassiveSkillNodeId"])?,
            name: t.require(&["Name"])?,
            class_start: t.require(&["Characters", "ClassStart"])?,
            keystone: t.require(&["IsKeystone", "Keystone"])?,
            notable: t.require(&["IsNotable", "Notable"])?,
            flavour: t.require(&["FlavourText"])?,
            only_image: t.require(&["IsJustIcon", "IsOnlyImage"])?,
            jewel_socket: t.require(&["IsJewelSocket", "JewelSocket"])?,
            ascendancy: t.require(&["Ascendancy", "AscendancyKey"])?,
            ascendancy_start: t.require(&["IsAscendancyStartingNode", "AscendancyStart"])?,
            points: t.require(&["SkillPointsGranted", "PassivePointsGranted"])?,
            multiple_choice: t.require(&["IsMultipleChoice", "MultipleChoice"])?,
            multiple_choice_option: t.require(&["IsMultipleChoiceOption", "MultipleChoiceOption"])?,
            anoint_only: t.require(&["IsAnointmentOnly", "AnointOnly"])?,
            mastery_group: t.require(&["MasteryGroup"])?,
            attribute: t.require(&["IsAttribute", "Attribute"])?,
            granted_skill: t.require(&["GrantedSkill"])?,
            weapon_points: t.require(&["WeaponPointsGranted"])?,
            free: t.pick(&["IsFree", "FreeAllocate"]),
            apply_to_armour: t.column_or_after(&["ApplyToArmour"], "IsFree", 1),
            constraint: t.pick(&["UnlockedBy", "ConstraintNode"]),
            ascendancy_unlock: t.pick(&["VisibleForAscendancy", "AscendancyUnlock"]),
            frame_art: t.pick(&["NodeFrameArt", "FrameArt"]),
        })
    }
}

struct Group {
    x: f64,
    y: f64,
    orbits: Vec<i32>,
    nodes: Vec<u32>,
}

struct Ascendancy {
    table: Table,
    id: String,
    internal_id: String,
    background: (f64, f64),
    replace_by: Option<String>,
}

struct Class {
    table: Table,
    name: String,
    ascendancies: Vec<Ascendancy>,
}

/// Where a node sits and which classes start on it, kept beside its table
/// for the ascendancy layout.
struct Placed {
    table: Table,
    group: usize,
    orbit: i32,
    orbit_index: i32,
    classes_start: Vec<String>,
}

struct Builder<'c, 'a> {
    ctx: &'c Ctx<'a>,
    ui: HashMap<String, UiImage>,
    rects: HashMap<String, Option<[f64; 4]>>,
    sheets: std::cell::RefCell<Vec<Sheet>>,
    describer: Rc<Descriptors>,
    passives: Rc<LoadedTable>,
    cols: Cols,
    by_graph_id: HashMap<i64, usize>,
}

impl<'c, 'a> Builder<'c, 'a> {
    fn ui_path(&self, name: &str) -> Result<String, String> {
        self.ui
            .get(&name.to_ascii_lowercase())
            .map(|image| image.path.clone())
            .ok_or_else(|| format!("Art/UIImages1.txt has no {}", name))
    }

    fn add(&self, sheet: usize, icon: &str, section: &'static str, alias: Option<&str>) {
        self.sheets.borrow_mut()[sheet].add(icon, section, alias);
    }

    fn deref(&self, row: Row<'_>, col: &str) -> Option<Ref> {
        self.ctx.rr.deref(row, col)
    }

    fn passive(&self, graph_id: i64) -> Option<Row<'_>> {
        self.by_graph_id.get(&graph_id).and_then(|&i| self.passives.row(i))
    }

    /// `describeStats` over a PassiveSkills row's stats, each at its value.
    fn describe(&self, row: Row<'_>) -> Vec<String> {
        let mut stats = Stats::new();
        for (k, stat) in self.ctx.rr.deref_list(row, self.cols.stats).iter().enumerate() {
            let value = self.cols.stat_values.get(k).map(|c| row.int(c)).unwrap_or(0) as f64;
            stats.set(&stat.id(), value, value);
        }
        self.describer.describe_stats(&mut stats).lines
    }

    /// The three frames of a PassiveSkillTreeNodeFrameArt row, as uiimage
    /// paths: normal, active, can-allocate.
    fn frames(&self, frame: &Ref) -> Result<[String; 3], String> {
        let row = frame.row();
        Ok([self.ui_path(row.str("Normal"))?, self.ui_path(row.str("Active"))?, self.ui_path(row.str("CanAllocate"))?])
    }

    fn overlay(alloc: &str, path: &str, unalloc: &str) -> Table {
        let mut t = Table::new();
        t.set("alloc", alloc).set("path", path).set("unalloc", unalloc);
        t
    }
}

/// Writes `<ctx.out>/<version>/`: the tree files, the sheets and the orbit
/// PNGs.
pub fn write(ctx: &Ctx, version: &str) -> Result<(), String> {
    let dir = ctx.out.join(version);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let Built { mut tree, sheets, orbit_art, rects } = build(ctx)?;
    let mut coords = Table::new();
    for sheet in &sheets {
        for (file, entries) in pack(ctx, sheet, &rects, &dir)? {
            coords.set(file, entries);
        }
    }
    tree.set("ddsCoords", coords);
    let mut assets = Table::new();
    for (art, row) in &orbit_art {
        for (column, lower, postfix) in ORBIT_STATES {
            let basename = format!("{}_orbit_{}", art, lower);
            for (name, file) in orbits::asset_names(art, postfix, &basename) {
                assets.set(name, Table::list([file]));
            }
            orbits::write(ctx, row.row().str(column), row.row().str("Mask"), &dir, &basename)?;
        }
    }
    tree.set("assets", assets);
    write_file(&dir.join("tree.lua"), format::lua_source(&tree).as_bytes())?;
    write_file(&dir.join("tree.json"), format::json(&Lua::Table(tree)).as_bytes())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {}", path.display(), e))
}

struct Built {
    tree: Table,
    sheets: Vec<Sheet>,
    orbit_art: Vec<(&'static str, Ref)>,
    rects: HashMap<String, Option<[f64; 4]>>,
}

fn build(ctx: &Ctx) -> Result<Built, String> {
    let trees = ctx.table("PassiveSkillTrees")?;
    let tree_row = trees.by_id("Default").ok_or("PassiveSkillTrees has no Default tree")?;
    let psg_path = format!("{}.psg", tree_row.str("PassiveSkillGraph"));
    let psg_bytes = crate::dat::relational::FileSource::fetch(ctx.files, &psg_path)
        .ok_or_else(|| format!("{} is not in this install", psg_path))?;
    if psg_bytes.get(..2) != Some(&[3, 0]) {
        return Err(format!("{}: only version 3 passive graphs are supported", psg_path));
    }
    let psg = crate::dat::psg::parse_psg(&psg_bytes)?;

    let ui_text = crate::pob_export::read_text(ctx, "art/uiimages1.txt").ok_or("art/uiimages1.txt is missing")?;
    let ui = parse_ui_images(&ui_text);
    let mut rects: HashMap<String, Option<[f64; 4]>> = HashMap::new();
    for image in ui.values() {
        let rect = match (image.x, image.y, image.width, image.height) {
            (Some(x), Some(y), Some(w), Some(h)) => Some([x, y, w, h]),
            _ => None,
        };
        match rects.get(&image.path) {
            None => {
                rects.insert(image.path.clone(), rect);
            }
            Some(seen) if *seen != rect => {
                rects.insert(image.path.clone(), None);
            }
            Some(_) => {}
        }
    }

    let passives = ctx.table("PassiveSkills")?;
    let cols = Cols::new(&passives)?;
    let mut by_graph_id = HashMap::new();
    for row in passives.rows() {
        by_graph_id.entry(row.int(cols.graph_id)).or_insert(row.index);
    }
    let describer = crate::pob_export::describer(ctx, &["passive_skill_stat_descriptions.csd"]);
    let sheets: Vec<Sheet> = ["skills", "skills-disabled", "group-background", "mastery-active-effect", "ascendancy-background", "oils", "lines", "jewel-sockets", "legion"]
        .into_iter()
        .map(Sheet::new)
        .collect();
    let b = Builder { ctx, ui, rects, sheets: std::cell::RefCell::new(sheets), describer, passives: Rc::clone(&passives), cols, by_graph_id };

    let connection_art = ctx.table("PassiveSkillTreeConnectionArt")?;
    let orbit_art: Vec<(&'static str, Ref)> = ORBIT_ART
        .into_iter()
        .filter_map(|id| {
            let row = connection_art.by_id(id);
            if row.is_none() {
                eprintln!("pob tree: connection art {} not found", id);
            }
            row.map(|r| (id, Ref { table: Rc::clone(&connection_art), index: r.index }))
        })
        .collect();

    let ui_art = b.deref(tree_row, "UIArt").ok_or("the Default tree has no UIArt")?;
    let art = ui_art.row();
    let small = b.ui_path(art.str("GroupBackgroundSmall"))?;
    let medium = b.ui_path(art.str("GroupBackgroundMedium"))?;
    let large = b.ui_path(art.str("GroupBackgroundLarge"))?;
    b.add(GROUP_BACKGROUND, &small, "groupBackground", Some("PSGroupBackground1"));
    b.add(GROUP_BACKGROUND, &medium, "groupBackground", Some("PSGroupBackground2"));
    b.add(GROUP_BACKGROUND, &large, "groupBackground", Some("PSGroupBackground3"));
    for (column, names) in [
        ("PassiveFrame", ["PSSkillFrame", "PSSkillFrameActive", "PSSkillFrameHighlighted"]),
        ("KeystoneFrame", ["KeystoneFrameUnallocated", "KeystoneFrameAllocated", "KeystoneFrameCanAllocate"]),
        ("NotableFrame", ["NotableFrameUnallocated", "NotableFrameAllocated", "NotableFrameCanAllocate"]),
    ] {
        let frame = b.deref(art, column).ok_or_else(|| format!("the Default tree's UIArt has no {}", column))?;
        let paths = b.frames(&frame)?;
        for (path, alias) in paths.iter().zip(names) {
            b.add(GROUP_BACKGROUND, path, "frame", Some(alias));
        }
    }
    b.add(GROUP_BACKGROUND, &small, "groupBackground", Some("PSGroupBackgroundSmallBlank"));
    b.add(GROUP_BACKGROUND, &medium, "groupBackground", Some("PSGroupBackgroundMediumBlank"));
    b.add(GROUP_BACKGROUND, &large, "groupBackground", Some("PSGroupBackgroundLargeBlank"));
    let jewel = b.deref(art, "JewelFrame").ok_or("the Default tree's UIArt has no JewelFrame")?;
    let [jewel_normal, jewel_active, jewel_can] = b.frames(&jewel)?;
    b.add(GROUP_BACKGROUND, &jewel_can, "frame", Some("JewelFrameCanAllocate"));
    b.add(GROUP_BACKGROUND, &jewel_active, "frame", Some("JewelFrameAllocated"));
    b.add(GROUP_BACKGROUND, &jewel_normal, "frame", Some("JewelFrameUnallocated"));
    let middle = b.ui_path("Art/2DArt/UIImages/InGame/PassiveSkillScreenAscendancyMiddle")?;
    b.add(GROUP_BACKGROUND, &middle, "frame", Some("AscendancyMiddle"));
    let start = b.ui_path("Art/2DArt/UIImages/InGame/PassiveSkillScreenStartNodeBackgroundInactive")?;
    b.add(GROUP_BACKGROUND, &start, "startNode", Some("PSStartNodeBackgroundInactive"));
    b.add(
        ASCENDANCY_BACKGROUND,
        "art/textures/interface/2d/2dart/uiimages/ingame/passivetree/passivetreemaincircle.dds",
        "AscendancyBackground",
        Some("BGTree"),
    );
    b.add(
        ASCENDANCY_BACKGROUND,
        "art/textures/interface/2d/2dart/uiimages/ingame/passivetree/passivetreemaincircleactive2.dds",
        "AscendancyBackground",
        Some("BGTreeActive"),
    );

    let jewel_art = ctx.table("PassiveJewelArt")?;
    let item_col = jewel_art.require(&["BaseItemTypesKey", "Item"])?;
    let art_col = jewel_art.require(&["SocketedImage", "JewelArt"])?;
    for row in jewel_art.rows() {
        let name = b.deref(row, item_col).map(|r| r.row().string("Name")).unwrap_or_default();
        if is_dnt(&name) {
            continue;
        }
        let path = b.ui_path(row.str(art_col))?;
        b.add(JEWEL_SOCKETS, &path, "jewelpassive", Some(&name));
    }
    let unique_art = ctx.table("PassiveJewelUniqueArt")?;
    let words_col = unique_art.require(&["Name", "WordsKey"])?;
    for row in unique_art.rows() {
        let name = b.deref(row, words_col).map(|r| r.row().string("Text")).unwrap_or_default();
        if is_dnt(&name) {
            continue;
        }
        let path = b.ui_path(row.str("JewelArt"))?;
        b.add(JEWEL_SOCKETS, &path, "jewelpassive", Some(&name));
    }
    for row in ctx.table("AlternatePassiveSkills")?.rows() {
        let icon = row.string("DDSIcon");
        b.add(LEGION, &icon, "legion", Some(&icon));
    }
    for art in LEGION_ART {
        let path = b.ui_path(art)?;
        b.add(LEGION, &path, "legion", Some(&path));
    }

    let ascendancy_table = ctx.table("Ascendancy")?;
    let asc = AscCols::new(&ascendancy_table)?;
    let mut classes: Vec<Class> = Vec::new();
    let mut replacements: BTreeMap<String, String> = BTreeMap::new();
    for &root in &psg.roots {
        let row = b.passive(root as i64).ok_or_else(|| format!("class start {} is not in PassiveSkills", root))?;
        if is_dnt(row.str(b.cols.name)) {
            continue;
        }
        for character in b.ctx.rr.deref_list(row, b.cols.class_start) {
            let ch = character.row();
            let name = ch.string("Name");
            if is_dnt(&name) {
                continue;
            }
            let mut background = Table::new();
            background
                .set("active", size_table(2000.0, 2000.0))
                .set("bg", size_table(2000.0, 2000.0))
                .set("image", format!("Classes{}", name))
                .set("section", "AscendancyBackground")
                .set("x", 0)
                .set("y", 0)
                .set("width", 1500)
                .set("height", 1500);
            let mut table = Table::new();
            table
                .set("name", name.as_str())
                .set("integerId", ch.int("IntegerId"))
                .set("base_str", ch.int("BaseStrength"))
                .set("base_dex", ch.int("BaseDexterity"))
                .set("base_int", ch.int("BaseIntelligence"))
                .set("background", background);
            b.add(ASCENDANCY_BACKGROUND, ch.str("PassiveTreeImage"), "AscendancyBackground", Some(&format!("Classes{}", name)));

            let mut ascendancies = Vec::new();
            for a in ascendancy_table.rows().filter(|a| a.key(asc.character) == Some(character.index)) {
                let asc_name = a.string("Name");
                if is_dnt(&asc_name) || a.bool(asc.disabled) {
                    continue;
                }
                let replace = asc.replace.and_then(|c| b.deref(a, c)).map(|r| r.row().string("Name"));
                if let Some(replaced) = &replace {
                    replacements.insert(replaced.clone(), asc_name.clone());
                }
                let mut entry = Table::new();
                entry.set("id", asc_name.as_str()).set("name", asc_name.as_str()).set("internalId", a.str("Id"));
                if let Some(replaced) = &replace {
                    entry.set("replace", replaced.as_str());
                }
                ascendancies.push(Ascendancy {
                    table: entry,
                    id: asc_name.clone(),
                    internal_id: a.string("Id"),
                    background: (0.0, 0.0),
                    replace_by: None,
                });
                b.add(ASCENDANCY_BACKGROUND, a.str("PassiveTreeImage"), "AscendancyBackground", Some(&format!("Classes{}", asc_name)));
                let ui_art = b.deref(a, "UIArt").ok_or_else(|| format!("ascendancy {} has no UIArt", asc_name))?;
                let passive = b.deref(ui_art.row(), "PassiveFrame").ok_or_else(|| format!("{} has no passive frame", asc_name))?;
                let notable = b.deref(ui_art.row(), "NotableFrame").ok_or_else(|| format!("{} has no notable frame", asc_name))?;
                let [p_normal, p_active, p_can] = b.frames(&passive)?;
                let [n_normal, n_active, n_can] = b.frames(&notable)?;
                b.add(GROUP_BACKGROUND, &p_can, "frame", Some(&format!("{}FrameSmallCanAllocate", asc_name)));
                b.add(GROUP_BACKGROUND, &p_normal, "frame", Some(&format!("{}FrameSmallNormal", asc_name)));
                b.add(GROUP_BACKGROUND, &p_active, "frame", Some(&format!("{}FrameSmallAllocated", asc_name)));
                b.add(GROUP_BACKGROUND, &n_normal, "frame", Some(&format!("{}FrameLargeNormal", asc_name)));
                b.add(GROUP_BACKGROUND, &n_can, "frame", Some(&format!("{}FrameLargeCanAllocate", asc_name)));
                b.add(GROUP_BACKGROUND, &n_active, "frame", Some(&format!("{}FrameLargeAllocated", asc_name)));
            }
            if !ascendancies.is_empty() {
                classes.push(Class { table, name, ascendancies });
            }
        }
    }

    let mut attributes: Vec<Table> = Vec::new();
    for id in ATTRIBUTES {
        let mut option = Table::new();
        option.set("id", id);
        match b.passive(id) {
            Some(base) if !is_dnt(base.str(b.cols.name)) => {
                option
                    .set("name", base.str(b.cols.name))
                    .set("icon", base.str(b.cols.icon))
                    .set("stats", Table::list(b.describe(base)));
                let icon = base.string(b.cols.icon);
                b.add(SKILLS, &icon, "normalActive", None);
            }
            _ => eprintln!("pob tree: base attribute {} not found", id),
        }
        attributes.push(option);
    }

    let lookups = Lookups::new(&b)?;
    let mut nodes: BTreeMap<u32, Placed> = BTreeMap::new();
    let mut groups: BTreeMap<usize, Group> = BTreeMap::new();
    let mut orbit_slots: BTreeMap<usize, i64> = BTreeMap::new();
    let mut ascendancy_groups: HashMap<String, (Option<u32>, Vec<usize>)> = HashMap::new();
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (0f64, 0f64, 0f64, 0f64);

    for (gi, group) in psg.groups.iter().enumerate() {
        let i = gi + 1;
        let mut is_ascendancy_group = false;
        let mut tree_group =
            Group { x: round_to(group.x as f64, 2), y: round_to(group.y as f64, 2), orbits: Vec::new(), nodes: Vec::new() };
        let mut orbit_keys: Vec<i32> = Vec::new();
        for passive in &group.nodes {
            let id = passive.skill_id;
            let radius = passive.radius as i32;
            let position = passive.position as i32;
            let mut node = Table::new();
            node.set("skill", id).set("group", i).set("orbit", radius).set("orbitIndex", position);
            let mut classes_start = Vec::new();
            if let Some(row) = b.passive(id as i64) {
                let kept = fill_node(&b, &lookups, row, id, i, &replacements, &attributes, &mut node, &mut is_ascendancy_group, &mut ascendancy_groups, &mut classes_start)?;
                if !kept {
                    continue;
                }
            }
            let connections: Vec<Table> = passive
                .connections
                .iter()
                .map(|c| {
                    let mut t = Table::new();
                    t.set("id", c.node_id).set("orbit", c.orbit);
                    t
                })
                .collect();
            node.set("connections", Table::list(connections));
            orbit_keys.push(radius + 1);
            let slot = orbit_slots.entry((radius + 1) as usize).or_insert(1);
            *slot = (*slot).max(position as i64);
            nodes.insert(id, Placed { table: node, group: i, orbit: radius, orbit_index: position, classes_start });
            tree_group.nodes.push(id);
        }
        tree_group.orbits = luajit::pairs_order(orbit_keys).into_iter().map(|k| k - 1).collect();
        if !tree_group.nodes.is_empty() {
            groups.insert(i, tree_group);
            if !is_ascendancy_group {
                min_x = min_x.min(group.x as f64);
                min_y = min_y.min(group.y as f64);
                max_x = max_x.max(group.x as f64);
                max_y = max_y.max(group.y as f64);
            }
        }
    }

    let jewel_slots = ctx.table("PassiveJewelSlots")?;
    let slot_col = jewel_slots.require(&["Slot", "Passive"])?;
    let jewel_slot_ids: Vec<i64> = jewel_slots
        .rows()
        .filter_map(|row| b.deref(row, slot_col).map(|r| r.row().int(b.cols.graph_id)))
        .collect();

    let mut skills_per_orbit: Vec<i64> = Vec::new();
    for i in 1.. {
        let Some(&slots) = orbit_slots.get(&i) else { break };
        skills_per_orbit.push(if i == 1 { slots } else { (slots as f64 / 12.0).ceil() as i64 * 12 });
    }
    let orbit_angles: Vec<Vec<f64>> = skills_per_orbit.iter().map(|&n| orbit_angles(n)).collect();

    let radius_tree = ((max_x - min_x).max(max_y - min_y) + 700.0) / 2.0;
    let hard_coded = radius_tree + 2800.0;
    for &root in &psg.roots {
        let start = nodes.get(&root).ok_or_else(|| format!("class start {} was left out of the tree", root))?;
        let group = &groups[&start.group];
        let angle_to_centre = group.y.atan2(group.x);
        let mut total = 0usize;
        let mut picked: Vec<usize> = Vec::new();
        for (ci, class) in classes.iter().enumerate() {
            for start_class in &start.classes_start {
                if *start_class == class.name {
                    total += class.ascendancies.len();
                    picked.push(ci);
                }
            }
        }
        let arc = *ARC_ANGLE.get(total).ok_or_else(|| format!("{} ascendancies share one class start", total))?;
        let start_angle = angle_to_centre - (arc / 2.0).to_radians();
        let angle_step = (arc / (total as f64 - 1.0)).to_radians();
        let mut j = 1.0;
        for ci in picked {
            for ai in 0..classes[ci].ascendancies.len() {
                let angle = start_angle + (j - 1.0) * angle_step;
                let (cx, cy) = (hard_coded * angle.cos(), hard_coded * angle.sin());
                let ascendancy_id = classes[ci].ascendancies[ai].id.clone();
                let Some((start_id, member_groups)) = ascendancy_groups.get(&ascendancy_id) else {
                    eprintln!("pob tree: ascendancy group {} not found", ascendancy_id);
                    continue;
                };
                let Some(asc_node) = start_id.and_then(|id| nodes.get(&id)) else {
                    eprintln!("pob tree: ascendancy node {} not found", ascendancy_id);
                    continue;
                };
                classes[ci].ascendancies[ai].background = (cx, cy);
                let internal_id = classes[ci].ascendancies[ai].internal_id.clone();
                let inner = ascendancy_table
                    .by_id(&internal_id)
                    .map(|r| r.int(asc.distance) as f64)
                    .ok_or_else(|| format!("ascendancy {} not found", internal_id))?;
                let inner_x = cx + angle_to_centre.cos() * inner;
                let inner_y = cy + angle_to_centre.sin() * inner;
                let node_angle = orbit_angles
                    .get(asc_node.orbit as usize)
                    .and_then(|a| a.get(asc_node.orbit_index as usize))
                    .copied()
                    .ok_or_else(|| format!("ascendancy start {} sits off its orbit", ascendancy_id))?;
                let orbit_radius = ORBIT_RADII.get(asc_node.orbit as usize).copied().unwrap_or(0.0);
                let new_x = inner_x - node_angle.sin() * orbit_radius;
                let new_y = inner_y + node_angle.cos() * orbit_radius;
                let anchor = &groups[&asc_node.group];
                let (offset_x, offset_y) = (new_x - anchor.x, new_y - anchor.y);
                for gid in member_groups.clone() {
                    let Some(g) = groups.get_mut(&gid) else { continue };
                    g.x += offset_x;
                    g.y += offset_y;
                    min_x = min_x.min(g.x - hard_coded / 2.0);
                    min_y = min_y.min(g.y - hard_coded / 2.0);
                    max_x = max_x.max(g.x + hard_coded / 2.0);
                    max_y = max_y.max(g.y + hard_coded / 2.0);
                }
                j += 1.0;
            }
        }
    }

    for (from, to) in &replacements {
        let find = |name: &str| {
            classes.iter().enumerate().find_map(|(ci, c)| c.ascendancies.iter().position(|a| a.id == name).map(|ai| (ci, ai)))
        };
        let (Some((fc, fa)), Some((tc, ta))) = (find(from), find(to)) else {
            eprintln!("pob tree: replaced ascendancy {} or {} not found", from, to);
            continue;
        };
        classes[fc].ascendancies[fa].replace_by = Some(to.clone());
        classes[tc].ascendancies[ta].background = classes[fc].ascendancies[fa].background;
    }

    let mut tree = Table::new();
    tree.set("tree", "Default")
        .set("min_x", min_x)
        .set("min_y", min_y)
        .set("max_x", max_x)
        .set("max_y", max_y);
    let class_tables: Vec<Table> = classes
        .into_iter()
        .map(|class| {
            let mut table = class.table;
            let ascendancies: Vec<Table> = class
                .ascendancies
                .into_iter()
                .map(|a| {
                    let mut t = a.table;
                    let mut background = Table::new();
                    background
                        .set("image", format!("Classes{}", a.id))
                        .set("section", "AscendancyBackground")
                        .set("x", a.background.0)
                        .set("y", a.background.1)
                        .set("width", 1500)
                        .set("height", 1500);
                    t.set("background", background);
                    if let Some(by) = a.replace_by {
                        t.set("replaceBy", by);
                    }
                    t
                })
                .collect();
            table.set("ascendancies", Table::list(ascendancies));
            table
        })
        .collect();
    tree.set("classes", Table::list(class_tables));
    let mut group_table = Table::new();
    for (i, g) in groups {
        let mut t = Table::new();
        t.set("x", g.x).set("y", g.y).set("orbits", Table::list(g.orbits)).set("nodes", Table::list(g.nodes));
        group_table.set(i, t);
    }
    tree.set("groups", group_table);
    let mut node_table = Table::new();
    for (id, placed) in nodes {
        node_table.set(id, placed.table);
    }
    tree.set("nodes", node_table);
    tree.set("jewelSlots", Table::list(jewel_slot_ids));
    tree.set("constants", constants(&skills_per_orbit, &orbit_angles));
    let mut node_overlay = Table::new();
    node_overlay
        .set("Normal", Builder::overlay("PSSkillFrameActive", "PSSkillFrameHighlighted", "PSSkillFrame"))
        .set("Notable", Builder::overlay("NotableFrameAllocated", "NotableFrameCanAllocate", "NotableFrameUnallocated"))
        .set("Keystone", Builder::overlay("KeystoneFrameAllocated", "KeystoneFrameCanAllocate", "KeystoneFrameUnallocated"))
        .set("Socket", Builder::overlay("JewelFrameAllocated", "JewelFrameCanAllocate", "JewelFrameUnallocated"));
    tree.set("nodeOverlay", node_overlay);
    let mut connection = Table::new();
    connection.set("default", "Character").set("ascendancy", "CharacterAscendancy");
    tree.set("connectionArt", connection);
    Ok(Built { tree, sheets: b.sheets.into_inner(), orbit_art, rects: b.rects })
}

fn size_table(width: f64, height: f64) -> Table {
    let mut t = Table::new();
    t.set("width", width).set("height", height);
    t
}

/// `math.floor(num * 10^places + 0.5) / 10^places`.
fn round_to(value: f64, places: i32) -> f64 {
    let multiplier = 10f64.powi(places);
    (value * multiplier + 0.5).floor() / multiplier
}

/// `CalcOrbitAngles`: the fixed PoE 1 tables for 16 and 40 slots, otherwise
/// `n + 1` even steps from 0 to 360 degrees, in radians.
fn orbit_angles(slots: i64) -> Vec<f64> {
    let degrees: Vec<f64> = match slots {
        16 => vec![0.0, 30.0, 45.0, 60.0, 90.0, 120.0, 135.0, 150.0, 180.0, 210.0, 225.0, 240.0, 270.0, 300.0, 315.0, 330.0],
        40 => vec![
            0.0, 10.0, 20.0, 30.0, 40.0, 45.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 130.0, 135.0, 140.0, 150.0,
            160.0, 170.0, 180.0, 190.0, 200.0, 210.0, 220.0, 225.0, 230.0, 240.0, 250.0, 260.0, 270.0, 280.0, 290.0, 300.0,
            310.0, 315.0, 320.0, 330.0, 340.0, 350.0,
        ],
        n => (0..=n).map(|i| 360.0 * i as f64 / n as f64).collect(),
    };
    degrees.into_iter().map(f64::to_radians).collect()
}

fn constants(skills_per_orbit: &[i64], orbit_angles: &[Vec<f64>]) -> Table {
    let mut classes = Table::new();
    for (name, i) in [
        ("StrDexIntClass", 0),
        ("StrClass", 1),
        ("DexClass", 2),
        ("IntClass", 3),
        ("StrDexClass", 4),
        ("StrIntClass", 5),
        ("DexIntClass", 6),
    ] {
        classes.set(name, i);
    }
    let mut attributes = Table::new();
    attributes.set("Strength", 0).set("Dexterity", 1).set("Intelligence", 2);
    let mut t = Table::new();
    t.set("classes", classes)
        .set("characterAttributes", attributes)
        .set("PSSCentreInnerRadius", 130)
        .set("skillsPerOrbit", Table::list(skills_per_orbit.iter().copied()))
        .set("orbitAnglesByOrbit", Table::list(orbit_angles.iter().map(|a| Table::list(a.iter().copied()))))
        .set("orbitRadii", Table::list(ORBIT_RADII));
    t
}

struct AscCols {
    character: &'static str,
    disabled: &'static str,
    replace: Option<&'static str>,
    distance: &'static str,
}

impl AscCols {
    fn new(t: &LoadedTable) -> Result<Self, String> {
        Ok(Self {
            character: t.require(&["Character", "Class"])?,
            disabled: t.require(&["Disabled", "isDisabled"])?,
            replace: t.pick(&["BaseAscendancy", "Replace"]),
            distance: t.require(&["TreeRegionVector", "distanceTree"])?,
        })
    }
}

/// The per-node lookups the script does with `GetRow`, built once: first
/// match wins, as there.
struct Lookups {
    blight_results: HashMap<usize, usize>,
    blight_recipes: HashMap<usize, usize>,
    blight_recipes_table: Option<Rc<LoadedTable>>,
    class_overrides: HashMap<usize, Ref>,
    ascendancy_overrides: HashMap<usize, Ref>,
    ascendancy_by_name: HashMap<String, Ref>,
}

impl Lookups {
    fn new(b: &Builder) -> Result<Self, String> {
        let ctx = b.ctx;
        let first_by_key = |table: &Rc<LoadedTable>, col: &str| -> HashMap<usize, usize> {
            let mut map = HashMap::new();
            for row in table.rows() {
                if let Some(k) = row.key(col) {
                    map.entry(k).or_insert(row.index);
                }
            }
            map
        };
        let overrides = |name: &str| -> Result<HashMap<usize, Ref>, String> {
            let mut map = HashMap::new();
            if let Some(table) = ctx.optional_table(name) {
                let original = table.require(&["SkillToOverride", "OriginalNode"])?;
                for row in table.rows() {
                    if let Some(k) = row.key(original) {
                        map.entry(k).or_insert(Ref { table: Rc::clone(&table), index: row.index });
                    }
                }
            }
            Ok(map)
        };
        let blight_results_table = ctx.optional_table("BlightCraftingResults");
        let blight_recipes_table = ctx.optional_table("BlightCraftingRecipes");
        let blight_results = match &blight_results_table {
            Some(t) => first_by_key(t, t.require(&["PassiveSkill", "PassiveSkillsKey"])?),
            None => HashMap::new(),
        };
        let blight_recipes = match &blight_recipes_table {
            Some(t) => first_by_key(t, t.require(&["BlightCraftingResult", "BlightCraftingResultsKey"])?),
            None => HashMap::new(),
        };
        let ascendancies = ctx.table("Ascendancy")?;
        let mut ascendancy_by_name = HashMap::new();
        for row in ascendancies.rows() {
            ascendancy_by_name.entry(row.string("Name")).or_insert(Ref { table: Rc::clone(&ascendancies), index: row.index });
        }
        Ok(Self {
            blight_results,
            blight_recipes,
            blight_recipes_table,
            class_overrides: overrides("ClassPassiveSkillOverrides")?,
            ascendancy_overrides: overrides("AscendancyPassiveSkillOverrides")?,
            ascendancy_by_name,
        })
    }
}

/// Everything the script sets on a node that has a PassiveSkills row.
/// `None` when the node is left out of the tree.
#[allow(clippy::too_many_arguments)]
fn fill_node(
    b: &Builder,
    lookups: &Lookups,
    row: Row<'_>,
    id: u32,
    group: usize,
    replacements: &BTreeMap<String, String>,
    attributes: &[Table],
    node: &mut Table,
    is_ascendancy_group: &mut bool,
    ascendancy_groups: &mut HashMap<String, (Option<u32>, Vec<usize>)>,
    classes_start: &mut Vec<String>,
) -> Result<bool, String> {
    let c = &b.cols;
    let name = row.string(c.name);
    if name.is_empty() || (is_dnt(&name) && !KEPT_DNT.contains(&name.as_str())) {
        return Ok(false);
    }
    let icon = row.string(c.icon);
    node.set("name", escape_ggg_string(&name)).set("stringId", row.str(c.id)).set("icon", icon.as_str());
    let flavour = row.str(c.flavour);
    if !flavour.is_empty() {
        node.set("flavourText", flavour.replace('\r', "").replace('\n', "\\n"));
    }
    let (keystone, notable, only_image, socket) =
        (row.bool(c.keystone), row.bool(c.notable), row.bool(c.only_image), row.bool(c.jewel_socket));
    let (anoint_only, is_start, attribute) = (row.bool(c.anoint_only), row.bool(c.ascendancy_start), row.bool(c.attribute));
    let row_id = row.string(c.id);
    if keystone {
        node.set("isKeystone", true);
        b.add(SKILLS, &icon, "keystoneActive", None);
        b.add(SKILLS_DISABLED, &icon, "keystoneInactive", None);
    } else if notable {
        node.set("isNotable", true);
        b.add(SKILLS, &icon, "notableActive", None);
        b.add(SKILLS_DISABLED, &icon, "notableInactive", None);
    } else if only_image {
        node.set("isOnlyImage", true);
    } else if socket {
        node.set("isJewelSocket", true);
        b.add(SKILLS, &icon, "socketActive", None);
        b.add(SKILLS_DISABLED, &icon, "socketInactive", None);
    } else {
        b.add(SKILLS, &icon, "normalActive", None);
        b.add(SKILLS_DISABLED, &icon, "normalInactive", None);
    }
    if socket && anoint_only {
        node.set("aliasPassiveSocket", row_id.as_str()).set("sinister", true).set("noRadius", true);
    }
    let ascendancy = b.deref(row, c.ascendancy);
    let ascendancy_name = ascendancy.as_ref().map(|a| a.row().string("Name"));
    if let (Some(asc), Some(asc_name)) = (&ascendancy, &ascendancy_name) {
        *is_ascendancy_group = true;
        let disabled = asc.row().table.pick(&["Disabled", "isDisabled"]).is_some_and(|col| asc.row().bool(col));
        if is_dnt(asc_name) || disabled {
            return Ok(false);
        }
        node.set("ascendancyName", asc_name.as_str());
        if is_start {
            node.set("isAscendancyStart", true).set("name", asc_name.as_str());
        }
        if socket {
            node.set("containJewelSocket", true);
            let jewel = b.deref(asc.row(), "UIArt").and_then(|ui| b.deref(ui.row(), "JewelFrame"));
            if let Some(frame) = jewel {
                let [normal, active, can] = b.frames(&frame)?;
                b.add(GROUP_BACKGROUND, &normal, "frame", None);
                b.add(GROUP_BACKGROUND, &active, "frame", None);
                b.add(GROUP_BACKGROUND, &can, "frame", None);
                node.set("nodeOverlay", Builder::overlay(&active, &can, &normal));
            }
        } else if !only_image {
            let size = if notable { "Large" } else { "Small" };
            node.set(
                "nodeOverlay",
                Builder::overlay(
                    &format!("{}Frame{}Allocated", asc_name, size),
                    &format!("{}Frame{}CanAllocate", asc_name, size),
                    &format!("{}Frame{}Normal", asc_name, size),
                ),
            );
        }
        let entry = ascendancy_groups.entry(asc_name.clone()).or_default();
        if is_start {
            entry.0 = Some(id);
        }
        if !entry.1.contains(&group) {
            entry.1.push(group);
        }
    }
    if let Some(frame) = c.frame_art.and_then(|col| b.deref(row, col)) {
        if !only_image {
            let [normal, active, can] = b.frames(&frame)?;
            b.add(GROUP_BACKGROUND, &normal, "frame", None);
            b.add(GROUP_BACKGROUND, &active, "frame", None);
            b.add(GROUP_BACKGROUND, &can, "frame", None);
            node.set("nodeOverlay", Builder::overlay(&active, &can, &normal));
        }
    }
    let constraint = c.constraint.map(|col| b.ctx.rr.deref_list(row, col)).unwrap_or_default();
    if !constraint.is_empty() {
        let mut unlock = Table::new();
        if let Some(asc) = c.ascendancy_unlock.and_then(|col| b.deref(row, col)) {
            unlock.set("ascendancy", asc.row().string("Name"));
        }
        unlock.set("nodes", Table::list(constraint.iter().map(|r| r.row().int(c.graph_id))));
        node.set("unlockConstraint", unlock).set("connectionArt", "CharacterPlanned");
    }
    let mut stats = b.describe(row);
    if let Some(effect) = b.deref(row, c.mastery_group).and_then(|m| {
        let art = m.row().table.pick(&["Art", "MasteryArt"])?;
        b.deref(m.row(), art)
    }) {
        let image = effect.row().table.pick(&["ActiveEffectImage", "Effect"]).map(|col| effect.row().string(col)).unwrap_or_default();
        node.set("activeEffectImage", image.as_str());
        let path = b.ui_path(&image)?;
        b.add(MASTERY_ACTIVE_EFFECT, &path, "masteryActiveEffect", Some(&image));
    }
    if attribute {
        node.set("options", Table::list(attributes.iter().cloned())).set("isAttribute", true);
    }
    if let Some(gem) = b.deref(row, c.granted_skill) {
        let skill_name = b.deref(gem.row(), "BaseItemType").map(|r| r.row().string("Name")).unwrap_or_default();
        for _ in gem.row().list_keys("GemEffects") {
            stats.push(format!("Grants Skill: {}", skill_name));
        }
    }
    let points = row.int(c.points);
    if points > 0 {
        stats.push(format!("Grants {} Passive Skill Point", points));
    }
    let weapon_points = row.int(c.weapon_points);
    if weapon_points > 0 {
        stats.push(format!("{} Passive Skill Points become Weapon Set Skill Points", weapon_points));
    }
    node.set("stats", Table::list(stats));
    if let Some(&result) = lookups.blight_results.get(&row.index) {
        let mut recipe = Vec::new();
        if let (Some(&recipe_row), Some(recipes)) = (lookups.blight_recipes.get(&result), &lookups.blight_recipes_table) {
            let items_col = recipes.pick(&["BlightCraftingItems", "Recipe"]).unwrap_or("BlightCraftingItems");
            let recipe_row = recipes.row(recipe_row).expect("indexed row exists");
            for item in b.ctx.rr.deref_list(recipe_row, items_col) {
                let short = item.row().string("NameShort");
                let oil_col = item.row().table.pick(&["BaseItemType", "Oil"]).unwrap_or("BaseItemType");
                let dds = b
                    .deref(item.row(), oil_col)
                    .and_then(|base| b.deref(base.row(), "ItemVisualIdentity"))
                    .map(|v| v.row().string("DDSFile"))
                    .unwrap_or_default();
                b.add(OILS, &dds, "oil", Some(&short));
                recipe.push(short);
            }
        }
        node.set("recipe", Table::list(recipe));
    }
    if let Some(switch) = lookups.class_overrides.get(&row.index) {
        node.set("isSwitchable", true);
        let character = switch.row().table.pick(&["CharacterToOverrideFor", "Character"]).and_then(|col| b.deref(switch.row(), col));
        let switched = switch.row().table.pick(&["Override", "SwitchedNode"]).and_then(|col| b.deref(switch.row(), col));
        if let (Some(character), Some(switched)) = (character, switched) {
            let info = switched_info(b, switched.row());
            let mut options = Table::new();
            options.set(character.row().string("Name"), info);
            node.set("options", options);
        }
    }
    if let Some(replacement) = ascendancy_name.as_ref().and_then(|n| replacements.get(n)) {
        node.set("isSwitchable", true);
        let mut info = Table::new();
        info.set("ascendancyName", replacement.as_str());
        if let Some(switch) = lookups.ascendancy_overrides.get(&row.index) {
            let switched = switch.row().table.pick(&["Override", "SwitchedNode"]).and_then(|col| b.deref(switch.row(), col));
            if let Some(switched) = switched {
                for (key, value) in switched_info(b, switched.row()).iter() {
                    info.set(key.clone(), value.clone());
                }
            }
        }
        let replacement_row = lookups.ascendancy_by_name.get(replacement);
        let jewel = replacement_row.and_then(|r| b.deref(r.row(), "UIArt")).and_then(|ui| b.deref(ui.row(), "JewelFrame"));
        match jewel {
            Some(frame) if socket => {
                let [normal, active, can] = b.frames(&frame)?;
                b.add(GROUP_BACKGROUND, &normal, "frame", None);
                b.add(GROUP_BACKGROUND, &active, "frame", None);
                b.add(GROUP_BACKGROUND, &can, "frame", None);
                info.set("nodeOverlay", Builder::overlay(&active, &can, &normal));
            }
            _ if !only_image => {
                info.set(
                    "nodeOverlay",
                    Builder::overlay(
                        &format!("{}FrameSmallAllocated", replacement),
                        &format!("{}FrameSmallCanAllocate", replacement),
                        &format!("{}FrameSmallNormal", replacement),
                    ),
                );
            }
            _ => {}
        }
        let mut options = Table::new();
        options.set(replacement.as_str(), info);
        node.set("options", options);
    }
    let starts = b.ctx.rr.deref_list(row, c.class_start);
    if !starts.is_empty() {
        let names: Vec<String> = starts.iter().map(|r| r.row().string("Name")).filter(|n| !is_dnt(n)).collect();
        classes_start.extend(names.iter().cloned());
        node.set("classesStart", Table::list(names));
    }
    if row.bool(c.multiple_choice) {
        node.set("isMultipleChoice", true);
    }
    if row.bool(c.multiple_choice_option) {
        node.set("isMultipleChoiceOption", true);
    }
    if c.free.is_some_and(|col| row.bool(col)) {
        node.set("isFreeAllocate", true);
    }
    if c.apply_to_armour.is_some_and(|col| row.bool_at(col)) {
        node.set("applyToArmour", true);
    }
    Ok(true)
}

/// The `nodeInfo` of a switched node: its id, name, icon and stat lines, with
/// its icon added to both skill sheets.
fn switched_info(b: &Builder, switched: Row<'_>) -> Table {
    let c = &b.cols;
    let icon = switched.string(c.icon);
    let mut info = Table::new();
    info.set("id", switched.int(c.graph_id)).set("name", switched.str(c.name)).set("icon", icon.as_str());
    let stats = b.describe(switched);
    info.set("stats", Table::list(stats));
    b.add(SKILLS, &icon, "normalActive", None);
    b.add(SKILLS_DISABLED, &icon, "normalInactive", None);
    info
}

/// `calculateDDSPack`: textures grouped by size and format, each group one
/// stacked file in `dir`, and every picture's layer in it (with its
/// rectangle, where the UI sheet index gives one).
fn pack(ctx: &Ctx, sheet: &Sheet, rects: &HashMap<String, Option<[f64; 4]>>, dir: &Path) -> Result<Vec<(String, Table)>, String> {
    if sheet.files.is_empty() {
        return Ok(Vec::new());
    }
    let paths: Vec<String> = sheet.files.keys().cloned().collect();
    let bytes = ctx.files.fetch_many(&paths);
    let mut stacks: BTreeMap<String, Vec<(&String, dds::Texture)>> = BTreeMap::new();
    for path in &paths {
        let texture = bytes.get(path).map(|b| payload(ctx, b)).ok_or_else(|| "not in this install".to_string()).and_then(|b| dds::read(&b));
        match texture {
            Ok(texture) => {
                let ident = format!("{}_{}_{}", texture.width, texture.height, texture.format.name());
                stacks.entry(ident).or_default().push((path, texture));
            }
            Err(e) => eprintln!("pob tree: {} left out of {}: {}", path, sheet.name, e),
        }
    }
    let mut out = Vec::new();
    for (ident, layers) in stacks {
        let file = format!("{}_{}.dds.zst", sheet.name, ident);
        let mut coords = Table::new();
        for (position, (path, _)) in layers.iter().enumerate() {
            let position = (position + 1) as f64;
            let rect = rects.get(&path.to_ascii_lowercase()).copied().flatten();
            for (_, alias) in &sheet.files[*path] {
                let key = alias.clone().unwrap_or_else(|| (*path).clone());
                match rect {
                    Some([x, y, w, h]) => coords.set(key, Table::list([x, y, w, h, position])),
                    None => coords.set(key, position),
                };
            }
        }
        let textures: Vec<&dds::Texture> = layers.iter().map(|(_, t)| t).collect();
        let packed = dds::stack(&textures).map_err(|e| format!("{}: {}", file, e))?;
        write_file(&dir.join(&file), &packed)?;
        out.push((file, coords));
    }
    Ok(out)
}

/// A texture's bytes, following the `*path` redirect some files hold.
fn payload(ctx: &Ctx, bytes: &[u8]) -> Vec<u8> {
    if let Some(target) = bytes.strip_prefix(b"*") {
        let target = String::from_utf8_lossy(target).trim().to_string();
        if let Some(b) = crate::dat::relational::FileSource::fetch(ctx.files, &target) {
            return b;
        }
    }
    bytes.to_vec()
}
