//! Ajout d'un filigrane texte à des documents PDF, y compris très longs et très lourds.
//!
//! Deux modes, au choix :
//! - [`Mode::Flatten`] (« incrusté ») : chaque page devient une image où le filigrane est
//!   peint. Impossible à retirer ; recommandé pour les pièces justificatives.
//! - [`Mode::Overlay`] (« superposé ») : le filigrane est ajouté par une mise à jour
//!   incrémentale du PDF. Quasi instantané quelle que soit la taille, le texte reste
//!   sélectionnable, mais un outil d'édition PDF peut retirer le filigrane.
//!
//! ```no_run
//! use filigrane_core::{CancelToken, Job, Mode, Progress, WatermarkSpec, apply};
//!
//! let job = Job {
//!     input: "releve.pdf".into(),
//!     output: "releve_filigrane.pdf".into(),
//!     spec: WatermarkSpec { text: "Pour dossier de location".into(), ..Default::default() },
//!     mode: Mode::default(),
//! };
//! let report = apply(&job, &|p: Progress| println!("{}", p.label()), &CancelToken::new())?;
//! println!("{} page(s) traitée(s)", report.pages_watermarked);
//! # Ok::<(), filigrane_core::Error>(())
//! ```

mod error;
mod flatten;
mod font;
mod geometry;
mod layout;
mod output;
mod overlay;
mod pages;
mod pdfium;
mod progress;
mod raster;
mod spec;

use std::path::{Path, PathBuf};

pub use crate::error::{Error, Result};
pub use crate::font::check_text;
pub use crate::pages::{PageRange, PageSelection};
pub use crate::pdfium::{PDFIUM_PATH_ENV, pdfium_available};
pub use crate::progress::{CancelToken, Progress, ProgressSink};
pub use crate::raster::PageImage;
pub use crate::spec::*;

/// Façon dont le fichier a été produit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Révision ajoutée à la fin du fichier d'origine, recopié à l'identique.
    Incremental,
    /// Fichier d'origine endommagé : document entièrement réécrit (et réparé).
    Rewritten,
    /// Pages converties en images avec le filigrane incrusté.
    Flattened,
}

/// Bilan d'un traitement réussi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub pages_total: u32,
    pub pages_watermarked: u32,
    /// Pages sélectionnées mais ignorées car leur structure est invalide.
    pub pages_skipped: u32,
    pub output_bytes: u64,
    pub method: Method,
}

/// Applique le filigrane décrit par `job`.
///
/// Le fichier de destination n'est remplacé qu'en cas de succès : une erreur ou une
/// annulation (via `cancel`) le laisse intact.
pub fn apply(job: &Job, progress: &dyn ProgressSink, cancel: &CancelToken) -> Result<Report> {
    job.spec.validate()?;
    if let Mode::Flatten(options) = job.mode {
        options.validate()?;
    }
    let block = font::TextBlock::layout(&job.spec.text, job.spec.weight)?;
    if !job.input.is_file() {
        return Err(Error::read(
            &job.input,
            std::io::Error::new(std::io::ErrorKind::NotFound, "fichier introuvable"),
        ));
    }
    output::ensure_distinct(&job.input, &job.output)?;

    match job.mode {
        Mode::Flatten(options) => flatten::run(job, &block, options, progress, cancel),
        Mode::Overlay(options) => overlay::run(job, &block, options.layer, progress, cancel),
    }
}

/// Page d'un document rendue pour l'aperçu, avec le nombre total de pages.
#[derive(Debug, Clone)]
pub struct PreviewPage {
    pub image: PageImage,
    pub page_count: u32,
}

/// Rend la page `index` (à partir de 0) de `path` pour un aperçu tenant dans un carré
/// de `max_side` pixels. Nécessite PDFium.
pub fn render_preview(path: &Path, index: u32, max_side: u32) -> Result<PreviewPage> {
    let pdfium = pdfium::pdfium()?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| flatten::open_error(path, e))?;
    let page_count = u32::try_from(document.pages().len()).unwrap_or(0);
    if page_count == 0 {
        return Err(Error::InvalidPdf("aucune page trouvée".into()));
    }
    let index = index.min(page_count - 1);
    let longest = {
        let page = document.pages().get(index as i32).map_err(|e| Error::Render {
            page: index + 1,
            message: e.to_string(),
        })?;
        f64::from(page.width().value.max(page.height().value)).max(1.0)
    };
    let image = raster::render_page(&document, index, f64::from(max_side) / longest)?;
    Ok(PreviewPage { image, page_count })
}

/// Nom de fichier proposé par défaut pour la sortie : `rapport.pdf` → `rapport_filigrane.pdf`.
pub fn default_output_path(input: &Path) -> PathBuf {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    input.with_file_name(format!("{stem}_filigrane.pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_name() {
        assert_eq!(
            default_output_path(Path::new("/tmp/Relevé 2026.PDF")),
            Path::new("/tmp/Relevé 2026_filigrane.pdf")
        );
    }
}
