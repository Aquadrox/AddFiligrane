//! Placement du filigrane sur une page, commun aux modes superposé et incrusté.
//!
//! Le repère est celui de la page *telle qu'elle s'affiche* : origine en bas à gauche,
//! unités en points, rotation `/Rotate` déjà appliquée.

use crate::geometry::Affine;
use crate::spec::{Angle, Density, FontSize, Layout, WatermarkSpec};

/// Marge laissée autour d'un filigrane centré de taille automatique.
const CENTERED_FILL: f64 = 0.85;
/// Rapport entre la diagonale de la page et la taille automatique en mosaïque.
const TILED_AUTO_DIVISOR: f64 = 85.0;
const TILED_MIN_AUTO_SIZE: f64 = 7.0;
/// Garde-fou contre les mosaïques démesurées (très grandes pages, texte minuscule).
const MAX_TILES: usize = 4_000;
/// Taille de grille examinée au plus avant élimination des tuiles hors page.
const MAX_GRID_CELLS: f64 = 64_000.0;
/// Au-delà, la page est jugée aberrante (le format PDF plafonne à 14 400 unités,
/// éventuellement agrandies par `/UserUnit`) et ne reçoit pas de filigrane.
const MAX_PAGE_SIDE: f64 = 1_000_000.0;

/// Calcule les transformations « bloc de texte (em, centré sur 0) → page affichée (pt) ».
/// Une page aux dimensions aberrantes (nulles, infinies, gigantesques) n'en reçoit aucune.
pub(crate) fn placements(
    page_width: f64,
    page_height: f64,
    block_width: f64,
    block_height: f64,
    spec: &WatermarkSpec,
) -> Vec<Affine> {
    let plausible = |side: f64| side.is_finite() && side > 0.0 && side <= MAX_PAGE_SIDE;
    if !(plausible(page_width) && plausible(page_height) && block_width > 0.0 && block_height > 0.0) {
        return Vec::new();
    }
    let angle = match spec.angle {
        Angle::Diagonal => page_height.atan2(page_width),
        Angle::Degrees(degrees) => f64::from(degrees).to_radians(),
    };
    let center = Affine::translate(page_width / 2.0, page_height / 2.0);

    match spec.layout {
        Layout::Centered => {
            let size = match spec.size {
                FontSize::Points(points) => f64::from(points),
                FontSize::Auto => fit_size(page_width, page_height, block_width, block_height, angle),
            };
            vec![Affine::scale(size, size).then(&Affine::rotate(angle)).then(&center)]
        }
        Layout::Tiled(density) => {
            let size = match spec.size {
                FontSize::Points(points) => f64::from(points),
                FontSize::Auto => (page_width.hypot(page_height) / TILED_AUTO_DIVISOR).max(TILED_MIN_AUTO_SIZE),
            };
            tiles(page_width, page_height, block_width, block_height, size, angle, density)
                .into_iter()
                .map(|(x, y)| {
                    Affine::scale(size, size)
                        .then(&Affine::translate(x, y))
                        .then(&Affine::rotate(angle))
                        .then(&center)
                })
                .collect()
        }
    }
}

/// Plus grande taille pour laquelle le bloc tourné tient dans la page.
fn fit_size(page_width: f64, page_height: f64, block_width: f64, block_height: f64, angle: f64) -> f64 {
    let (sin, cos) = (angle.sin().abs(), angle.cos().abs());
    let rotated_width = block_width * cos + block_height * sin;
    let rotated_height = block_width * sin + block_height * cos;
    let size = (page_width / rotated_width).min(page_height / rotated_height) * CENTERED_FILL;
    if size.is_finite() { size.max(1.0) } else { 1.0 }
}

