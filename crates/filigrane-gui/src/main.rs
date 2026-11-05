//! Filigrane PDF : interface graphique.
// Pas de console noire derrière la fenêtre sous Windows (sauf en débogage).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod preview;
mod settings;
mod style;
mod worker;

use std::path::PathBuf;

use eframe::Renderer;

/// Force le moteur d'affichage : `wgpu` (Direct3D 12 / Vulkan) ou `glow` (OpenGL).
const RENDERER_ENV: &str = "FILIGRANE_RENDERER";

fn main() -> eframe::Result {
    // Un PDF passé en argument (« Ouvrir avec », glisser sur l'icône) est ouvert d'emblée.
    let initial_input = std::env::args_os().nth(1).map(PathBuf::from);
    let preferred = match std::env::var(RENDERER_ENV).as_deref() {
        Ok("glow" | "opengl") => Renderer::Glow,
        _ => Renderer::Wgpu,
    };

    let mut result = run(preferred, initial_input.clone());
    // Sans pilote graphique récent (machine virtuelle, bureau à distance…), wgpu peut
    // échouer à démarrer : on réessaie avec OpenGL avant d'abandonner.
    if let Err(error) = &result
        && preferred == Renderer::Wgpu
    {
        eprintln!("Affichage wgpu indisponible ({error}), essai avec OpenGL.");
        result = run(Renderer::Glow, initial_input);
    }
    if let Err(error) = &result {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Filigrane PDF")
            .set_description(format!(
                "Impossible d'ouvrir la fenêtre : {error}\n\nMettez à jour le pilote de la carte graphique, \
                 ou utilisez filigrane-cli.exe."
            ))
            .show();
    }
    result
}

fn run(renderer: Renderer, initial_input: Option<PathBuf>) -> eframe::Result {
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
        renderer,
        // Indispensable pour pouvoir relancer la fenêtre avec un autre moteur.
        run_and_return: true,
        ..Default::default()
    };
    eframe::run_native(
        "Filigrane PDF",
        options,
        Box::new(move |cc| Ok(Box::new(app::FiligraneApp::new(cc, initial_input)))),
    )
}
