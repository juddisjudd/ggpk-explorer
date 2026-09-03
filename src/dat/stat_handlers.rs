//! The stat-description value handlers (`negate`, `milliseconds_to_seconds`,
//! `passive_hash`, …) that a `.csd` names after a description string.
//!
//! This is the single source of truth for both rendering stat text and for
//! writing `stat_value_handlers.json`. Numeric handlers apply
//! `value * multiplier / divisor + addend` and then print at `precision`
//! decimals — trailing zeros kept only when `fixed`. Relational handlers look
//! the value up in a DAT table instead.

/// How a handler turns a raw stat value into display text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// Arithmetic, then formatting.
    Int {
        multiplier: Option<f64>,
        divisor: Option<f64>,
        addend: Option<f64>,
        precision: Option<u32>,
        fixed: bool,
    },
    /// Looks the value up in a DAT table.
    Relational {
        dat_file: &'static str,
        value_column: &'static str,
        /// Column matched against the value; `None` matches the row index.
        index_column: Option<&'static str>,
        /// Only rows where this column equals this value are considered.
        predicate: Option<(&'static str, i64)>,
    },
    /// Marks the line rather than changing the number.
    Str,
    /// Leaves the value alone.
    Noop,
}

pub struct Handler {
    pub name: &'static str,
    pub kind: Kind,
}

const fn int(
    name: &'static str,
    multiplier: Option<f64>,
    divisor: Option<f64>,
    addend: Option<f64>,
    precision: Option<u32>,
    fixed: bool,
) -> Handler {
    Handler { name, kind: Kind::Int { multiplier, divisor, addend, precision, fixed } }
}

const fn rel(
    name: &'static str,
    dat_file: &'static str,
    value_column: &'static str,
    index_column: Option<&'static str>,
    predicate: Option<(&'static str, i64)>,
) -> Handler {
    Handler { name, kind: Kind::Relational { dat_file, value_column, index_column, predicate } }
}

const fn noop(name: &'static str) -> Handler {
    Handler { name, kind: Kind::Noop }
}

const fn string(name: &'static str) -> Handler {
    Handler { name, kind: Kind::Str }
}

pub static HANDLERS: &[Handler] = &[
    noop("tq_noop"),
    int("30%_of_value", Some(30.0), Some(100.0), None, None, false),
    int("60%_of_value", Some(60.0), Some(100.0), None, None, false),
    int("deciseconds_to_seconds", None, Some(10.0), None, None, false),
    int("divide_by_three", None, Some(3.0), None, None, false),
    int("divide_by_five", None, Some(5.0), None, None, false),
    int("divide_by_one_hundred", None, Some(100.0), None, None, false),
    int("divide_by_one_hundred_and_negate", None, Some(-100.0), None, None, false),
    int("divide_by_one_hundred_0dp", None, Some(100.0), None, Some(0), false),
    int("divide_by_one_hundred_1dp", None, Some(100.0), None, Some(1), true),
    int("divide_by_one_hundred_2dp", None, Some(100.0), None, Some(2), true),
    int("divide_by_one_hundred_2dp_if_required", None, Some(100.0), None, Some(2), false),
    int("divide_by_two_0dp", None, Some(2.0), None, Some(0), false),
    int("divide_by_six", None, Some(6.0), None, None, false),
    int("divide_by_ten_0dp", None, Some(10.0), None, Some(0), false),
    int("divide_by_ten_1dp", None, Some(10.0), None, Some(1), true),
    int("divide_by_twelve", None, Some(12.0), None, None, false),
    int("divide_by_fifteen_0dp", None, Some(15.0), None, Some(0), false),
    int("divide_by_twenty_then_double_0dp", Some(2.0), Some(20.0), None, Some(0), false),
    int("milliseconds_to_seconds", None, Some(1000.0), None, None, false),
    int("milliseconds_to_seconds_halved", None, Some(500.0), None, None, false),
    int("milliseconds_to_seconds_0dp", None, Some(1000.0), None, Some(0), false),
    int("milliseconds_to_seconds_1dp", None, Some(1000.0), None, Some(1), true),
    int("milliseconds_to_seconds_2dp", None, Some(1000.0), None, Some(2), true),
    int("milliseconds_to_seconds_2dp_if_required", None, Some(1000.0), None, Some(2), false),
    int("multiplicative_damage_modifier", None, None, Some(100.0), None, false),
    int("multiplicative_permyriad_damage_modifier", None, Some(100.0), Some(100.0), None, false),
    int("multiply_by_four", Some(4.0), None, None, None, false),
    int("multiply_by_four_and_", Some(4.0), None, None, None, false),
    int("negate", Some(-1.0), None, None, None, false),
    int("old_leech_percent", None, Some(5.0), None, None, false),
    int("old_leech_permyriad", None, Some(500.0), None, None, false),
    int("per_minute_to_per_second", None, Some(60.0), None, Some(1), false),
    int("per_minute_to_per_second_0dp", None, Some(60.0), None, Some(0), false),
    int("per_minute_to_per_second_1dp", None, Some(60.0), None, Some(1), true),
    int("per_minute_to_per_second_2dp", None, Some(60.0), None, Some(2), true),
    int("per_minute_to_per_second_2dp_if_required", None, Some(60.0), None, Some(2), false),
    int("permyriad_per_minute_to_%_per_second", None, Some(6000.0), None, Some(1), false),
    int("times_twenty", Some(20.0), None, None, None, false),
    int("times_one_point_five", Some(1.5), None, None, None, false),
    int("double", Some(2.0), None, None, None, false),
    int("negate_and_double", Some(-2.0), None, None, None, false),
    int("divide_by_four", None, Some(4.0), None, None, false),
    int("divide_by_ten_1dp_if_required", None, Some(10.0), None, Some(1), false),
    int("divide_by_fifty", None, Some(50.0), None, None, false),
    int("multiply_by_ten", Some(10.0), None, None, None, false),
    int("multiply_by_one_hundred", Some(100.0), None, None, None, false),
    int("divide_by_one_thousand", None, Some(1000.0), None, None, false),
    int("plus_two_hundred", None, None, Some(200.0), None, false),
    int("divide_by_twenty", None, Some(20.0), None, None, false),
    int("locations_to_metres", None, Some(10.0), None, None, false),
    int("invert_chance", Some(-1.0), None, Some(100.0), None, false),
    int("one_hundred_divide_by_value", Some(100.0), None, None, Some(2), false),
    int("add_one", None, None, Some(1.0), None, false),
    int("subtract_one", None, None, Some(-1.0), None, false),
    int("divide_by_ten_thousand_1dp", None, Some(10000.0), None, Some(1), false),
    rel("mod_value_to_item_class", "ItemClasses", "Name", None, None),
    rel("tempest_mod_text", "Mods", "Name", None, Some(("GenerationType", 8))),
    rel("tree_expansion_jewel_passive", "PassiveTreeExpansionJewelSizes", "Name", None, None),
    rel("passive_hash", "PassiveSkills", "Name", Some("PassiveSkillGraphId"), None),
    rel("ultimatum_wager_type_hash", "UltimatumWagerTypes", "DisplayText", Some("HASH16"), None),
    rel("specific_skill", "SkillGemsForUniqueStat", "SkillGems", Some("Index"), None),
    rel("passive_keystone_index", "PassiveKeystoneList", "DisplayText", None, None),
    rel("mages_legacy_index", "UniqueMagesLegacy", "DisplayText", None, None),
    string("canonical_line"),
    string("markup"),
    string("reminderstring"),
    noop("weapon_tree_unique_base_type_name"),
    noop("canonical_stat"),
    noop("display_indexable_support"),
    noop("affliction_reward_type"),
    noop("metamorphosis_reward_description"),
    noop("display_indexable_skill"),
    noop("display_indexable_non_active_support"),
];

pub fn lookup(name: &str) -> Option<&'static Handler> {
    HANDLERS.iter().find(|h| h.name == name)
}

