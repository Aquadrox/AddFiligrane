# Télécharge la bibliothèque PDFium (bblanchon/pdfium-binaries) dans .pdfium\.
# La version doit rester alignée avec la fonctionnalité `pdfium_*` de pdfium-render.
param(
    [string]$Version = $(if ($env:PDFIUM_VERSION) { $env:PDFIUM_VERSION } else { "8125" }),
    [string]$Platform = "win-x64"
)
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root ".pdfium"
New-Item -ItemType Directory -Force -Path $dest | Out-Null

$url = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$Version/pdfium-$Platform.tgz"
$archive = Join-Path $env:TEMP "pdfium-$Platform.tgz"
Write-Host "Téléchargement de $url"
Invoke-WebRequest -Uri $url -OutFile $archive
tar -xzf $archive -C $dest
Remove-Item $archive
Write-Host "PDFium installé dans $dest"
