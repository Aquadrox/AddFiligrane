//! Fenêtre principale : formulaire à gauche, aperçu au centre, action en bas.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{self, Align, Color32, Key, KeyboardShortcut, Layout, Modifiers, RichText};
use filigrane_core::{Method, Report, WatermarkSpec};

use crate::preview::Preview;
use crate::settings::{Arrangement, PRESET_COLORS, Protection, Settings, Spacing};
use crate::worker::Worker;

const OPEN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const RUN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
const DPI_CHOICES: [(u32, &str); 4] = [
    (100, "100 ppp — fichier léger"),
    (150, "150 ppp — recommandé"),
    (200, "200 ppp — net"),
    (300, "300 ppp — impression"),
];

enum Status {
    Idle,
    Running(Worker),
    Succeeded {
        output: PathBuf,
        report: Report,
        elapsed: Duration,
    },
    Failed(String),
    Cancelled,
}

pub struct FiligraneApp {
    settings: Settings,
    input: Option<PathBuf>,
    input_size: Option<u64>,
    /// Destination choisie explicitement ; sinon `<nom>_filigrane.pdf`.
    output_override: Option<PathBuf>,
    preview: Preview,
    status: Status,
}

impl FiligraneApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_input: Option<PathBuf>) -> Self {
        crate::style::apply(&cc.egui_ctx);
        let settings = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default();
        let mut app = Self {
            settings,
            input: None,
            input_size: None,
            output_override: None,
            preview: Preview::default(),
            status: Status::Idle,
        };
        if let Some(path) = initial_input.filter(|path| path.is_file()) {
            app.open_input(&cc.egui_ctx, path);
        }
        app
    }

    fn output_path(&self) -> Option<PathBuf> {
        self.output_override
            .clone()
            .or_else(|| self.input.as_deref().map(filigrane_core::default_output_path))
    }

    fn is_running(&self) -> bool {
        matches!(self.status, Status::Running(_))
    }

    fn open_input(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.is_running() {
            return;
        }
        self.input_size = std::fs::metadata(&path).ok().map(|m| m.len());
        if let Some(dir) = path.parent() {
            self.settings.last_directory = Some(dir.to_path_buf());
        }
        self.preview.set_source(ctx, Some(&path));
        self.input = Some(path);
        self.output_override = None;
        self.status = Status::Idle;
    }

    fn pick_input(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choisir un document PDF")
            .add_filter("Documents PDF", &["pdf", "PDF"]);
        if let Some(dir) = &self.settings.last_directory {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_input(ctx, path);
        }
    }

    fn pick_output(&mut self) {
        let Some(current) = self.output_path() else {
            return;
        };
        let mut dialog = rfd::FileDialog::new()
            .set_title("Enregistrer le PDF filigrané sous")
            .add_filter("Documents PDF", &["pdf"]);
        if let Some(dir) = current.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = current.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
        if let Some(mut path) = dialog.save_file() {
            if path.extension().is_none() {
                path.set_extension("pdf");
            }
            self.output_override = Some(path);
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let (Some(input), Some(output)) = (self.input.clone(), self.output_path()) else {
            return;
        };
        let Ok(spec) = self.settings.to_spec() else {
            return;
        };
        if output.exists() && !confirm_overwrite(&output) {
            return;
        }
        let job = filigrane_core::Job {
            input,
            output,
            spec,
            mode: self.settings.mode(),
        };
        let ctx = ctx.clone();
        self.status = Status::Running(Worker::spawn(job, move || ctx.request_repaint()));
    }

    fn poll_worker(&mut self) {
        let Status::Running(worker) = &mut self.status else {
            return;
        };
        let Some(outcome) = worker.try_finish() else {
            return;
        };
        self.status = match outcome.result {
            Ok(report) => Status::Succeeded {
                output: outcome.output,
                report,
                elapsed: outcome.elapsed,
            },
            Err(filigrane_core::Error::Cancelled) => Status::Cancelled,
            Err(error) => Status::Failed(error.to_string()),
        };
    }

    fn handle_drops_and_shortcuts(&mut self, ctx: &egui::Context) {
        let dropped: Option<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .find(|path| is_pdf(path))
        });
        if let Some(path) = dropped {
            self.open_input(ctx, path);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN_SHORTCUT)) && !self.is_running() {
            self.pick_input(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&RUN_SHORTCUT)) && self.can_start().is_ok() {
            self.start(ctx);
        }
    }

    /// `Ok` si le traitement peut démarrer, sinon la raison à afficher.
    fn can_start(&self) -> Result<WatermarkSpec, String> {
        if self.is_running() {
            return Err("Traitement en cours…".into());
        }
        if self.input.is_none() {
            return Err("Choisissez d'abord un document PDF.".into());
        }
        if let Some(error) = self.preview.error() {
            return Err(error.to_owned());
        }
        self.settings.to_spec()
    }
}

