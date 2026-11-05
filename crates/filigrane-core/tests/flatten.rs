//! Mode incrusté : rendu PDFium, filigrane peint dans les pixels, écriture en flux.

mod common;

use std::fs;

use common::*;
use filigrane_core::{CancelToken, Error, FlattenOptions, Method, Mode, PageSelection, Progress};

fn flatten(dpi: u32) -> Mode {
    Mode::Flatten(FlattenOptions { dpi, jpeg_quality: 85 })
}

#[test]
fn pages_become_images_with_the_watermark_burnt_in() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(
        &input,
        &[PageSpec::a4(), PageSpec::a4()],
        Structure {
            modern: true,
            ..Structure::default()
        },
    );

    let report = run(&job(&input, &output, centered_spec("CONFIDENTIEL"), flatten(100))).unwrap();
    assert_eq!(report.method, Method::Flattened);
    assert_eq!((report.pages_total, report.pages_watermarked), (2, 2));
    assert_eq!(report.output_bytes, fs::metadata(&output).unwrap().len());

    let doc = load(&output);
    for (_, page_id) in doc.get_pages() {
        let content = page_content(&doc, page_id);
        assert!(content.contains("/Im0 Do"), "{content}");
        assert!(
            !content.contains("Page de test"),
            "le texte d'origine ne doit plus exister"
        );
        let page = doc.get_dictionary(page_id).unwrap();
        let media_box: Vec<f32> = page
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_float().unwrap())
            .collect();
        assert_eq!(media_box, vec![0.0, 0.0, 595.0, 842.0]);
        let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
        let image_id = resources
            .get(b"XObject")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Im0")
            .unwrap()
            .as_reference()
            .unwrap();
        let image = doc.get_object(image_id).unwrap().as_stream().unwrap();
        assert_eq!(image.dict.get(b"Filter").unwrap().as_name().unwrap(), b"DCTDecode");
        // 595 pt à 100 ppp ≈ 826 pixels.
        assert_eq!(image.dict.get(b"Width").unwrap().as_i64().unwrap(), 826);
    }
    assert_centered(&render(&output, 0), "page 1");
}

#[test]
fn rotated_pages_keep_their_displayed_orientation() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &[PageSpec::a4().rotated(90)], Structure::default());

    run(&job(&input, &output, centered_spec("PAYSAGE"), flatten(72))).unwrap();
    let doc = load(&output);
    let page = doc.get_dictionary(doc.get_pages()[&1]).unwrap();
    let media_box: Vec<f32> = page
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_float().unwrap())
        .collect();
    assert_eq!(
        media_box,
        vec![0.0, 0.0, 842.0, 595.0],
        "la page affichée est en paysage"
    );
    assert!(!page.has(b"Rotate"));
    assert_centered(&render(&output, 0), "page 1");
}

#[test]
fn unselected_pages_are_kept_without_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &vec![PageSpec::a4(); 3], Structure::default());

    let mut spec = centered_spec("COPIE");
    spec.pages = "2".parse::<PageSelection>().unwrap();
    let report = run(&job(&input, &output, spec, flatten(72))).unwrap();
    assert_eq!((report.pages_total, report.pages_watermarked), (3, 1));
    assert_eq!(load(&output).get_pages().len(), 3);
    assert!(red_pixels(&render(&output, 0)).is_empty());
    assert_centered(&render(&output, 1), "page 2");
    assert!(red_pixels(&render(&output, 2)).is_empty());
}

#[test]
fn tiled_watermark_covers_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &[PageSpec::a4()], Structure::default());

    run(&job(&input, &output, spec("Pour mandat de vente"), flatten(100))).unwrap();
    let image = render(&output, 0);
    let red = red_pixels(&image);
    // Chaque quart de la page reçoit une part du filigrane.
    let (w, h) = (f64::from(image.pixel_width), f64::from(image.pixel_height));
    for (qx, qy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let count = red
            .iter()
            .filter(|(x, y)| {
                (*x >= qx * w / 2.0 && *x < (qx + 1.0) * w / 2.0) && (*y >= qy * h / 2.0 && *y < (qy + 1.0) * h / 2.0)
            })
            .count();
        assert!(count > 200, "quart ({qx}, {qy}) : {count} pixels");
    }
}

#[test]
fn cancellation_leaves_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &vec![PageSpec::a4(); 12], Structure::default());

    let cancel = CancelToken::new();
    let progress = |p: Progress| {
        if matches!(p, Progress::Pages { done, .. } if done > 0) {
            cancel.cancel();
        }
    };
    let error = filigrane_core::apply(&job(&input, &output, spec("X"), flatten(72)), &progress, &cancel).unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn invalid_options_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &[PageSpec::a4()], Structure::default());
    let error = run(&job(&input, &output, spec("X"), flatten(10))).unwrap_err();
    assert!(matches!(error, Error::InvalidParameter(_)), "{error:?}");

    fs::write(&input, b"%PDF-1.7 tronque").unwrap();
    let error = run(&job(&input, &output, spec("X"), flatten(72))).unwrap_err();
    assert!(matches!(error, Error::InvalidPdf(_)), "{error:?}");
    assert!(!output.exists());
}
