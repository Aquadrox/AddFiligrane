//! Mode superposé : le filigrane devient un objet graphique (« Form XObject ») partagé
//! par toutes les pages, ajouté au contenu existant sans le réécrire.
//!
//! Le document n'est jamais chargé entièrement : il est projeté en mémoire, le contenu
//! des flux (images, polices…) est ignoré à l'analyse et le fichier est recopié octet
//! par octet avant d'y ajouter une révision. Le coût est donc celui d'une copie de
//! fichier, quelle que soit la taille du document.

mod font_embed;
mod incremental;

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId, Stream, dictionary};

use crate::error::{Error, Result};
use crate::font::TextBlock;
use crate::geometry::{Affine, Rect, format_number};
use crate::layout::placements;
use crate::output::AtomicOutput;
use crate::progress::{CancelToken, Progress, ProgressSink};
use crate::spec::{Job, Layer};
use crate::{Method, Report};

use self::incremental::PreviousRevision;

/// Format « Lettre US », taille par défaut quand `/MediaBox` est absente ou invalide.
const DEFAULT_MEDIA_BOX: Rect = Rect {
    x0: 0.0,
    y0: 0.0,
    x1: 612.0,
    y1: 792.0,
};
/// Profondeur maximale de l'arbre des pages, contre les cycles.
const MAX_TREE_DEPTH: usize = 64;
const XOBJECT_NAME: &str = "FiligranePDF";

pub(crate) fn run(
    job: &Job,
    block: &TextBlock,
    layer: Layer,
    progress: &dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Report> {
    progress.report(Progress::Opening);
    let mapping = map_file(&job.input)?;
    let bytes: &[u8] = &mapping;

    let mut doc = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            filter: Some(skip_stream_data),
            ..LoadOptions::default()
        },
    )
    .map_err(|e| match e {
        lopdf::Error::Decryption(_) | lopdf::Error::InvalidPassword => Error::EncryptedOverlay,
        other => Error::InvalidPdf(other.to_string()),
    })?;
    if doc.trailer.has(b"Encrypt") || doc.encryption_state.is_some() {
        return Err(Error::EncryptedOverlay);
    }
    // Une révision ne peut s'ajouter qu'à un fichier dont la structure est saine ;
    // sinon le document est entièrement réécrit (ce qui le répare au passage).
    let incremental = bytes.starts_with(b"%PDF-") && doc.xref_start != 0 && doc.xref_recovery.is_none();
    if !incremental {
        doc = Document::load_mem(bytes).map_err(|e| Error::InvalidPdf(e.to_string()))?;
    }
    cancel.check()?;

    let pages = doc.get_pages();
    let pages_total = u32::try_from(pages.len()).unwrap_or(u32::MAX);
    if pages_total == 0 {
        return Err(Error::InvalidPdf("aucune page trouvée".into()));
    }
    job.spec.pages.ensure_matches(pages_total)?;

    let first_free = doc
        .trailer
        .get(b"Size")
        .and_then(Object::as_i64)
        .ok()
        .and_then(|size| u32::try_from(size).ok())
        .unwrap_or(0)
        .max(doc.max_id + 1);
    let mut builder = EditBuilder::new(&doc, block, job, layer, IdAllocator::new(first_free));

    let mut pages_watermarked = 0;
    let mut pages_skipped = 0;
    for (&number, &page_id) in &pages {
        if job.spec.pages.includes(number) {
            if number % 64 == 0 {
                cancel.check()?;
            }
            if builder.watermark_page(page_id) {
                pages_watermarked += 1;
            } else {
                pages_skipped += 1;
            }
        }
        if number % 256 == 0 || number == pages_total {
            progress.report(Progress::Pages {
                done: number,
                total: pages_total,
            });
        }
    }
    let (objects, next_free_id) = builder.finish();

    let mut output = AtomicOutput::create(&job.output)?;
    let method = if incremental {
        let previous = PreviousRevision {
            bytes,
            xref_start: doc.xref_start as u64,
            xref_type: doc.reference_table.cross_reference_type,
            trailer: &doc.trailer,
        };
        incremental::write(&mut output, &previous, objects, next_free_id, progress, cancel)?;
        Method::Incremental
    } else {
        progress.report(Progress::Finalizing);
        for (id, object) in objects {
            doc.objects.insert(id, object);
        }
        doc.max_id = doc.max_id.max(next_free_id - 1);
        doc.save_to(output.writer()).map_err(|e| output.write_error(e))?;
        Method::Rewritten
    };
    cancel.check()?;
    progress.report(Progress::Finalizing);
    let output_bytes = output.commit()?;

    Ok(Report {
        pages_total,
        pages_watermarked,
        pages_skipped,
        output_bytes,
        method,
    })
}

