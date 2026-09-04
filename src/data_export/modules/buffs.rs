//! `buffs.json`, `buff_visuals.json` and `audio.json`.

use crate::data_export::json::{self, int, text, Obj, J};
use crate::data_export::Ctx;
use std::collections::HashMap;

/// Buff category numbers the client shows a label for. Rows carrying a number
/// past the end of this list are uncategorised, and new categories keep being
/// appended, so an unknown number is left off rather than guessed at.
const CATEGORIES: [&str; 17] = [
    "Buff",
    "Debuff",
    "Charge",
    "Flask",
    "Hex",
    "Active skill",
    "Buff shrine",
    "PVP flag",
    "Spell shrine",
    "PVP team",
    "Labyrinth trap",
    "Aspect",
    "Herald",
    "Mark",
    "Stolen",
    "Link",
    "Charm",
];

pub fn buffs(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("BuffDefinitions")?;
    let stats_column = table.require(&["Stats", "StatsKeys"])?;
    let visual_column = table.require(&["BuffVisual", "BuffVisualsKey"])?;
    let templates = ctx.optional_table("BuffTemplates");
    let by_definition = group_by_key(ctx, "BuffTemplates", "BuffDefinition");
    let template_users = template_users(ctx);
    let translations = ctx.translations("stat_descriptions");

    let root = table
        .rows()
        .map(|row| {
            let mut stats = ctx.rr.deref_list_ids(row, stats_column);
            let flags = ctx.rr.deref_list_ids(row, "GrantedFlags");
            let flag_count = flags.len();
            stats.extend(flags);
            let category = usize::try_from(row.int("BuffCategory"))
                .ok()
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| CATEGORIES.get(i))
                .copied();

            // Every template built on this buff, and whatever grants them.
            let mut by_source: Vec<(&str, Vec<J>)> = Vec::new();
            let mut built: Vec<(String, J)> = Vec::new();
            for &index in by_definition.get(&row.index).map(Vec::as_slice).unwrap_or_default() {
                let Some(template) = templates.as_ref().and_then(|t| t.row(index)) else { continue };
                for (table_name, users) in template_users.get(&index).map(Vec::as_slice).unwrap_or_default() {
                    let entry = Obj::new()
                        .set("id", text(users))
                        .or_null("item", None)
                        .or_null("name", None)
                        .set("template", text(template.id()))
                        .or_null("stat_text", None)
                        .or_null("stats", None)
                        .build();
                    match by_source.iter_mut().find(|(name, _)| name == table_name) {
                        Some((_, list)) => list.push(entry),
                        None => by_source.push((table_name, vec![entry])),
                    }
                }

                // The template supplies a value per stat, then a 1 for each
                // granted flag. Stats past the end of that list are not part
                // of this template and are left out rather than invented.
                let mut values = template.list_int("Buff_StatValues");
                values.extend(std::iter::repeat(1).take(flag_count));
                let covered: Vec<String> = stats.iter().take(values.len()).cloned().collect();
                let paired: Vec<(String, J)> =
                    covered.iter().zip(&values).map(|(id, v)| (id.clone(), int(*v))).collect();
                let ranges: Vec<(i32, i32)> =
                    values.iter().take(covered.len()).map(|&v| (v as i32, v as i32)).collect();
                let lines = translations.translate_ranges(&covered, &ranges);
                let radius = template.int("AuraRadius");
                built.push((
                    template.id().to_string(),
                    Obj::new()
                        .or_null("aura_radius_metres", (radius != 0).then(|| J::Num(radius as f64 / 10.0)))
                        .or_null("stats", (!paired.is_empty()).then(|| J::Obj(paired)))
                        .or_null("stat_text", (!lines.is_empty()).then(|| json::strings(&lines)))
                        .or_null("visuals", ctx.rr.deref_id(template, "BuffVisual").map(text))
                        .build(),
                ));
            }

            let entry = Obj::new()
                .set("description", text(row.str("Description")))
                .set("invisible", J::Bool(row.bool("Invisible")))
                .set("name", text(row.str("Name")))
                .set("removable", J::Bool(row.bool("Removable")))
                .set("stats", json::strings(&stats))
                .set("visuals", text(ctx.rr.deref_id(row, visual_column).unwrap_or_default()))
                .or_null("category", category.map(text))
                .or_null(
                    "sources",
                    (!by_source.is_empty())
                        .then(|| J::Obj(by_source.into_iter().map(|(k, v)| (k.to_string(), J::Arr(v))).collect())),
                )
                .or_null("templates", (!built.is_empty()).then(|| J::Obj(built)))
                .or_null("stack_limit", Some(row.int("BuffLimit")).filter(|v| *v != 0).map(int))
                .build();
            (row.id().to_string(), entry)
        })
        .collect();

    ctx.write("buffs", &J::Obj(root))
}

