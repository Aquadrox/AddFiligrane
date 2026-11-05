//! Images de pages et dessin matriciel du filigrane (mode incrusté et aperçu).

use pdfium_render::prelude::{PdfColor, PdfDocument, PdfRenderConfig};

use crate::error::{Error, Result};
use crate::font::TextBlock;
use crate::geometry::Affine;
use crate::layout::placements;
use crate::spec::WatermarkSpec;

/// Nombre maximal de pixels d'une page rendue (environ 160 Mo en RGBA).
pub const MAX_PAGE_PIXELS: u64 = 40_000_000;

/// Page rendue en pixels RGBA (non prémultipliés, mais opaques).
#[derive(Clone)]
pub struct PageImage {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba: Vec<u8>,
    /// Dimensions de la page affichée, en points.
    pub width: f64,
    pub height: f64,
}

impl std::fmt::Debug for PageImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageImage")
            .field("pixel_width", &self.pixel_width)
            .field("pixel_height", &self.pixel_height)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl PageImage {
    /// Page blanche, utile pour l'aperçu sans document.
    pub fn blank(width: f64, height: f64, pixels_per_point: f64) -> Self {
        let pixel_width = (width * pixels_per_point).round().max(1.0) as u32;
        let pixel_height = (height * pixels_per_point).round().max(1.0) as u32;
        Self {
            pixel_width,
            pixel_height,
            rgba: vec![255; pixel_width as usize * pixel_height as usize * 4],
            width,
            height,
        }
    }

    /// Dessine le filigrane sur l'image.
    pub fn draw_watermark(&mut self, spec: &WatermarkSpec) -> Result<()> {
        spec.validate()?;
        let block = TextBlock::layout(&spec.text, spec.weight)?;
        let outline = block.outline().ok_or(Error::EmptyText)?;
        self.draw_block(&block, &outline, spec);
        Ok(())
    }

    pub(crate) fn draw_block(&mut self, block: &TextBlock, outline: &tiny_skia::Path, spec: &WatermarkSpec) {
        let Some(mut pixmap) = tiny_skia::PixmapMut::from_bytes(&mut self.rgba, self.pixel_width, self.pixel_height)
        else {
            return;
        };
        let [r, g, b] = [spec.color.r, spec.color.g, spec.color.b];
        let alpha = (spec.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(r, g, b, alpha);
        paint.anti_alias = true;

        // Points (origine en bas à gauche) → pixels (origine en haut à gauche).
        let to_pixels = Affine::new(
            f64::from(self.pixel_width) / self.width,
            0.0,
            0.0,
            -f64::from(self.pixel_height) / self.height,
            0.0,
            f64::from(self.pixel_height),
        );
        for placement in placements(self.width, self.height, block.width, block.height, spec) {
            let m = placement.then(&to_pixels);
            let transform =
                tiny_skia::Transform::from_row(m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32);
            pixmap.fill_path(outline, &paint, tiny_skia::FillRule::Winding, transform, None);
        }
    }

    /// Pixels RGB (sans canal alpha), pour l'encodage JPEG.
    pub(crate) fn to_rgb(&self) -> Vec<u8> {
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect()
    }
}

/// Rend la page `index` (à partir de 0) à `pixels_per_point` pixels par point, en
/// réduisant la résolution si l'image dépasserait [`MAX_PAGE_PIXELS`].
pub(crate) fn render_page(document: &PdfDocument<'_>, index: u32, pixels_per_point: f64) -> Result<PageImage> {
    let render_error = |message: String| Error::Render {
        page: index + 1,
        message,
    };
    let page = document
        .pages()
        .get(index as i32)
        .map_err(|e| render_error(e.to_string()))?;
    let (width, height) = (f64::from(page.width().value), f64::from(page.height().value));
    if !(width > 0.0 && height > 0.0) {
        return Err(render_error("dimensions de page invalides".into()));
    }

    let pixels = width * height * pixels_per_point * pixels_per_point;
    let scale = if pixels > MAX_PAGE_PIXELS as f64 {
        pixels_per_point * (MAX_PAGE_PIXELS as f64 / pixels).sqrt()
    } else {
        pixels_per_point
    };
    let config = PdfRenderConfig::new()
        .set_target_width((width * scale).round().max(1.0) as i32)
        .set_target_height((height * scale).round().max(1.0) as i32)
        .clear_before_rendering(true)
        .set_clear_color(PdfColor::WHITE)
        .render_annotations(true)
        .render_form_data(true)
        // Sans cette limite, PDFium garde les images décodées de chaque page rendue.
        .limit_render_image_cache_size(true);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| render_error(e.to_string()))?;

    let mut rgba = bitmap.as_rgba_bytes();
    // Le fond est blanc et opaque ; on force l'alpha pour garantir une image opaque.
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    Ok(PageImage {
        pixel_width: bitmap.width() as u32,
        pixel_height: bitmap.height() as u32,
        rgba,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Angle, FontSize, Layout};

    fn red_pixels(image: &PageImage) -> Vec<(u32, u32)> {
        image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, p)| p[0] > 200 && p[1] < 150 && p[2] < 150)
            .map(|(i, _)| (i as u32 % image.pixel_width, i as u32 / image.pixel_width))
            .collect()
    }

    #[test]
    fn centered_watermark_is_centered_and_rises_to_the_right() {
        let mut image = PageImage::blank(595.0, 842.0, 0.5);
        let spec = WatermarkSpec {
            text: "CONFIDENTIEL".into(),
            layout: Layout::Centered,
            size: FontSize::Auto,
            angle: Angle::Diagonal,
            opacity: 1.0,
            ..WatermarkSpec::default()
        };
        image.draw_watermark(&spec).unwrap();
        let red = red_pixels(&image);
        assert!(red.len() > 500, "{}", red.len());
        let n = red.len() as f64;
        let (cx, cy) = red
            .iter()
            .fold((0.0, 0.0), |(x, y), p| (x + f64::from(p.0) / n, y + f64::from(p.1) / n));
        assert!((cx - f64::from(image.pixel_width) / 2.0).abs() < 10.0, "cx={cx}");
        assert!((cy - f64::from(image.pixel_height) / 2.0).abs() < 10.0, "cy={cy}");
        // Covariance négative en pixels (y vers le bas) = texte montant vers la droite.
        let cov: f64 = red.iter().map(|p| (f64::from(p.0) - cx) * (f64::from(p.1) - cy)).sum();
        assert!(cov < 0.0, "{cov}");
    }

    #[test]
    fn opacity_blends_with_the_background() {
        let mut image = PageImage::blank(100.0, 100.0, 2.0);
        let spec = WatermarkSpec {
            text: "████".into(),
            layout: Layout::Centered,
            angle: Angle::Degrees(0.0),
            opacity: 0.5,
            color: crate::Rgb::new(0, 0, 0),
            ..WatermarkSpec::default()
        };
        image.draw_watermark(&spec).unwrap();
        let darkest = image.rgba.as_chunks::<4>().0.iter().map(|p| p[0]).min().unwrap();
        assert!((120..=135).contains(&darkest), "{darkest}");
    }
}
