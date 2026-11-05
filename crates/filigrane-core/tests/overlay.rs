//! Mode superposé : mise à jour incrémentale, placement, cas limites de structure.

mod common;

use std::fs;

use common::*;
use filigrane_core::{CancelToken, Error, Layer, Method, Mode, OverlayOptions, PageSelection, Progress};
use lopdf::{Object, dictionary};

fn overlay() -> Mode {
    Mode::Overlay(OverlayOptions::default())
}

#[test]
fn classic_file_gets_an_incremental_revision() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(
        &input,
        &[PageSpec::a4(), PageSpec::a4(), PageSpec::a4()],
        Structure::default(),
    );

    let report = run(&job(&input, &output, centered_spec("CONFIDENTIEL"), overlay())).unwrap();
    assert_eq!(report.method, Method::Incremental);
    assert_eq!(
        (report.pages_total, report.pages_watermarked, report.pages_skipped),
        (3, 3, 0)
    );

    let original = fs::read(&input).unwrap();
    let produced = fs::read(&output).unwrap();
    assert_eq!(report.output_bytes, produced.len() as u64);
    assert!(
        produced.starts_with(&original),
        "le fichier d'origine doit être recopié à l'identique"
    );
    assert!(
        produced.len() - original.len() < 30_000,
        "révision trop lourde : {}",
        produced.len() - original.len()
    );

    let doc = load(&output);
    assert_eq!(doc.get_pages().len(), 3);
    for page in 0..3 {
        assert_centered(&render(&output, page), &format!("page {}", page + 1));
    }
}

#[test]
fn modern_file_with_object_streams_is_supported() {
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
    assert!(lopdf::Document::load(&input).unwrap().objects.len() > 3);

    let report = run(&job(&input, &output, centered_spec("COPIE"), overlay())).unwrap();
    assert_eq!(report.method, Method::Incremental);
    assert!(fs::read(&output).unwrap().starts_with(&fs::read(&input).unwrap()));
    load(&output);
    assert_centered(&render(&output, 1), "page 2");
}

#[test]
fn rotated_and_cropped_pages_are_watermarked_as_displayed() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    let cropped = PageSpec {
        media_box: [0.0, 0.0, 800.0, 600.0],
        crop_box: Some([100.0, 50.0, 700.0, 550.0]),
        ..PageSpec::a4()
    };
    let pages = [
        PageSpec::a4().rotated(90),
        PageSpec::a4().rotated(180),
        PageSpec::a4().rotated(270),
        PageSpec::a4().rotated(-90),
        cropped,
        cropped.rotated(90),
        PageSpec {
            media_box: [-300.0, -400.0, 300.0, 400.0],
            ..PageSpec::a4()
        },
    ];
    build_pdf(&input, &pages, Structure::default());

    run(&job(&input, &output, centered_spec("CONFIDENTIEL"), overlay())).unwrap();
    for index in 0..pages.len() as u32 {
        assert_centered(&render(&output, index), &format!("page {}", index + 1));
    }
}

#[test]
fn original_graphics_state_does_not_leak_into_the_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    // Contenu qui agrandit le repère et change la couleur sans restaurer l'état.
    let page = PageSpec {
        extra_content: "3 0 0 3 50 50 cm 0 1 0 rg",
        ..PageSpec::a4()
    };
    build_pdf(
        &input,
        &[page],
        Structure {
            split_contents: true,
            ..Structure::default()
        },
    );

    run(&job(&input, &output, centered_spec("ISOLÉ"), overlay())).unwrap();
    assert_centered(&render(&output, 0), "page 1");
}

#[test]
fn inherited_and_shared_resources_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(
        &input,
        &[PageSpec::a4(), PageSpec::a4()],
        Structure {
            inherited_resources: true,
            ..Structure::default()
        },
    );

    run(&job(&input, &output, centered_spec("COPIE"), overlay())).unwrap();
    let doc = load(&output);
    let pages = doc.get_pages();
    let resources: Vec<_> = pages
        .values()
        .map(|id| {
            doc.get_dictionary(*id)
                .unwrap()
                .get(b"Resources")
                .unwrap()
                .as_reference()
                .unwrap()
        })
        .collect();
    assert_eq!(
        resources[0], resources[1],
        "les ressources partagées ne doivent être copiées qu'une fois"
    );
    let dict = doc.get_dictionary(resources[0]).unwrap();
    assert!(
        dict.get(b"Font").unwrap().as_dict().unwrap().has(b"F1"),
        "la police d'origine doit rester"
    );
    assert!(dict.get(b"XObject").unwrap().as_dict().unwrap().has(b"FiligranePDF"));
}

