//! File search over the bundle index: a small query language, typo-tolerant matching and ranking, run on every core.

use crate::bundles::index::Index;
use memchr::memmem::Finder;
use rayon::prelude::*;

/// What a search box holds, split into what each word asks for.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Query {
    /// Words the path must contain, lowercase.
    pub terms: Vec<String>,
    /// File endings with their leading dot, lowercase; a file needs one of them.
    pub extensions: Vec<String>,
    /// Folders the path must pass through, lowercase, each ending in a slash.
    pub folders: Vec<String>,
    /// Text the path must not contain, lowercase.
    pub excluded: Vec<String>,
}

impl Query {
    /// `sword *.dds art/ !shadow`: words, `*.ext` or `.ext` endings, `folder/` and `!not this`.
    pub fn parse(text: &str) -> Query {
        let mut query = Query::default();
        for token in text.split_whitespace() {
            let lower = token.to_ascii_lowercase().replace('\\', "/");
            if let Some(rest) = lower.strip_prefix('!').filter(|rest| !rest.is_empty()) {
                query.excluded.push(rest.to_string());
            } else if let Some(ending) = lower.strip_prefix('*').filter(|rest| rest.starts_with('.') && rest.len() > 1) {
                query.extensions.push(ending.to_string());
            } else if lower.starts_with('.') && lower.len() > 1 && !lower.contains('/') {
                query.extensions.push(lower);
            } else if lower.ends_with('/') && lower.len() > 1 {
                query.folders.push(lower);
            } else {
                query.terms.push(lower);
            }
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.extensions.is_empty() && self.folders.is_empty() && self.excluded.is_empty()
    }
}

/// How loosely a word may match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The word as typed, or with one slip when nothing else matches: for narrowing the tree.
    Exact,
    /// Also letters in order with gaps between them, ranked below a whole match: for jumping to a file.
    Ranked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub hash: u64,
    pub score: i32,
    /// Every word of the query is in the path as typed.
    pub exact: bool,
}

#[derive(Debug, Default)]
pub struct Found {
    /// The best matches first, at most as many as were asked for.
    pub hits: Vec<Hit>,
    /// How many files matched in all.
    pub total: usize,
    /// Whether nothing matched as typed, so words were matched with one slip allowed.
    pub typos: bool,
}

struct Word<'a> {
    text: &'a [u8],
    finder: Finder<'a>,
}

fn word_start(path: &[u8], at: usize) -> bool {
    at == 0 || matches!(path[at - 1], b'/' | b'_' | b'-' | b'.' | b' ')
}

/// The letters of `word` in order within `path[from..]`, close enough together to be meant; returns how many letters were skipped.
fn in_order(path: &[u8], from: usize, word: &[u8]) -> Option<usize> {
    let first = from + path[from..].iter().position(|b| *b == word[0])?;
    let (mut at, mut skipped) = (first + 1, 0);
    for letter in &word[1..] {
        let step = path[at..].iter().position(|b| b == letter)?;
        skipped += step;
        at += step + 1;
    }
    (skipped <= word.len() + 4).then_some(skipped)
}

/// `word` with one slip somewhere in `path[from..]`: a wrong letter, two neighbours swapped, or a letter too many or too few.
fn one_slip(path: &[u8], from: usize, word: &[u8]) -> bool {
    let (n, path) = (word.len(), &path[from..]);
    let wrong_or_swapped = |window: &[u8]| {
        let (mut wrong, mut count) = ([0usize; 2], 0);
        for i in 0..n {
            if window[i] != word[i] {
                if count == 2 {
                    return false;
                }
                wrong[count] = i;
                count += 1;
            }
        }
        count == 1 || (count == 2 && wrong[1] == wrong[0] + 1 && window[wrong[0]] == word[wrong[1]] && window[wrong[1]] == word[wrong[0]])
    };
    let one_too_many = |window: &[u8]| (0..n).any(|skip| window[..skip] == word[..skip] && window[skip..] == word[skip + 1..]);
    let one_too_few = |window: &[u8]| (1..n).any(|skip| window[..skip] == word[..skip] && window[skip + 1..] == word[skip..]);
    path.windows(n).any(wrong_or_swapped) || (n >= 5 && path.windows(n - 1).any(one_too_many)) || path.windows(n + 1).any(one_too_few)
}

