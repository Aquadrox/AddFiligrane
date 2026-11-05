//! Réglages du formulaire, mémorisés d'une session à l'autre, et leur conversion
//! vers les types du cœur.

use std::path::PathBuf;

use filigrane_core::{
    Angle, Density, FlattenOptions, FontSize, FontWeight, Layer, Layout, Mode, OverlayOptions, PageSelection, Rgb,
    WatermarkSpec,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protection {
    /// Filigrane fondu dans l'image des pages.
    Flatten,
    /// Filigrane ajouté par-dessus le contenu.
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arrangement {
    Tiled,
    Centered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Spacing {
    Low,
    Normal,
    High,
}

/// Couleurs proposées d'un clic.
pub const PRESET_COLORS: [(&str, [u8; 3]); 4] = [
    ("Rouge", [0xD0, 0x1F, 0x1F]),
    ("Gris", [0x70, 0x70, 0x70]),
    ("Bleu", [0x1F, 0x4F, 0xD0]),
    ("Noir", [0x10, 0x10, 0x10]),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub text: String,
    pub bold: bool,
    pub auto_size: bool,
    pub size_points: f32,
    pub color: [u8; 3],
    pub opacity_percent: f32,
    pub diagonal: bool,
    pub angle_degrees: f32,
    pub arrangement: Arrangement,
    pub spacing: Spacing,
    pub protection: Protection,
    pub dpi: u32,
    pub jpeg_quality: u8,
    pub background: bool,
    pub all_pages: bool,
    pub page_ranges: String,
    /// Dernier dossier ouvert, pour les boîtes de dialogue.
    pub last_directory: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        let spec = WatermarkSpec::default();
        let flatten = FlattenOptions::default();
        Self {
            text: String::new(),
            bold: false,
            auto_size: true,
            size_points: 14.0,
            color: [spec.color.r, spec.color.g, spec.color.b],
            opacity_percent: spec.opacity * 100.0,
            diagonal: false,
            angle_degrees: 30.0,
            arrangement: Arrangement::Tiled,
            spacing: Spacing::Normal,
            protection: Protection::Flatten,
            dpi: flatten.dpi,
            jpeg_quality: flatten.jpeg_quality,
            background: false,
            all_pages: true,
            page_ranges: String::new(),
            last_directory: None,
        }
    }
}

impl Settings {
    /// Description du filigrane, ou message d'erreur à afficher.
    pub fn to_spec(&self) -> Result<WatermarkSpec, String> {
        let pages = if self.all_pages {
            PageSelection::All
        } else {
            self.page_ranges
                .parse()
                .map_err(|e: filigrane_core::Error| e.to_string())?
        };
        let spec = WatermarkSpec {
            text: self.text.clone(),
            weight: if self.bold {
                FontWeight::Bold
            } else {
                FontWeight::Regular
            },
            size: if self.auto_size {
                FontSize::Auto
            } else {
                FontSize::Points(self.size_points)
            },
            color: Rgb::new(self.color[0], self.color[1], self.color[2]),
            opacity: self.opacity_percent / 100.0,
            angle: if self.diagonal {
                Angle::Diagonal
            } else {
                Angle::Degrees(self.angle_degrees)
            },
            layout: match self.arrangement {
                Arrangement::Centered => Layout::Centered,
                Arrangement::Tiled => Layout::Tiled(match self.spacing {
                    Spacing::Low => Density::Low,
                    Spacing::Normal => Density::Normal,
                    Spacing::High => Density::High,
                }),
            },
            pages,
        };
        spec.validate().map_err(|e| e.to_string())?;
        filigrane_core::check_text(&spec.text, spec.weight).map_err(|e| e.to_string())?;
        Ok(spec)
    }

    pub fn mode(&self) -> Mode {
        match self.protection {
            Protection::Flatten => Mode::Flatten(FlattenOptions {
                dpi: self.dpi,
                jpeg_quality: self.jpeg_quality,
            }),
            Protection::Overlay => Mode::Overlay(OverlayOptions {
                layer: if self.background {
                    Layer::Background
                } else {
                    Layer::Foreground
                },
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_recommended_settings() {
        let settings = Settings::default();
        assert_eq!(settings.mode(), Mode::Flatten(FlattenOptions::default()));
        assert_eq!(
            settings.to_spec().unwrap_err(),
            filigrane_core::Error::EmptyText.to_string()
        );

        let spec = Settings {
            text: "Pour mandat de vente".into(),
            ..Settings::default()
        }
        .to_spec()
        .unwrap();
        let expected = WatermarkSpec {
            text: "Pour mandat de vente".into(),
            ..WatermarkSpec::default()
        };
        assert_eq!(spec, expected);
    }

    #[test]
    fn form_values_are_converted() {
        let settings = Settings {
            text: "COPIE".into(),
            bold: true,
            auto_size: false,
            size_points: 48.0,
            color: [1, 2, 3],
            opacity_percent: 25.0,
            diagonal: true,
            arrangement: Arrangement::Centered,
            protection: Protection::Overlay,
            background: true,
            all_pages: false,
            page_ranges: "2-3".into(),
            ..Settings::default()
        };
        let spec = settings.to_spec().unwrap();
        assert_eq!(spec.weight, FontWeight::Bold);
        assert_eq!(spec.size, FontSize::Points(48.0));
        assert_eq!(spec.color, Rgb::new(1, 2, 3));
        assert!((spec.opacity - 0.25).abs() < 1e-6);
        assert_eq!(spec.angle, Angle::Diagonal);
        assert_eq!(spec.layout, Layout::Centered);
        assert!(spec.pages.includes(2) && !spec.pages.includes(1));
        assert_eq!(
            settings.mode(),
            Mode::Overlay(OverlayOptions {
                layer: Layer::Background
            })
        );
    }

    #[test]
    fn errors_are_user_readable() {
        let invalid_pages = Settings {
            text: "x".into(),
            all_pages: false,
            page_ranges: "3-1".into(),
            ..Settings::default()
        };
        assert!(
            invalid_pages
                .to_spec()
                .unwrap_err()
                .contains("Sélection de pages invalide")
        );

        let unsupported = Settings {
            text: "OK 中".into(),
            ..Settings::default()
        };
        assert!(unsupported.to_spec().unwrap_err().contains("U+4E2D"));
    }

    /// eframe stocke les réglages en RON : un champ absent (version précédente de
    /// l'application) doit prendre sa valeur par défaut au lieu d'effacer le reste.
    #[test]
    fn older_saved_settings_still_load() {
        let restored: Settings = ron::from_str(r#"(text: "Ancien", bold: true)"#).unwrap();
        assert_eq!(restored.text, "Ancien");
        assert!(restored.bold);
        assert_eq!(restored.protection, Protection::Flatten);
    }
}
