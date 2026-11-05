//! Filigrane PDF : interface graphique.
// Pas de console noire derrière la fenêtre sous Windows (sauf en débogage).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod preview;
mod settings;
mod style;
mod worker;

fn main() -> eframe::Result {
    // Un PDF passé en argument (« Ouvrir avec », glisser sur l'icône) est ouvert d'emblée.
    let initial_input = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon/icon.png")).expect("icône PNG valide");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Filigrane PDF")
            .with_app_id("filigrane-pdf")
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([900.0, 640.0])
            .with_icon(icon)
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Filigrane PDF",
        options,
        Box::new(|cc| Ok(Box::new(app::FiligraneApp::new(cc, initial_input)))),
    )
}
