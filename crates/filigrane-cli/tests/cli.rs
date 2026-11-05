//! Exécution réelle du binaire.

use std::path::Path;
use std::process::Command;

use lopdf::{Document, Object, Stream, dictionary};

fn write_sample_pdf(path: &Path, pages: usize) {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let kids: Vec<Object> = (0..pages)
        .map(|_| {
            let content = doc.add_object(Stream::new(dictionary! {}, b"0 0 1 rg 50 50 100 100 re f".to_vec()));
            doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            })
            .into()
        })
        .collect();
    doc.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Count" => pages as i64, "Kids" => kids }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_filigrane-cli"))
}

#[test]
fn watermarks_with_default_output_name() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("avis.pdf");
    write_sample_pdf(&input, 2);

    let output = cli()
        .args([
            input.to_str().unwrap(),
            "-t",
            "Pour dossier de location",
            "--mode",
            "superpose",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let produced = dir.path().join("avis_filigrane.pdf");
    assert_eq!(Document::load(&produced).unwrap().get_pages().len(), 2);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("2 page(s) sur 2"), "{stdout}");
}

#[test]
fn flatten_mode_works_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let (input, produced) = (dir.path().join("a.pdf"), dir.path().join("b.pdf"));
    write_sample_pdf(&input, 1);

    let output = cli()
        .args([
            input.to_str().unwrap(),
            "-o",
            produced.to_str().unwrap(),
            "-t",
            "COPIE",
            "--dpi",
            "72",
            "-q",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty(), "--quiet ne doit rien afficher");
    assert_eq!(Document::load(&produced).unwrap().get_pages().len(), 1);
}

#[test]
fn errors_are_reported_in_french_with_a_failure_code() {
    let dir = tempfile::tempdir().unwrap();
    let output = cli()
        .args([dir.path().join("absent.pdf").to_str().unwrap(), "-t", "X"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Erreur : Impossible de lire"), "{stderr}");
}
