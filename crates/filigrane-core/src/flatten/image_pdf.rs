//! Écriture en flux d'un PDF dont chaque page est une image JPEG.
//!
//! Chaque page est écrite dès qu'elle est prête ; seuls les décalages des objets sont
//! gardés en mémoire. L'arbre des pages et la table de références sont écrits à la fin.

use std::io::{self, Write};

use crate::geometry::format_number;
use crate::output::CountingWriter;

const CATALOG_ID: u32 = 1;
const PAGES_ID: u32 = 2;
const INFO_ID: u32 = 3;
const FIRST_PAGE_OBJECT_ID: u32 = 4;

/// Page encodée, prête à être écrite.
#[derive(Debug)]
pub(crate) struct EncodedPage {
    pub jpeg: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// Dimensions affichées, en points.
    pub width: f64,
    pub height: f64,
}

#[derive(Debug)]
pub(crate) struct ImagePdfWriter {
    /// Décalage de chaque objet, indexé par identifiant (l'indice 0 est inutilisé).
    offsets: Vec<u64>,
    page_ids: Vec<u32>,
}

impl ImagePdfWriter {
    pub fn start<W: Write>(out: &mut CountingWriter<W>) -> io::Result<Self> {
        // Le commentaire binaire signale aux outils de transfert que le fichier n'est pas du texte.
        out.write_all(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")?;
        Ok(Self {
            offsets: vec![0; FIRST_PAGE_OBJECT_ID as usize],
            page_ids: Vec::new(),
        })
    }

    fn begin_object<W: Write>(&mut self, out: &mut CountingWriter<W>, id: u32) -> io::Result<()> {
        let index = id as usize;
        if self.offsets.len() <= index {
            self.offsets.resize(index + 1, 0);
        }
        self.offsets[index] = out.position();
        writeln!(out, "{id} 0 obj")
    }

    fn next_id(&self) -> u32 {
        self.offsets.len() as u32
    }

    pub fn add_page<W: Write>(&mut self, out: &mut CountingWriter<W>, page: &EncodedPage) -> io::Result<()> {
        let image_id = self.next_id();
        self.begin_object(out, image_id)?;
        write!(
            out,
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            page.pixel_width,
            page.pixel_height,
            page.jpeg.len()
        )?;
        out.write_all(&page.jpeg)?;
        out.write_all(b"\nendstream\nendobj\n")?;

        let (width, height) = (format_number(page.width), format_number(page.height));
        let content = format!("q {width} 0 0 {height} 0 0 cm /Im0 Do Q\n");
        let content_id = self.next_id();
        self.begin_object(out, content_id)?;
        write!(
            out,
            "<< /Length {} >>\nstream\n{content}endstream\nendobj\n",
            content.len()
        )?;

        let page_id = self.next_id();
        self.begin_object(out, page_id)?;
        write!(
            out,
            "<< /Type /Page /Parent {PAGES_ID} 0 R /MediaBox [0 0 {width} {height}] \
             /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>\nendobj\n"
        )?;
        self.page_ids.push(page_id);
        Ok(())
    }

    pub fn finish<W: Write>(mut self, out: &mut CountingWriter<W>, producer: &str) -> io::Result<()> {
        self.begin_object(out, PAGES_ID)?;
        write!(out, "<< /Type /Pages /Count {} /Kids [", self.page_ids.len())?;
        for (index, id) in self.page_ids.iter().enumerate() {
            if index % 16 == 0 {
                out.write_all(b"\n")?;
            }
            write!(out, "{id} 0 R ")?;
        }
        out.write_all(b"] >>\nendobj\n")?;

        self.begin_object(out, CATALOG_ID)?;
        write!(out, "<< /Type /Catalog /Pages {PAGES_ID} 0 R >>\nendobj\n")?;

        self.begin_object(out, INFO_ID)?;
        write!(out, "<< /Producer {} >>\nendobj\n", literal_string(producer))?;

        let xref_start = out.position();
        write!(out, "xref\n0 {}\n0000000000 65535 f\r\n", self.offsets.len())?;
        for offset in &self.offsets[1..] {
            write!(out, "{offset:010} 00000 n\r\n")?;
        }
        write!(
            out,
            "trailer\n<< /Size {} /Root {CATALOG_ID} 0 R /Info {INFO_ID} 0 R >>\nstartxref\n{xref_start}\n%%EOF\n",
            self.offsets.len()
        )
    }
}

/// Chaîne littérale PDF ; les caractères non ASCII sont encodés en UTF-16BE.
fn literal_string(text: &str) -> String {
    if text.is_ascii() {
        let escaped = text.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
        format!("({escaped})")
    } else {
        let hex: String = text.encode_utf16().map(|u| format!("{u:04X}")).collect();
        format!("<FEFF{hex}>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_strings_are_escaped() {
        assert_eq!(literal_string("a(b)\\"), "(a\\(b\\)\\\\)");
        assert_eq!(literal_string("é"), "<FEFF00E9>");
    }

    #[test]
    fn produces_a_document_lopdf_can_read() {
        let mut out = CountingWriter::new(Vec::new());
        let mut writer = ImagePdfWriter::start(&mut out).unwrap();
        for _ in 0..3 {
            let page = EncodedPage {
                jpeg: vec![0xFF, 0xD8, 0xFF, 0xD9],
                pixel_width: 10,
                pixel_height: 20,
                width: 100.0,
                height: 200.5,
            };
            writer.add_page(&mut out, &page).unwrap();
        }
        writer.finish(&mut out, "Test").unwrap();
        let bytes = out.into_inner();
        let doc = lopdf::Document::load_mem(&bytes).unwrap();
        assert_eq!(doc.get_pages().len(), 3);
        assert!(doc.xref_recovery.is_none(), "la table de références doit être exacte");
        let first = doc.get_pages()[&1];
        let media_box = doc
            .get_dictionary(first)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(media_box[3].as_float().unwrap(), 200.5);
    }
}
