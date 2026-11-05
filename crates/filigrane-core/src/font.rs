//! Police intégrée (DejaVu Sans) et mise en forme du texte du filigrane.
//!
//! Toutes les mesures sont exprimées en « em » : 1.0 correspond à la taille de police.

use std::sync::OnceLock;

use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::error::{Error, Result};
use crate::spec::{FontWeight, MAX_TEXT_CHARS, MAX_TEXT_LINES};

static REGULAR_DATA: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");
static BOLD_DATA: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans-Bold.ttf");

/// Police TrueType intégrée à l'exécutable.
pub(crate) struct Font {
    pub data: &'static [u8],
    pub face: Face<'static>,
    pub postscript_name: &'static str,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("postscript_name", &self.postscript_name)
            .finish_non_exhaustive()
    }
}

pub(crate) fn font(weight: FontWeight) -> &'static Font {
    static REGULAR: OnceLock<Font> = OnceLock::new();
    static BOLD: OnceLock<Font> = OnceLock::new();
    let (cell, data, postscript_name) = match weight {
        FontWeight::Regular => (&REGULAR, REGULAR_DATA, "DejaVuSans"),
        FontWeight::Bold => (&BOLD, BOLD_DATA, "DejaVuSans-Bold"),
    };
    cell.get_or_init(|| Font {
        data,
        face: Face::parse(data, 0).expect("la police intégrée est un TrueType valide"),
        postscript_name,
    })
}

impl Font {
    fn units_per_em(&self) -> f64 {
        f64::from(self.face.units_per_em())
    }

    /// Convertit des unités de police en em.
    pub fn em(&self, units: impl Into<f64>) -> f64 {
        units.into() / self.units_per_em()
    }

    pub fn ascender(&self) -> f64 {
        self.em(self.face.ascender())
    }

    /// Valeur négative (sous la ligne de base).
    pub fn descender(&self) -> f64 {
        self.em(self.face.descender())
    }

    pub fn cap_height(&self) -> f64 {
        self.face
            .capital_height()
            .map_or(self.ascender() * 0.75, |h| self.em(h))
    }

    pub fn advance(&self, glyph: u16) -> f64 {
        self.em(self.face.glyph_hor_advance(GlyphId(glyph)).unwrap_or(0))
    }

    fn line_height(&self) -> f64 {
        let natural = self.ascender() - self.descender() + self.em(self.face.line_gap());
        natural.max(1.15)
    }
}

/// Glyphe positionné dans sa ligne.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Glyph {
    pub id: u16,
    pub ch: char,
    /// Abscisse depuis le début de la ligne.
    pub x: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub glyphs: Vec<Glyph>,
    /// Abscisse du début de la ligne (lignes centrées sur 0).
    pub x: f64,
    pub baseline: f64,
    pub width: f64,
}

/// Bloc de texte mis en forme, centré sur l'origine, en em.
#[derive(Debug, Clone)]
pub(crate) struct TextBlock {
    pub font: &'static Font,
    pub lines: Vec<Line>,
    pub width: f64,
    pub height: f64,
}

impl TextBlock {
    /// Met en forme `text`, ou explique pourquoi il ne peut pas l'être.
    pub fn layout(text: &str, weight: FontWeight) -> Result<Self> {
        let font = font(weight);
        let lines = split_lines(text);

        let char_count: usize = lines.iter().map(|line| line.chars().count()).sum();
        if char_count == 0 || lines.iter().all(|line| line.trim().is_empty()) {
            return Err(Error::EmptyText);
        }
        if char_count > MAX_TEXT_CHARS {
            return Err(Error::TextTooLong {
                len: char_count,
                max: MAX_TEXT_CHARS,
            });
        }
        if lines.len() > MAX_TEXT_LINES {
            return Err(Error::TooManyLines {
                lines: lines.len(),
                max: MAX_TEXT_LINES,
            });
        }

        let mut unsupported = Vec::new();
        let mut shaped = Vec::with_capacity(lines.len());
        for line in &lines {
            let mut glyphs = Vec::new();
            let mut x = 0.0;
            for ch in line.chars() {
                let id = (!ch.is_control())
                    .then(|| font.face.glyph_index(ch))
                    .flatten()
                    .map(|id| id.0)
                    .filter(|&id| id != 0);
                match id {
                    Some(id) => {
                        glyphs.push(Glyph { id, ch, x });
                        x += font.advance(id);
                    }
                    None if !unsupported.contains(&ch) => unsupported.push(ch),
                    None => {}
                }
            }
            shaped.push((glyphs, x));
        }
        if !unsupported.is_empty() {
            return Err(Error::UnsupportedCharacters(unsupported));
        }

        let line_height = font.line_height();
        let ascender = font.ascender();
        let height = ascender - font.descender() + line_height * (shaped.len() - 1) as f64;
        let width = shaped.iter().map(|(_, w)| *w).fold(0.0, f64::max);
        let lines = shaped
            .into_iter()
            .enumerate()
            .map(|(index, (glyphs, line_width))| Line {
                glyphs,
                x: -line_width / 2.0,
                baseline: height / 2.0 - ascender - line_height * index as f64,
                width: line_width,
            })
            .collect();

        Ok(Self {
            font,
            lines,
            width,
            height,
        })
    }

