//! Ligne de commande de Filigrane PDF.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use filigrane_core::{
    Angle, CancelToken, Density, FlattenOptions, FontSize, FontWeight, Job, Layer, Layout, Method, Mode,
    OverlayOptions, PageSelection, Progress, Rgb, WatermarkSpec,
};

/// Ajoute un filigrane texte à un document PDF.
#[derive(Debug, Parser)]
#[command(name = "filigrane-cli", version, about, long_about = None)]
struct Cli {
    /// Document PDF à filigraner.
    input: PathBuf,

    /// Fichier produit (par défaut : <nom>_filigrane.pdf à côté de l'original).
    #[arg(short, long, value_name = "FICHIER")]
    output: Option<PathBuf>,

    /// Texte du filigrane ; « \n » passe à la ligne.
    #[arg(short, long, value_name = "TEXTE")]
    text: String,

    /// incruste : filigrane fondu dans l'image des pages, impossible à retirer ;
    /// superpose : ajout rapide et léger, texte d'origine sélectionnable.
    #[arg(short, long, value_enum, default_value_t = CliMode::Incruste)]
    mode: CliMode,

    /// Texte répété sur toute la page ou unique au centre.
    #[arg(long, value_enum, default_value_t = CliLayout::Mosaique)]
    layout: CliLayout,

    /// Densité de la mosaïque.
    #[arg(long, value_enum, default_value_t = CliDensity::Normale)]
    density: CliDensity,

    /// Taille du texte en points, ou « auto ».
    #[arg(long, value_name = "POINTS|auto", default_value = "auto", value_parser = parse_size)]
    size: FontSize,

    /// Angle en degrés, ou « diagonale » pour suivre la diagonale de chaque page.
    #[arg(long, value_name = "DEGRÉS|diagonale", default_value = "30", value_parser = parse_angle, allow_hyphen_values = true)]
    angle: Angle,

    /// Couleur au format #RRGGBB.
    #[arg(long, value_name = "#RRGGBB", default_value = "#D01F1F", value_parser = parse_color)]
    color: Rgb,

    /// Opacité en pourcentage (5 à 100).
    #[arg(long, value_name = "%", default_value_t = 40, value_parser = clap::value_parser!(u8).range(5..=100))]
    opacity: u8,

    /// Texte en gras.
    #[arg(long)]
    bold: bool,

    /// Pages à filigraner, par exemple « 1-3, 7, 10- » (toutes par défaut).
    #[arg(long, value_name = "PAGES", default_value = "", value_parser = parse_pages)]
    pages: PageSelection,

    /// Mode incrusté : résolution en points par pouce.
    #[arg(long, default_value_t = FlattenOptions::default().dpi)]
    dpi: u32,

    /// Mode incrusté : qualité JPEG (30 à 100).
    #[arg(long, default_value_t = FlattenOptions::default().jpeg_quality)]
    quality: u8,

    /// Mode superposé : placer le filigrane sous le contenu plutôt qu'au-dessus.
    #[arg(long)]
    background: bool,

