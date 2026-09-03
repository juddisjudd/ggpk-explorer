use crate::ggpk::reader::GgpkReader;
use crate::settings::AppSettings;

fn murmur_hash64a(key: &[u8], seed: u64) -> u64 {
    let m: u64 = 0xc6a4a7935bd1e995;
    let r: u8 = 47;
    let len = key.len() as u64;
    let mut h: u64 = seed ^ (len.wrapping_mul(m));
    let n_blocks = len / 8;
    let md = key;
    for i in 0..n_blocks {
        let idx = (i * 8) as usize;
        let mut k: u64 = u64::from_le_bytes(md[idx..idx+8].try_into().unwrap());
        k = k.wrapping_mul(m);
        k ^= k >> r;
        k = k.wrapping_mul(m);
        h ^= k;
        h = h.wrapping_mul(m);
    }
    let remainder_idx = (n_blocks * 8) as usize;
    let remaining_len = (len & 7) as usize;
    if remaining_len > 0 {
        let mut k: u64 = 0;
        for i in 0..remaining_len {
             k ^= (md[remainder_idx + i] as u64) << (8 * i);
        }
        h ^= k;
        h = h.wrapping_mul(m);
    }
    h ^= h >> r;
    h = h.wrapping_mul(m);
    h ^= h >> r;
    h
}

pub fn run_inspect() -> Result<(), Box<dyn std::error::Error>> {
    let settings = AppSettings::load();
    let ggpk_path = settings.ggpk_path.ok_or("No GGPK Path")?;

    println!("Opening GGPK at: {}", ggpk_path);
    let reader = GgpkReader::open(&ggpk_path)?;

    println!("--- GGPK INSPECTOR ---");


    if let Ok(Some(index_file_record)) = reader.read_file_by_path("Bundles2/_.index.bin") {
        println!("Found Bundles2/_.index.bin");
        let data = reader.get_data_slice(index_file_record.data_offset, index_file_record.data_length)?;
        let mut cursor = std::io::Cursor::new(data);

        if let Ok(bundle) = crate::bundles::bundle::Bundle::read_header(&mut cursor) {
             if let Ok(decomp) = bundle.decompress(&mut cursor) {
                 if let Ok(index) = crate::bundles::index::Index::read(&decomp) {
                     println!("Index Loaded: {} files", index.files.len());

                     let target = "data/balance/activeskills.datc64";
                     let hash = murmur_hash64a(target.to_lowercase().as_bytes(), 0x1337b33f);
                     if let Some(file) = index.files.get(&hash) {
                         println!("Verified Hash for '{}': {:016x}", target, hash);
                         println!("  Bundle Index: {}", file.bundle_index);
                     }
                 }
             }
        }
    }


    if let Ok(entries) = reader.list_files_in_directory("Bundles2") {
        println!("Bundles2 Children: {:?}", entries);
    }

    Ok(())
}

pub const USAGE: &str = "\
ggpk-explorer — Path of Exile asset explorer

USAGE:
    ggpk-explorer                          Launch the GUI
    ggpk-explorer inspect                  Print GGPK/bundle index diagnostics
    ggpk-explorer export <PATH> [OPTIONS]  Extract game files, e.g. Art/2DArt
    ggpk-explorer export-data [OPTIONS]    Write RePoE-style semantic JSON dumps

EXPORT OPTIONS:
    -o, --out <DIR>        Output folder (default: ./export)
        --textures <FMT>   dds (default), png or webp
        --audio <FMT>      ogg (default) or wav
        --data <FMT>       original (default) or json for .dat/.csd
        --dry-run          Count the files instead of writing them
    Plus the source options below (--ggpk / --steam / --cdn / --schema).

EXPORT-DATA OPTIONS:
    -o, --out <DIR>        Output folder (default: ./data)
        --ggpk <FILE>      Content.ggpk to read (default: saved setting)
        --steam <DIR>      Bundles2 folder to read (default: saved setting)
        --cdn [<VERSION>]  Read from the patch CDN, no install needed
        --schema <FILE>    dat-schema JSON (default: the cached schema.min.json)
        --only <A,B,...>   Run only these modules
        --images           Also export item, skill and buff icons as png + webp
        --trade-stats      Add official trade site search ids to the stat text
        --flat             Write into <DIR> itself, not a patch-version subfolder
        --version <VER>    Name the patch instead of reading it from the install
        --poe1             Read a Path of Exile 1 install instead of PoE 2
    -l, --list             List module names and exit
        --ls <PREFIX>      List indexed game files under a path prefix and exit
        --cat <PATH>       Write one game file to stdout (text) and exit