/// Row indices of `table` grouped by the row `column` points at.
fn group_by_key(ctx: &Ctx, table: &str, column: &str) -> HashMap<usize, Vec<usize>> {
    let Some(table) = ctx.optional_table(table) else { return HashMap::new() };
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for row in table.rows() {
        if let Some(target) = row.key(column) {
            out.entry(target).or_default().push(row.index);
        }
    }
    out
}

/// What grants each buff template: the mod, passive or league modifier that
/// names it, keyed by the template row.
fn template_users(ctx: &Ctx) -> HashMap<usize, Vec<(&'static str, String)>> {
    let mut out: HashMap<usize, Vec<(&'static str, String)>> = HashMap::new();
    // (table, column, whether the column holds a list)
    let sources: [(&'static str, &str, bool); 3] = [
        ("Mods", "BuffTemplate", false),
        ("PassiveSkills", "PassiveSkillBuffs", true),
        ("UltimatumModifiers", "BuffTemplates", true),
    ];
    for (name, column, is_list) in sources {
        let Some(table) = ctx.optional_table(name) else { continue };
        if !table.has_col(column) {
            continue;
        }
        for row in table.rows() {
            let targets =
                if is_list { row.list_keys(column) } else { row.key(column).into_iter().collect() };
            for target in targets {
                out.entry(target).or_default().push((name, row.id().to_string()));
            }
        }
    }
    out
}

/// `buff_visuals.json`: each visual with its icon and name, and everything
/// that puts it on screen.
pub fn buff_visuals(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("BuffVisuals")?;
    let definitions = ctx.optional_table("BuffDefinitions");
    let templates = ctx.optional_table("BuffTemplates");
    let by_definition = group_by_key(ctx, "BuffDefinitions", "BuffVisual");
    let by_template = group_by_key(ctx, "BuffTemplates", "BuffVisual");

    let root = table
        .rows()
        .map(|row| {
            let mut sources: Vec<(&str, Vec<J>)> = Vec::new();
            let push = |name: &'static str, entries: Vec<J>, sources: &mut Vec<(&str, Vec<J>)>| {
                if !entries.is_empty() {
                    sources.push((name, entries));
                }
            };
            let from_definitions: Vec<J> = by_definition
                .get(&row.index)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|&i| definitions.as_ref()?.row(i))
                .map(|buff| visual_source(ctx, buff, None))
                .collect();
            push("BuffDefinitions", from_definitions, &mut sources);

            let from_templates: Vec<J> = by_template
                .get(&row.index)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|&i| templates.as_ref()?.row(i))
                .map(|template| {
                    let buff = ctx.rr.deref(template, "BuffDefinition");
                    visual_source(ctx, template, buff.as_ref().map(|b| b.row()))
                })
                .collect();
            push("BuffTemplates", from_templates, &mut sources);

            super::images::export(ctx, row.str("BuffDDSFile"), None);
            export_frame(ctx, row.str("ExtraArt"));

            let entry = Obj::new()
                .or_null("description", json::opt_text(row.str("BuffDescription")))
                .or_null("icon", json::opt_text(row.str("BuffDDSFile")))
                .or_null("name", json::opt_text(row.str("BuffName")))
                .or_null(
                    "sources",
                    (!sources.is_empty())
                        .then(|| J::Obj(sources.into_iter().map(|(k, v)| (k.to_string(), J::Arr(v))).collect())),
                )
                .or_null("sounds", None)
                .or_null("custom_frame", json::opt_text(row.str("ExtraArt")))
                .build();
            (row.id().to_string(), entry)
        })
        .collect();
    ctx.write("buff_visuals", &J::Obj(root))
}

