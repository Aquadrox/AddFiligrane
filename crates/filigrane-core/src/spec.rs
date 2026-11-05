use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::pages::PageSelection;

/// Nombre maximal de caractères du texte de filigrane.
pub const MAX_TEXT_CHARS: usize = 300;
/// Nombre maximal de lignes du texte de filigrane.
pub const MAX_TEXT_LINES: usize = 6;
/// Bornes de la taille de police explicite, en points.
pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 4.0..=400.0;
/// Bornes de l'opacité.
pub const OPACITY_RANGE: std::ops::RangeInclusive<f32> = 0.05..=1.0;
/// Bornes de la résolution de rendu du mode incrusté.
pub const DPI_RANGE: std::ops::RangeInclusive<u32> = 72..=600;
/// Bornes de la qualité JPEG du mode incrusté.
pub const JPEG_QUALITY_RANGE: std::ops::RangeInclusive<u8> = 30..=100;

/// Apparence et placement du filigrane, indépendamment de la façon de l'appliquer.
#[derive(Debug, Clone, PartialEq)]
pub struct WatermarkSpec {
    /// Texte, éventuellement sur plusieurs lignes (`\n`).
    pub text: String,
    pub weight: FontWeight,
    pub size: FontSize,
    pub color: Rgb,
    /// Opacité entre 0.05 (presque invisible) et 1 (opaque).
    pub opacity: f32,
    pub angle: Angle,
    pub layout: Layout,
    pub pages: PageSelection,
}

impl Default for WatermarkSpec {
    fn default() -> Self {
        Self {
            text: String::new(),
            weight: FontWeight::Regular,
            size: FontSize::Auto,
            color: Rgb::new(0xD0, 0x1F, 0x1F),
            opacity: 0.4,
            angle: Angle::Degrees(30.0),
            layout: Layout::Tiled(Density::Normal),
            pages: PageSelection::All,
        }
    }
}

