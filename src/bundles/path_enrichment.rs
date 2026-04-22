use std::collections::HashMap;
use crate::bundles::index::{Index, FileInfo, BundleInfo, murmur_hash64a, fnv1a64};
use crate::bundles::cdn::CdnBundleLoader;
use crate::dat::reader::{DatReader, DatValue};
use crate::dat::schema::Schema;
use crate::ggpk::reader::GgpkReader;

/// (table_name, &[column_names_that_are_@file])
/// Array columns (marked [string] in schema) are handled automatically via DatValue::List.
static DAT_FILE_FIELDS: &[(&str, &[&str])] = &[
    // ── Video (.bk2) ──────────────────────────────────────────────────────────
    ("AdvancedSkillsTutorial",            &["International_BK2File", "China_BK2File"]),
    ("ArchetypeRewards",                  &["BK2File"]),
    ("ActiveSkills",                      &["VideoClip"]),
    ("CurrencyUseEffects",                &["BK2File", "BK2File2"]),
    ("MicrotransactionSocialFrameVariations", &["BK2File"]),
    ("SkillGems",                         &["TutorialVideo"]),

    // ── Audio (.ogg) ──────────────────────────────────────────────────────────
    ("Ascendancy",                        &["OGGFile"]),
    ("AwardDisplay",                      &["OGGFile"]),
    ("Characters",                        &["IntroSoundFile"]),
    ("CurrencyUseEffects",                &["SoundFile"]),
    ("NPCTalkDialogueTextAudio",          &["OGGFiles"]),
    ("SkillGems",                         &["OGGFile"]),

    // ── Textures (.dds) ───────────────────────────────────────────────────────
    ("AchievementSetRewards",             &["NotificationIcon"]),
    ("ActiveSkills",                      &["Icon_DDSFile"]),
    ("AlternatePassiveSkills",            &["DDSIcon"]),
    ("Ascendancy",                        &["PassiveTreeImage"]),
    ("AtlasNode",                         &["UniqueArt", "Node_DDSFile"]),
    ("AtlasNodeDefinition",               &["Node_DDSFile"]),
    ("BuffVisuals",                       &["BuffDDSFile"]),
    ("Characters",                        &["PassiveTreeImage"]),
    ("DynamicStashSlots",                 &["Icon_DDSFile"]),
    ("EndgameMapContent",                 &["Icon"]),
    ("Incursion2Crafting",                &["Icon_DDSFile", "GlowIcon_DDSFile"]),
    ("Incursion2Medallions",              &["Icon_DDSFile"]),
    ("Incursion2RoomPerLevel",            &["Icon_DDSFile"]),
    ("Incursion2Rooms",                   &["Icon_DDSFile"]),
    ("ItemVisualIdentity",                &["DDSFile"]),
    ("MapSeries",                         &["BaseIcon_DDSFile", "Infected_DDSFile", "Shaper_DDSFile",
                                            "Elder_DDSFile", "Drawn_DDSFile", "Delirious_DDSFile",
                                            "UberBlight_DDSFile"]),
    ("MapStashSpecialTypeEntries",        &["DDSFile", "DDSFileNew"]),
    ("MtxTypeGameSpecific",               &["DDSFile"]),
    ("PassiveSkillTreeMasteryArt",        &["InactiveIcon", "ActiveIcon"]),
    ("PassiveSkillTreeNodeFrameArt",      &["Mask"]),
    ("PassiveSkills",                     &["Icon_DDSFile"]),
    ("Quest",                             &["Icon_DDSFile"]),
    ("QuestRewardType",                   &["Icon_DDSFile"]),
    ("SkillGems",                         &["UI_Image"]),
    ("SupportGems",                       &["Icon"]),

    // ── Animated objects (.ao / .act / .ais) ──────────────────────────────────
    ("ActiveSkillRequirementIcons",       &["AOFile"]),
    ("ActiveSkills",                      &["AIFile", "AiScript"]),
    ("AnimatedObjectFlags",               &["AOFile"]),
    ("AreaInfluenceDoodads",              &["AOFiles"]),
    ("Characters",                        &["AOFile", "ACTFile", "LoginScreen"]),
    ("Chests",                            &["AOFiles"]),
    ("EndgameMapDecorations",             &["AnimatedObject", "AdditionalAnimatedObjects"]),
    ("HideoutDoodads",                    &["Variation_AOFiles"]),
    ("MicrotransactionPortalVariations",  &["AOFile", "MapAOFile", "PortalEffect", "PortalEffectLarge"]),
    ("MiscProjectileMod",                 &["AOFile"]),
    ("MonsterVarieties",                  &["AOFiles", "ACTFiles", "AISFile", "SinkAnimation_AOFile"]),
    ("Projectiles",                       &["AOFiles", "Stuck_AOFile", "Bounce_AOFile"]),
    ("ShapeShiftForms",                   &["AOFile"]),
    ("ShapeShiftVisualIdentity",          &["AOFile"]),

    // ── Effect packages (.epk / .pet) ─────────────────────────────────────────
    ("BuffVisualShapeShiftOverride",      &["EPKFiles"]),
    ("BuffVisuals",                       &["EPKFile", "EPKFiles", "EPKFiles1", "EPKFiles2"]),
    ("ItemVisualEffect",                  &["DaggerEPKFile", "BowEPKFile", "OneHandedMaceEPKFile",
                                            "OneHandedSwordEPKFile", "TwoHandedSwordEPKFile",
                                            "TwoHandedStaffEPKFile", "TwoHandedMaceEPKFile",
                                            "OneHandedAxeEPKFile", "TwoHandedAxeEPKFile",
                                            "ClawEPKFile", "PETFile", "Shield", "OnHitEffect"]),
    ("ItemVisualIdentity",                &["AOFile", "AOFile2", "EPKFile",
                                            "MarauderSMFiles", "RangerSMFiles", "WitchSMFiles",
                                            "DuelistDexSMFiles", "TemplarSMFiles", "ShadowSMFiles",
                                            "ScionSMFiles", "SMFiles"]),
    ("Melee",                             &["SurgeEffect_EPKFile"]),

    // ── Other ─────────────────────────────────────────────────────────────────
    ("EndgameMapBiomes",                  &["GroundType1", "GroundType2", "GroundTypeCorrupted1",
                                            "GroundTypeCorrupted2", "GroundTypeSanctified"]),
    ("Hideouts",                          &["HideoutFile"]),
    ("MiniQuestStates",                   &["TSIFile"]),
    ("Topologies",                        &["DGRFile"]),
];

