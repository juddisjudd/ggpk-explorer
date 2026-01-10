mod app;
pub mod tree_view; // pub needed for actions
mod content_view;
mod dat_viewer;
pub mod hex_viewer;
pub mod settings_window;
pub mod export_window;
pub mod json_viewer;
pub mod syntax;
pub mod texture_loader;

fn load_icon() -> eframe::egui::IconData {
    let (icon_rgba, icon_width, icon_height) = {
        let icon_bytes = include_bytes!("../../assets/icon-256x256.png");
        let image = image::load_from_memory(icon_bytes)
            .expect("Failed to open icon path")
            .into_rgba8();
        let (width, height) = image.dimensions();
        (image.into_raw(), width, height)
    };
    
    eframe::egui::IconData {
        rgba: icon_rgba,
        width: icon_width,
        height: icon_height,
    }
}

fn configure_cjk_fonts(ctx: &eframe::egui::Context) {
    let mut fonts = eframe::egui::FontDefinitions::default();
    
    // Define font groups to load. We want one from each group if possible.
    let font_groups = [
        // Group 1: CJK (Chinese, Japanese, Korean)
        (
            "cjk", 
            vec![
                "C:/Windows/Fonts/malgun.ttf",   // Korean / General (Malgun Gothic)
                "C:/Windows/Fonts/msyh.ttf",     // Chinese (Microsoft YaHei) - specific file
                "C:/Windows/Fonts/msyh.ttc",     // Chinese (Microsoft YaHei) - collection
                "C:/Windows/Fonts/meiryo.ttc",   // Japanese (Meiryo)
                "C:/Windows/Fonts/simhei.ttf",   // Simplified Chinese (SimHei)
                "C:/Windows/Fonts/arialuni.ttf", // Arial Unicode MS
            ]
        ),
        // Group 2: Thai
        (
            "thai",
            vec![
                "C:/Windows/Fonts/LeelawUI.ttf", // Leelawadee UI (Win 10/11 Standard)
                "C:/Windows/Fonts/Leelawad.ttf", // Leelawadee (Older)
                "C:/Windows/Fonts/tahoma.ttf",   // Tahoma (Common fallback)
            ]
        )
    ];

    for (name, candidates) in font_groups {
        for path_str in candidates {
            let path = std::path::Path::new(path_str);
            if path.exists() {
                 if let Ok(data) = std::fs::read(path) {
                     println!("Loading {} font from: {}", name, path_str);
                     
                     fonts.font_data.insert(
                        name.to_owned(),
                        eframe::egui::FontData::from_owned(data),
                     );
                     
                     // Append to default families as fallback
                     if let Some(vec) = fonts.families.get_mut(&eframe::egui::FontFamily::Proportional) {
                         vec.push(name.to_owned());
                     }
                     if let Some(vec) = fonts.families.get_mut(&eframe::egui::FontFamily::Monospace) {
                         vec.push(name.to_owned());
                     }
                     
                     break; // Found a valid font for this group, stop searching this group
                 }
            }
        }
    }
    
    ctx.set_fonts(fonts);
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("GGPK Explorer")
            .with_decorations(true)
            .with_icon(load_icon()),
        ..Default::default()
    };
    
    eframe::run_native(
        "GGPK Explorer",
        options,
        Box::new(|cc| {
            configure_cjk_fonts(&cc.egui_ctx);
            Ok(Box::new(app::ExplorerApp::new(cc)))
        }),
    )
}