/// How well one word matches a lowercase path, and whether it is there as typed; the file name counts for more than the folders above it.
fn score_word(path: &[u8], name_start: usize, word: &Word, mode: Mode, slips: bool) -> Option<(i32, bool)> {
    if let Some(at) = word.finder.find(&path[name_start..]).map(|at| at + name_start) {
        let stem_end = path[name_start..].iter().rposition(|b| *b == b'.').map_or(path.len(), |dot| name_start + dot);
        let whole = at == name_start && at + word.text.len() == stem_end;
        return Some((100 + 30 * i32::from(word_start(path, at)) + 30 * i32::from(whole), true));
    }
    if let Some(at) = word.finder.find(path) {
        return Some((60 + 10 * i32::from(word_start(path, at)), true));
    }
    if slips && word.text.len() >= 4 {
        if one_slip(path, name_start, word.text) {
            return Some((40, false));
        }
        if one_slip(path, 0, word.text) {
            return Some((30, false));
        }
    }
    if mode == Mode::Ranked && word.text.len() >= 2 {
        return in_order(path, name_start, word.text).map(|skipped| (25 - skipped as i32, false));
    }
    None
}

fn score_path(path: &[u8], query: &Query, words: &[Word], mode: Mode, slips: bool) -> Option<(i32, bool)> {
    if !query.extensions.is_empty() && !query.extensions.iter().any(|ending| path.ends_with(ending.as_bytes())) {
        return None;
    }
    let passes_through = |folder: &str| memchr::memmem::find_iter(path, folder.as_bytes()).any(|at| at == 0 || path[at - 1] == b'/');
    if !query.folders.iter().all(|folder| passes_through(folder)) || query.excluded.iter().any(|text| memchr::memmem::find(path, text.as_bytes()).is_some()) {
        return None;
    }
    let name_start = path.iter().rposition(|b| *b == b'/').map_or(0, |slash| slash + 1);
    let (mut score, mut exact) = (0, true);
    for word in words {
        let (points, as_typed) = score_word(path, name_start, word, mode, slips)?;
        score += points;
        exact &= as_typed;
    }
    // Among equal matches the file nearer the top, with the shorter path, is the likelier one.
    let depth = path.iter().filter(|b| **b == b'/').count();
    Some((score - 2 * depth as i32 - (path.len() / 16) as i32, exact))
}

fn pass(index: &Index, query: &Query, mode: Mode, slips: bool, boost: &(dyn Fn(u64) -> i32 + Sync)) -> Vec<Hit> {
    let words: Vec<Word> = query.terms.iter().map(|term| Word { text: term.as_bytes(), finder: Finder::new(term.as_bytes()) }).collect();
    index
        .files
        .par_iter()
        .map_init(Vec::new, |lower: &mut Vec<u8>, (hash, file)| {
            if file.path.is_empty() {
                return None;
            }
            lower.clear();
            lower.extend(file.path.bytes().map(|b| b.to_ascii_lowercase()));
            score_path(lower, query, &words, mode, slips).map(|(score, exact)| Hit { hash: *hash, score: score + boost(*hash), exact })
        })
        .flatten()
        .collect()
}

/// Every file the query matches, in no order, and whether none matched as typed so one slip per word was allowed.
pub fn matches(index: &Index, query: &Query, mode: Mode, boost: &(dyn Fn(u64) -> i32 + Sync)) -> (Vec<Hit>, bool) {
    if query.is_empty() {
        return (Vec::new(), false);
    }
    let hits = pass(index, query, mode, false, boost);
    match !query.terms.is_empty() && !hits.iter().any(|hit| hit.exact) {
        true => (pass(index, query, mode, true, boost), true),
        false => (hits, false),
    }
}

/// Puts the best `limit` of `hits` first, in order; the rest follow in no order.
pub fn rank(index: &Index, mut hits: Vec<Hit>, limit: usize) -> Vec<Hit> {
    let path = |hit: &Hit| index.files.get(&hit.hash).map_or("", |file| file.path.as_str());
    let best_first = |a: &Hit, b: &Hit| b.score.cmp(&a.score).then_with(|| path(a).cmp(path(b)));
    let sorted = limit.min(hits.len());
    if sorted > 0 && sorted < hits.len() {
        hits.select_nth_unstable_by(sorted, best_first);
    }
    hits[..sorted].sort_unstable_by(best_first);
    hits
}

