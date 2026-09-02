//! World-space layout of a `.psg` graph the way the game lays it out.
//!
//! The psg stores the main tree in final coordinates (verified: every
//! non-ascendancy group matches the official web-tree export to <0.01 units)
//! but "parks" each ascendancy cluster far outside the tree. The client moves
//! each enabled ascendancy onto an outer ring of illustration plates:
//! radius [`ASCENDANCY_RING_RADIUS`], one slot every [`ASCENDANCY_SLOT_STEP_DEG`]
//! degrees, slots grouped by `Ascendancy.TreeRegionAngle` (the class direction)
//! in `Ascendancy` table order and centred on that direction. The cluster is
//! translated so its start group lands on the plate centre.

use crate::dat::psg::PsgFile;
use crate::ui::atlas_node_db::SkillGraphDatabase;
use eframe::egui::{pos2, vec2, Pos2, Vec2};
use std::collections::HashMap;

pub const ASCENDANCY_RING_RADIUS: f32 = 15537.0;
pub const ASCENDANCY_SLOT_STEP_DEG: f32 = 12.0;

/// World units per pixel of `PassiveTreeMainCircle`: the client draws the
/// 4000 px texture one pixel per unit (the official web export tags every
/// sheet at half scale for the same reason).
pub const RING_PX: f32 = 1.0;
pub const MAIN_CIRCLE_SIZE: f32 = 4000.0 * RING_PX;

/// The ring texture carries the six class-start roundels, each a mount
/// around a transparent hole centred at this texture radius (alpha profile
/// along a class direction: mount 1396–1492, hole 1504–1624, mount
/// 1636–1780). The class-start nodes themselves sit ~1440–1490 units from the
/// centre, so the client starts their connectors at the roundel, not at the
/// node's stored position (in game the first nodes are ~180 units from the
/// quatrefoil, which only the roundel radius reproduces).
pub const CLASS_START_RING_RADIUS: f32 = 1564.0 * RING_PX;

/// `Characters`/`Ascendancy.PassiveTreeImage` are 1500 px textures holding a
/// circle inscribed to the edges (corners are transparent), drawn at two
/// units per pixel like the ring; the art reaches the ring's outer rim and
/// shows dimly through the ring's soft inner vignette, as in game.
pub const CLASS_ILLUSTRATION_SIZE: f32 = 3000.0 * RING_PX;
pub const ASCENDANCY_PLATE_SIZE: f32 = CLASS_ILLUSTRATION_SIZE;

/// Where a class start's connectors begin: the roundel on the ring along
/// the node's own direction.
pub fn class_start_anchor(node_pos: Pos2) -> Pos2 {
    let v = node_pos.to_vec2();
    let len = v.length();
    if len <= 0.0 {
        return node_pos;
    }
    (v / len * CLASS_START_RING_RADIUS).to_pos2()
}

/// The roundel's ornate mount is opaque from about 72 px out to 170 px from
/// the hole centre; connectors to the first nodes stop under it, so they
/// never cross the quatrefoil.
pub const CLASS_START_MOUNT_RADIUS: f32 = 130.0 * RING_PX;

/// End point of the connector between a class start and `other`: on the
/// line from the roundel to `other`, just inside the mount.
pub fn class_start_line_end(node_pos: Pos2, other: Pos2) -> Pos2 {
    let anchor = class_start_anchor(node_pos);
    let d = other - anchor;
    let len = d.length();
    if len <= CLASS_START_MOUNT_RADIUS {
        return anchor;
    }
    anchor + d / len * CLASS_START_MOUNT_RADIUS
}

/// Ascendancies whose start group is not at the plate centre in the official
/// export (multi-group layouts placed by hand). Everything else is within
/// ~10 units of the centre, which is the start node's own orbit offset.
const START_OFFSET_OVERRIDES: &[(&str, f32, f32)] = &[
    ("Druid1", -1150.0, -673.0),
    ("Druid2", -1150.0, -673.0),
    ("Huntress1", 555.0, 68.0),
    ("Huntress2", 110.0, 386.0),
    ("Huntress3", 555.0, 68.0),
    ("Mercenary1", 2.0, 486.0),
    ("Monk1", 338.0, -444.0),
    ("Warrior2", -421.0, 242.0),
    ("Sorceress3", 0.0, -486.0),
];

/// Offset from an ascendancy's start group to the centre of its plate.
pub fn plate_nudge(ascendancy_id: &str) -> Vec2 {
    START_OFFSET_OVERRIDES
        .iter()
        .find(|(id, _, _)| *id == ascendancy_id)
        .map(|(_, x, y)| vec2(*x, *y))
        .unwrap_or(Vec2::ZERO)
}

#[derive(Debug, Clone)]
pub struct Plate {
    pub ascendancy: usize,
    pub center: Pos2,
}

