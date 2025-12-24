// use std::env;
use std::path::PathBuf;

fn main() {
    // 1. Try env var
    // 2. Try local 'ooz' subdirectory (Git Submodule)
    // 3. Try sibling directory
    // 4. Hardcoded fallback
    let ooz_path = std::env::var("OOZ_PATH").map(PathBuf::from).unwrap_or_else(|_| {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let local = PathBuf::from(manifest).join("ooz");
        
        if local.exists() {
            local
        } else if PathBuf::from("../ooz").exists() {
             PathBuf::from("../ooz")
        } else {
             PathBuf::from(r"s:\_projects_\_poe2_\ooz")
        }
    });

    if !ooz_path.exists() {
        println!("cargo:warning=ooz directory not found at {:?}", ooz_path);
        // We panic here because build cannot proceed without ooz
        panic!("ooz directory not found");
    }

    println!("cargo:rerun-if-changed={}", ooz_path.display());

    let mut build = cc::Build::new();
    
    build
        .cpp(true)
        .std("c++17")
        .define("BUN_BUILD_DLL", "1")
        .define("OOZ_BUILD_DLL", "1") // Prevents kraken.cpp from defining main()
        .flag("/EHsc")
        .warnings(false) // Suppress warnings intentionally to avoid MSVC treating them as errors if configured that way, or just to clean output
        .include(&ooz_path)
        .include(ooz_path.join("simde"));

    let files = vec![
        "bun.cpp",
        "kraken.cpp",
        "bitknit.cpp",
        "lzna.cpp",
        "compr_entropy.cpp",
        "compr_kraken.cpp",
        "compr_leviathan.cpp",
        "compr_match_finder.cpp",
        "compr_mermaid.cpp",
        "compr_multiarray.cpp",
        "compr_tans.cpp",
        "compress.cpp", 
        "fnv.cpp", 
        "murmur.cpp", 
        "utf.cpp", 
        "util.cpp",
    ];

    for file in files {
        let p = ooz_path.join(file);
        if p.exists() {
             build.file(p);
        } else {
            println!("cargo:warning=File not found: {:?}", p);
        }
    }

    build.compile("ooz");

    println!("cargo:rustc-link-lib=static=ooz");

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().unwrap();
    }
}