/// The best `limit` files for a query, best first. `boost` adds to a file's score, for files opened lately.
pub fn search(index: &Index, query: &Query, mode: Mode, limit: usize, boost: &(dyn Fn(u64) -> i32 + Sync)) -> Found {
    let (hits, typos) = matches(index, query, mode, boost);
    let total = hits.len();
    let mut hits = rank(index, hits, limit);
    hits.truncate(limit);
    Found { hits, total, typos }
}

/// Where the query's words sit in `path`, as byte ranges, for showing a match in a list.
pub fn highlights(path: &str, query: &Query) -> Vec<std::ops::Range<usize>> {
    let lower = path.to_ascii_lowercase();
    let name_start = lower.rfind('/').map_or(0, |slash| slash + 1);
    let mut ranges: Vec<std::ops::Range<usize>> = query
        .terms
        .iter()
        .filter_map(|term| lower[name_start..].find(term.as_str()).map(|at| at + name_start).or_else(|| lower.find(term.as_str())).map(|at| at..at + term.len()))
        .collect();
    ranges.sort_by_key(|range| range.start);
    ranges.dedup_by(|next, kept| next.start < kept.end);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundles::index::FileInfo;

    fn index(paths: &[&str]) -> Index {
        let files = paths.iter().enumerate().map(|(i, path)| (i as u64, FileInfo { path_hash: i as u64, bundle_index: 0, file_offset: 0, file_size: 0, path: path.to_string() })).collect();
        Index { bundles: Vec::new(), files }
    }

    fn found(index: &Index, text: &str, mode: Mode) -> Vec<String> {
        search(index, &Query::parse(text), mode, 50, &|_| 0).hits.iter().map(|hit| index.files[&hit.hash].path.clone()).collect()
    }

    const PATHS: [&str; 7] = [
        "art/2dart/items/weapons/sword.dds",
        "art/2dart/items/weapons/swordfish_shadow.dds",
        "metadata/items/weapons/onehandweapons/onehandswords/sword1.ot",
        "metadata/monsters/hellion/hellion.ao",
        "Metadata/Monsters/Hellion/HellionBoss.act",
        "data/schema.datc64",
        "shadercache/sword.bin",
    ];

    #[test]
    fn a_query_is_split_into_words_endings_folders_and_exclusions() {
        let query = Query::parse("Sword *.DDS .ot art/ !shadow metadata/items");
        assert_eq!(query.terms, vec!["sword", "metadata/items"]);
        assert_eq!(query.extensions, vec![".dds", ".ot"]);
        assert_eq!(query.folders, vec!["art/"]);
        assert_eq!(query.excluded, vec!["shadow"]);
        assert!(Query::parse("  ").is_empty());
    }

    #[test]
    fn the_file_named_for_the_word_comes_first_and_case_does_not_matter() {
        let index = index(&PATHS);
        let sword = found(&index, "sword", Mode::Ranked);
        assert_eq!(sword[..2], ["shadercache/sword.bin", "art/2dart/items/weapons/sword.dds"], "the two files named sword, the shorter path first");
        assert_eq!(sword.len(), 4);
        assert_eq!(found(&index, "HELLION boss", Mode::Ranked), vec!["Metadata/Monsters/Hellion/HellionBoss.act"]);
    }

    #[test]
    fn endings_folders_and_exclusions_narrow_the_matches() {
        let index = index(&PATHS);
        assert_eq!(found(&index, "sword *.dds !shadow", Mode::Exact), vec!["art/2dart/items/weapons/sword.dds"]);
        assert_eq!(found(&index, "sword metadata/", Mode::Exact), vec!["metadata/items/weapons/onehandweapons/onehandswords/sword1.ot"]);
        assert_eq!(found(&index, "*.ao", Mode::Exact), vec!["metadata/monsters/hellion/hellion.ao"]);
        assert_eq!(found(&index, "monsters/ !boss", Mode::Exact), vec!["metadata/monsters/hellion/hellion.ao"]);
    }

    #[test]
    fn a_slip_is_forgiven_only_when_nothing_matches_as_typed() {
        let index = index(&PATHS);
        for typed in ["shcema", "schena", "schemma", "scema"] {
            let result = search(&index, &Query::parse(typed), Mode::Exact, 50, &|_| 0);
            assert!(result.typos, "{} should need the forgiving pass", typed);
            assert_eq!(result.hits.len(), 1, "{} should find the schema table", typed);
        }
        let result = search(&index, &Query::parse("schema"), Mode::Exact, 50, &|_| 0);
        assert!(!result.typos && result.total == 1);
        assert_eq!(found(&index, "shcema", Mode::Ranked)[0], "data/schema.datc64", "a slip outranks letters that merely appear in order");
        assert!(found(&index, "zzzz", Mode::Ranked).is_empty());
    }

    #[test]
    fn letters_in_order_match_only_when_ranking_and_rank_below_a_whole_word() {
        let index = index(&PATHS);
        assert!(found(&index, "hlnboss", Mode::Exact).is_empty());
        assert_eq!(found(&index, "hlnboss", Mode::Ranked), vec!["Metadata/Monsters/Hellion/HellionBoss.act"]);
        let ranked = found(&index, "swd", Mode::Ranked);
        assert!(ranked.contains(&"art/2dart/items/weapons/sword.dds".to_string()));
    }

    #[test]
    fn a_file_opened_lately_is_lifted_and_only_the_best_are_kept() {
        let index = index(&PATHS);
        let lifted = search(&index, &Query::parse("sword"), Mode::Ranked, 2, &|hash| if hash == 6 { 500 } else { 0 });
        assert_eq!((lifted.total, lifted.hits.len()), (4, 2));
        assert_eq!(index.files[&lifted.hits[0].hash].path, "shadercache/sword.bin");
    }

    /// `cargo test --release search_speed_on_the_cached_index -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn search_speed_on_the_cached_index() {
        use crate::settings::{AppSettings, Game};
        for game in [Game::Poe1, Game::Poe2] {
            let Ok(index) = Index::load_from_cache(AppSettings::index_cache_path(game)) else { continue };
            println!("{}: {} files", game.label(), index.files.len());
            for (text, mode) in [("sword", Mode::Ranked), ("hellion boss", Mode::Ranked), ("mods *.datc64", Mode::Exact), ("art/ !dds axe", Mode::Exact), ("shcema", Mode::Ranked), ("zzqqxxyy", Mode::Ranked)] {
                let started = std::time::Instant::now();
                let result = search(&index, &Query::parse(text), mode, 300, &|_| 0);
                let first = result.hits.first().map_or("-", |hit| index.files[&hit.hash].path.as_str());
                println!("  {:<16} {:>8} matches in {:>4} ms{}  first: {}", text, result.total, started.elapsed().as_millis(), if result.typos { " (forgiving)" } else { "" }, first);
                assert!(started.elapsed().as_millis() < 1500, "a search should answer while the user types");
            }
        }
    }

    /// `cargo test --release extension_census -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn extension_census() {
        use crate::settings::{AppSettings, Game};
        for game in [Game::Poe1, Game::Poe2] {
            let Ok(index) = Index::load_from_cache(AppSettings::index_cache_path(game)) else { continue };
            let mut by: std::collections::HashMap<String, (usize, u64)> = std::collections::HashMap::new();
            for file in index.files.values() {
                let name = file.path.rsplit('/').next().unwrap_or("");
                let ext = name.rsplit_once('.').map_or("(none)", |(_, ext)| ext).to_ascii_lowercase();
                let entry = by.entry(ext).or_default();
                entry.0 += 1;
                entry.1 += file.file_size as u64;
            }
            let mut rows: Vec<_> = by.into_iter().collect();
            rows.sort_by_key(|(_, (count, _))| std::cmp::Reverse(*count));
            println!("{}: {} files, {} bundles", game.label(), index.files.len(), index.bundles.len());
            for (ext, (count, bytes)) in rows.iter().take(70) {
                println!("  {:<14} {:>8} files {:>9.1} MB", ext, count, *bytes as f64 / 1_048_576.0);
            }
        }
    }

    #[test]
    fn the_words_of_a_query_are_found_again_for_display() {
        let query = Query::parse("hellion boss");
        assert_eq!(highlights("Metadata/Monsters/Hellion/HellionBoss.act", &query), vec![26..33, 33..37]);
    }
}
