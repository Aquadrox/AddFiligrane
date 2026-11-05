//! Fabrication de PDF de test et vérifications communes.
#![allow(dead_code)]

use std::path::Path;

use filigrane_core::{
    Angle, CancelToken, FontSize, Job, Layout, Mode, PageImage, Progress, Report, Rgb, WatermarkSpec,
};
use lopdf::xref::XrefType;
use lopdf::{Document, Object, ObjectId, SaveOptions, Stream, dictionary};

/// Description d'une page de test.
#[derive(Debug, Clone, Copy)]
pub struct PageSpec {
    pub media_box: [f32; 4],
    pub crop_box: Option<[f32; 4]>,
    pub rotate: Option<i64>,
    /// Contenu brut ajouté après le texte de la page.
    pub extra_content: &'static str,
}

impl PageSpec {
    pub fn a4() -> Self {
        Self {
            media_box: [0.0, 0.0, 595.0, 842.0],
            crop_box: None,
            rotate: None,
            extra_content: "",
        }
    }

    pub fn rotated(mut self, rotate: i64) -> Self {
        self.rotate = Some(rotate);
        self
    }
}

/// Variantes de structure du document.
#[derive(Debug, Clone, Copy, Default)]
pub struct Structure {
    /// Table de références et flux d'objets compressés (PDF 1.5+).
    pub modern: bool,
    /// Ressources portées par le nœud `/Pages` au lieu des pages.
    pub inherited_resources: bool,
    /// Contenu découpé en plusieurs flux (tableau `/Contents`).
    pub split_contents: bool,
}

pub fn build_pdf(path: &Path, pages: &[PageSpec], structure: Structure) {
    let mut doc = build_document(pages, structure);
    let mut file = std::fs::File::create(path).unwrap();
    if structure.modern {
        doc.save_with_options(
            &mut file,
            SaveOptions {
                use_object_streams: true,
                use_xref_streams: true,
                ..SaveOptions::default()
            },
        )
        .unwrap();
    } else {
        doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
        doc.save_to(&mut file).unwrap();
    }
}

pub fn build_document(pages: &[PageSpec], structure: Structure) -> Document {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });

    let mut kids = Vec::new();
    for (index, spec) in pages.iter().enumerate() {
        let text = format!(
            "BT /F1 18 Tf 40 {} Td (Page de test {}) Tj ET\n0 0 1 rg 40 40 60 20 re f\n{}",
            spec.media_box[3] - 60.0,
            index + 1,
            spec.extra_content
        );
        let contents: Object = if structure.split_contents {
            let (first, second) = text.split_at(text.len() / 2);
            let first = doc.add_object(Stream::new(dictionary! {}, first.as_bytes().to_vec()));
            let second = doc.add_object(Stream::new(dictionary! {}, second.as_bytes().to_vec()));
            vec![Object::Reference(first), Object::Reference(second)].into()
        } else {
            doc.add_object(Stream::new(dictionary! {}, text.into_bytes())).into()
        };
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => spec.media_box.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
            "Contents" => contents,
        };
        if !structure.inherited_resources {
            page.set("Resources", resources_id);
        }
        if let Some(crop) = spec.crop_box {
            page.set("CropBox", crop.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>());
        }
        if let Some(rotate) = spec.rotate {
            page.set("Rotate", rotate);
        }
        kids.push(Object::Reference(doc.add_object(page)));
    }

    let mut pages_dict = dictionary! { "Type" => "Pages", "Count" => kids.len() as i64, "Kids" => kids };
    if structure.inherited_resources {
        pages_dict.set("Resources", resources_id);
    }
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc
}

pub fn spec(text: &str) -> WatermarkSpec {
    WatermarkSpec {
        text: text.into(),
        color: Rgb::new(230, 0, 0),
        opacity: 1.0,
        ..WatermarkSpec::default()
    }
}

/// Filigrane unique, centré, sur la diagonale : pratique pour vérifier le placement.
pub fn centered_spec(text: &str) -> WatermarkSpec {
    WatermarkSpec {
        layout: Layout::Centered,
        size: FontSize::Auto,
        angle: Angle::Diagonal,
        ..spec(text)
    }
}

pub fn job(input: &Path, output: &Path, spec: WatermarkSpec, mode: Mode) -> Job {
    Job {
        input: input.to_owned(),
        output: output.to_owned(),
        spec,
        mode,
    }
}

pub fn run(job: &Job) -> filigrane_core::Result<Report> {
    filigrane_core::apply(job, &|_: Progress| {}, &CancelToken::new())
}

pub fn render(path: &Path, page_index: u32) -> PageImage {
    filigrane_core::render_preview(path, page_index, 400)
        .unwrap_or_else(|e| panic!("rendu de {} impossible : {e}", path.display()))
        .image
}

/// Pixels nettement rouges (le filigrane de test), en coordonnées image.
pub fn red_pixels(image: &PageImage) -> Vec<(f64, f64)> {
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, p)| p[0] > 180 && p[1] < 110 && p[2] < 110)
        .map(|(i, _)| {
            let i = i as u32;
            (f64::from(i % image.pixel_width), f64::from(i / image.pixel_width))
        })
        .collect()
}

/// Barycentre des pixels et signe de leur covariance (négatif = montant vers la droite).
pub fn centroid_and_slope(pixels: &[(f64, f64)]) -> ((f64, f64), f64) {
    let n = pixels.len() as f64;
    let (cx, cy) = pixels.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0 / n, y + p.1 / n));
    let cov = pixels.iter().map(|p| (p.0 - cx) * (p.1 - cy)).sum::<f64>() / n;
    ((cx, cy), cov)
}

/// Vérifie qu'un filigrane centré l'est bien sur l'image rendue.
pub fn assert_centered(image: &PageImage, context: &str) {
    let red = red_pixels(image);
    assert!(red.len() > 300, "{context} : filigrane absent ({} pixels)", red.len());
    let ((cx, cy), cov) = centroid_and_slope(&red);
    let (w, h) = (f64::from(image.pixel_width), f64::from(image.pixel_height));
    assert!(
        (cx - w / 2.0).abs() < w * 0.04,
        "{context} : x={cx} au lieu de {}",
        w / 2.0
    );
    assert!(
        (cy - h / 2.0).abs() < h * 0.04,
        "{context} : y={cy} au lieu de {}",
        h / 2.0
    );
    assert!(cov < 0.0, "{context} : le texte devrait monter vers la droite");
}

pub fn load(path: &Path) -> Document {
    let doc = Document::load(path).unwrap_or_else(|e| panic!("{} illisible : {e}", path.display()));
    assert!(
        doc.xref_recovery.is_none(),
        "{} : références croisées incohérentes",
        path.display()
    );
    doc
}

/// Flux de contenu d'une page, concaténés.
pub fn page_content(doc: &Document, page_id: ObjectId) -> String {
    String::from_utf8_lossy(&doc.get_page_content(page_id)).into_owned()
}
