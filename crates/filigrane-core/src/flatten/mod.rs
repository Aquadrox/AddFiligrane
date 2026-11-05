//! Mode incrusté : chaque page est rendue en image par PDFium, le filigrane est peint
//! dans l'image, puis la page est réécrite en JPEG. Le filigrane fait alors partie des
//! pixels : il ne peut plus être retiré en éditant le PDF.
//!
//! Les pages sont traitées par lots : rendu séquentiel (PDFium n'est pas parallélisable),
//! puis dessin et compression en parallèle, puis écriture dans l'ordre. La mémoire
//! utilisée est bornée par la taille d'un lot et par la réouverture périodique du
//! document, quelle que soit sa longueur.

mod image_pdf;

use image::ExtendedColorType;
use image::codecs::jpeg::JpegEncoder;
use pdfium_render::prelude::{PdfiumError, PdfiumInternalError};
use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::font::TextBlock;
use crate::output::AtomicOutput;
use crate::pdfium::pdfium;
use crate::progress::{CancelToken, Progress, ProgressSink};
use crate::raster::{PageImage, render_page};
use crate::spec::{FlattenOptions, Job};
use crate::{Method, Report};

use self::image_pdf::{EncodedPage, ImagePdfWriter};

const MAX_BATCH: usize = 4;
/// Volume de données du document que PDFium peut garder en mémoire entre deux réouvertures.
const READ_BUDGET: u64 = 192 << 20;

pub(crate) fn run(
    job: &Job,
    block: &TextBlock,
    options: FlattenOptions,
    progress: &dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Report> {
    progress.report(Progress::Opening);
    let pdfium = pdfium()?;
    let open = || {
        pdfium
            .load_pdf_from_file(&job.input, None)
            .map_err(|e| open_error(&job.input, e))
    };
    let mut document = open()?;
    let pages_total = u32::try_from(document.pages().len()).unwrap_or(0);
    if pages_total == 0 {
        return Err(Error::InvalidPdf("aucune page trouvée".into()));
    }
    job.spec.pages.ensure_matches(pages_total)?;
    let outline = block.outline().ok_or(Error::EmptyText)?;

    let mut output = AtomicOutput::create(&job.output)?;
    let mut writer = ImagePdfWriter::start(output.writer()).map_err(|e| output.write_error(e))?;
    let pixels_per_point = f64::from(options.dpi) / 72.0;
    let batch_size = rayon::current_num_threads().clamp(1, MAX_BATCH) as u32;
    let reopen_every = reopen_interval(&job.input, pages_total);
    let mut pages_since_open = 0;
    let mut pages_watermarked = 0;

    progress.report(Progress::Pages {
        done: 0,
        total: pages_total,
    });
    let mut start = 0;
    while start < pages_total {
        cancel.check()?;
        if pages_since_open >= reopen_every {
            // PDFium garde en mémoire tous les objets lus : rouvrir le document les libère.
            drop(document);
            document = open()?;
            pages_since_open = 0;
        }
        let end = (start + batch_size).min(pages_total);
        pages_since_open += end - start;
        let rendered = (start..end)
            .map(|index| render_page(&document, index, pixels_per_point))
            .collect::<Result<Vec<PageImage>>>()?;

        let encoded = rendered
            .into_par_iter()
            .zip(start..end)
            .map(|(mut image, index)| {
                let selected = job.spec.pages.includes(index + 1);
                if selected {
                    image.draw_block(block, &outline, &job.spec);
                }
                encode(&image, options.jpeg_quality, index).map(|page| (page, selected))
            })
            .collect::<Result<Vec<_>>>()?;

        for (page, selected) in &encoded {
            writer
                .add_page(output.writer(), page)
                .map_err(|e| output.write_error(e))?;
            pages_watermarked += u32::from(*selected);
        }
        progress.report(Progress::Pages {
            done: end,
            total: pages_total,
        });
        start = end;
    }

    cancel.check()?;
    progress.report(Progress::Finalizing);
    let producer = concat!("Filigrane PDF ", env!("CARGO_PKG_VERSION"));
    writer
        .finish(output.writer(), producer)
        .map_err(|e| output.write_error(e))?;
    let output_bytes = output.commit()?;

    Ok(Report {
        pages_total,
        pages_watermarked,
        pages_skipped: 0,
        output_bytes,
        method: Method::Flattened,
    })
}

/// Nombre de pages à traiter avant de rouvrir le document, pour qu'environ
/// [`READ_BUDGET`] octets du fichier au plus restent en mémoire dans PDFium.
fn reopen_interval(path: &std::path::Path, pages_total: u32) -> u32 {
    let file_size = std::fs::metadata(path).map_or(0, |m| m.len());
    let bytes_per_page = (file_size / u64::from(pages_total.max(1))).max(1);
    u32::try_from(READ_BUDGET / bytes_per_page)
        .unwrap_or(u32::MAX)
        .clamp(8, 2_000)
}

fn encode(image: &PageImage, quality: u8, index: u32) -> Result<EncodedPage> {
    let mut jpeg = Vec::with_capacity(image.rgba.len() / 16);
    JpegEncoder::new_with_quality(&mut jpeg, quality)
        .encode(
            &image.to_rgb(),
            image.pixel_width,
            image.pixel_height,
            ExtendedColorType::Rgb8,
        )
        .map_err(|e| Error::Render {
            page: index + 1,
            message: e.to_string(),
        })?;
    Ok(EncodedPage {
        jpeg,
        pixel_width: image.pixel_width,
        pixel_height: image.pixel_height,
        width: image.width,
        height: image.height,
    })
}

pub(crate) fn open_error(path: &std::path::Path, error: PdfiumError) -> Error {
    match error {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => Error::PasswordProtected,
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FileError) | PdfiumError::IoError(_) => {
            Error::read(path, std::io::Error::other(error.to_string()))
        }
        other => Error::InvalidPdf(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_pages_trigger_more_frequent_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pdf");
        std::fs::write(&path, vec![0u8; 10_000_000]).unwrap();
        assert_eq!(reopen_interval(&path, 10), 201);
        assert_eq!(reopen_interval(&path, 1), 20);
        assert_eq!(reopen_interval(&path, 100_000), 2_000);
        assert_eq!(reopen_interval(&dir.path().join("absent.pdf"), 5), 2_000);
    }
}
