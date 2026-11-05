# Assemble dist\ : archive portable (exe + pdfium.dll + licences) et, avec -Installer,
# l'installeur Inno Setup. À lancer après `cargo build --release --workspace`.
param([switch]$Installer)
$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = Join-Path $root "dist"
$stage = Join-Path $dist "FiligranePDF"
Remove-Item -Recurse -Force $dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $stage "licences") | Out-Null

Copy-Item (Join-Path $root "target\release\filigrane-pdf.exe") (Join-Path $stage "Filigrane PDF.exe")
Copy-Item (Join-Path $root "target\release\filigrane-cli.exe") $stage
Copy-Item (Join-Path $root ".pdfium\bin\pdfium.dll") $stage
Copy-Item (Join-Path $root "README.md") $stage
Copy-Item (Join-Path $root "LICENSE") (Join-Path $stage "licences\LICENSE-Filigrane-PDF.txt")
Copy-Item (Join-Path $root "assets\fonts\LICENSE-DejaVu.txt") (Join-Path $stage "licences")
Copy-Item (Join-Path $root ".pdfium\LICENSE") (Join-Path $stage "licences\LICENSE-PDFium.txt")

$zip = Join-Path $dist "FiligranePDF-$version-windows-x64.zip"
Compress-Archive -Path $stage -DestinationPath $zip
Write-Host "Archive : $zip"

if ($Installer) {
    $iscc = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if (-not $iscc) {
        choco install innosetup --no-progress -y | Out-Null
        $iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    } else {
        $iscc = $iscc.Source
    }
    & $iscc "/DAppVersion=$version" "/DSourceDir=$stage" "/DOutputDir=$dist" (Join-Path $PSScriptRoot "filigrane-pdf.iss")
    if ($LASTEXITCODE -ne 0) { throw "Échec d'Inno Setup" }
}
Remove-Item -Recurse -Force $stage