";

/// Parses `export-data` arguments and runs the export, reporting to stdout.
pub fn run_data_export(args: &[String]) -> Result<(), String> {
    use crate::data_export::{source::GameFiles, DataExportOptions};
    use std::path::PathBuf;
    use std::sync::Arc;

    let mut out = PathBuf::from("data");
    let mut ggpk: Option<String> = None;
    let mut steam: Option<String> = None;
    let mut cdn_version: Option<Option<String>> = None;
    let mut schema_path: Option<String> = None;
    let mut options = DataExportOptions::default();
    let mut is_poe2 = true;
    let mut ls: Option<String> = None;
    let mut cat: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        // Takes the next argument, or explains which flag was left dangling.
        let value = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            args.get(*i).cloned().ok_or_else(|| format!("{} needs a value", arg))
        };
        match arg {
            "-o" | "--out" => out = PathBuf::from(value(&mut i)?),
            "--ggpk" => ggpk = Some(value(&mut i)?),
            "--steam" => steam = Some(value(&mut i)?),
            "--schema" => schema_path = Some(value(&mut i)?),
            "--only" => {
                options.only = value(&mut i)?.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
            }
            "--cdn" => {
                let explicit = args.get(i + 1).filter(|v| !v.starts_with('-')).cloned();
                if explicit.is_some() {
                    i += 1;
                }
                cdn_version = Some(explicit);
            }
            "--ls" => ls = Some(value(&mut i)?),
            "--cat" => cat = Some(value(&mut i)?),
            "--images" => options.images = true,
            "--trade-stats" => options.trade_stats = true,
            "--flat" => options.flat = true,
            "--version" => options.version = Some(value(&mut i)?),
            "--poe1" => is_poe2 = false,
            "-l" | "--list" => {
                for name in crate::data_export::module_names() {
                    println!("{}", name);
                }
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{}", USAGE);
                return Ok(());
            }
            other => return Err(format!("Unknown option {}\n\n{}", other, USAGE)),
        }
        i += 1;
    }

    let settings = AppSettings::load();
    let schema = load_schema(schema_path.or_else(|| settings.schema_local_path.clone()))?;

    let cdn = cdn_version.map(|explicit| {
        let version = explicit.unwrap_or_else(|| settings.poe2_patch_version.clone());
        println!("Using patch CDN version {}", version);
        crate::bundles::cdn::CdnBundleLoader::new(&AppSettings::get_app_data_dir().join("cache"), Some(&version))
    });

    let ggpk = ggpk.or_else(|| if steam.is_some() || cdn.is_some() { None } else { settings.ggpk_path.clone() });
    let steam = steam.or_else(|| if ggpk.is_some() || cdn.is_some() { None } else { settings.steam_path.clone() });

    // The patch names the output folder: taken from the CDN version when one
    // was asked for, otherwise from the install's own client log.
    if options.version.is_none() && !options.flat {
        options.version = match &cdn {
            Some(cdn) => Some(cdn.patch_version().to_string()),
            None => install_root(ggpk.as_deref(), steam.as_deref())
                .as_deref()
                .and_then(crate::data_export::detect_version),
        };
        match &options.version {
            Some(version) => println!("Exporting patch {}", version),
            None => println!("Could not tell which patch this install is on; writing to {}", out.display()),
        }
    }

    let (reader, steam_loader, index) = open_source(ggpk, steam, cdn.as_ref())?;
    println!("Index loaded: {} files", index.files.len());

    let files = GameFiles::new(reader, Arc::new(index), steam_loader, cdn);

    if let Some(prefix) = ls {
        for path in files.list_dir(&prefix) {
            println!("{}", path);
        }
        return Ok(());
    }

    if let Some(path) = cat {
        use crate::dat::relational::FileSource;
        let bytes = files.fetch(&path).ok_or_else(|| format!("{} is not in the index", path))?;
        print!("{}", crate::parsers::utils::decode_text_lossy(&bytes));
        return Ok(());
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let out_display = out.display().to_string();
    std::thread::spawn(move || {
        crate::data_export::run(files, schema, is_poe2, out, options, tx);
    });

    let mut failed = 0;
    for status in rx {
        match status {
            crate::export::ExportStatus::Progress { current, total, filename } => {
                println!("[{}/{}] {}", current, total, filename);
            }
            crate::export::ExportStatus::Complete { errors, message, .. } => {
                failed = errors;
                println!("{}", message);
            }
            crate::export::ExportStatus::Error(e) => return Err(e),
        }
    }
    if failed > 0 {
        return Err(format!("{} module(s) failed — see {}/data_export_errors.log", failed, out_display));
    }
    Ok(())
}

