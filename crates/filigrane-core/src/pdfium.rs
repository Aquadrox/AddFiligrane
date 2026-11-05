//! Chargement de la bibliothèque PDFium, une seule fois par processus.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::Pdfium;

use crate::error::{Error, Result};

/// Variable d'environnement : chemin du fichier PDFium ou de son dossier.
pub const PDFIUM_PATH_ENV: &str = "FILIGRANE_PDFIUM";

/// Instance partagée de PDFium (ses appels sont sérialisés par pdfium-render).
pub(crate) fn pdfium() -> Result<&'static Pdfium> {
    static INSTANCE: OnceLock<Result<Pdfium, String>> = OnceLock::new();
    INSTANCE
        .get_or_init(bind)
        .as_ref()
        .map_err(|message| Error::PdfiumUnavailable(message.clone()))
}

/// Indique si PDFium est utilisable, sans renvoyer d'erreur.
pub fn pdfium_available() -> bool {
    pdfium().is_ok()
}

fn bind() -> Result<Pdfium, String> {
    let mut tried = Vec::new();
    for candidate in candidates() {
        if !candidate.is_file() {
            continue;
        }
        match Pdfium::bind_to_library(&candidate) {
            Ok(bindings) => return Ok(Pdfium::new(bindings)),
            Err(error) => tried.push(format!("{} : {error}", candidate.display())),
        }
    }
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(error) if tried.is_empty() => Err(format!("{error}")),
        Err(_) => Err(tried.join(" ; ")),
    }
}

/// Emplacements examinés, par ordre de priorité.
fn candidates() -> Vec<PathBuf> {
    let library_in = |dir: &Path| Pdfium::pdfium_platform_library_name_at_path(dir);
    let mut paths = Vec::new();
    if let Some(value) = std::env::var_os(PDFIUM_PATH_ENV) {
        let path = PathBuf::from(value);
        paths.push(library_in(&path));
        paths.push(library_in(&path.join("lib")));
        paths.push(library_in(&path.join("bin")));
        paths.push(path);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        paths.push(library_in(&dir));
        paths.push(library_in(&dir.join("pdfium")));
    }
    paths
}
