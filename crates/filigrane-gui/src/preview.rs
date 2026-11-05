//! Aperçu fidèle : la vraie page du document, rendue par PDFium, avec le filigrane
//! dessiné par le même code que le mode incrusté.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use filigrane_core::{PageImage, PreviewPage, WatermarkSpec};

/// Côté du carré dans lequel la page est rendue, en pixels.
const RENDER_SIDE: u32 = 1100;
/// Page affichée tant qu'aucun document n'est choisi (A4 portrait).
const BLANK_PAGE: (f64, f64) = (595.276, 841.89);

struct Pending {
    page_index: u32,
    receiver: Receiver<filigrane_core::Result<PreviewPage>>,
}

pub struct Preview {
    source: Option<PathBuf>,
    page_index: u32,
    page_count: Option<u32>,
    base: Option<PageImage>,
    pending: Option<Pending>,
    error: Option<String>,
    texture: Option<TextureHandle>,
    /// Ce qui a servi à produire la texture actuelle.
    drawn_for: Option<(u32, String)>,
    base_generation: u32,
}

impl std::fmt::Debug for Preview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preview")
            .field("source", &self.source)
            .field("page_index", &self.page_index)
            .field("page_count", &self.page_count)
            .finish_non_exhaustive()
    }
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            source: None,
            page_index: 0,
            page_count: None,
            base: Some(blank_page()),
            pending: None,
            error: None,
            texture: None,
            drawn_for: None,
            base_generation: 0,
        }
    }
}

fn blank_page() -> PageImage {
    let longest = BLANK_PAGE.0.max(BLANK_PAGE.1);
    PageImage::blank(BLANK_PAGE.0, BLANK_PAGE.1, f64::from(RENDER_SIDE) / longest)
}

impl Preview {
    pub fn page_count(&self) -> Option<u32> {
        self.page_count
    }

    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    /// Erreur d'ouverture du document, le cas échéant.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_source(&mut self, ctx: &egui::Context, source: Option<&Path>) {
        self.source = source.map(Path::to_path_buf);
        self.page_index = 0;
        self.page_count = None;
        self.error = None;
        match source {
            Some(_) => self.request(ctx, 0),
            None => {
                self.pending = None;
                self.replace_base(Some(blank_page()));
            }
        }
    }

    fn replace_base(&mut self, base: Option<PageImage>) {
        self.base = base;
        self.base_generation = self.base_generation.wrapping_add(1);
    }

    fn request(&mut self, ctx: &egui::Context, page_index: u32) {
        let Some(source) = self.source.clone() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(filigrane_core::render_preview(&source, page_index, RENDER_SIDE));
            ctx.request_repaint();
        });
        self.pending = Some(Pending { page_index, receiver });
    }

    /// Récupère un rendu terminé et met à jour l'image si le filigrane a changé.
    pub fn update(&mut self, ctx: &egui::Context, spec: Option<&WatermarkSpec>) {
        if let Some(pending) = &self.pending
            && let Ok(result) = pending.receiver.try_recv()
        {
            self.page_index = pending.page_index;
            self.pending = None;
            match result {
                Ok(page) => {
                    self.page_count = Some(page.page_count);
                    self.error = None;
                    self.replace_base(Some(page.image));
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.replace_base(None);
                }
            }
        }

        let Some(base) = &self.base else {
            self.texture = None;
            self.drawn_for = None;
            return;
        };
        let spec_on_page = spec.filter(|s| s.pages.includes(self.page_index + 1));
        let key = (self.base_generation, format!("{spec_on_page:?}"));
        if self.drawn_for.as_ref() == Some(&key) {
            return;
        }
        let mut image = base.clone();
        if let Some(spec) = spec_on_page {
            // Les erreurs de saisie sont signalées dans le formulaire.
            let _ = image.draw_watermark(spec);
        }
        let color_image =
            ColorImage::from_rgba_unmultiplied([image.pixel_width as usize, image.pixel_height as usize], &image.rgba);
        match &mut self.texture {
            Some(texture) => texture.set(color_image, TextureOptions::LINEAR),
            None => self.texture = Some(ctx.load_texture("apercu", color_image, TextureOptions::LINEAR)),
        }
        self.drawn_for = Some(key);
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if let Some(count) = self.page_count.filter(|count| *count > 1) {
            ui.horizontal(|ui| {
                let busy = self.pending.is_some();
                if ui
                    .add_enabled(!busy && self.page_index > 0, egui::Button::new("◀"))
                    .on_hover_text("Page précédente")
                    .clicked()
                {
                    let target = self.page_index - 1;
                    self.request(ui.ctx(), target);
                }
                ui.label(format!("Page {} / {count}", self.page_index + 1));
                if ui
                    .add_enabled(!busy && self.page_index + 1 < count, egui::Button::new("▶"))
                    .on_hover_text("Page suivante")
                    .clicked()
                {
                    let target = self.page_index + 1;
                    self.request(ui.ctx(), target);
                }
                if busy {
                    ui.spinner();
                }
            });
            ui.add_space(6.0);
        }

        let available = ui.available_rect_before_wrap();
        let Some(texture) = &self.texture else {
            ui.centered_and_justified(|ui| {
                if self.pending.is_some() {
                    ui.spinner();
                } else if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            });
            return;
        };

        let image_size = texture.size_vec2();
        let scale = (available.width() / image_size.x)
            .min(available.height() / image_size.y)
            .min(1.0);
        let size = image_size * scale;
        let rect = egui::Rect::from_center_size(available.center(), size);
        let painter = ui.painter_at(available);
        painter.rect_filled(rect.translate(egui::vec2(3.0, 4.0)), 2.0, Color32::from_black_alpha(40));
        painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, Color32::from_gray(190)),
            egui::StrokeKind::Outside,
        );
        ui.allocate_rect(available, egui::Sense::hover());
    }
}
