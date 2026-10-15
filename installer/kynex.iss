; KYNEX's setup.exe (Inno Setup 6): built by tools/package.ps1, which gives
; the version and where the programs are:
;   ISCC /DVersion=0.1.0 /DBin=..\target\release /DOut=..\target\dist kynex.iss
;
; Installs for this user alone (no administrator) unless asked for all users.
; What KYNEX keeps — projects in Documents\KYNEX, settings in
; %APPDATA%\kynex — stays when it is removed.

#ifndef Version
  #define Version "0.1.0"
#endif
#ifndef Bin
  #define Bin "..\target\release"
#endif
#ifndef Out
  #define Out "..\target\dist"
#endif

[Setup]
AppId={{C0F0B42D-6E71-449D-B34E-A61CD6182BDB}
AppName=KYNEX
AppVersion={#Version}
AppVerName=KYNEX {#Version}
AppPublisher=SMIC
DefaultDirName={autopf}\KYNEX
DefaultGroupName=KYNEX
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#Out}
OutputBaseFilename=KYNEX-{#Version}-setup
SetupIconFile=..\apps\kynex\assets\kynex.ico
UninstallDisplayIcon={app}\kynex-studio.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#Bin}\kynex-studio.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Bin}\kynex.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\KYNEX"; Filename: "{app}\kynex-studio.exe"
Name: "{autodesktop}\KYNEX"; Filename: "{app}\kynex-studio.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\kynex-studio.exe"; Description: "{cm:LaunchProgram,KYNEX}"; Flags: nowait postinstall skipifsilent
