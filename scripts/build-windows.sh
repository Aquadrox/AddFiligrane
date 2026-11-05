#!/usr/bin/env bash
# Compile l'application Windows depuis Linux (MinGW-w64) et assemble l'archive portable
# dans dist/. Prérequis : `rustup target add x86_64-pc-windows-gnu` et le paquet
# gcc-mingw-w64-x86-64 (Debian/Ubuntu).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=x86_64-pc-windows-gnu
PDFIUM_VERSION="${PDFIUM_VERSION:-8125}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
PDFIUM_DIR="$ROOT/.pdfium-win"

cd "$ROOT"
cargo build --release --workspace --target "$TARGET"

if [ ! -f "$PDFIUM_DIR/bin/pdfium.dll" ]; then
  mkdir -p "$PDFIUM_DIR"
  curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${PDFIUM_VERSION}/pdfium-win-x64.tgz" \
    | tar -xz -C "$PDFIUM_DIR"
fi

STAGE="$ROOT/dist/FiligranePDF"
rm -rf "$STAGE" && mkdir -p "$STAGE/licences"
cp "target/$TARGET/release/filigrane-pdf.exe" "$STAGE/Filigrane PDF.exe"
cp "target/$TARGET/release/filigrane-cli.exe" "$STAGE/"
cp "$PDFIUM_DIR/bin/pdfium.dll" "$STAGE/"
cp README.md "$STAGE/"
cp LICENSE "$STAGE/licences/LICENSE-Filigrane-PDF.txt"
cp assets/fonts/LICENSE-DejaVu.txt "$STAGE/licences/"
cp "$PDFIUM_DIR/LICENSE" "$STAGE/licences/LICENSE-PDFium.txt"

ARCHIVE="$ROOT/dist/FiligranePDF-$VERSION-windows-x64.zip"
rm -f "$ARCHIVE"
(cd "$ROOT/dist" && python3 -m zipfile -c "$ARCHIVE" FiligranePDF)
rm -rf "$STAGE"
echo "Archive : $ARCHIVE"
