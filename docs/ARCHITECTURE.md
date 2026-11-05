# Architecture

```
crates/
  filigrane-core/   bibliothèque : toute la logique, sans interface
  filigrane-gui/    application Windows (egui/eframe) → « Filigrane PDF.exe »
  filigrane-cli/    ligne de commande → filigrane-cli.exe
assets/             police DejaVu Sans (intégrée à l'exécutable), icône
packaging/windows/  archive portable et installeur Inno Setup
scripts/            téléchargement de PDFium, génération de l'icône
```

L'interface et la ligne de commande ne font que remplir un `Job` et appeler
`filigrane_core::apply`, qui renvoie un `Report` ou une `Error` dont le message est
directement affichable.

## Le cœur (`filigrane-core`)

| Module | Rôle |
|---|---|
| `spec` | Paramètres : `WatermarkSpec` (texte, taille, couleur, angle…), `Mode`, `Job`, validation |
| `pages` | Sélection de pages `1-3, 7, 10-` |
| `font` | Police intégrée, mise en forme du texte (lignes centrées), contour vectoriel des glyphes |
| `layout` | Placement commun aux deux modes : une transformation par occurrence du texte (centré ou mosaïque en quinconce), dans le repère de la page **affichée** |
| `overlay` | Mode superposé : mise à jour incrémentale du PDF |
| `flatten` | Mode incrusté : rendu PDFium, filigrane peint, réécriture en JPEG |
| `raster` | Rendu d'une page et dessin matriciel du filigrane (mode incrusté **et** aperçu) |
| `output` | Écriture atomique : fichier temporaire renommé seulement en cas de succès |
| `progress` | Avancement et annulation |

### Mode superposé : ne jamais réécrire le document

Un PDF peut être complété sans être réécrit (ISO 32000-1, §7.5.6) : on recopie le
fichier d'origine octet par octet, puis on ajoute à la fin les seuls objets modifiés et
une nouvelle table de références chaînée à l'ancienne par `/Prev`.

1. Le fichier est **projeté en mémoire** (`memmap2`) : le système ne charge que ce qui
   est lu.
2. lopdf l'analyse avec un filtre qui **jette le contenu des flux** (images, polices) :
   seuls les dictionnaires restent en mémoire.
3. Un unique objet graphique (« Form XObject ») contient le texte ; la police est un
   **sous-ensemble** de DejaVu Sans limité aux glyphes utilisés (quelques Ko), avec une
   table `ToUnicode` pour la recherche et le copier-coller.
4. Pour chaque page : nouvelles ressources (copiées une seule fois quand elles sont
   partagées) et nouveau tableau `/Contents` = `q` + contenu d'origine + `Q` + dessin du
   filigrane. Les flux de dessin identiques (pages de même format) sont partagés.
5. Le dessin est marqué `/Artifact /Watermark` pour les lecteurs d'écran.

Si la structure du fichier est abîmée (table de références incohérente, octets parasites
avant l'en-tête), le document est entièrement réécrit par lopdf, ce qui le répare.

Les rotations `/Rotate`, les `CropBox` décalées et les attributs hérités de l'arbre des
pages sont pris en compte : le filigrane apparaît comme prévu sur la page telle qu'elle
s'affiche.

### Mode incrusté : mémoire bornée

PDFium (le moteur PDF de Chrome) rend les pages par lots de 4 ; le dessin du filigrane
(tiny-skia) et la compression JPEG se font en parallèle ; les pages sont écrites dès
qu'elles sont prêtes, seuls leurs décalages restent en mémoire.

PDFium garde en mémoire tout objet lu tant que le document est ouvert : le document est
donc rouvert régulièrement, à une fréquence calculée pour qu'environ 192 Mo du fichier au
plus restent chargés. Une page est rendue à 40 mégapixels au maximum.

## Mesures

PC de développement (6 cœurs), binaires `--release`. « Mémoire » = pic de mémoire
privée du processus.

| Document | Mode | Durée | Mémoire | Sortie |
|---|---|---|---|---|
| 20 000 pages, 5,8 Mo | superposé | 3,0 s | 166 Mo | 9,8 Mo |
| 800 pages, **1,87 Go** (photos) | superposé | 6,5 s | 24 Mo | 1,87 Go |
| 800 pages, 1,87 Go (photos) | incrusté, 150 ppp | 59 s | 331 Mo | 1,37 Go |

Pour reproduire : `cargo run --release -p filigrane-core --example generate_pdf -- gros.pdf 800 2`.

## Tests

- **Unitaires** (dans chaque module) : géométrie, mise en page, police, sélection de
  pages, table de références, écriture atomique, conversions du formulaire.
- **Intégration** (`crates/filigrane-core/tests`) : des PDF sont fabriqués à la volée
  (table classique ou flux d'objets compressés, pages tournées ou recadrées, ressources
  héritées ou partagées, contenu qui ne restaure pas son état graphique, table de
  références faussée, fichier chiffré, 40 Mo de données…), filigranés, puis **rendus par
  PDFium** : on vérifie sur les pixels que le filigrane est présent, centré et orienté
  comme prévu.
- **Ligne de commande** (`crates/filigrane-cli/tests`) : exécution réelle du binaire.

Les tests ont besoin de PDFium : `scripts/fetch-pdfium.sh` (Linux) ou
`scripts/fetch-pdfium.ps1` (Windows) le place dans `.pdfium/`, que `.cargo/config.toml`
indique aux tests.
