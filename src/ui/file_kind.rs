//! One kind per file ending, each with a colour, so a list of files can be read at a glance.

use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Table,
    Object,
    Material,
    Image,
    Model,
    World,
    Audio,
    Video,
    Text,
    Other,
}

const TABLE_ENDINGS: [&str; 6] = ["dat", "dat64", "datc64", "datl", "datl64", "datcl64"];

/// Whether a file is a DAT table: it ends like one and is not a file that only shares the ending.
pub fn is_table(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let ending = name.rsplit_once('.').map_or("", |(_, ending)| ending);
    TABLE_ENDINGS.contains(&ending) && name != "mat_table.dat" && !lower.starts_with("shadercache")
}

impl FileKind {
    /// Every kind a file ending can name, in the order a legend lists them.
    pub const NAMED: [FileKind; 9] = [FileKind::Table, FileKind::Object, FileKind::Material, FileKind::Image, FileKind::Model, FileKind::World, FileKind::Audio, FileKind::Video, FileKind::Text];

    /// The file endings of a kind, lowercase, without the dot.
    pub fn endings(self) -> &'static [&'static str] {
        match self {
            FileKind::Table => &["dat", "dat64", "datc64", "datl", "datl64", "datcl64", "csd", "psg"],
            FileKind::Object => &["ao", "aoc", "ot", "it", "act", "epk"],
            FileKind::Material => &["mat", "fxgraph", "pet", "trl", "atl", "ffx", "fgp"],
            FileKind::Image => &["dds", "header", "png", "jpg", "jpeg", "bmp", "webp"],
            FileKind::Model => &["fmt", "tgm", "smd", "sm", "ast", "amd", "tmd"],
            FileKind::World => &["arm", "dgr", "tgt", "tdt", "tst", "rs", "gt", "gft", "et", "ecf", "cht", "clt", "dct", "ddt", "dlp", "mtd", "mtp", "tgr", "tsi", "toy", "gm", "env", "hideout"],
            FileKind::Audio => &["ogg", "wav", "mp3", "bank", "wem"],
            FileKind::Video => &["bk2", "mp4"],
            FileKind::Text => &["txt", "json", "xml", "hlsl", "inc", "h", "properties", "ui", "cfg", "ini", "csv", "atlas", "filter"],
            FileKind::Other => &[],
        }
    }

    pub fn of(path: &str) -> FileKind {
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some((_, ending)) = name.rsplit_once('.') else { return FileKind::Other };
        let ending = ending.to_ascii_lowercase();
        if TABLE_ENDINGS.contains(&ending.as_str()) && !is_table(path) {
            return FileKind::Other;
        }
        FileKind::NAMED.into_iter().find(|kind| kind.endings().contains(&ending.as_str())).unwrap_or(FileKind::Other)
    }

    /// The word a type column shows.
    pub fn label(self) -> &'static str {
        match self {
            FileKind::Table => "DATA",
            FileKind::Object => "OBJECT",
            FileKind::Material => "MATERIAL",
            FileKind::Image => "IMAGE",
            FileKind::Model => "MODEL",
            FileKind::World => "WORLD",
            FileKind::Audio => "AUDIO",
            FileKind::Video => "VIDEO",
            FileKind::Text => "TEXT",
            FileKind::Other => "BINARY",
        }
    }

    /// What a legend calls the kind.
    pub fn name(self) -> &'static str {
        match self {
            FileKind::Table => "Data tables",
            FileKind::Object => "Game objects",
            FileKind::Material => "Materials and effects",
            FileKind::Image => "Textures",
            FileKind::Model => "Models",
            FileKind::World => "Rooms and terrain",
            FileKind::Audio => "Audio",
            FileKind::Video => "Video",
            FileKind::Text => "Text",
            FileKind::Other => "Other",
        }
    }

    pub fn colour(self, dark_mode: bool) -> egui::Color32 {
        let (dark, light) = match self {
            FileKind::Table => ((251, 191, 36), (180, 120, 0)),
            FileKind::Object => ((96, 165, 250), (37, 99, 235)),
            FileKind::Material => ((192, 132, 252), (147, 51, 234)),
            FileKind::Image => ((74, 222, 128), (22, 163, 74)),
            FileKind::Model => ((251, 146, 60), (234, 88, 12)),
            FileKind::World => ((45, 212, 191), (13, 148, 136)),
            FileKind::Audio => ((244, 114, 182), (219, 39, 119)),
            FileKind::Video => ((251, 113, 133), (225, 29, 72)),
            FileKind::Text => ((148, 163, 184), (100, 116, 139)),
            FileKind::Other => ((92, 92, 100), (170, 170, 180)),
        };
        let (r, g, b) = if dark_mode { dark } else { light };
        egui::Color32::from_rgb(r, g, b)
    }

    /// A file search that finds this kind: `*.dds *.png`.
    pub fn query(self) -> String {
        self.endings().iter().map(|ending| format!("*.{}", ending)).collect::<Vec<_>>().join(" ")
    }
}

/// The kind's dot, centred on `centre`.
pub fn dot(painter: &egui::Painter, centre: egui::Pos2, kind: FileKind, dark_mode: bool) {
    painter.circle_filled(centre, 3.5, kind.colour(dark_mode));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_known_by_its_last_ending_whatever_its_case() {
        assert_eq!(FileKind::of("data/Mods.DATC64"), FileKind::Table);
        assert_eq!(FileKind::of("art/2dart/sword.dds.header"), FileKind::Image);
        assert_eq!(FileKind::of("metadata/monsters/hellion/hellion.ao"), FileKind::Object);
        assert_eq!(FileKind::of("art/models/rig.ast"), FileKind::Model);
        assert_eq!(FileKind::of("metadata/terrain/room.arm"), FileKind::World);
        assert_eq!(FileKind::of("gateway_list.txt"), FileKind::Text);
        assert_eq!(FileKind::of("folder.with.dots/noending"), FileKind::Other);
        assert_eq!(FileKind::of("shadercache/abc.bin"), FileKind::Other);
    }

    #[test]
    fn a_file_that_only_ends_like_a_table_is_not_one() {
        assert!(is_table("data/balance/Mods.datc64") && is_table("Data/Mods.dat") && is_table("data/french/mods.datcl64"));
        assert!(!is_table("data/mat_table.dat") && !is_table("ShaderCacheD3D11.packed/0.dat") && !is_table("data/statdescriptions/x.csd"));
        assert_eq!(FileKind::of("data/mat_table.dat"), FileKind::Other);
        assert_eq!(FileKind::of("data/statdescriptions/x.csd"), FileKind::Table);
    }

    #[test]
    fn no_ending_belongs_to_two_kinds_and_every_kind_has_its_own_colour() {
        let mut seen = std::collections::HashSet::new();
        for kind in FileKind::NAMED {
            for ending in kind.endings() {
                assert!(seen.insert(*ending), "{} is listed twice", ending);
            }
        }
        for dark_mode in [true, false] {
            let colours: std::collections::HashSet<_> = FileKind::NAMED.into_iter().chain([FileKind::Other]).map(|kind| kind.colour(dark_mode)).collect();
            assert_eq!(colours.len(), 10);
        }
        assert_eq!(FileKind::Video.query(), "*.bk2 *.mp4");
    }
}