    /// Tous les glyphes, sans doublon, dans l'ordre d'apparition.
    pub fn unique_glyphs(&self) -> Vec<(u16, char)> {
        let mut seen = Vec::new();
        for glyph in self.lines.iter().flat_map(|line| &line.glyphs) {
            if !seen.iter().any(|(id, _)| *id == glyph.id) {
                seen.push((glyph.id, glyph.ch));
            }
        }
        seen
    }

    /// Contour vectoriel du texte (em, axe y vers le haut), pour le rendu matriciel.
    pub fn outline(&self) -> Option<tiny_skia::Path> {
        let mut sink = PathSink {
            builder: tiny_skia::PathBuilder::new(),
            scale: 1.0 / self.font.units_per_em() as f32,
            dx: 0.0,
            dy: 0.0,
        };
        for line in &self.lines {
            for glyph in &line.glyphs {
                sink.dx = (line.x + glyph.x) as f32;
                sink.dy = line.baseline as f32;
                self.font.face.outline_glyph(GlyphId(glyph.id), &mut sink);
            }
        }
        sink.builder.finish()
    }
}

/// Vérifie que `text` peut servir de filigrane avec cette graisse.
pub fn check_text(text: &str, weight: FontWeight) -> Result<()> {
    TextBlock::layout(text, weight).map(|_| ())
}

/// Normalise les fins de ligne, remplace les tabulations, retire les lignes vides
/// en début et en fin ainsi que les espaces de fin de ligne.
fn split_lines(text: &str) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace(['\r'], "\n").replace('\t', " ");
    let mut lines: Vec<String> = normalized.split('\n').map(|line| line.trim_end().to_owned()).collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    let first_non_empty = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    lines.drain(..first_non_empty);
    lines
}

struct PathSink {
    builder: tiny_skia::PathBuilder,
    scale: f32,
    dx: f32,
    dy: f32,
}

impl PathSink {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.scale + self.dx, y * self.scale + self.dy)
    }
}

impl OutlineBuilder for PathSink {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x, y) = self.point(x, y);
        self.builder.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x2, y2) = self.point(x2, y2);
        let (x, y) = self.point(x, y);
        self.builder.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_text_is_supported() {
        let block =
            TextBlock::layout("Pour mandat de vente – Agence Éole – 05/11/2026 €", FontWeight::Regular).unwrap();
        assert_eq!(block.lines.len(), 1);
        assert!(block.width > 10.0, "{}", block.width);
        assert!(block.height > 1.0 && block.height < 1.5, "{}", block.height);
    }

    #[test]
    fn lines_are_centered_and_stacked() {
        let block = TextBlock::layout("\n  \nCOPIE\nNe pas diffuser\n\n", FontWeight::Bold).unwrap();
        assert_eq!(block.lines.len(), 2);
        let [first, second] = [&block.lines[0], &block.lines[1]];
        assert!((first.x + first.width / 2.0).abs() < 1e-9);
        assert!((second.x + second.width / 2.0).abs() < 1e-9);
        assert!(first.baseline > second.baseline);
        assert_eq!(block.width, first.width.max(second.width));
        // Le bloc est centré verticalement : il déborde autant au-dessus qu'en dessous.
        let top = first.baseline + block.font.ascender();
        let bottom = second.baseline + block.font.descender();
        assert!((top + bottom).abs() < 1e-9, "{top} {bottom}");
    }

    #[test]
    fn rejects_empty_long_and_unsupported_text() {
        assert!(matches!(
            TextBlock::layout(" \n\t ", FontWeight::Regular),
            Err(Error::EmptyText)
        ));
        let long = "a".repeat(MAX_TEXT_CHARS + 1);
        assert!(matches!(
            TextBlock::layout(&long, FontWeight::Regular),
            Err(Error::TextTooLong { .. })
        ));
        let many = ["a"; MAX_TEXT_LINES + 1].join("\n");
        assert!(matches!(
            TextBlock::layout(&many, FontWeight::Regular),
            Err(Error::TooManyLines { .. })
        ));
        match TextBlock::layout("OK 中文 中", FontWeight::Regular) {
            Err(Error::UnsupportedCharacters(chars)) => assert_eq!(chars, vec!['中', '文']),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unique_glyphs_are_deduplicated() {
        let block = TextBlock::layout("abba", FontWeight::Regular).unwrap();
        let chars: Vec<char> = block.unique_glyphs().iter().map(|(_, c)| *c).collect();
        assert_eq!(chars, vec!['a', 'b']);
    }

    #[test]
    fn outline_covers_the_text_box() {
        let block = TextBlock::layout("HHH", FontWeight::Regular).unwrap();
        let bounds = block.outline().unwrap().bounds();
        assert!(f64::from(bounds.left()) >= -block.width / 2.0 - 0.1);
        assert!(f64::from(bounds.right()) <= block.width / 2.0 + 0.1);
        assert!(bounds.top() < bounds.bottom());
    }
}