#[test]
fn existing_resource_names_are_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    let mut doc = build_document(&[PageSpec::a4()], Structure::default());
    let page_id = doc.get_pages()[&1];
    let resources_id = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .as_reference()
        .unwrap();
    let existing = doc.add_object(dictionary! { "Type" => "XObject", "Subtype" => "Form" });
    doc.get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", dictionary! { "FiligranePDF" => existing });
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    doc.save(&input).unwrap();

    run(&job(&input, &output, centered_spec("COPIE"), overlay())).unwrap();
    let doc = load(&output);
    let page = doc.get_dictionary(doc.get_pages()[&1]).unwrap();
    let resources = doc
        .get_dictionary(page.get(b"Resources").unwrap().as_reference().unwrap())
        .unwrap();
    let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    assert_eq!(xobjects.get(b"FiligranePDF").unwrap().as_reference().unwrap(), existing);
    assert!(xobjects.has(b"FiligranePDF1"));
    assert!(page_content(&doc, doc.get_pages()[&1]).contains("/FiligranePDF1 Do"));
}

#[test]
fn only_selected_pages_are_modified() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &vec![PageSpec::a4(); 5], Structure::default());

    let mut spec = centered_spec("COPIE");
    spec.pages = "2, 4-".parse::<PageSelection>().unwrap();
    let report = run(&job(&input, &output, spec, overlay())).unwrap();
    assert_eq!(report.pages_watermarked, 3);

    let doc = load(&output);
    for (number, id) in doc.get_pages() {
        let marked = page_content(&doc, id).contains("/FiligranePDF Do");
        assert_eq!(marked, [2, 4, 5].contains(&number), "page {number}");
    }
    assert!(red_pixels(&render(&output, 0)).is_empty());
    assert_centered(&render(&output, 1), "page 2");
}

#[test]
fn background_layer_draws_before_the_content() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &[PageSpec::a4()], Structure::default());

    let mode = Mode::Overlay(OverlayOptions {
        layer: Layer::Background,
    });
    run(&job(&input, &output, centered_spec("FOND"), mode)).unwrap();
    let doc = load(&output);
    let content = page_content(&doc, doc.get_pages()[&1]);
    let watermark = content.find("/FiligranePDF Do").unwrap();
    let text = content.find("Page de test").unwrap();
    assert!(watermark < text, "{content}");
    assert_centered(&render(&output, 0), "page 1");
}

#[test]
fn watermark_text_is_extractable_and_marked_as_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &[PageSpec::a4()], Structure::default());
    run(&job(
        &input,
        &output,
        spec("Pour mandat de vente – été 2026"),
        overlay(),
    ))
    .unwrap();

    let doc = load(&output);
    let content = page_content(&doc, doc.get_pages()[&1]);
    assert!(content.contains("/Artifact <</Type /Pagination /Subtype /Watermark>> BDC"));
    // La table ToUnicode permet de retrouver le texte exact (copier-coller, recherche).
    let to_unicode = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .find_map(|s| {
            let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
            let text = String::from_utf8_lossy(&data).into_owned();
            text.contains("beginbfchar").then_some(text)
        })
        .expect("table ToUnicode");
    for ch in "Pour mandat de vente – été 2026".chars().filter(|c| *c != ' ') {
        let code = format!("<{:04X}>", u32::from(ch));
        assert!(to_unicode.contains(&code), "{ch} absent de ToUnicode");
    }
}

#[test]
fn damaged_cross_references_fall_back_to_a_full_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &[PageSpec::a4(), PageSpec::a4()], Structure::default());
    // Fausse la position annoncée de la table de références.
    let mut bytes = fs::read(&input).unwrap();
    let marker = bytes.windows(9).rposition(|w| w == b"startxref").unwrap();
    let number_start = marker + 10;
    let number_end = number_start + bytes[number_start..].iter().position(|b| !b.is_ascii_digit()).unwrap();
    let wrong = format!("{:0width$}", 17, width = number_end - number_start);
    bytes.splice(number_start..number_end, wrong.bytes());
    fs::write(&input, &bytes).unwrap();

    let report = run(&job(&input, &output, centered_spec("RÉPARÉ"), overlay())).unwrap();
    assert_eq!(report.method, Method::Rewritten);
    load(&output);
    assert_centered(&render(&output, 1), "page 2");
}

