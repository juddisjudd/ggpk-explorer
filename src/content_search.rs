//! Text search inside game files: a scan over the bundles holding the files in scope, on every core, with matches sent as they are found.

use crate::bundles::extract::{decompress_bundle, read_file};
use crate::bundles::index::{FileInfo, Index, GGPK_LOOSE_FILE_SENTINEL};
use crate::bundles::steam::{SteamBundleLoader, LOOSE_FILE_SENTINEL};
use crate::ggpk::reader::GgpkReader;
use crate::search::{matches, Mode, Query};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Kinds of file with no text worth reading: textures, meshes, audio, video, fonts and compiled shaders.
const NOT_TEXT: [&str; 30] = [
    "dds", "header", "tgm", "smd", "fmt", "ast", "tdt", "tmd", "ogg", "wav", "mp3", "bank", "bk2", "png", "jpg", "jpeg", "bmp", "webp", "ico", "ttf", "otf", "spritefont", "bin", "psg", "fgp", "itemblob", "zip", "dll", "exe", "lnk",
];
const TABLES: [&str; 6] = ["dat", "dat64", "datc64", "datl", "datl64", "datcl64"];
const LARGEST_FILE: u32 = 24 * 1024 * 1024;
const PER_FILE: usize = 20;
/// How much of a long line is kept either side of a match; little before it, so the match is always in view.
const BEFORE: usize = 48;
const AFTER: usize = 200;
/// A search stops itself here: more matches than anyone reads, and a sign the text is too common.
pub const MOST_HITS: usize = 5000;

pub struct ContentQuery {
    pub text: String,
    /// Which files to read, in the file search's own words: `*.ao metadata/ !french`.
    pub scope: Query,
    pub match_case: bool,
    /// Whether DAT tables are read too; their strings are UTF-16 inside binary rows.
    pub tables: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineHit {
    pub hash: u64,
    /// The line of a text file, from 1; 0 for a string inside a table.
    pub line: u32,
    pub text: String,
    /// Where the match sits in `text`.
    pub mark: Range<usize>,
}

pub enum Event {
    Hits(Vec<LineHit>),
    Progress { files: usize, of: usize },
    Done { files: usize, stopped: bool },
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(String::new(), |(_, ext)| ext.to_ascii_lowercase())
}

/// The files a search reads: inside the scope, not a known binary kind, and not huge.
pub fn in_scope<'a>(index: &'a Index, query: &ContentQuery) -> Vec<&'a FileInfo> {
    let chosen: Option<HashSet<u64>> = (!query.scope.is_empty()).then(|| matches(index, &query.scope, Mode::Exact, &|_| 0).0.iter().map(|hit| hit.hash).collect());
    index
        .files
        .values()
        .filter(|file| !file.path.is_empty() && file.file_size > 0 && file.file_size <= LARGEST_FILE)
        .filter(|file| !file.path.get(..11).is_some_and(|start| start.eq_ignore_ascii_case("shadercache")))
        .filter(|file| chosen.as_ref().is_none_or(|chosen| chosen.contains(&file.path_hash)))
        .filter(|file| {
            let ext = extension(&file.path);
            !NOT_TEXT.contains(&ext.as_str()) && (query.tables || !TABLES.contains(&ext.as_str()))
        })
        .collect()
}

/// Where `needle` starts in `haystack`, up to `limit` places; ASCII letters match either case unless `match_case`.
fn find_all(haystack: &[u8], needle: &[u8], match_case: bool, limit: usize) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    if match_case {
        return memchr::memmem::find_iter(haystack, needle).take(limit).collect();
    }
    let (lower, upper) = (needle[0].to_ascii_lowercase(), needle[0].to_ascii_uppercase());
    let last = haystack.len() - needle.len();
    memchr::memchr2_iter(lower, upper, &haystack[..=last]).filter(|at| haystack[*at..*at + needle.len()].eq_ignore_ascii_case(needle)).take(limit).collect()
}

fn tidy(text: String) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// The line a match sits in, cut to a readable length, and where the match is within it. `wide` text is UTF-16.
fn line_around(data: &[u8], at: usize, len: usize, wide: bool) -> (String, Range<usize>) {
    let step = if wide { 2 } else { 1 };
    let unit = |i: usize| if wide { u16::from_le_bytes([data[i], data[i + 1]]) } else { data[i] as u16 };
    let ends_line = |i: usize| matches!(unit(i), 0 | 0x0A | 0x0D);
    let (mut start, mut end) = (at, at + len);
    while start >= step && !ends_line(start - step) && at - start < BEFORE * step {
        start -= step;
    }
    while end + step <= data.len() && !ends_line(end) && end - (at + len) < AFTER * step {
        end += step;
    }
    let decode = |bytes: &[u8]| match wide {
        true => String::from_utf16_lossy(&bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect::<Vec<u16>>()),
        false => String::from_utf8_lossy(bytes).into_owned(),
    };
    let cut = start >= step && !ends_line(start - step);
    let before = tidy(decode(&data[start..at]));
    let before = format!("{}{}", if cut { "…" } else { "" }, before.trim_start());
    let found = tidy(decode(&data[at..at + len]));
    let after = tidy(decode(&data[at + len..end]));
    let mark = before.len()..before.len() + found.len();
    (format!("{}{}{}", before, found, after.trim_end()), mark)
}

