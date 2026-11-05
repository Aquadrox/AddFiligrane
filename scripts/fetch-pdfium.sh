#!/usr/bin/env bash
# Télécharge la bibliothèque PDFium (bblanchon/pdfium-binaries) dans .pdfium/.
# La version doit rester alignée avec la fonctionnalité `pdfium_*` de pdfium-render.
set -euo pipefail

PDFIUM_VERSION="${PDFIUM_VERSION:-8125}"
PLATFORM="${1:-linux-x64}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/.pdfium"

mkdir -p "$DEST"
URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${PDFIUM_VERSION}/pdfium-${PLATFORM}.tgz"
echo "Téléchargement de $URL"
curl -fsSL "$URL" | tar -xz -C "$DEST"
echo "PDFium installé dans $DEST"
