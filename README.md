# Filigrane PDF

Ajoute un filigrane texte (« Pour mandat de vente – Agence Dupont – 05/11/2026 ») à un
document PDF, même très long ou très lourd. Application Windows, écrite en Rust.

- **Simple** : choisir ou glisser-déposer un PDF, taper le texte, cliquer. L'aperçu montre
  la vraie page avec le filigrane, en direct.
- **Deux protections au choix**
  - **Incrusté** (par défaut) : le filigrane est fondu dans l'image de chaque page, comme
    sur DossierFacile. Impossible à retirer : recommandé pour les pièces d'identité et
    justificatifs. Le texte d'origine n'est plus sélectionnable.
  - **Superposé** : le filigrane est ajouté par-dessus les pages sans réécrire le
    document. Quelques secondes pour 20 000 pages ou 2 Go, fichier presque inchangé,
    texte toujours sélectionnable — mais un éditeur PDF peut retirer le filigrane.
- **Réglages** : mosaïque ou texte centré, densité, taille, inclinaison (ou diagonale),
  couleur, opacité, gras, pages concernées (`1-3, 7, 10-`). Ils sont mémorisés.
- **Sûr** : le fichier d'origine n'est jamais modifié ; en cas d'erreur ou d'annulation,
  aucun fichier partiel n'est laissé.

## Installation (Windows 10/11, 64 bits)

Télécharger depuis les *Releases* :

- `FiligranePDF-x.y.z-installeur.exe` : installation sans droits administrateur, menu
  Démarrer, et option « Ajouter un filigrane » au clic droit sur un PDF ;
- ou `FiligranePDF-x.y.z-windows-x64.zip` : version portable, à décompresser
  (garder `pdfium.dll` à côté de `Filigrane PDF.exe`).

## Ligne de commande

```powershell
filigrane-cli.exe avis.pdf -t "Pour dossier de location – 05/11/2026"
filigrane-cli.exe gros.pdf -o sortie.pdf -t "CONFIDENTIEL" --mode superpose --layout centre --angle diagonale --bold
filigrane-cli.exe --help
```

## Développement

Prérequis : Rust stable (1.88 ou plus récent).

```powershell
./scripts/fetch-pdfium.ps1        # Linux : ./scripts/fetch-pdfium.sh
cargo test --workspace
cargo run -p filigrane-gui        # l'application
cargo build --release --workspace # puis ./packaging/windows/package.ps1 [-Installer]
```

La CI GitHub Actions (`.github/workflows`) vérifie le format, clippy et les tests sous
Linux et Windows, et produit l'archive portable ; une étiquette `vX.Y.Z` publie l'archive
et l'installeur.

Architecture, choix techniques et mesures de performance : [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Licences

Code sous licence MIT. Police DejaVu Sans (licence Bitstream Vera,
`assets/fonts/LICENSE-DejaVu.txt`). PDFium (BSD), distribué à côté de l'exécutable.
