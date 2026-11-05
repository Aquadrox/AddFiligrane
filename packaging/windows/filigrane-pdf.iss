; Installeur Windows (Inno Setup 6), sans droits administrateur.
; Ajoute « Ajouter un filigrane » au menu contextuel des fichiers PDF.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F3C2B1E-8E5A-4C1B-9B7E-2A4D5F6C7B81}
AppName=Filigrane PDF
AppVersion={#AppVersion}
AppPublisher=Florian Langlois
DefaultDirName={localappdata}\Programs\Filigrane PDF
DefaultGroupName=Filigrane PDF
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=FiligranePDF-{#AppVersion}-installeur
SetupIconFile=..\..\assets\icon\icon.ico
UninstallDisplayIcon={app}\Filigrane PDF.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "contextmenu"; Description: "Ajouter « Ajouter un filigrane » au clic droit sur les PDF"; GroupDescription: "Intégration :"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{group}\Filigrane PDF"; Filename: "{app}\Filigrane PDF.exe"
Name: "{group}\{cm:UninstallProgram,Filigrane PDF}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\Filigrane PDF"; Filename: "{app}\Filigrane PDF.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.pdf\shell\FiligranePDF"; ValueType: string; ValueName: ""; ValueData: "Ajouter un filigrane"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.pdf\shell\FiligranePDF"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\Filigrane PDF.exe"""; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.pdf\shell\FiligranePDF\command"; ValueType: string; ValueName: ""; ValueData: """{app}\Filigrane PDF.exe"" ""%1"""; Tasks: contextmenu

[Run]
Filename: "{app}\Filigrane PDF.exe"; Description: "{cm:LaunchProgram,Filigrane PDF}"; Flags: nowait postinstall skipifsilent