impl WatermarkSpec {
    /// Vérifie les paramètres (le texte est contrôlé à la mise en forme, voir
    /// [`crate::check_text`]).
    pub fn validate(&self) -> Result<()> {
        if !OPACITY_RANGE.contains(&self.opacity) {
            return Err(Error::InvalidParameter(format!(
                "L'opacité doit être comprise entre {:.0} % et {:.0} %.",
                OPACITY_RANGE.start() * 100.0,
                OPACITY_RANGE.end() * 100.0
            )));
        }
        if let FontSize::Points(points) = self.size
            && !FONT_SIZE_RANGE.contains(&points)
        {
            return Err(Error::InvalidParameter(format!(
                "La taille du texte doit être comprise entre {} et {} points.",
                FONT_SIZE_RANGE.start(),
                FONT_SIZE_RANGE.end()
            )));
        }
        if let Angle::Degrees(degrees) = self.angle
            && !(degrees.is_finite() && (-360.0..=360.0).contains(&degrees))
        {
            return Err(Error::InvalidParameter(
                "L'angle doit être compris entre -360° et 360°.".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FontSize {
    /// Taille adaptée à chaque page : grand texte pour un filigrane centré,
    /// texte discret pour une mosaïque.
    #[default]
    Auto,
    /// Taille fixe en points typographiques.
    Points(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Angle {
    /// Suit la diagonale de chaque page, du coin inférieur gauche au coin supérieur droit.
    Diagonal,
    /// Angle fixe en degrés, sens trigonométrique (positif = montant vers la droite).
    Degrees(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Un seul texte au centre de la page.
    Centered,
    /// Texte répété sur toute la page, en quinconce.
    Tiled(Density),
}

/// Densité de la mosaïque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Composantes normalisées entre 0 et 1.
    pub fn unit(self) -> [f32; 3] {
        [self.r, self.g, self.b].map(|c| f32::from(c) / 255.0)
    }

    /// Lit une couleur au format `#RRGGBB` (le `#` est facultatif).
    pub fn from_hex(input: &str) -> Result<Self> {
        let hex = input.trim().trim_start_matches('#');
        let invalid = || Error::InvalidParameter(format!("Couleur « {input} » invalide : utilisez le format #RRGGBB."));
        if hex.len() != 6 || !hex.is_ascii() {
            return Err(invalid());
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| invalid());
        Ok(Self::new(channel(0)?, channel(2)?, channel(4)?))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Façon d'appliquer le filigrane au document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Chaque page est convertie en image avec le filigrane incrusté.
    /// Le filigrane ne peut plus être retiré, mais le texte n'est plus sélectionnable.
    Flatten(FlattenOptions),
    /// Le filigrane est ajouté par-dessus (ou dessous) le contenu existant, sans
    /// réécrire le document. Très rapide, même sur des fichiers énormes.
    Overlay(OverlayOptions),
}

impl Default for Mode {
    fn default() -> Self {
        Self::Flatten(FlattenOptions::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlattenOptions {
    /// Résolution de rendu, en points par pouce.
    pub dpi: u32,
    /// Qualité de compression JPEG (30 à 100).
    pub jpeg_quality: u8,
}

impl Default for FlattenOptions {
    fn default() -> Self {
        Self {
            dpi: 150,
            jpeg_quality: 85,
        }
    }
}

impl FlattenOptions {
    pub fn validate(&self) -> Result<()> {
        if !DPI_RANGE.contains(&self.dpi) {
            return Err(Error::InvalidParameter(format!(
                "La résolution doit être comprise entre {} et {} ppp.",
                DPI_RANGE.start(),
                DPI_RANGE.end()
            )));
        }
        if !JPEG_QUALITY_RANGE.contains(&self.jpeg_quality) {
            return Err(Error::InvalidParameter(format!(
                "La qualité JPEG doit être comprise entre {} et {}.",
                JPEG_QUALITY_RANGE.start(),
                JPEG_QUALITY_RANGE.end()
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverlayOptions {
    pub layer: Layer,
}

/// Position du filigrane par rapport au contenu existant (mode superposé).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layer {
    /// Au-dessus du contenu : toujours visible, y compris sur les scans.
    #[default]
    Foreground,
    /// Sous le contenu : plus discret, mais masqué par les images opaques.
    Background,
}

/// Travail complet : quoi, où, comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub input: PathBuf,
    pub output: PathBuf,
    pub spec: WatermarkSpec,
    pub mode: Mode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_spec_is_valid() {
        WatermarkSpec::default().validate().unwrap();
        FlattenOptions::default().validate().unwrap();
    }

    #[test]
    fn rejects_out_of_range_parameters() {
        let base = WatermarkSpec::default();
        let cases = [
            WatermarkSpec {
                opacity: 0.0,
                ..base.clone()
            },
            WatermarkSpec {
                opacity: 1.5,
                ..base.clone()
            },
            WatermarkSpec {
                size: FontSize::Points(1.0),
                ..base.clone()
            },
            WatermarkSpec {
                angle: Angle::Degrees(f32::NAN),
                ..base.clone()
            },
        ];
        for spec in cases {
            assert!(spec.validate().is_err(), "{spec:?}");
        }
        assert!(
            FlattenOptions {
                dpi: 20,
                jpeg_quality: 85
            }
            .validate()
            .is_err()
        );
        assert!(
            FlattenOptions {
                dpi: 150,
                jpeg_quality: 5
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn hex_colors_round_trip() {
        let color = Rgb::from_hex("#d01f1f").unwrap();
        assert_eq!(color, Rgb::new(0xD0, 0x1F, 0x1F));
        assert_eq!(color.to_hex(), "#D01F1F");
        assert_eq!(Rgb::from_hex("00ff00").unwrap(), Rgb::new(0, 255, 0));
        for bad in ["", "#12345", "#GGGGGG", "#ééé"] {
            assert!(Rgb::from_hex(bad).is_err(), "{bad}");
        }
    }
}