#[derive(Debug, Default)]
pub struct TreeLayout {
    /// Translation applied to each psg group (zero for the main tree).
    pub group_offset: Vec<Vec2>,
    /// Groups belonging to disabled / unused ascendancies are not drawn.
    pub group_hidden: Vec<bool>,
    pub group_ascendancy: Vec<Option<usize>>,
    pub plates: Vec<Plate>,
    pub node_pos: HashMap<u32, Pos2>,
    pub node_group: HashMap<u32, usize>,
    /// Ascendancy row index -> ring angle (degrees, clockwise from north).
    pub slots: HashMap<usize, f32>,
}

/// Angle of a node on its orbit, clockwise from north. PoE 2 orbits are
/// evenly spaced: theta = position / capacity * 2pi.
pub fn orbit_angle(orbit: u32, position: u32, passives_per_orbit: &[u8]) -> f32 {
    let capacity = passives_per_orbit.get(orbit as usize).copied().unwrap_or(12) as f32;
    if capacity <= 0.0 {
        return 0.0;
    }
    position as f32 / capacity * std::f32::consts::TAU
}

/// Point at `radius` along `angle_deg` (clockwise from north, y down).
pub fn polar(radius: f32, angle_deg: f32) -> Pos2 {
    let a = angle_deg.to_radians();
    pos2(radius * a.sin(), -radius * a.cos())
}

/// Ring slot angle for every enabled ascendancy (variants share their base's slot).
pub fn ascendancy_slots(db: &SkillGraphDatabase) -> HashMap<usize, f32> {
    let mut by_dir: HashMap<i32, Vec<usize>> = HashMap::new();
    for (i, a) in db.ascendancies.iter().enumerate() {
        if a.is_enabled() {
            by_dir.entry(a.tree_region_angle).or_default().push(i);
        }
    }
    let mut slots = HashMap::new();
    for (dir, mut members) in by_dir {
        // Variants keep table order but go after the base ascendancies.
        members.sort_by_key(|&i| (db.ascendancies[i].base_ascendancy.is_some(), i));
        let n = members.len() as f32;
        for (k, &i) in members.iter().enumerate() {
            let angle = dir as f32 + (k as f32 - (n - 1.0) / 2.0) * ASCENDANCY_SLOT_STEP_DEG;
            slots.insert(i, angle.rem_euclid(360.0));
        }
    }
    for (i, a) in db.ascendancies.iter().enumerate() {
        if let Some(base) = a.base_ascendancy {
            if let Some(&angle) = slots.get(&base) {
                slots.insert(i, angle);
            }
        }
    }
    slots
}

pub fn compute(psg: &PsgFile, db: Option<&SkillGraphDatabase>) -> TreeLayout {
    let n = psg.groups.len();
    let mut layout = TreeLayout {
        group_offset: vec![Vec2::ZERO; n],
        group_hidden: vec![false; n],
        group_ascendancy: vec![None; n],
        ..Default::default()
    };
    let radii = psg.orbit_radii();

    if let Some(db) = db {
        if psg.graph_type == 0 {
            for (gi, group) in psg.groups.iter().enumerate() {
                layout.group_ascendancy[gi] = group
                    .nodes
                    .iter()
                    .find_map(|n| db.nodes.get(&n.skill_id).and_then(|i| i.ascendancy));
            }
            layout.slots = ascendancy_slots(db);

            // Start group per ascendancy.
            let mut start_group: HashMap<usize, usize> = HashMap::new();
            for (gi, group) in psg.groups.iter().enumerate() {
                for node in &group.nodes {
                    if let Some(info) = db.nodes.get(&node.skill_id) {
                        if info.is_ascendancy_start {
                            if let Some(a) = info.ascendancy {
                                start_group.entry(a).or_insert(gi);
                            }
                        }
                    }
                }
            }

            let mut translation: HashMap<usize, Vec2> = HashMap::new();
            for (&asc, &gi) in &start_group {
                let a = &db.ascendancies[asc];
                let Some(&angle) = layout.slots.get(&asc) else { continue };
                let center = polar(ASCENDANCY_RING_RADIUS, angle);
                let nudge = plate_nudge(&a.id);
                let origin = pos2(psg.groups[gi].x, psg.groups[gi].y);
                translation.insert(asc, (center + nudge) - origin);
                if a.base_ascendancy.is_none() {
                    layout.plates.push(Plate { ascendancy: asc, center });
                }
            }

            for gi in 0..n {
                if let Some(asc) = layout.group_ascendancy[gi] {
                    match translation.get(&asc) {
                        Some(t) if db.ascendancies[asc].is_enabled() => layout.group_offset[gi] = *t,
                        _ => layout.group_hidden[gi] = true,
                    }
                }
            }
        }
    }

    for (gi, group) in psg.groups.iter().enumerate() {
        if group.is_proxy {
            continue;
        }
        let origin = pos2(group.x, group.y) + layout.group_offset[gi];
        for node in &group.nodes {
            let r = radii.get(node.radius as usize).copied().unwrap_or(node.radius as f32 * 50.0);
            let theta = orbit_angle(node.radius, node.position, &psg.passives_per_orbit);
            let pos = pos2(origin.x + theta.sin() * r, origin.y - theta.cos() * r);
            layout.node_pos.insert(node.skill_id, pos);
            layout.node_group.insert(node.skill_id, gi);
        }
    }
    layout
}