/// Projette le fichier en mémoire : le système charge les pages à la demande, ce qui
/// permet de traiter des fichiers plus gros que la mémoire disponible.
#[allow(unsafe_code)]
fn map_file(path: &Path) -> Result<memmap2::Mmap> {
    let file = File::open(path).map_err(|e| Error::read(path, e))?;
    let len = file.metadata().map_err(|e| Error::read(path, e))?.len();
    if len == 0 {
        return Err(Error::InvalidPdf("fichier vide".into()));
    }
    // SAFETY: la projection est en lecture seule et ne vit que le temps du traitement.
    // Si un autre programme modifiait le fichier pendant ce temps, les octets lus
    // pourraient changer : le PDF produit serait incohérent, comme avec toute lecture
    // concurrente, mais aucune donnée Rust ne dépend de leur stabilité.
    unsafe { memmap2::Mmap::map(&file) }.map_err(|e| Error::read(path, e))
}

/// Ignore le contenu des flux à l'analyse : seuls les dictionnaires sont utiles.
/// Les flux d'objets doivent rester intacts, lopdf en extrait les objets compressés.
///
/// lopdf conserve l'objet modifié sur place pour les objets de premier niveau, mais
/// la valeur renvoyée pour ceux extraits d'un flux d'objets : il faut faire les deux.
fn skip_stream_data(id: ObjectId, object: &mut Object) -> Option<(ObjectId, Object)> {
    if let Object::Stream(stream) = object
        && !stream.dict.has_type(b"ObjStm")
        && !stream.dict.has_type(b"XRef")
    {
        stream.content = Vec::new();
    }
    Some((id, object.clone()))
}

#[derive(Debug)]
pub(crate) struct IdAllocator {
    next: u32,
}

impl IdAllocator {
    pub fn new(first: u32) -> Self {
        Self { next: first.max(1) }
    }

    pub fn next(&mut self) -> ObjectId {
        let id = (self.next, 0);
        self.next += 1;
        id
    }
}

/// Accumule les objets à ajouter ou remplacer.
struct EditBuilder<'a> {
    doc: &'a Document,
    block: &'a TextBlock,
    job: &'a Job,
    layer: Layer,
    ids: IdAllocator,
    objects: Vec<(ObjectId, Object)>,
    xobject_id: ObjectId,
    /// Flux `q` partagé qui isole le contenu d'origine (premier plan uniquement).
    save_state_id: Option<ObjectId>,
    /// Flux de dessin déjà créés, par contenu (beaucoup de pages ont la même géométrie).
    draw_streams: HashMap<Vec<u8>, ObjectId>,
    /// Ressources partagées déjà complétées : original → (copie complétée, nom du filigrane).
    shared_resources: HashMap<ObjectId, (ObjectId, String)>,
}

impl<'a> EditBuilder<'a> {
    fn new(doc: &'a Document, block: &'a TextBlock, job: &'a Job, layer: Layer, mut ids: IdAllocator) -> Self {
        let font = font_embed::embed(block, &mut ids);
        let xobject_id = ids.next();
        let mut objects = font.objects.clone();
        objects.push((xobject_id, Object::Stream(watermark_xobject(block, job, &font))));
        Self {
            doc,
            block,
            job,
            layer,
            ids,
            objects,
            xobject_id,
            save_state_id: None,
            draw_streams: HashMap::new(),
            shared_resources: HashMap::new(),
        }
    }