/// `value * multiplier / divisor + addend`, or the value unchanged for a
/// handler that only marks the line.
pub fn apply(kind: &Kind, value: f64) -> f64 {
    match kind {
        Kind::Int { multiplier, divisor, addend, .. } => {
            // Named for what it does, not for the arithmetic: 100 over the value.
            let mut v = value;
            if let Some(m) = multiplier {
                v *= m;
            }
            if let Some(d) = divisor {
                v /= d;
            }
            if let Some(a) = addend {
                v += a;
            }
            v
        }
        _ => value,
    }
}

/// Decimals to print at, and whether trailing zeros are kept.
pub fn precision(kind: &Kind) -> (Option<u32>, bool) {
    match kind {
        Kind::Int { precision, fixed, .. } => (*precision, *fixed),
        _ => (None, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_matches_the_published_catalogue() {
        let h = |n: &str| lookup(n).map(|h| h.kind).expect(n);
        assert_eq!(apply(&h("negate"), 20.0), -20.0);
        assert_eq!(apply(&h("30%_of_value"), 200.0), 60.0);
        assert_eq!(apply(&h("invert_chance"), 30.0), 70.0);
        assert_eq!(apply(&h("multiplicative_permyriad_damage_modifier"), 500.0), 105.0);
        assert_eq!(apply(&h("milliseconds_to_seconds"), 1500.0), 1.5);
        assert_eq!(precision(&h("divide_by_ten_1dp")), (Some(1), true));
        assert_eq!(precision(&h("divide_by_one_hundred_2dp_if_required")), (Some(2), false));
    }

    #[test]
    fn every_name_is_unique() {
        let mut names: Vec<&str> = HANDLERS.iter().map(|h| h.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate handler name in the catalogue");
    }
}