impl TreeLayout {
    pub fn is_node_hidden(&self, skill_id: u32) -> bool {
        self.node_group.get(&skill_id).map(|&g| self.group_hidden[g]).unwrap_or(false)
    }

    pub fn node_ascendancy(&self, skill_id: u32) -> Option<usize> {
        self.node_group.get(&skill_id).and_then(|&g| self.group_ascendancy[g])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polar_is_clockwise_from_north() {
        let p = polar(10.0, 0.0);
        assert!((p.x).abs() < 1e-4 && (p.y + 10.0).abs() < 1e-4);
        let p = polar(10.0, 90.0);
        assert!((p.x - 10.0).abs() < 1e-4 && p.y.abs() < 1e-4);
        let p = polar(10.0, 240.0);
        assert!(p.x < 0.0 && p.y > 0.0);
    }
}

#[cfg(test)]
mod real_data_tests {
    use super::*;
    use std::sync::Arc;

    /// Lays out the real character tree and checks every enabled ascendancy
    /// got a ring slot. `cargo test --release -- --ignored real_tree_layout --nocapture`
    #[test]
    #[ignore]
    fn real_tree_layout() {
        let settings = crate::settings::AppSettings::load();
        let ggpk_path = settings.ggpk_path.expect("no ggpk_path configured");
        let reader = Arc::new(crate::ggpk::reader::GgpkReader::open(&ggpk_path).unwrap());
        let cache_path = crate::settings::AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        let index = crate::bundles::index::Index::load_from_cache(&cache_path).expect("run the app once to build the index cache");
        let schema_text = std::fs::read_to_string(crate::settings::AppSettings::get_app_data_dir().join("schema.min.json")).unwrap();
        let schema: crate::dat::schema::Schema = serde_json::from_str(&schema_text).unwrap();
        let db = crate::ui::content_view::build_skill_graph_db(Some(&reader), &index, None, &schema).unwrap();

        let fi = index.files.values().find(|f| f.path.eq_ignore_ascii_case("metadata/passiveskillgraph.psg")).unwrap();
        let bytes = crate::ui::content_view::extract_bundle_file_sync(fi, &index, Some(&reader), None).unwrap();
        let psg = crate::dat::psg::parse_psg(&bytes).unwrap();
        let layout = compute(&psg, Some(&db));

        println!("characters: {:?}", db.playable_characters().iter().map(|&c| db.characters[c].name.clone()).collect::<Vec<_>>());
        let mut plates: Vec<_> = layout.plates.iter().map(|p| (layout.slots[&p.ascendancy], db.ascendancies[p.ascendancy].id.clone(), db.ascendancies[p.ascendancy].name.clone(), p.center)).collect();
        plates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for (angle, id, name, c) in &plates {
            println!("{:6.1}  {:11} {:22} ({:8.0},{:8.0})", angle, id, name, c.x, c.y);
        }
        let enabled = db.ascendancies.iter().filter(|a| a.is_enabled() && a.base_ascendancy.is_none()).count();
        assert_eq!(plates.len(), enabled, "every enabled ascendancy should get a plate");
        let hidden = layout.group_hidden.iter().filter(|h| **h).count();
        println!("hidden groups (legacy ascendancies): {}", hidden);
        assert!(hidden > 0);
        // Start node of each plated ascendancy sits near its plate.
        for p in &layout.plates {
            let start = db.nodes.iter().find(|(_, i)| i.is_ascendancy_start && i.ascendancy == Some(p.ascendancy)).map(|(id, _)| *id).unwrap();
            let pos = layout.node_pos[&start];
            let d = (pos - p.center).length();
            assert!(d < 1500.0, "{} start node {} units from its plate", db.ascendancies[p.ascendancy].id, d);
        }
        let paths = crate::ui::content_view::collect_needed_texture_paths(&psg, &db);
        let missing: Vec<_> = paths.iter().filter(|p| crate::ui::content_view::resolve_texture_path(&index, p).is_none()).cloned().collect();
        println!("texture paths: {} needed, {} unresolved", paths.len(), missing.len());
        for m in missing.iter().take(20) {
            println!("   missing {}", m);
        }
    }
}