    fn finish(self) -> (Vec<(ObjectId, Object)>, u32) {
        (self.objects, self.ids.next)
    }

    /// Ajoute le filigrane à une page. Renvoie `false` si la page est inexploitable.
    fn watermark_page(&mut self, page_id: ObjectId) -> bool {
        let doc = self.doc;
        let Ok(original) = doc.get_dictionary(page_id) else {
            return false;
        };
        let Some(contents) = self.content_references(original) else {
            return false;
        };

        let media_box = self.inherited_rect(original, b"MediaBox").unwrap_or(DEFAULT_MEDIA_BOX);
        let crop_box = self
            .inherited_rect(original, b"CropBox")
            .and_then(|crop| crop.intersect(&media_box))
            .unwrap_or(media_box);
        let rotation = self
            .inherited(original, b"Rotate")
            .and_then(|r| r.as_i64().ok())
            .map_or(0, normalize_rotation);

        let (resources, name) = self.resources_with_watermark(original);
        let draw = self.draw_operations(crop_box, rotation, &name);

        let mut new_contents = Vec::with_capacity(contents.len() + 2);
        match self.layer {
            Layer::Foreground if contents.is_empty() => new_contents.push(self.shared_stream(draw).into()),
            Layer::Foreground => {
                // q … Q autour du contenu d'origine : son état graphique (repère, couleurs)
                // ne doit pas déformer le filigrane dessiné ensuite.
                new_contents.push(self.save_state_stream().into());
                new_contents.extend(contents);
                let restore_and_draw = [b"Q\n".as_slice(), &draw].concat();
                new_contents.push(self.shared_stream(restore_and_draw).into());
            }
            Layer::Background => {
                new_contents.push(self.shared_stream(draw).into());
                new_contents.extend(contents);
            }
        }

        let mut page = original.clone();
        page.set("Contents", Object::Array(new_contents));
        page.set("Resources", resources);
        self.objects.push((page_id, Object::Dictionary(page)));
        true
    }

    /// Références des flux de contenu de la page, ou `None` si leur forme est invalide.
    fn content_references(&self, page: &Dictionary) -> Option<Vec<Object>> {
        let references = |items: &[Object]| -> Option<Vec<Object>> {
            items
                .iter()
                .map(|item| matches!(item, Object::Reference(_)).then(|| item.clone()))
                .collect()
        };
        match page.get(b"Contents") {
            Err(_) | Ok(Object::Null) => Some(Vec::new()),
            Ok(Object::Reference(id)) => match self.doc.get_object(*id) {
                Ok(Object::Array(items)) => references(items),
                _ => Some(vec![Object::Reference(*id)]),
            },
            Ok(Object::Array(items)) => references(items),
            Ok(_) => None,
        }
    }

    /// Valeur résolue d'un attribut héritable (§7.7.3.4).
    fn inherited(&self, page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
        self.inherited_raw(page, key).map(|value| self.resolve(value))
    }

    /// Valeur brute (éventuellement une référence) d'un attribut héritable, en
    /// remontant l'arbre des pages.
    fn inherited_raw(&self, page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
        let mut node = page;
        for _ in 0..MAX_TREE_DEPTH {
            if let Ok(value) = node.get(key) {
                return Some(value);
            }
            let parent = node.get(b"Parent").ok()?.as_reference().ok()?;
            node = self.doc.get_dictionary(parent).ok()?;
        }
        None
    }

