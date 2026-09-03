//! Hoists whatever is the same at every level of a skill out of the per-level
//! tables and into one `static` block, which is how `skills.json` avoids
//! repeating a constant forty times.
//!
//! A value is hoisted when every level agrees on it; agreeing levels then drop
//! the key entirely. Nested objects and arrays are folded the same way, so a
//! block where only one field varies keeps just that field per level.

use super::json::J;

/// Folds `levels` (the values of a per-level table) and returns what they all
/// share. Levels are edited in place to drop everything hoisted.
pub fn extract(levels: &mut [J]) -> Option<J> {
    let representative = levels.last()?.clone();
    let (shared, _) = fold(&representative, levels);
    shared
}

/// `(what every level agrees on, whether the levels can drop it entirely)`.
fn fold(representative: &J, levels: &mut [J]) -> (Option<J>, bool) {
    match representative {
        J::Obj(_) => fold_object(representative, levels),
        J::Arr(_) => fold_array(representative, levels),
        // A key every level leaves empty is dropped rather than recorded as a
        // shared null.
        J::Null => (None, levels.iter().all(|level| matches!(level, J::Null))),
        other => {
            let same = levels.iter().all(|level| level == other);
            if same {
                (Some(other.clone()), true)
            } else {
                (None, false)
            }
        }
    }
}

fn fold_object(representative: &J, levels: &mut [J]) -> (Option<J>, bool) {
    let J::Obj(fields) = representative else { return (None, false) };
    let mut shared: Vec<(String, J)> = Vec::new();
    let mut cleared = true;
    let mut clearable: Vec<String> = Vec::new();

    for (key, value) in fields {
        // An empty level shares nothing, and a level missing the key cannot
        // agree about it.
        let present = levels.iter().all(|level| !level.is_empty() && level.get(key).is_some());
        if !present {
            cleared = false;
            continue;
        }
        let mut nested: Vec<J> = levels.iter().map(|level| level.get(key).cloned().unwrap_or(J::Null)).collect();
        let (value_shared, value_cleared) = fold(value, &mut nested);
        for (level, folded) in levels.iter_mut().zip(nested) {
            level.set(key, folded);
        }
        if let Some(value_shared) = value_shared {
            shared.push((key.clone(), value_shared));
        }
        if value_cleared {
            clearable.push(key.clone());
        } else {
            cleared = false;
        }
    }

    for key in &clearable {
        for level in levels.iter_mut() {
            remove(level, key);
        }
    }
    ((!shared.is_empty()).then_some(J::Obj(shared)), cleared)
}

fn fold_array(representative: &J, levels: &mut [J]) -> (Option<J>, bool) {
    let J::Arr(items) = representative else { return (None, false) };
    let nulls = levels.iter().filter(|level| matches!(level, J::Null)).count();
    if nulls == levels.len() {
        return (None, true);
    }
    if nulls > 0 {
        return (None, false);
    }
    // Lists of different lengths describe different things, not a shared one.
    if levels.iter().any(|level| matches!(level, J::Arr(l) if l.len() != items.len()) || !matches!(level, J::Arr(_)))
    {
        return (None, false);
    }
    if items.is_empty() {
        return (Some(J::Arr(Vec::new())), true);
    }

    let mut shared: Option<Vec<J>> = None;
    let mut cleared = true;
    let mut clearable: Vec<usize> = Vec::new();

    for (i, value) in items.iter().enumerate() {
        let mut nested: Vec<J> = levels.iter().map(|level| element(level, i)).collect();
        let (value_shared, value_cleared) = fold(value, &mut nested);
        for (level, folded) in levels.iter_mut().zip(nested) {
            set_element(level, i, folded);
        }
        if let Some(value_shared) = value_shared {
            shared.get_or_insert_with(|| vec![J::Null; items.len()])[i] = value_shared;
        }
        if value_cleared {
            clearable.push(i);
        } else {
            cleared = false;
        }
    }

    for &i in &clearable {
        for level in levels.iter_mut() {
            set_element(level, i, J::Null);
        }
    }
    (shared.map(J::Arr), cleared)
}

fn element(value: &J, index: usize) -> J {
    match value {
        J::Arr(items) => items.get(index).cloned().unwrap_or(J::Null),
        _ => J::Null,
    }
}

fn set_element(value: &mut J, index: usize, item: J) {
    if let J::Arr(items) = value {
        if let Some(slot) = items.get_mut(index) {
            *slot = item;
        }
    }
}

pub fn remove(value: &mut J, key: &str) {
    if let J::Obj(fields) = value {
        fields.retain(|(k, _)| k != key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_export::json::{int, text, Obj};

    fn level(cooldown: i64, name: &str) -> J {
        Obj::new().set("cooldown", int(cooldown)).set("name", text(name)).build()
    }

    #[test]
    fn a_field_every_level_agrees_on_moves_to_static() {
        let mut levels = vec![level(500, "same"), level(700, "same")];
        let shared = extract(&mut levels).expect("something is shared");
        assert_eq!(shared.get("name"), Some(&text("same")));
        assert_eq!(shared.get("cooldown"), None);
        // The agreed field is gone from the levels; the varying one stays.
        assert_eq!(levels[0].get("name"), None);
        assert_eq!(levels[0].get("cooldown"), Some(&int(500)));
    }

    #[test]
    fn a_level_that_agrees_entirely_ends_up_empty() {
        let mut levels = vec![level(500, "same"), level(500, "same")];
        let shared = extract(&mut levels).expect("something is shared");
        assert_eq!(shared.get("cooldown"), Some(&int(500)));
        assert_eq!(levels[0], J::Obj(Vec::new()));
        assert_eq!(levels[1], J::Obj(Vec::new()));
    }

    #[test]
    fn nested_blocks_fold_one_field_at_a_time() {
        let nested = |cost: i64| {
            Obj::new().set("costs", Obj::new().set("Mana", int(cost)).set("Life", int(9)).build()).build()
        };
        let mut levels = vec![nested(10), nested(20)];
        let shared = extract(&mut levels).expect("something is shared");
        assert_eq!(shared.get("costs").and_then(|c| c.get("Life")), Some(&int(9)));
        assert_eq!(levels[0].get("costs").and_then(|c| c.get("Mana")), Some(&int(10)));
        assert_eq!(levels[0].get("costs").and_then(|c| c.get("Life")), None);
    }

    #[test]
    fn lists_of_different_lengths_are_left_alone() {
        let with = |items: Vec<J>| Obj::new().set("stats", J::Arr(items)).build();
        let mut levels = vec![with(vec![int(1)]), with(vec![int(1), int(2)])];
        assert_eq!(extract(&mut levels), None);
        assert_eq!(levels[0].get("stats"), Some(&J::Arr(vec![int(1)])));
    }

    #[test]
    fn an_identical_list_is_hoisted_whole() {
        let with = || Obj::new().set("stats", J::Arr(vec![int(1), int(2)])).build();
        let mut levels = vec![with(), with()];
        let shared = extract(&mut levels).expect("something is shared");
        assert_eq!(shared.get("stats"), Some(&J::Arr(vec![int(1), int(2)])));
        assert_eq!(levels[0], J::Obj(Vec::new()));
    }
}
