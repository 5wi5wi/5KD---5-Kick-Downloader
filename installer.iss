#ifndef MyAppName
  #define MyAppName "5KD - 5wi's Kick Downloader"
#endif
#ifndef MyAppShortName
  #define MyAppShortName "5KD"
#endif
#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif
#ifndef MyAppPublisher
  #define MyAppPublisher "5wi"
#endif
#ifndef MySourceExe
  #define MySourceExe "target\release\5KD.exe"
#endif
#ifndef MyIconFile
  #define MyIconFile "target\app_icon.ico"
#endif
#ifndef MyOutputDir
  #define MyOutputDir "dist"
#endif
#ifndef MyOutputBaseFilename
  #define MyOutputBaseFilename "5KD-windows-x86_64-setup"
#endif
#ifndef MyArchMode
  #define MyArchMode "x64compatible"
#endif

[Setup]
AppId={{6BD1EEA2-34D4-4548-BE8D-F864B781FAF0}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppShortName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#MyOutputDir}
OutputBaseFilename={#MyOutputBaseFilename}
SetupIconFile={#MyIconFile}
UninstallDisplayIcon={app}\5KD.exe
UninstallDisplayName={#MyAppName}
ArchitecturesAllowed={#MyArchMode}
ArchitecturesInstallIn64BitMode={#MyArchMode}
Compression=lzma2/fast
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#MySourceExe}"; DestDir: "{app}"; DestName: "5KD.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\5KD.exe"
Name: "{autodesktop}\{#MyAppShortName}"; Filename: "{app}\5KD.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\5KD.exe"; Description: "{cm:LaunchProgram,{#StringChange(MyAppShortName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