#[test]
fn encrypted_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    let mut doc = build_document(&[PageSpec::a4()], Structure::default());
    let encrypt = doc.add_object(dictionary! {
        "Filter" => "Standard", "V" => 2, "R" => 3, "Length" => 128, "P" => -4,
        "O" => Object::string_literal(vec![0u8; 32]), "U" => Object::string_literal(vec![0u8; 32]),
    });
    doc.trailer.set("Encrypt", encrypt);
    doc.trailer.set(
        "ID",
        vec![
            Object::string_literal(vec![1u8; 16]),
            Object::string_literal(vec![1u8; 16]),
        ],
    );
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    doc.save(&input).unwrap();

    let error = run(&job(&input, &output, spec("X"), overlay())).unwrap_err();
    assert!(matches!(error, Error::EncryptedOverlay), "{error:?}");
    assert!(!output.exists());
}

#[test]
fn cancellation_leaves_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    build_pdf(&input, &vec![PageSpec::a4(); 3], Structure::default());

    let cancel = CancelToken::new();
    let progress = |p: Progress| {
        if matches!(p, Progress::Pages { .. }) {
            cancel.cancel();
        }
    };
    let error = filigrane_core::apply(&job(&input, &output, spec("X"), overlay()), &progress, &cancel).unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "seul le fichier source doit rester"
    );
}

#[test]
fn invalid_inputs_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));

    fs::write(&input, b"ceci n'est pas un PDF").unwrap();
    let error = run(&job(&input, &output, spec("X"), overlay())).unwrap_err();
    assert!(matches!(error, Error::InvalidPdf(_)), "{error:?}");

    build_pdf(&input, &[PageSpec::a4()], Structure::default());
    let error = run(&job(&input, &input, spec("X"), overlay())).unwrap_err();
    assert!(matches!(error, Error::SameInputOutput), "{error:?}");

    let error = run(&job(&dir.path().join("absent.pdf"), &output, spec("X"), overlay())).unwrap_err();
    assert!(matches!(error, Error::Read { .. }), "{error:?}");

    let mut out_of_range = spec("X");
    out_of_range.pages = "5-9".parse().unwrap();
    let error = run(&job(&input, &output, out_of_range, overlay())).unwrap_err();
    assert!(matches!(error, Error::NoPageSelected { total: 1, .. }), "{error:?}");

    let error = run(&job(&input, &output, spec("   "), overlay())).unwrap_err();
    assert!(matches!(error, Error::EmptyText), "{error:?}");
    assert!(!output.exists());
}

#[test]
fn heavy_file_is_copied_without_being_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    // 40 Mo de données incompressibles dans un flux attaché à la page.
    let mut doc = build_document(&[PageSpec::a4(); 2], Structure::default());
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let noise: Vec<u8> = (0..40_000_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let blob = doc.add_object(lopdf::Stream::new(dictionary! {}, noise));
    let page_id = doc.get_pages()[&1];
    doc.get_dictionary_mut(page_id).unwrap().set("PieceInfo", blob);
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    doc.save(&input).unwrap();

    let mut copied = std::sync::Mutex::new(0u64);
    let progress = |p: Progress| {
        if let Progress::Copying { done, .. } = p {
            *copied.lock().unwrap() = done;
        }
    };
    let report = filigrane_core::apply(
        &job(&input, &output, spec("LOURD"), overlay()),
        &progress,
        &CancelToken::new(),
    )
    .unwrap();
    let input_len = fs::metadata(&input).unwrap().len();
    assert_eq!(*copied.get_mut().unwrap(), input_len);
    assert!(report.output_bytes - input_len < 100_000);
    load(&output);
}

#[test]
fn absurd_page_sizes_are_skipped_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("in.pdf"), dir.path().join("out.pdf"));
    let huge = PageSpec {
        media_box: [0.0, 0.0, 1e30, 1e30],
        ..PageSpec::a4()
    };
    build_pdf(&input, &[huge, PageSpec::a4()], Structure::default());

    let started = std::time::Instant::now();
    let report = run(&job(&input, &output, spec("X"), overlay())).unwrap();
    assert!(started.elapsed().as_secs() < 5);
    assert_eq!((report.pages_watermarked, report.pages_skipped), (1, 1));
    assert_centered_or_tiled(&output);
}

fn assert_centered_or_tiled(output: &std::path::Path) {
    assert!(
        !red_pixels(&render(output, 1)).is_empty(),
        "la page normale doit être filigranée"
    );
}
