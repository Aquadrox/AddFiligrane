//! Génère un PDF de test volumineux, écrit en flux (mémoire constante).
//!
//! ```text
//! cargo run --release -p filigrane-core --example generate_pdf -- sortie.pdf 5000 [Mo_d_image_par_page]
//! ```
//! Chaque page contient un texte et, si demandé, une photo JPEG (bruit) d'environ la
//! taille indiquée : de quoi simuler un long scan.

use std::fs::File;
use std::io::{BufWriter, Write};

use image::ExtendedColorType;
use image::codecs::jpeg::JpegEncoder;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let usage = "usage : generate_pdf <sortie.pdf> <pages> [Mo d'image par page]";
    let path = args.next().expect(usage);
    let pages: u32 = args.next().and_then(|v| v.parse().ok()).expect(usage);
    let image_mb: f64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);

    let jpeg = (image_mb > 0.0).then(|| noise_jpeg(image_mb));
    let mut out = Counting {
        inner: BufWriter::with_capacity(1 << 20, File::create(&path)?),
        position: 0,
    };
    let mut offsets: Vec<u64> = vec![0; 4];
    let mut kids = Vec::with_capacity(pages as usize);
    out.write_all(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")?;

    let begin = |out: &mut Counting, offsets: &mut Vec<u64>, id: usize| -> std::io::Result<()> {
        if offsets.len() <= id {
            offsets.resize(id + 1, 0);
        }
        offsets[id] = out.position;
        writeln!(out, "{id} 0 obj")
    };

    // 3 : police partagée.
    begin(&mut out, &mut offsets, 3)?;
    out.write_all(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n")?;

    for page in 1..=pages {
        let mut next = offsets.len();
        let image_id = jpeg.as_ref().map(|(data, w, h)| {
            let id = next;
            next += 1;
            (id, data, *w, *h)
        });
        if let Some((id, data, w, h)) = image_id {
            begin(&mut out, &mut offsets, id)?;
            write!(
                out,
                "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
                data.len()
            )?;
            out.write_all(data)?;
            out.write_all(b"\nendstream\nendobj\n")?;
        }
        let mut content = String::new();
        if image_id.is_some() {
            content.push_str("q 495 0 0 600 50 150 cm /Im0 Do Q\n");
        }
        content.push_str(&format!("BT /F1 28 Tf 60 780 Td (Page {page} sur {pages}) Tj ET\n"));
        let content_id = next;
        begin(&mut out, &mut offsets, content_id)?;
        write!(
            out,
            "<< /Length {} >>\nstream\n{content}endstream\nendobj\n",
            content.len()
        )?;

        let page_id = content_id + 1;
        begin(&mut out, &mut offsets, page_id)?;
        let xobject = image_id.map_or(String::new(), |(id, ..)| format!("/XObject << /Im0 {id} 0 R >>"));
        write!(
            out,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
             /Resources << /Font << /F1 3 0 R >> {xobject} >> /Contents {content_id} 0 R >>\nendobj\n"
        )?;
        kids.push(page_id);
    }

    begin(&mut out, &mut offsets, 2)?;
    write!(out, "<< /Type /Pages /Count {pages} /Kids [")?;
    for id in &kids {
        write!(out, "{id} 0 R ")?;
    }
    out.write_all(b"] >>\nendobj\n")?;
    begin(&mut out, &mut offsets, 1)?;
    out.write_all(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n")?;

    let xref = out.position;
    write!(out, "xref\n0 {}\n0000000000 65535 f\r\n", offsets.len())?;
    for offset in &offsets[1..] {
        write!(out, "{offset:010} 00000 n\r\n")?;
    }
    write!(
        out,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len()
    )?;
    out.flush()?;
    println!("{path} : {pages} pages, {:.1} Mo", out.position as f64 / 1e6);
    Ok(())
}

/// JPEG de bruit (incompressible) d'environ `megabytes` Mo.
fn noise_jpeg(megabytes: f64) -> (Vec<u8>, u32, u32) {
    // Le bruit en qualité 90 coûte environ 1,6 octet par pixel.
    let pixels = (megabytes * 1e6 / 1.6).max(10_000.0);
    let width = (pixels * 495.0 / 600.0).sqrt() as u32;
    let height = (pixels / f64::from(width)) as u32;
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let rgb: Vec<u8> = (0..width as usize * height as usize * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let mut data = Vec::new();
    JpegEncoder::new_with_quality(&mut data, 90)
        .encode(&rgb, width, height, ExtendedColorType::Rgb8)
        .expect("encodage JPEG");
    (data, width, height)
}

struct Counting {
    inner: BufWriter<File>,
    position: u64,
}

impl Write for Counting {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.position += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
