//! `world_areas.json` — every area with its level, connections, monster packs
//! and the terrain topologies it is built from.

use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;
use crate::dat::relational::Row;
use std::collections::HashMap;

pub fn world_areas(ctx: &Ctx) -> Result<(), String> {
    let areas = ctx.table("WorldAreas")?;
    let packs = ctx.optional_table("MonsterPacks");
    let packs_by_area = packs_by_area(ctx);
    let entries_by_pack = entries_by_pack(ctx);

    let root = areas
        .rows()
        .map(|area| {
            let pack_rows = packs_by_area.get(&area.index).map(Vec::as_slice).unwrap_or_default();
            let pack_json: Vec<J> = pack_rows
                .iter()
                .filter_map(|&index| packs.as_ref()?.row(index))
                .map(|pack| pack_json(ctx, pack, &entries_by_pack))
                .collect();

            let topologies: Vec<J> = ctx
                .rr
                .deref_list(area, "Topologies")
                .iter()
                .map(|t| {
                    let t = t.row();
                    Obj::new()
                        .set("file", text(t.str("DGRFile")))
                        .set("id", text(t.id()))
                        .set("unknown", unnamed_columns(t))
                        .build()
                })
                .collect();

            let entry = Obj::new()
                .set("act", int(area.int("Act")))
                .set("area_level", int(area.int("AreaLevel")))
                .set("area_mods", json::strings(ctx.rr.deref_list_ids(area, "AreaMods")))
                .set("bosses", json::strings(ctx.rr.deref_list_ids(area, "Bosses_MonsterVarietiesKeys")))
                .set("connections", json::strings(ctx.rr.deref_list_ids(area, "Connections")))
                .or_null("environment", ctx.rr.deref_id(area, "Environment").map(text))
                .set("has_waypoint", J::Bool(area.bool("HasWaypoint")))
                .set("id", text(area.id()))
                .set("is_town", J::Bool(area.bool("IsTown")))
                .set("loading_screens", json::strings(area.list_str("LoadingScreens")))
                .set("name", text(area.str("Name")))
                .set("tags", json::strings(ctx.rr.deref_list_ids(area, "Tags")))
                .or_null("topologies", (!topologies.is_empty()).then(|| J::Arr(topologies)))
                .or_null("area_type_tags", None)
                .or_null("parent_town", ctx.rr.deref_id(area, "ParentTown").map(text))
                .or_null("packs", (!pack_json.is_empty()).then(|| J::Arr(pack_json)))
                .or_null("terrain_plugins", ctx.rr.deref_id(area, "TerrainPlugins").map(text))
                .build();
            (area.id().to_string(), entry)
        })
        .collect();

    ctx.write("world_areas", &J::Obj(root))
}

fn pack_json(ctx: &Ctx, pack: Row<'_>, entries: &HashMap<usize, Vec<usize>>) -> J {
    let entry_table = ctx.optional_table("MonsterPackEntries");
    let monsters: Vec<(String, J)> = entries
        .get(&pack.index)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|&index| entry_table.as_ref()?.row(index))
        .map(|row| {
            (
                row.id().to_string(),
                Obj::new()
                    .or_null("monster_variety", ctx.rr.deref_id(row, "MonsterVarietiesKey").map(text))
                    .set("flag", J::Bool(row.bool("Flag")))
                    .set("weight", int(row.int("Weight")))
                    .build(),
            )
        })
        .collect();

    let counts = pack.list_int("AdditionalCounts");
    let additional: Vec<(String, J)> = ctx
        .rr
        .deref_list(pack, "AdditionalMonsters")
        .iter()
        .enumerate()
        .map(|(i, monster)| {
            (monster.id(), Obj::new().set("count", int(counts.get(i).copied().unwrap_or(0))).build())
        })
        .collect();

    let spawn_chance = pack.int("BossMonsterSpawnChance");
    Obj::new()
        .or_null("additional_monsters", (!additional.is_empty()).then(|| J::Obj(additional)))
        .set("boss_chance", int(spawn_chance))
        .set("boss_count", int(pack.int("BossCount")))
        .set("boss_monster_spawn_chance", int(spawn_chance))
        .set("boss_monsters", json::strings(ctx.rr.deref_list_ids(pack, "BossMonsters")))
        .set("id", text(pack.id()))
        .set("max_count", int(pack.int("MaxCount")))
        .set("min_count", int(pack.int("MinCount")))
        .or_null("monsters", (!monsters.is_empty()).then(|| J::Obj(monsters)))
        .set("tags", json::strings(ctx.rr.deref_list_ids(pack, "Tags")))
        .or_null("formation", ctx.rr.deref_id(pack, "PackFormation").map(text))
        .build()
}

/// The columns the community schema has not named yet, in column order. They
/// carry the numbers a topology is laid out with, and are reported as-is so
/// nothing is lost while the schema catches up.
fn unnamed_columns(row: Row<'_>) -> J {
    use crate::dat::reader::DatValue;
    let cells = row.table.def.columns.iter().enumerate().filter(|(_, c)| c.name.is_none()).map(|(i, column)| {
        match row.cell(i) {
            Some(DatValue::Int(v)) => int(*v),
            Some(DatValue::Long(v)) => int(*v as i64),
            Some(DatValue::Bool(b)) => J::Bool(*b),
            Some(DatValue::Float(f)) => json::float(*f),
            Some(DatValue::String(s)) => text(s),
            Some(DatValue::List(count, offset)) => {
                let items = row.table.reader.read_list_values(*offset, *count, column).unwrap_or_default();
                J::Arr(
                    items
                        .iter()
                        .map(|v| match v {
                            DatValue::Int(v) => int(*v),
                            DatValue::Float(f) => json::float(*f),
                            DatValue::String(s) => text(s),
                            _ => J::Null,
                        })
                        .collect(),
                )
            }
            _ => J::Null,
        }
    });
    J::Arr(cells.collect())
}

fn packs_by_area(ctx: &Ctx) -> HashMap<usize, Vec<usize>> {
    let Some(table) = ctx.optional_table("MonsterPacks") else { return HashMap::new() };
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for row in table.rows() {
        for area in row.list_keys("WorldAreas") {
            out.entry(area).or_default().push(row.index);
        }
    }
    out
}

fn entries_by_pack(ctx: &Ctx) -> HashMap<usize, Vec<usize>> {
    let Some(table) = ctx.optional_table("MonsterPackEntries") else { return HashMap::new() };
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for row in table.rows() {
        if let Some(pack) = row.key("MonsterPacksKey") {
            out.entry(pack).or_default().push(row.index);
        }
    }
    out
}
