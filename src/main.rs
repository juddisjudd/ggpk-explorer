#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ggpk;
mod dat;
mod ooz;
pub mod bundles;
mod ui;
pub mod settings;
pub mod cli;
pub mod update;
pub mod export;

fn main() -> eframe::Result<()> {
    env_logger::init();
    

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "inspect" {

        if let Err(e) = cli::run_inspect() {
            eprintln!("Inspection failed: {}", e);
        }
        return Ok(());
    }


    ui::run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ooz_link() {
        println!("Testing ooz linking...");
        unsafe {
            let ptr = ooz::sys::BunMemAlloc(10);
            assert!(!ptr.is_null());
            ooz::sys::BunMemFree(ptr);
        }
    }
}