impl eframe::App for FiligraneApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_worker();
        self.handle_drops_and_shortcuts(&ctx);
        let spec = self.settings.to_spec().ok();
        self.preview.update(&ctx, spec.as_ref());

        egui::Panel::bottom("actions")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(16, 12)))
            .show(ui, |ui| self.actions(ui));

        egui::Panel::left("reglages")
            .resizable(false)
            .exact_size(430.0)
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(16, 12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| self.form(ui));
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(ui.style())
                    .fill(ui.visuals().extreme_bg_color)
                    .inner_margin(16),
            )
            .show(ui, |ui| {
                let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
                self.preview.show(ui);
                if hovering {
                    drop_overlay(ui);
                }
            });

        if let Status::Running(worker) = &self.status
            && !worker.is_cancelling()
        {
            // Rafraîchit le chronomètre même sans nouvelle étape.
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }
}

impl FiligraneApp {
    fn form(&mut self, ui: &mut egui::Ui) {
        let running = self.is_running();
        ui.add_enabled_ui(!running, |ui| {
            self.document_section(ui);
            self.text_section(ui);
            self.appearance_section(ui);
            self.protection_section(ui);
            self.pages_section(ui);
        });
    }

    fn document_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "1. Document", |ui| match &self.input {
            Some(path) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📄").size(20.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(file_name(path)).strong())
                            .on_hover_text(path.display().to_string());
                        let mut details = Vec::new();
                        if let Some(count) = self.preview.page_count() {
                            details.push(format!("{count} page(s)"));
                        }
                        if let Some(size) = self.input_size {
                            details.push(human_size(size));
                        }
                        if self.preview.is_loading() && self.preview.page_count().is_none() {
                            details.push("analyse…".into());
                        }
                        ui.label(RichText::new(details.join(" · ")).weak());
                    });
                });
                if let Some(error) = self.preview.error() {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                if ui.button("Changer de document…").clicked() {
                    self.pick_input(ui.ctx());
                }
            }
            None => {
                let button = egui::Button::new(RichText::new("📂  Choisir un PDF…").size(16.0))
                    .min_size(egui::vec2(ui.available_width(), 44.0));
                if ui.add(button).on_hover_text("Ctrl+O").clicked() {
                    self.pick_input(ui.ctx());
                }
                ui.label(RichText::new("ou glissez-déposez un fichier dans la fenêtre").weak());
            }
        });
    }

    fn text_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "2. Texte du filigrane", |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.settings.text)
                    .hint_text("Ex. : Pour mandat de vente – Agence Dupont")
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui
                    .small_button("+ date du jour")
                    .on_hover_text("Ajoute la date : un filigrane daté est plus difficile à réutiliser.")
                    .clicked()
                {
                    let date = chrono::Local::now().format("%d/%m/%Y").to_string();
                    let text = self.settings.text.trim_end().to_owned();
                    self.settings.text = if text.is_empty() {
                        date
                    } else {
                        format!("{text} – {date}")
                    };
                }
                ui.add(egui::Label::new(RichText::new("Précisez le destinataire et l'usage.").weak().small()).wrap());
            });
            if !self.settings.text.trim().is_empty()
                && let Err(error) = filigrane_core::check_text(&self.settings.text, self.weight())
            {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        });
    }

    fn weight(&self) -> filigrane_core::FontWeight {
        if self.settings.bold {
            filigrane_core::FontWeight::Bold
        } else {
            filigrane_core::FontWeight::Regular
        }
    }

    fn appearance_section(&mut self, ui: &mut egui::Ui) {
        let settings = &mut self.settings;
        section(ui, "3. Apparence", |ui| {
            egui::Grid::new("apparence")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Disposition");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut settings.arrangement, Arrangement::Tiled, "Mosaïque");
                        ui.selectable_value(&mut settings.arrangement, Arrangement::Centered, "Centré");
                    });
                    ui.end_row();

                    if settings.arrangement == Arrangement::Tiled {
                        ui.label("Densité");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut settings.spacing, Spacing::Low, "Faible");
                            ui.selectable_value(&mut settings.spacing, Spacing::Normal, "Normale");
                            ui.selectable_value(&mut settings.spacing, Spacing::High, "Forte");
                        });
                        ui.end_row();
                    }

                    ui.label("Taille");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut settings.auto_size, "Auto");
                        ui.add_enabled(
                            !settings.auto_size,
                            egui::Slider::new(&mut settings.size_points, 6.0..=200.0)
                                .suffix(" pt")
                                .logarithmic(true)
                                .max_decimals(0),
                        );
                    });
                    ui.end_row();

                    ui.label("Inclinaison");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut settings.diagonal, "Diagonale")
                            .on_hover_text("Suit la diagonale de chaque page");
                        ui.add_enabled(
                            !settings.diagonal,
                            egui::Slider::new(&mut settings.angle_degrees, -90.0..=90.0)
                                .suffix("°")
                                .step_by(5.0),
                        );
                    });
                    ui.end_row();

                    ui.label("Couleur");
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgb(&mut settings.color);
                        for (name, rgb) in PRESET_COLORS {
                            let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                            let selected = settings.color == rgb;
                            let swatch = egui::Button::new("")
                                .fill(color)
                                .min_size(egui::vec2(20.0, 20.0))
                                .stroke(if selected {
                                    egui::Stroke::new(2.0, ui.visuals().selection.stroke.color)
                                } else {
                                    egui::Stroke::NONE
                                });
                            if ui.add(swatch).on_hover_text(name).clicked() {
                                settings.color = rgb;
                            }
                        }
                    });
                    ui.end_row();

                    ui.label("Opacité");
                    ui.add(
                        egui::Slider::new(&mut settings.opacity_percent, 5.0..=100.0)
                            .suffix(" %")
                            .max_decimals(0),
                    );
                    ui.end_row();

                    ui.label("Style");
                    ui.checkbox(&mut settings.bold, "Gras");
                    ui.end_row();
                });
        });
    }

    fn protection_section(&mut self, ui: &mut egui::Ui) {
        let settings = &mut self.settings;
        section(ui, "4. Protection", |ui| {
            choice(
                ui,
                &mut settings.protection,
                Protection::Flatten,
                "Incrusté (recommandé)",
                "Le filigrane est fondu dans l'image de chaque page : impossible à retirer. \
                 Idéal pour les pièces d'identité et justificatifs. Le texte n'est plus sélectionnable.",
            );
            choice(
                ui,
                &mut settings.protection,
                Protection::Overlay,
                "Superposé (rapide)",
                "Ajouté par-dessus les pages : quasi instantané même pour des milliers de pages, \
                 fichier presque inchangé. Peut être retiré avec un éditeur PDF.",
            );
            egui::CollapsingHeader::new("Options avancées").show(ui, |ui| match settings.protection {
                Protection::Flatten => {
                    ui.horizontal(|ui| {
                        ui.label("Résolution");
                        let current = DPI_CHOICES
                            .iter()
                            .find(|(dpi, _)| *dpi == settings.dpi)
                            .map_or_else(|| format!("{} ppp", settings.dpi), |(_, label)| (*label).to_owned());
                        egui::ComboBox::from_id_salt("dpi")
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for (dpi, label) in DPI_CHOICES {
                                    ui.selectable_value(&mut settings.dpi, dpi, label);
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Qualité JPEG");
                        ui.add(egui::Slider::new(&mut settings.jpeg_quality, 50..=100));
                    });
                }
                Protection::Overlay => {
                    ui.horizontal(|ui| {
                        ui.label("Position");
                        ui.selectable_value(&mut settings.background, false, "Au-dessus du contenu");
                        ui.selectable_value(&mut settings.background, true, "En dessous");
                    });
                    ui.label(
                        RichText::new("« En dessous » est masqué par les images opaques (scans).")
                            .weak()
                            .small(),
                    );
                }
            });
        });
    }

    fn pages_section(&mut self, ui: &mut egui::Ui) {
        let settings = &mut self.settings;
        section(ui, "5. Pages", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut settings.all_pages, true, "Toutes");
                ui.radio_value(&mut settings.all_pages, false, "Sélection :");
                ui.add_enabled(
                    !settings.all_pages,
                    egui::TextEdit::singleline(&mut settings.page_ranges)
                        .hint_text("1-3, 7, 10-")
                        .desired_width(f32::INFINITY),
                );
            });
            if !settings.all_pages
                && let Err(error) = settings.page_ranges.parse::<filigrane_core::PageSelection>()
            {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        });
    }

    fn actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Enregistrer sous :");
            match self.output_path() {
                Some(path) => {
                    ui.label(RichText::new(file_name(&path)).strong())
                        .on_hover_text(path.display().to_string());
                    if ui
                        .add_enabled(!self.is_running(), egui::Button::new("Modifier…"))
                        .clicked()
                    {
                        self.pick_output();
                    }
                }
                None => {
                    ui.label(RichText::new("—").weak());
                }
            }
        });
        ui.add_space(8.0);

        let ctx = ui.ctx().clone();
        let mut start = false;
        let mut cancel = false;
        ui.horizontal(|ui| match &self.status {
            Status::Running(worker) => {
                let progress = worker.progress();
                let mut bar = egui::ProgressBar::new(progress.fraction().unwrap_or(0.0))
                    .text(format!(
                        "{}  ·  {}",
                        progress.label(),
                        format_duration(worker.elapsed())
                    ))
                    .desired_width(ui.available_width() - 120.0);
                if progress.fraction().is_none() {
                    bar = bar.animate(true);
                }
                ui.add(bar);
                let label = if worker.is_cancelling() {
                    "Annulation…"
                } else {
                    "Annuler"
                };
                cancel = ui
                    .add_enabled(
                        !worker.is_cancelling(),
                        egui::Button::new(label).min_size(egui::vec2(110.0, 32.0)),
                    )
                    .clicked();
            }
            _ => {
                let ready = self.can_start();
                let caption = RichText::new("Ajouter le filigrane").size(17.0).strong();
                let caption = if ready.is_ok() {
                    caption.color(Color32::WHITE)
                } else {
                    caption
                };
                let button = egui::Button::new(caption)
                    .fill(if ready.is_ok() {
                        ui.visuals().selection.bg_fill
                    } else {
                        ui.visuals().widgets.inactive.bg_fill
                    })
                    .min_size(egui::vec2(240.0, 40.0));
                let response = ui.add_enabled(ready.is_ok(), button);
                start = response.clicked();
                match ready {
                    Ok(_) => {
                        response.on_hover_text("Ctrl+Entrée");
                    }
                    Err(reason) => {
                        response.on_disabled_hover_text(&reason);
                        ui.label(RichText::new(reason).weak());
                    }
                }
            }
        });
        if cancel && let Status::Running(worker) = &self.status {
            worker.cancel();
        }
        if start {
            self.start(&ctx);
        }

        match &self.status {
            Status::Succeeded {
                output,
                report,
                elapsed,
            } => {
                ui.add_space(8.0);
                let output = output.clone();
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::from_rgb(0x1E, 0x8E, 0x3E), "✔");
                    ui.label(success_message(report, *elapsed));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Afficher dans le dossier").clicked() {
                            let _ = opener::reveal(&output);
                        }
                        if ui.button("Ouvrir le PDF").clicked() {
                            let _ = opener::open(&output);
                        }
                    });
                });
            }
            Status::Failed(message) => {
                ui.add_space(8.0);
                ui.colored_label(ui.visuals().error_fg_color, format!("✖ {message}"));
            }
            Status::Cancelled => {
                ui.add_space(8.0);
                ui.label(RichText::new("Traitement annulé : aucun fichier n'a été modifié.").weak());
            }
            Status::Idle | Status::Running(_) => {}
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.add_space(4.0);
    egui::Frame::group(ui.style()).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add_contents(ui);
    });
    ui.add_space(8.0);
}

