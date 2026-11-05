//! Incorporation de la police : sous-ensemble TrueType limité aux glyphes du texte,
//! police composite `Type0` / `Identity-H`, table `ToUnicode` pour le copier-coller.

use std::collections::HashMap;
use std::fmt::Write as _;

use lopdf::{Dictionary, Object, Stream, dictionary};

use super::IdAllocator;
use crate::font::TextBlock;

/// Police prête à être référencée depuis une ressource `/Font`.
pub(super) struct EmbeddedFont {
    pub font_id: lopdf::ObjectId,
    /// Identifiant de glyphe d'origine → code (CID) dans la police incorporée.
    pub codes: HashMap<u16, u16>,
    pub objects: Vec<(lopdf::ObjectId, Object)>,
}

impl EmbeddedFont {
    /// Chaîne hexadécimale des codes d'une suite de glyphes, pour l'opérateur `Tj`.
    pub fn encode(&self, glyphs: impl IntoIterator<Item = u16>) -> String {
        let mut hex = String::from("<");
        for glyph in glyphs {
            let _ = write!(hex, "{:04X}", self.codes[&glyph]);
        }
        hex.push('>');
        hex
    }
}

pub(super) fn embed(block: &TextBlock, ids: &mut IdAllocator) -> EmbeddedFont {
    let font = block.font;
    let glyphs = block.unique_glyphs();

    let mut remapper = subsetter::GlyphRemapper::new();
    for (glyph, _) in &glyphs {
        remapper.remap(*glyph);
    }
    // En cas d'échec du sous-ensemble (improbable avec la police intégrée), on
    // incorpore la police entière et les codes sont les identifiants d'origine.
    let (font_file, codes): (Vec<u8>, HashMap<u16, u16>) = match subsetter::subset(font.data, 0, &remapper) {
        Ok(data) => (
            data,
            glyphs
                .iter()
                .map(|(g, _)| (*g, remapper.get(*g).unwrap_or(0)))
                .collect(),
        ),
        Err(_) => (font.data.to_vec(), glyphs.iter().map(|(g, _)| (*g, *g)).collect()),
    };

    let base_font = format!("{}+{}", subset_tag(&glyphs), font.postscript_name);
    let units = |value: f64| Object::Integer((value * 1000.0).round() as i64);

    let mut file_stream = Stream::new(dictionary! { "Length1" => font_file.len() as i64 }, font_file);
    let _ = file_stream.compress();
    let file_id = ids.next();

    let bbox = font.face.global_bounding_box();
    let descriptor_id = ids.next();
    let descriptor = dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => Object::Name(base_font.clone().into_bytes()),
        "Flags" => 32,
        "FontBBox" => vec![
            units(font.em(bbox.x_min)), units(font.em(bbox.y_min)),
            units(font.em(bbox.x_max)), units(font.em(bbox.y_max)),
        ],
        "ItalicAngle" => 0,
        "Ascent" => units(font.ascender()),
        "Descent" => units(font.descender()),
        "CapHeight" => units(font.cap_height()),
        "StemV" => 80,
        "FontFile2" => file_id,
    };

    let mut widths: Vec<(u16, i64)> = glyphs
        .iter()
        .map(|(glyph, _)| (codes[glyph], (font.advance(*glyph) * 1000.0).round() as i64))
        .collect();
    widths.sort_unstable();
    let w_array: Vec<Object> = widths
        .iter()
        .flat_map(|&(code, width)| [Object::Integer(i64::from(code)), Object::Array(vec![width.into()])])
        .collect();

    let cid_font_id = ids.next();
    let cid_font = dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => Object::Name(base_font.clone().into_bytes()),
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor_id,
        "DW" => 1000,
        "W" => w_array,
        "CIDToGIDMap" => "Identity",
    };

    let to_unicode_id = ids.next();
    let mut cmap_entries: Vec<(u16, char)> = glyphs.iter().map(|(g, ch)| (codes[g], *ch)).collect();
    cmap_entries.sort_unstable();
    let mut to_unicode = Stream::new(Dictionary::new(), to_unicode_cmap(&cmap_entries).into_bytes());
    let _ = to_unicode.compress();

    let font_id = ids.next();
    let type0 = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => Object::Name(base_font.into_bytes()),
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(cid_font_id)],
        "ToUnicode" => to_unicode_id,
    };

    EmbeddedFont {
        font_id,
        codes,
        objects: vec![
            (file_id, Object::Stream(file_stream)),
            (descriptor_id, Object::Dictionary(descriptor)),
            (cid_font_id, Object::Dictionary(cid_font)),
            (to_unicode_id, Object::Stream(to_unicode)),
            (font_id, Object::Dictionary(type0)),
        ],
    }
}

/// Préfixe de six majuscules exigé pour les polices partielles, dérivé des glyphes.
fn subset_tag(glyphs: &[(u16, char)]) -> String {
    let mut hash: u32 = 0x811C_9DC5;
    for (glyph, _) in glyphs {
        for byte in glyph.to_be_bytes() {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }
    (0..6)
        .map(|i| char::from(b'A' + ((hash >> (i * 5)) % 26) as u8))
        .collect()
}

fn to_unicode_cmap(entries: &[(u16, char)]) -> String {
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in entries.chunks(100) {
        let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
        for (code, ch) in chunk {
            let mut utf16 = [0u16; 2];
            let units: String = ch.encode_utf16(&mut utf16).iter().map(|u| format!("{u:04X}")).collect();
            let _ = writeln!(cmap, "<{code:04X}> <{units}>");
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMapResource defineresource pop\nend\nend\n");
    cmap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::FontWeight;

    #[test]
    fn subset_is_small_and_codes_are_dense() {
        let block = TextBlock::layout("Confidentiel – été", FontWeight::Regular).unwrap();
        let mut ids = IdAllocator::new(100);
        let font = embed(&block, &mut ids);
        assert_eq!(font.objects.len(), 5);
        let unique = block.unique_glyphs().len();
        let mut codes: Vec<u16> = font.codes.values().copied().collect();
        codes.sort_unstable();
        // `.notdef` occupe le code 0 ; les glyphes du texte suivent sans trou.
        assert_eq!(codes, (1..=unique as u16).collect::<Vec<_>>());
        let Object::Stream(file) = &font.objects[0].1 else {
            panic!()
        };
        assert!(
            file.content.len() < 40_000,
            "sous-ensemble trop gros : {}",
            file.content.len()
        );
    }

    #[test]
    fn to_unicode_handles_astral_characters() {
        let cmap = to_unicode_cmap(&[(1, 'A'), (2, '𝄞')]);
        assert!(cmap.contains("<0001> <0041>"));
        assert!(cmap.contains("<0002> <D834DD1E>"));
    }

    #[test]
    fn subset_tag_is_six_uppercase_letters() {
        let tag = subset_tag(&[(3, 'a'), (70, 'b')]);
        assert_eq!(tag.len(), 6);
        assert!(tag.chars().all(|c| c.is_ascii_uppercase()));
    }
}