    fn resolve(&self, object: &'a Object) -> &'a Object {
        match object {
            Object::Reference(id) => self.doc.get_object(*id).unwrap_or(&Object::Null),
            other => other,
        }
    }

    fn inherited_rect(&self, page: &'a Dictionary, key: &[u8]) -> Option<Rect> {
        let array = self.inherited(page, key)?.as_array().ok()?;
        let values: Vec<f64> = array
            .iter()
            .map(|v| self.resolve(v).as_float().ok().map(f64::from).filter(|v| v.is_finite()))
            .collect::<Option<_>>()?;
        let [xa, ya, xb, yb] = values[..] else {
            return None;
        };
        let rect = Rect::new(xa, ya, xb, yb);
        (rect.width() > 0.0 && rect.height() > 0.0).then_some(rect)
    }

    /// Ressources de la page complétées par le filigrane, et le nom sous lequel il y figure.
    /// Des ressources partagées par plusieurs pages ne sont copiées qu'une fois.
    fn resources_with_watermark(&mut self, page: &'a Dictionary) -> (Object, String) {
        let raw = self.inherited_raw(page, b"Resources");
        match raw {
            Some(Object::Reference(id)) => {
                if let Some((copy_id, name)) = self.shared_resources.get(id) {
                    return (Object::Reference(*copy_id), name.clone());
                }
                let original = self.doc.get_dictionary(*id).ok();
                let (resources, name) = self.add_watermark_to(original);
                let copy_id = self.ids.next();
                self.objects.push((copy_id, Object::Dictionary(resources)));
                self.shared_resources.insert(*id, (copy_id, name.clone()));
                (Object::Reference(copy_id), name)
            }
            Some(Object::Dictionary(dict)) => {
                let (resources, name) = self.add_watermark_to(Some(dict));
                (Object::Dictionary(resources), name)
            }
            _ => {
                let (resources, name) = self.add_watermark_to(None);
                (Object::Dictionary(resources), name)
            }
        }
    }

    fn add_watermark_to(&self, resources: Option<&Dictionary>) -> (Dictionary, String) {
        let mut resources = resources.cloned().unwrap_or_default();
        let mut xobjects = match resources.get(b"XObject") {
            Ok(Object::Reference(id)) => self.doc.get_dictionary(*id).cloned().unwrap_or_default(),
            Ok(Object::Dictionary(dict)) => dict.clone(),
            _ => Dictionary::new(),
        };
        let name = (0..)
            .map(|i| {
                if i == 0 {
                    XOBJECT_NAME.to_owned()
                } else {
                    format!("{XOBJECT_NAME}{i}")
                }
            })
            .find(|candidate| !xobjects.has(candidate.as_bytes()))
            .expect("un nom libre existe toujours");
        xobjects.set(name.as_bytes(), Object::Reference(self.xobject_id));
        resources.set("XObject", Object::Dictionary(xobjects));
        (resources, name)
    }

    /// Opérateurs qui dessinent le filigrane sur une page, marqués comme artefact
    /// de pagination pour que les lecteurs d'écran l'ignorent.
    fn draw_operations(&self, crop_box: Rect, rotation: i64, name: &str) -> Vec<u8> {
        let (display_width, display_height) = if rotation % 180 == 0 {
            (crop_box.width(), crop_box.height())
        } else {
            (crop_box.height(), crop_box.width())
        };
        let to_user = display_to_user(crop_box, rotation);
        let mut ops = String::from("/Artifact <</Type /Pagination /Subtype /Watermark>> BDC\nq\n");
        for placement in placements(
            display_width,
            display_height,
            self.block.width,
            self.block.height,
            &self.job.spec,
        ) {
            let matrix = placement.then(&to_user);
            ops.push_str(&format!("q {} cm /{name} Do Q\n", matrix.to_pdf_operands()));
        }
        ops.push_str("Q\nEMC\n");
        ops.into_bytes()
    }

    fn shared_stream(&mut self, content: Vec<u8>) -> ObjectId {
        if let Some(id) = self.draw_streams.get(&content) {
            return *id;
        }
        let id = self.ids.next();
        let mut stream = Stream::new(Dictionary::new(), content.clone());
        if content.len() > 512 {
            let _ = stream.compress();
        }
        self.objects.push((id, Object::Stream(stream)));
        self.draw_streams.insert(content, id);
        id
    }

    fn save_state_stream(&mut self) -> ObjectId {
        match self.save_state_id {
            Some(id) => id,
            None => {
                let id = self.shared_stream(b"q\n".to_vec());
                self.save_state_id = Some(id);
                id
            }
        }
    }
}

/// Objet graphique partagé : le texte, en em, centré sur l'origine.
fn watermark_xobject(block: &TextBlock, job: &Job, font: &font_embed::EmbeddedFont) -> Stream {
    let [r, g, b] = job.spec.color.unit().map(f64::from);
    let mut content = format!(
        "/G0 gs\n{} {} {} rg\nBT\n/F0 1 Tf\n",
        format_number(r),
        format_number(g),
        format_number(b)
    );
    for line in &block.lines {
        content.push_str(&format!(
            "1 0 0 1 {} {} Tm {} Tj\n",
            format_number(line.x),
            format_number(line.baseline),
            font.encode(line.glyphs.iter().map(|g| g.id))
        ));
    }
    content.push_str("ET\n");

    let margin = 0.3;
    let (half_width, half_height) = (block.width / 2.0 + margin, block.height / 2.0 + margin);
    let opacity = Object::Real(job.spec.opacity);
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![
                Object::Real(-half_width as f32), Object::Real(-half_height as f32),
                Object::Real(half_width as f32), Object::Real(half_height as f32),
            ],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F0" => font.font_id },
                "ExtGState" => dictionary! {
                    "G0" => dictionary! { "Type" => "ExtGState", "ca" => opacity.clone(), "CA" => opacity },
                },
            },
        },
        content.into_bytes(),
    )
}