fn choice<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, option: T, title: &str, description: &str) {
    ui.radio_value(value, option, RichText::new(title).strong());
    ui.indent(title, |ui| {
        ui.add(egui::Label::new(RichText::new(description).weak().small()).wrap());
    });
    ui.add_space(4.0);
}

fn drop_overlay(ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("depot")));
    painter.rect_filled(rect, 8.0, Color32::from_rgba_unmultiplied(30, 80, 200, 40));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Déposez le PDF ici",
        egui::FontId::proportional(24.0),
        ui.visuals().strong_text_color(),
    );
}

fn confirm_overwrite(path: &Path) -> bool {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("Remplacer le fichier ?")
        .set_description(format!("« {} » existe déjà. Le remplacer ?", file_name(path)))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["octets", "Ko", "Mo", "Go"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} octets")
    } else {
        format!("{value:.1} {}", UNITS[unit]).replace('.', ",")
    }
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{:.1} s", duration.as_secs_f64()).replace('.', ",")
    } else {
        format!("{} min {:02} s", seconds / 60, seconds % 60)
    }
}

pub(crate) fn success_message(report: &Report, elapsed: Duration) -> String {
    let how = match report.method {
        Method::Flattened => "incrusté",
        Method::Incremental => "superposé",
        Method::Rewritten => "superposé (document réparé)",
    };
    let mut message = format!(
        "Filigrane {how} sur {} page(s) sur {} · {} · {}",
        report.pages_watermarked,
        report.pages_total,
        human_size(report.output_bytes),
        format_duration(elapsed)
    );
    if report.pages_skipped > 0 {
        message.push_str(&format!(" · {} page(s) invalide(s) ignorée(s)", report.pages_skipped));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(512), "512 octets");
        assert_eq!(human_size(4_800_000), "4,8 Mo");
        assert_eq!(human_size(2_500_000_000), "2,5 Go");
    }

    #[test]
    fn durations_are_human_readable() {
        assert_eq!(format_duration(Duration::from_millis(3_240)), "3,2 s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2 min 05 s");
    }

    #[test]
    fn success_message_mentions_skipped_pages() {
        let report = Report {
            pages_total: 10,
            pages_watermarked: 9,
            pages_skipped: 1,
            output_bytes: 1_000,
            method: Method::Incremental,
        };
        let message = success_message(&report, Duration::from_secs(1));
        assert!(message.contains("9 page(s) sur 10"), "{message}");
        assert!(message.contains("1 page(s) invalide(s)"), "{message}");
    }

    #[test]
    fn only_pdf_files_are_accepted() {
        assert!(is_pdf(Path::new("a/b.PDF")));
        assert!(!is_pdf(Path::new("a/b.docx")));
        assert!(!is_pdf(Path::new("a/pdf")));
    }
}