/// A custom buff frame is a rectangle of a shared UI sheet rather than a file
/// of its own, so it is cut out before being written.
fn export_frame(ctx: &Ctx, name: &str) {
    if !ctx.options.images || name.is_empty() {
        return;
    }
    let sheet = ctx.ui_images();
    let Some(image) = sheet.get(name) else { return };
    super::images::export_as(
        ctx,
        &image.source,
        name,
        Some(super::images::Compose::Crop { x1: image.x1, y1: image.y1, x2: image.x2, y2: image.y2 }),
    );
}

/// One thing that shows a visual. A template borrows its name, description and
/// category from the buff it is built on.
fn visual_source(ctx: &Ctx, row: crate::dat::relational::Row<'_>, buff: Option<crate::dat::relational::Row<'_>>) -> J {
    let own_category = row
        .table
        .has_col("BuffCategory")
        .then(|| category_name(row.int("BuffCategory")))
        .flatten();
    let category = buff.and_then(|b| category_name(b.int("BuffCategory"))).or(own_category);
    let name = json::opt_text(row.str("Name")).or_else(|| buff.and_then(|b| json::opt_text(b.str("Name"))));
    let description = json::opt_text(row.str("Description"))
        .or_else(|| buff.and_then(|b| json::opt_text(b.str("Description"))));
    Obj::new()
        .set("id", text(row.id()))
        .or_null("buff_id", buff.map(|b| text(b.id())))
        .or_null("buff_category", category.map(text))
        .or_null("item", ctx.rr.deref_id(row, "BaseType").map(text))
        .or_null("description", description)
        .or_null("name", name)
        .build()
}

fn category_name(value: i64) -> Option<&'static str> {
    usize::try_from(value).ok()?.checked_sub(1).and_then(|i| CATEGORIES.get(i)).copied()
}

pub fn audio(ctx: &Ctx) -> Result<(), String> {
    let table = ctx.table("NPCTextAudio")?;
    let mut root: Vec<(String, J)> = table
        .rows()
        .map(|row| {
            let npcs = ctx.rr.deref_list(row, "NPCs").into_iter().map(|npc| {
                let npc = npc.row();
                Obj::new()
                    .set("id", text(npc.id()))
                    .set("name", text(npc.str("Name")))
                    .set("short_name", text(npc.str("ShortName")))
                    .build()
            });
            let characters: Vec<String> = ctx
                .rr
                .deref_list(row, "Characters")
                .iter()
                .map(|c| c.row().string("Name"))
                .collect();
            let entry = Obj::new()
                .set("npcs", json::arr(npcs))
                .or_null("characters", (!characters.is_empty()).then(|| json::strings(&characters)))
                .or_null("events", None)
                .set("text", text(row.str("Text")))
                .set("audio", json::strings(row.list_str("AudioFiles")))
                .or_null("mono", None)
                .or_null("stereo", None)
                .or_null("video", None)
                .build();
            (row.id().to_string(), entry)
        })
        .collect();

    // Character-specific lines live in a second table, keyed by the same ids.
    if let Some(events) = ctx.optional_table("CharacterEventTextAudio") {
        for row in events.rows() {
            let event = ctx.rr.deref_id(row, "Event");
            let character = ctx.rr.deref(row, "Character").map(|c| c.row().string("Name"));
            for line in ctx.rr.deref_list(row, "TextAudio") {
                let line = line.row();
                let entry = Obj::new()
                    .or_null("npcs", None)
                    .or_null("characters", character.clone().map(|c| json::strings([c])))
                    .or_null("events", event.clone().map(|e| json::strings([e])))
                    .set("text", text(line.str("Text")))
                    .set("audio", json::strings([line.str("SoundFile")]))
                    .or_null("mono", None)
                    .or_null("stereo", None)
                    .or_null("video", None)
                    .build();
                let id = line.id().to_string();
                match root.iter_mut().find(|(k, _)| *k == id) {
                    Some(slot) => slot.1 = entry,
                    None => root.push((id, entry)),
                }
            }
        }
    }

    ctx.write("audio", &J::Obj(root))
}