fn line_number(data: &[u8], at: usize, wide: bool) -> u32 {
    let breaks = match wide {
        true => data[..at].as_chunks::<2>().0.iter().filter(|pair| **pair == [0x0A, 0]).count(),
        false => memchr::memchr_iter(b'\n', &data[..at]).count(),
    };
    breaks as u32 + 1
}

/// The text to look for, as bytes in the two encodings the game writes.
struct Needles {
    narrow: Vec<u8>,
    wide: Vec<u8>,
    match_case: bool,
}

impl Needles {
    fn new(text: &str, match_case: bool) -> Self {
        Needles { narrow: text.as_bytes().to_vec(), wide: text.encode_utf16().flat_map(u16::to_le_bytes).collect(), match_case }
    }
}

fn search_file(file: &FileInfo, data: &[u8], needles: &Needles, hits: &mut Vec<LineHit>) {
    let table = TABLES.contains(&extension(&file.path).as_str());
    let before = hits.len();
    for (needle, wide) in [(&needles.wide, true), (&needles.narrow, false)] {
        for at in find_all(data, needle, needles.match_case, PER_FILE - (hits.len() - before)) {
            let (text, mark) = line_around(data, at, needle.len(), wide);
            let line = if table { 0 } else { line_number(data, at, wide) };
            hits.push(LineHit { hash: file.path_hash, line, text, mark });
        }
    }
}