/// Extracts raw game files under a path, converting textures, audio and DAT
/// tables on the way out — the same work the GUI's tree export does.
pub fn run_file_export(args: &[String]) -> Result<(), String> {
    use crate::ui::export_window::{AudioFormat, DataFormat, ExportSettings, PsgFormat, TextureFormat};
    use std::path::PathBuf;
    use std::sync::Arc;

    let mut prefix: Option<String> = None;
    let mut out = PathBuf::from("export");
    let mut ggpk: Option<String> = None;
    let mut steam: Option<String> = None;
    let mut cdn_version: Option<Option<String>> = None;
    let mut schema_path: Option<String> = None;
    let mut settings = ExportSettings { psg_format: PsgFormat::Original, is_poe2: true, ..Default::default() };
    let mut dry_run = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let value = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            args.get(*i).cloned().ok_or_else(|| format!("{} needs a value", arg))
        };
        match arg {
            "-o" | "--out" => out = PathBuf::from(value(&mut i)?),
            "--ggpk" => ggpk = Some(value(&mut i)?),
            "--steam" => steam = Some(value(&mut i)?),
            "--schema" => schema_path = Some(value(&mut i)?),
            "--cdn" => {
                let explicit = args.get(i + 1).filter(|v| !v.starts_with('-')).cloned();
                if explicit.is_some() {
                    i += 1;
                }
                cdn_version = Some(explicit);
            }
            "--textures" => {
                settings.texture_format = match value(&mut i)?.to_ascii_lowercase().as_str() {
                    "dds" => TextureFormat::OriginalDds,
                    "png" => TextureFormat::Png,
                    "webp" => TextureFormat::WebP,
                    other => return Err(format!("--textures takes dds, png or webp, not {}", other)),
                }
            }
            "--audio" => {
                settings.audio_format = match value(&mut i)?.to_ascii_lowercase().as_str() {
                    "ogg" | "original" => AudioFormat::Original,
                    "wav" => AudioFormat::Wav,
                    other => return Err(format!("--audio takes ogg or wav, not {}", other)),
                }
            }
            "--data" => {
                settings.data_format = match value(&mut i)?.to_ascii_lowercase().as_str() {
                    "original" => DataFormat::Original,
                    "json" => DataFormat::Json,
                    other => return Err(format!("--data takes original or json, not {}", other)),
                }
            }
            "--poe1" => settings.is_poe2 = false,
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                println!("{}", USAGE);
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("Unknown option {}\n\n{}", other, USAGE)),
            path => prefix = Some(path.to_string()),
        }
        i += 1;
    }

    let prefix = prefix.ok_or_else(|| format!("Name a path to export, e.g. Art/2DArt\n\n{}", USAGE))?;
    let saved = AppSettings::load();
    let schema = load_schema(schema_path.or_else(|| saved.schema_local_path.clone())).ok();

    let cdn = cdn_version.map(|explicit| {
        let version = explicit.unwrap_or_else(|| saved.poe2_patch_version.clone());
        crate::bundles::cdn::CdnBundleLoader::new(&AppSettings::get_app_data_dir().join("cache"), Some(&version))
    });
    let ggpk = ggpk.or_else(|| if steam.is_some() || cdn.is_some() { None } else { saved.ggpk_path.clone() });
    let steam = steam.or_else(|| if ggpk.is_some() || cdn.is_some() { None } else { saved.steam_path.clone() });
    let (reader, steam_loader, index) = open_source(ggpk, steam, cdn.as_ref())?;

    // A path with no extension is a folder; everything under it comes along.
    // An empty one would otherwise match the entries whose path never resolved.
    let wanted = prefix.trim_end_matches('/').to_ascii_lowercase();
    if wanted.is_empty() {
        return Err(format!("Name a path to export, e.g. Art/2DArt\n\n{}", USAGE));
    }
    let hashes: Vec<u64> = index
        .files
        .iter()
        .filter(|(_, file)| {
            let path = file.path.to_ascii_lowercase();
            path == wanted || path.starts_with(&format!("{}/", wanted))
        })
        .map(|(hash, _)| *hash)
        .collect();

    if hashes.is_empty() {
        return Err(format!("Nothing in the index under {}", prefix));
    }
    println!("{} files under {}", hashes.len(), prefix);
    if dry_run {
        return Ok(());
    }

    let index = Arc::new(index);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        crate::export::run_export(hashes, reader, Some(index), settings, out, cdn, steam_loader, schema, tx, None);
    });

    let mut failed = 0;
    for status in rx {
        match status {
            crate::export::ExportStatus::Progress { current, total, filename } => {
                // Only the milestones; a texture folder can hold tens of thousands.
                if current == total || current % 500 == 0 {
                    println!("[{}/{}] {}", current, total, filename);
                }
            }
            crate::export::ExportStatus::Complete { errors, message, .. } => {
                failed = errors;
                println!("{}", message);
            }
            crate::export::ExportStatus::Error(e) => return Err(e),
        }
    }
    if failed > 0 {
        return Err(format!("{} file(s) failed; see export_errors.log", failed));
    }
    Ok(())
}