    /// N'affiche rien d'autre que les erreurs.
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliMode {
    Incruste,
    Superpose,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliLayout {
    Mosaique,
    Centre,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliDensity {
    Faible,
    Normale,
    Forte,
}

fn parse_size(value: &str) -> Result<FontSize, String> {
    if value.eq_ignore_ascii_case("auto") {
        return Ok(FontSize::Auto);
    }
    value
        .replace(',', ".")
        .parse::<f32>()
        .map(FontSize::Points)
        .map_err(|_| format!("« {value} » n'est ni un nombre ni « auto »"))
}

fn parse_angle(value: &str) -> Result<Angle, String> {
    if value.eq_ignore_ascii_case("diagonale") || value.eq_ignore_ascii_case("diagonal") {
        return Ok(Angle::Diagonal);
    }
    value
        .replace(',', ".")
        .parse::<f32>()
        .map(Angle::Degrees)
        .map_err(|_| format!("« {value} » n'est ni un nombre ni « diagonale »"))
}

fn parse_color(value: &str) -> Result<Rgb, String> {
    Rgb::from_hex(value).map_err(|e| e.to_string())
}

fn parse_pages(value: &str) -> Result<PageSelection, String> {
    value.parse().map_err(|e: filigrane_core::Error| e.to_string())
}

impl Cli {
    fn into_job(self) -> Job {
        let output = self
            .output
            .unwrap_or_else(|| filigrane_core::default_output_path(&self.input));
        let layout = match self.layout {
            CliLayout::Centre => Layout::Centered,
            CliLayout::Mosaique => Layout::Tiled(match self.density {
                CliDensity::Faible => Density::Low,
                CliDensity::Normale => Density::Normal,
                CliDensity::Forte => Density::High,
            }),
        };
        let mode = match self.mode {
            CliMode::Incruste => Mode::Flatten(FlattenOptions {
                dpi: self.dpi,
                jpeg_quality: self.quality,
            }),
            CliMode::Superpose => Mode::Overlay(OverlayOptions {
                layer: if self.background {
                    Layer::Background
                } else {
                    Layer::Foreground
                },
            }),
        };
        Job {
            input: self.input,
            output,
            spec: WatermarkSpec {
                text: self.text.replace("\\n", "\n"),
                weight: if self.bold {
                    FontWeight::Bold
                } else {
                    FontWeight::Regular
                },
                size: self.size,
                color: self.color,
                opacity: f32::from(self.opacity) / 100.0,
                angle: self.angle,
                layout,
                pages: self.pages,
            },
            mode,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let quiet = cli.quiet;
    let job = cli.into_job();

    let interactive = std::io::stderr().is_terminal();
    let last_line = Mutex::new(String::new());
    let progress = |progress: Progress| {
        if quiet {
            return;
        }
        let line = match progress.fraction() {
            Some(fraction) => format!("{} ({:.0} %)", progress.label(), fraction * 100.0),
            None => progress.label(),
        };
        let mut last = last_line.lock().unwrap_or_else(|e| e.into_inner());
        if *last != line {
            let mut stderr = std::io::stderr().lock();
            let _ = if interactive {
                write!(stderr, "\r\x1b[2K{line}")
            } else {
                writeln!(stderr, "{line}")
            };
            *last = line;
        }
    };

    let started = Instant::now();
    let result = filigrane_core::apply(&job, &progress, &CancelToken::new());
    if interactive && !quiet {
        eprintln!();
    }
    match result {
        Ok(report) => {
            if !quiet {
                let how = match report.method {
                    Method::Flattened => "filigrane incrusté",
                    Method::Incremental => "filigrane superposé",
                    Method::Rewritten => "filigrane superposé, document réparé",
                };
                println!(
                    "{} : {} page(s) sur {} ({how}), {:.1} Mo, {:.1} s",
                    job.output.display(),
                    report.pages_watermarked,
                    report.pages_total,
                    report.output_bytes as f64 / 1e6,
                    started.elapsed().as_secs_f64()
                );
                if report.pages_skipped > 0 {
                    eprintln!("Attention : {} page(s) invalide(s) ignorée(s).", report.pages_skipped);
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Erreur : {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Job {
        let mut full = vec!["filigrane-cli"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full).unwrap().into_job()
    }

    #[test]
    fn defaults_match_the_recommended_settings() {
        let job = parse(&["dossier/avis.pdf", "-t", "Pour location"]);
        assert_eq!(job.output, PathBuf::from("dossier/avis_filigrane.pdf"));
        assert_eq!(job.mode, Mode::Flatten(FlattenOptions::default()));
        assert_eq!(job.spec.layout, Layout::Tiled(Density::Normal));
        assert_eq!(job.spec.angle, Angle::Degrees(30.0));
        assert_eq!(job.spec.pages, PageSelection::All);
    }

    #[test]
    fn options_are_mapped() {
        let job = parse(&[
            "a.pdf",
            "-o",
            "b.pdf",
            "-t",
            "Ligne 1\\nLigne 2",
            "--mode",
            "superpose",
            "--background",
            "--layout",
            "centre",
            "--size",
            "36,5",
            "--angle",
            "-45",
            "--color",
            "#00ff00",
            "--opacity",
            "70",
            "--bold",
            "--pages",
            "2-4",
        ]);
        assert_eq!(job.output, PathBuf::from("b.pdf"));
        assert_eq!(job.spec.text, "Ligne 1\nLigne 2");
        assert_eq!(
            job.mode,
            Mode::Overlay(OverlayOptions {
                layer: Layer::Background
            })
        );
        assert_eq!(job.spec.layout, Layout::Centered);
        assert_eq!(job.spec.size, FontSize::Points(36.5));
        assert_eq!(job.spec.angle, Angle::Degrees(-45.0));
        assert_eq!(job.spec.color, Rgb::new(0, 255, 0));
        assert!((job.spec.opacity - 0.7).abs() < 1e-6);
        assert_eq!(job.spec.weight, FontWeight::Bold);
        assert!(job.spec.pages.includes(3) && !job.spec.pages.includes(5));
    }

    #[test]
    fn invalid_values_are_rejected() {
        for args in [
            vec!["a.pdf"],
            vec!["a.pdf", "-t", "x", "--opacity", "0"],
            vec!["a.pdf", "-t", "x", "--color", "rouge"],
            vec!["a.pdf", "-t", "x", "--pages", "0"],
            vec!["a.pdf", "-t", "x", "--size", "grand"],
        ] {
            let mut full = vec!["filigrane-cli"];
            full.extend(args.iter().copied());
            assert!(Cli::try_parse_from(&full).is_err(), "{args:?}");
        }
    }

    #[test]
    fn diagonal_angle_keyword() {
        let job = parse(&["a.pdf", "-t", "x", "--angle", "diagonale"]);
        assert_eq!(job.spec.angle, Angle::Diagonal);
    }
}