/// Reads every file in scope and reports matches through `send` as each bundle is done; `cancel` stops it between bundles.
pub fn run(index: &Index, reader: Option<&GgpkReader>, steam: Option<&SteamBundleLoader>, query: &ContentQuery, cancel: &AtomicBool, send: &(dyn Fn(Event) + Sync)) {
    let files = in_scope(index, query);
    let total = files.len();
    let mut by_bundle: HashMap<u32, Vec<&FileInfo>> = HashMap::new();
    files.into_iter().for_each(|file| by_bundle.entry(file.bundle_index).or_default().push(file));
    let mut bundles: Vec<(u32, Vec<&FileInfo>)> = by_bundle.into_iter().collect();
    bundles.sort_by_key(|(bundle, _)| *bundle);

    let needles = Needles::new(&query.text, query.match_case);
    let (done, found) = (AtomicUsize::new(0), AtomicUsize::new(0));
    bundles.par_iter().for_each(|(bundle, files)| {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let loose = matches!(*bundle, GGPK_LOOSE_FILE_SENTINEL | LOOSE_FILE_SENTINEL);
        let unpacked = if loose { None } else { decompress_bundle(*bundle, index, reader, steam) };
        let mut hits = Vec::new();
        for file in files {
            let own = if loose { read_file(file, index, reader, steam, None).ok() } else { None };
            let (start, size) = (file.file_offset as usize, file.file_size as usize);
            let data = match (&own, &unpacked) {
                (Some(own), _) => Some(own.as_slice()),
                (None, Some(unpacked)) => unpacked.get(start..start + size),
                (None, None) => None,
            };
            if let Some(data) = data {
                search_file(file, data, &needles, &mut hits);
            }
        }
        let files_done = done.fetch_add(files.len(), Ordering::Relaxed) + files.len();
        if !hits.is_empty() {
            if found.fetch_add(hits.len(), Ordering::Relaxed) + hits.len() >= MOST_HITS {
                cancel.store(true, Ordering::Relaxed);
            }
            send(Event::Hits(hits));
        }
        send(Event::Progress { files: files_done, of: total });
    });
    send(Event::Done { files: done.load(Ordering::Relaxed), stopped: cancel.load(Ordering::Relaxed) });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn file(path: &str) -> FileInfo {
        FileInfo { path_hash: 7, bundle_index: 0, file_offset: 0, file_size: 1, path: path.to_string() }
    }

    fn found(path: &str, data: &[u8], text: &str, match_case: bool) -> Vec<(u32, String, String)> {
        let mut hits = Vec::new();
        search_file(&file(path), data, &Needles::new(text, match_case), &mut hits);
        hits.into_iter().map(|hit| (hit.line, hit.text.clone(), hit.text[hit.mark].to_string())).collect()
    }

    #[test]
    fn a_match_in_plain_text_comes_back_with_its_line() {
        let text = b"version 2\nextends \"Metadata/Parent\"\n\tfoo = \"Hellion roar\"\nlast";
        assert_eq!(found("a/b.ao", text, "hellion", false), vec![(3, "foo = \"Hellion roar\"".to_string(), "Hellion".to_string())]);
        assert!(found("a/b.ao", text, "hellion", true).is_empty(), "match case means the capital counts");
        assert_eq!(found("a/b.ao", text, "version", true)[0].0, 1);
    }

    #[test]
    fn a_match_in_utf16_text_comes_back_with_its_line() {
        let mut data = vec![0xFF, 0xFE];
        data.extend(wide("first line\r\nsecond has a Sword in it\r\nthird"));
        assert_eq!(found("a/b.mat", &data, "SWORD", false), vec![(2, "second has a Sword in it".to_string(), "Sword".to_string())]);
    }

    #[test]
    fn a_string_inside_a_table_is_cut_at_its_own_ends() {
        let mut data = vec![3u8, 0, 0, 0, 0xFE, 0xFE, 0, 0];
        data.extend(wide("Metadata/Monsters/Hellion\0Brutal Hellion of the Wastes\0another"));
        assert_eq!(found("data/monstervarieties.datc64", &data, "hellion", false), vec![(0, "Metadata/Monsters/Hellion".to_string(), "Hellion".to_string()), (0, "Brutal Hellion of the Wastes".to_string(), "Hellion".to_string())]);
    }

    #[test]
    fn one_file_gives_at_most_a_screenful_and_a_long_line_is_cut() {
        let text = "x ".repeat(400) + "needle" + &" y".repeat(400);
        let hits = found("a.txt", text.as_bytes(), "needle", false);
        assert!(hits[0].1.starts_with('…') && hits[0].1.chars().count() <= BEFORE + AFTER + 7 && hits[0].2 == "needle");
        let many = "needle\n".repeat(100);
        assert_eq!(found("a.txt", many.as_bytes(), "needle", false).len(), PER_FILE);
    }

    #[test]
    fn the_scope_leaves_out_binary_kinds_and_tables_unless_asked() {
        let paths = ["art/a.dds", "art/a.dds.header", "metadata/a.ao", "metadata/b.ot", "data/mods.datc64", "audio/a.ogg"];
        let files = paths.iter().enumerate().map(|(i, path)| (i as u64, FileInfo { path_hash: i as u64, bundle_index: 0, file_offset: 0, file_size: 10, path: path.to_string() })).collect();
        let index = Index { bundles: Vec::new(), files };
        let names = |scope: &str, tables: bool| {
            let query = ContentQuery { text: "x".into(), scope: Query::parse(scope), match_case: false, tables };
            let mut found: Vec<&str> = in_scope(&index, &query).iter().map(|file| file.path.as_str()).collect();
            found.sort();
            found
        };
        assert_eq!(names("", false), ["metadata/a.ao", "metadata/b.ot"]);
        assert_eq!(names("", true), ["data/mods.datc64", "metadata/a.ao", "metadata/b.ot"]);
        assert_eq!(names("*.ot", true), ["metadata/b.ot"]);
    }

    /// `cargo test --release content_search_real_data -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn content_search_real_data() {
        let (reader, index) = crate::parsers::real_source();
        for (text, scope, tables) in [("skeleton", "metadata/monsters/ *.ao", false), ("Skeleton", "*.datc64 monstervarieties", true), ("skill_gem", "", false)] {
            let query = ContentQuery { text: text.to_string(), scope: Query::parse(scope), match_case: false, tables };
            let (hits, files) = (std::sync::Mutex::new(Vec::new()), AtomicUsize::new(0));
            let started = std::time::Instant::now();
            run(&index, Some(&reader), None, &query, &AtomicBool::new(false), &|event| match event {
                Event::Hits(found) => hits.lock().unwrap().extend(found),
                Event::Done { files: read, .. } => files.store(read, Ordering::Relaxed),
                Event::Progress { .. } => {}
            });
            let hits = hits.into_inner().unwrap();
            println!("{:?} in {:?} (tables {}): {} matches in {} files read, {:.1} s", text, scope, tables, hits.len(), files.load(Ordering::Relaxed), started.elapsed().as_secs_f32());
            for hit in hits.iter().take(3) {
                println!("    {}:{}  {}", index.files[&hit.hash].path, hit.line, hit.text);
            }
            assert!(!hits.is_empty(), "the game's files mention {}", text);
        }
    }
}