/// Centres des tuiles dans le repère tourné (pt, origine au centre de la page).
fn tiles(
    page_width: f64,
    page_height: f64,
    block_width: f64,
    block_height: f64,
    size: f64,
    angle: f64,
    density: Density,
) -> Vec<(f64, f64)> {
    let (gap_ratio, gap_lines) = match density {
        Density::Low => (0.9, 4.5),
        Density::Normal => (0.45, 2.6),
        Density::High => (0.15, 1.2),
    };
    let tile_width = block_width * size;
    let tile_height = block_height * size;
    let mut step_x = tile_width + (tile_width * gap_ratio).max(2.0 * size);
    let mut step_y = tile_height + gap_lines * size;

    let reach = page_width.hypot(page_height) / 2.0 + tile_width.hypot(tile_height);
    let radius = tile_width.hypot(tile_height) / 2.0;
    let (sin, cos) = angle.sin_cos();
    let (half_width, half_height) = (page_width / 2.0, page_height / 2.0);
    loop {
        // La grille est bornée avant toute allocation ; la liste des tuiles retenues aussi.
        let columns = (reach / step_x).ceil() + 1.0;
        let rows = (reach / step_y).ceil() + 1.0;
        if (2.0 * columns + 1.0) * (2.0 * rows + 1.0) > MAX_GRID_CELLS {
            step_x *= 1.25;
            step_y *= 1.25;
            continue;
        }
        let (columns, rows) = (columns as i64, rows as i64);
        let mut centers = Vec::new();
        'grid: for row in -rows..=rows {
            // Une rangée sur deux est décalée d'une demi-tuile (quinconce).
            let offset = if row.rem_euclid(2) == 1 { step_x / 2.0 } else { 0.0 };
            let y = row as f64 * step_y;
            for column in -columns..=columns {
                let x = column as f64 * step_x + offset;
                // Centre de la tuile dans le repère de la page (origine au centre).
                let (px, py) = (x * cos - y * sin, x * sin + y * cos);
                let dx = (px.abs() - half_width).max(0.0);
                let dy = (py.abs() - half_height).max(0.0);
                if dx.hypot(dy) <= radius {
                    centers.push((x, y));
                    if centers.len() > MAX_TILES {
                        break 'grid;
                    }
                }
            }
        }
        if centers.len() <= MAX_TILES {
            return centers;
        }
        // Trop de tuiles : on espace la mosaïque plutôt que d'en couper une partie.
        step_x *= 1.25;
        step_y *= 1.25;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A4: (f64, f64) = (595.276, 841.89);

    fn spec(layout: Layout, size: FontSize, angle: Angle) -> WatermarkSpec {
        WatermarkSpec {
            text: "x".into(),
            layout,
            size,
            angle,
            ..WatermarkSpec::default()
        }
    }

    /// Coins du bloc de texte une fois placé sur la page.
    fn corners(m: &Affine, bw: f64, bh: f64) -> [(f64, f64); 4] {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(sx, sy)| m.apply(sx * bw / 2.0, sy * bh / 2.0))
    }

    #[test]
    fn centered_auto_fits_inside_the_page() {
        for angle in [
            Angle::Diagonal,
            Angle::Degrees(0.0),
            Angle::Degrees(-45.0),
            Angle::Degrees(90.0),
        ] {
            for (w, h) in [A4, (A4.1, A4.0), (2000.0, 300.0)] {
                let m = placements(w, h, 12.0, 1.2, &spec(Layout::Centered, FontSize::Auto, angle));
                assert_eq!(m.len(), 1);
                let (cx, cy) = m[0].apply(0.0, 0.0);
                assert!((cx - w / 2.0).abs() < 1e-6 && (cy - h / 2.0).abs() < 1e-6);
                for (x, y) in corners(&m[0], 12.0, 1.2) {
                    assert!((-1e-6..=w + 1e-6).contains(&x), "{angle:?} {w}x{h}: x={x}");
                    assert!((-1e-6..=h + 1e-6).contains(&y), "{angle:?} {w}x{h}: y={y}");
                }
            }
        }
    }

    #[test]
    fn diagonal_follows_the_page_diagonal() {
        let m = placements(
            A4.0,
            A4.1,
            10.0,
            1.0,
            &spec(Layout::Centered, FontSize::Points(20.0), Angle::Diagonal),
        );
        // Le vecteur de base horizontal du texte pointe vers le coin supérieur droit.
        let slope = m[0].b / m[0].a;
        assert!((slope - A4.1 / A4.0).abs() < 1e-9, "{slope}");
        assert!((m[0].a.hypot(m[0].b) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn tiles_cover_the_whole_page() {
        let (bw, bh) = (10.0, 1.2);
        for density in [Density::Low, Density::Normal, Density::High] {
            let m = placements(
                A4.0,
                A4.1,
                bw,
                bh,
                &spec(Layout::Tiled(density), FontSize::Points(12.0), Angle::Degrees(30.0)),
            );
            assert!(m.len() > 10, "{density:?}: {}", m.len());
            let centers: Vec<_> = m.iter().map(|t| t.apply(0.0, 0.0)).collect();
            // Aucun point de la page n'est loin d'une tuile.
            for ix in 0..=20 {
                for iy in 0..=20 {
                    let (x, y) = (A4.0 * ix as f64 / 20.0, A4.1 * iy as f64 / 20.0);
                    let nearest = centers
                        .iter()
                        .map(|(cx, cy)| (cx - x).hypot(cy - y))
                        .fold(f64::MAX, f64::min);
                    assert!(nearest < 12.0 * bw * 1.2, "{density:?}: ({x}, {y}) à {nearest}");
                }
            }
        }
    }

    #[test]
    fn denser_mosaic_has_more_tiles() {
        let count = |density| {
            placements(
                A4.0,
                A4.1,
                10.0,
                1.2,
                &spec(Layout::Tiled(density), FontSize::Auto, Angle::Degrees(30.0)),
            )
            .len()
        };
        assert!(count(Density::Low) < count(Density::Normal));
        assert!(count(Density::Normal) < count(Density::High));
    }

    #[test]
    fn tile_count_is_bounded_on_huge_pages() {
        let m = placements(
            14_400.0,
            14_400.0,
            2.0,
            1.2,
            &spec(Layout::Tiled(Density::High), FontSize::Points(4.0), Angle::Degrees(0.0)),
        );
        assert!(!m.is_empty() && m.len() <= MAX_TILES, "{}", m.len());
    }

    #[test]
    fn absurd_pages_get_no_watermark_and_cost_nothing() {
        let tiled = spec(
            Layout::Tiled(Density::High),
            FontSize::Points(4.0),
            Angle::Degrees(30.0),
        );
        for (w, h) in [
            (1e30, 1e30),
            (f64::INFINITY, 100.0),
            (f64::NAN, 100.0),
            (0.0, 100.0),
            (-5.0, 100.0),
        ] {
            assert!(placements(w, h, 2.0, 1.2, &tiled).is_empty(), "{w}x{h}");
        }
        let elongated = placements(MAX_PAGE_SIDE, 10.0, 2.0, 1.2, &tiled);
        assert!(
            !elongated.is_empty() && elongated.len() <= MAX_TILES,
            "{}",
            elongated.len()
        );
    }

    #[test]
    fn tiles_never_lie_entirely_outside_the_page() {
        let (bw, bh) = (8.0, 1.2);
        let m = placements(
            A4.0,
            A4.1,
            bw,
            bh,
            &spec(
                Layout::Tiled(Density::Normal),
                FontSize::Points(14.0),
                Angle::Degrees(30.0),
            ),
        );
        let reach = 14.0 * bw.hypot(bh) / 2.0;
        for t in &m {
            let (x, y) = t.apply(0.0, 0.0);
            assert!(x > -reach - 1e-6 && x < A4.0 + reach + 1e-6 && y > -reach - 1e-6 && y < A4.1 + reach + 1e-6);
        }
    }
}