/// Ramène `/Rotate` à 0, 90, 180 ou 270 (les autres valeurs sont invalides).
fn normalize_rotation(rotate: i64) -> i64 {
    let r = rotate.rem_euclid(360);
    if r % 90 == 0 { r } else { 0 }
}

/// Repère « page affichée » (origine en bas à gauche de la zone visible, rotation
/// appliquée) → espace utilisateur de la page. `/Rotate` tourne dans le sens horaire.
fn display_to_user(crop: Rect, rotation: i64) -> Affine {
    let (w, h) = (crop.width(), crop.height());
    match rotation {
        90 => Affine::new(0.0, 1.0, -1.0, 0.0, crop.x0 + w, crop.y0),
        180 => Affine::new(-1.0, 0.0, 0.0, -1.0, crop.x0 + w, crop.y0 + h),
        270 => Affine::new(0.0, -1.0, 1.0, 0.0, crop.x0, crop.y0 + h),
        _ => Affine::translate(crop.x0, crop.y0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_is_normalized() {
        assert_eq!(normalize_rotation(-90), 270);
        assert_eq!(normalize_rotation(450), 90);
        assert_eq!(normalize_rotation(45), 0);
    }

    /// Les coins de la page affichée tombent sur ceux de la zone visible, dans
    /// l'ordre imposé par une rotation horaire.
    #[test]
    fn display_corners_map_to_crop_box_corners() {
        let crop = Rect::new(10.0, 20.0, 110.0, 220.0); // 100 × 200
        let cases = [
            // rotation, (coin bas-gauche affiché → utilisateur), (haut-droit affiché → utilisateur)
            (0, (0.0, 0.0), (10.0, 20.0), (100.0, 200.0), (110.0, 220.0)),
            (90, (0.0, 0.0), (110.0, 20.0), (200.0, 100.0), (10.0, 220.0)),
            (180, (0.0, 0.0), (110.0, 220.0), (100.0, 200.0), (10.0, 20.0)),
            (270, (0.0, 0.0), (10.0, 220.0), (200.0, 100.0), (110.0, 20.0)),
        ];
        for (rotation, bl, bl_user, tr, tr_user) in cases {
            let m = display_to_user(crop, rotation);
            assert_eq!(m.apply(bl.0, bl.1), bl_user, "rotation {rotation}");
            assert_eq!(m.apply(tr.0, tr.1), tr_user, "rotation {rotation}");
        }
    }
}