/// The game folder holding the logs: the one containing `Content.ggpk`, or the
/// parent of a Steam `Bundles2` directory.
fn install_root(ggpk: Option<&str>, steam: Option<&str>) -> Option<std::path::PathBuf> {
    let path = ggpk.or(steam)?;
    std::path::Path::new(path).parent().map(|p| p.to_path_buf())
}

fn load_schema(path: Option<String>) -> Result<crate::dat::schema::Schema, String> {
    let path = path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| AppSettings::get_app_data_dir().join("schema.min.json"));
    if !path.exists() {
        return Err(format!(
            "No schema at {}. Download it from \
             https://github.com/poe-tool-dev/dat-schema/releases/latest/download/schema.min.json \
             or run the GUI once, then pass --schema.",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e))
}

type OpenedSource = (
    Option<std::sync::Arc<GgpkReader>>,
    Option<crate::bundles::steam::SteamBundleLoader>,
    crate::bundles::index::Index,
);

/// Opens whichever source was named and reads its bundle index, reusing the
/// GUI's cached index when it matches the GGPK being opened.
fn open_source(
    ggpk: Option<String>,
    steam: Option<String>,
    cdn: Option<&crate::bundles::cdn::CdnBundleLoader>,
) -> Result<OpenedSource, String> {
    use std::sync::Arc;

    if let Some(path) = ggpk {
        println!("Opening GGPK at {}", path);
        let reader = Arc::new(GgpkReader::open(&path).map_err(|e| format!("Failed to open GGPK: {}", e))?);
        let cache = AppSettings::get_app_data_dir().join(crate::settings::INDEX_CACHE_FILENAME);
        if let Ok(index) = crate::bundles::index::Index::load_from_cache(&cache) {
            println!("Index loaded from cache");
            return Ok((Some(reader), None, index));
        }
        let record = reader
            .read_file_by_path("Bundles2/_.index.bin")
            .map_err(|e| format!("Failed to find the bundle index: {}", e))?
            .ok_or("Bundles2/_.index.bin not found — this GGPK has no bundle index")?;
        let data = reader
            .get_data_slice(record.data_offset, record.data_length)
            .map_err(|e| format!("Failed to read the bundle index: {}", e))?;
        let index = read_index_bundle(data)?;
        return Ok((Some(reader), None, index));
    }

    if let Some(dir) = steam {
        println!("Opening Steam bundles at {}", dir);
        let loader = crate::bundles::steam::SteamBundleLoader::new(std::path::PathBuf::from(&dir));
        let bytes = loader.load_index_bytes().map_err(|e| format!("Failed to read _.index.bin: {}", e))?;
        let index = read_index_bundle(&bytes)?;
        return Ok((None, Some(loader), index));
    }

    if let Some(cdn) = cdn {
        println!("Fetching the bundle index from the patch CDN");
        let index = cdn.fetch_index().map_err(|e| format!("Failed to fetch the CDN index: {}", e))?;
        return Ok((None, None, index));
    }

    Err("No data source. Pass --ggpk, --steam or --cdn, or set one in the GUI first.".to_string())
}

fn read_index_bundle(data: &[u8]) -> Result<crate::bundles::index::Index, String> {
    let mut cursor = std::io::Cursor::new(data);
    let bundle = crate::bundles::bundle::Bundle::read_header(&mut cursor)
        .map_err(|e| format!("Bundle header error: {}", e))?;
    let decompressed = bundle.decompress(&mut cursor).map_err(|e| format!("Decompress error: {}", e))?;
    crate::bundles::index::Index::read(&decompressed).map_err(|e| format!("Index parse error: {}", e))
}
