; Inno Setup script for the Windows installer.
; Built by the release workflow:
;   ISCC /DAppVersion=0.1.0 /DSourceDir=<folder with the exes and kinetto.ico> packaging\windows\kinetto.iss
; The installer lands in target\installer.

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif
#ifndef SourceDir
  #error Pass /DSourceDir=<folder with kinetto.exe, kinetto-cli.exe and kinetto.ico>
#endif

[Setup]
; Never change AppId: Windows uses it to find the installed copy to upgrade.
AppId={{447B6C2E-70B9-4557-93DD-DBCA4C1CB5E1}
AppName=Kinetto
AppVersion={#AppVersion}
AppVerName=Kinetto {#AppVersion}
AppPublisher=Kinetto contributors
AppPublisherURL=https://github.com/sudo-py-dev/kinetto
AppSupportURL=https://github.com/sudo-py-dev/kinetto/issues
DefaultDirName={autopf}\Kinetto
DefaultGroupName=Kinetto
DisableProgramGroupPage=yes
; Installs for one user without admin rights, or for everyone if they choose.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
SetupIconFile={#SourceDir}\kinetto.ico
UninstallDisplayIcon={app}\kinetto.ico
OutputDir=..\..\target\installer
OutputBaseFilename=kinetto-{#AppVersion}-windows-x86_64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\kinetto.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\kinetto-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\kinetto.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Kinetto"; Filename: "{app}\kinetto.exe"; IconFilename: "{app}\kinetto.ico"
Name: "{autodesktop}\Kinetto"; Filename: "{app}\kinetto.exe"; IconFilename: "{app}\kinetto.ico"; Tasks: desktopicon

[Registry]
; Double-clicking a .kinetto project opens it in Kinetto.
Root: HKA; Subkey: "Software\Classes\.kinetto\OpenWithProgids"; ValueType: string; ValueName: "Kinetto.Project"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Kinetto.Project"; ValueType: string; ValueName: ""; ValueData: "Kinetto project"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Kinetto.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\kinetto.ico"
Root: HKA; Subkey: "Software\Classes\Kinetto.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\kinetto.exe"" ""%1"""

[Run]
Filename: "{app}\kinetto.exe"; Description: "{cm:LaunchProgram,Kinetto}"; Flags: nowait postinstall skipifsilent