/// Scans all dat files listed in DAT_FILE_FIELDS, extracts path strings from
/// @file-typed columns, and attempts to resolve any remaining unpathed file hashes.
/// Returns the number of newly resolved paths.
pub fn enrich_paths_from_dat(
    index: &mut Index,
    schema: &Schema,
    reader: &GgpkReader,
    cdn_loader: Option<&CdnBundleLoader>,
) -> u32 {
    let mut resolved = 0u32;
    // Bundle decompression cache keyed by bundle_index — avoids re-decompressing
    // the same bundle for every dat file it contains.
    let mut bundle_cache: HashMap<u32, Vec<u8>> = HashMap::new();

    // Collect (table_name → sorted, deduped column names) so we can join
    // multiple static rows that target the same table.
    let mut table_map: HashMap<&str, Vec<&str>> = HashMap::new();
    for (table_name, cols) in DAT_FILE_FIELDS {
        let entry = table_map.entry(table_name).or_default();
        for &col in *cols {
            if !entry.contains(&col) {
                entry.push(col);
            }
        }
    }

    for (table_name, col_names) in &table_map {
        let schema_table = match schema.tables.iter().find(|t| t.name == *table_name) {
            Some(t) => t,
            None => continue,
        };

        // Find the dat file in the index — try .dat64 first, then .dat
        let dat_path_candidates = [
            format!("Data/{}.dat64", table_name),
            format!("Data/{}.dat", table_name),
        ];
        let dat_info = dat_path_candidates.iter().find_map(|p| {
            index.files.iter().find(|(_, v)| v.path.eq_ignore_ascii_case(p))
                .map(|(k, v)| (*k, v.clone()))
        });
        let (_, dat_file_info) = match dat_info {
            Some(x) => x,
            None => continue,
        };

        // Load file bytes using cached bundle decompression
        let dat_bytes = match load_file_bytes_cached(
            &dat_file_info, &index.bundles, reader, cdn_loader, &mut bundle_cache,
        ) {
            Some(b) => b,
            None => continue,
        };

        let dat_reader = match DatReader::new(dat_bytes, &dat_file_info.path) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Find column indices for the requested @file columns
        let col_indices: Vec<(usize, bool)> = schema_table.columns.iter().enumerate()
            .filter_map(|(i, col)| {
                let name = col.name.as_deref().unwrap_or("");
                if col_names.contains(&name) { Some((i, col.array)) } else { None }
            })
            .collect();

        if col_indices.is_empty() {
            continue;
        }

        for row_idx in 0..dat_reader.row_count {
            let row = match dat_reader.read_row(row_idx, schema_table) {
                Ok(r) => r,
                Err(_) => continue,
            };

            for &(col_idx, is_array) in &col_indices {
                if col_idx >= row.len() {
                    continue;
                }
                match &row[col_idx] {
                    DatValue::String(s) if !s.is_empty() => {
                        if try_resolve(index, s) {
                            resolved += 1;
                        }
                    }
                    DatValue::List(count, offset) if is_array && *count > 0 => {
                        let elem_col = &schema_table.columns[col_idx];
                        if let Ok(items) = dat_reader.read_list_values(*offset, *count, elem_col) {
                            for item in items {
                                if let DatValue::String(s) = item {
                                    if !s.is_empty() && try_resolve(index, &s) {
                                        resolved += 1;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    resolved
}

/// Tries both Murmur64A and FNV1a (original + lowercase) to match `path` to
/// an unresolved file hash in the index. Returns true if a match was found.
fn try_resolve(index: &mut Index, path: &str) -> bool {
    let bytes = path.as_bytes();
    let lower = path.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();

    for &hash in &[
        murmur_hash64a(bytes),
        murmur_hash64a(lower_bytes),
        fnv1a64(bytes),
        fnv1a64(lower_bytes),
    ] {
        if let Some(info) = index.files.get_mut(&hash) {
            if info.path.is_empty() {
                info.path = path.to_string();
                return true;
            }
        }
    }
    false
}

/// Loads the decompressed bytes for `file_info` from either the GGPK or CDN,
/// using `bundle_cache` to avoid re-decompressing the same bundle twice.
fn load_file_bytes_cached(
    file_info: &FileInfo,
    bundles: &[BundleInfo],
    reader: &GgpkReader,
    cdn_loader: Option<&CdnBundleLoader>,
    bundle_cache: &mut HashMap<u32, Vec<u8>>,
) -> Option<Vec<u8>> {
    let bi = file_info.bundle_index;

    let decompressed = if let Some(cached) = bundle_cache.get(&bi) {
        cached
    } else {
        let bundle_info = bundles.get(bi as usize)?;
        let raw = fetch_raw_bundle(bundle_info, reader, cdn_loader)?;
        let mut cursor = std::io::Cursor::new(raw);
        let header = crate::bundles::bundle::Bundle::read_header(&mut cursor).ok()?;
        let data = header.decompress(&mut cursor).ok()?;
        bundle_cache.insert(bi, data);
        bundle_cache.get(&bi)?
    };

    let start = file_info.file_offset as usize;
    let end = start + file_info.file_size as usize;
    if end <= decompressed.len() {
        Some(decompressed[start..end].to_vec())
    } else {
        None
    }
}

fn fetch_raw_bundle(
    bundle_info: &BundleInfo,
    reader: &GgpkReader,
    cdn_loader: Option<&CdnBundleLoader>,
) -> Option<Vec<u8>> {
    // Try GGPK first
    for cand in &[
        format!("Bundles2/{}", bundle_info.name),
        format!("Bundles2/{}.bundle.bin", bundle_info.name),
    ] {
        if let Ok(Some(rec)) = reader.read_file_by_path(cand) {
            if let Ok(data) = reader.get_data_slice(rec.data_offset, rec.data_length) {
                return Some(data.to_vec());
            }
        }
    }
    // Fallback to CDN
    if let Some(cdn) = cdn_loader {
        let name = if bundle_info.name.ends_with(".bundle.bin") {
            bundle_info.name.clone()
        } else {
            format!("{}.bundle.bin", bundle_info.name)
        };
        return cdn.fetch_bundle(&name).ok();
    }
    None
}
