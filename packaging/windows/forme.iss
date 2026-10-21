; forme Windows installer input.
; The release pipeline must define AppVersion/AppId and sign the resulting EXE.
; This script installs a complete release but never embeds model keys.
#define ProductName "forme"
#define ProductVersion "0.0.1"
#define ProductPublisher "forme contributors"
#define ReleaseRoot "..\.."

[Setup]
AppId={{9F0D7A2B-5BF8-4A6F-9AC1-5A2D3F4E6B7C}
AppName={#ProductName}
AppVersion={#ProductVersion}
AppPublisher={#ProductPublisher}
DefaultDirName={autopf}\forme
DefaultGroupName=forme
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
OutputBaseFilename=forme-setup
UninstallDisplayIcon={app}\bin\forme-webd.exe
SetupLogging=yes
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Files]
Source: "{#ReleaseRoot}\bin\*.exe"; DestDir: "{app}\bin"; Flags: ignoreversion recursesubdirs
Source: "{#ReleaseRoot}\share\forme\web\*"; DestDir: "{app}\share\forme\web"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#ReleaseRoot}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseRoot}\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseRoot}\bundle-inventory.json"; DestDir: "{app}"; Flags: ignoreversion

[Dirs]
Name: "{commonappdata}\forme\config"
Name: "{commonappdata}\forme\brain"
Name: "{commonappdata}\forme\web-auth"
Name: "{commonappdata}\forme\host-state"
Name: "{commonappdata}\forme\keys"
Name: "{commonappdata}\forme\logs"
Name: "{commonappdata}\forme\backups"
Name: "{commonappdata}\forme\workspaces"

[Run]
Filename: "{app}\bin\forme-hostd.exe"; Parameters: "--install-service --config ""{commonappdata}\forme\config\host.json"""; Flags: runhidden waituntilterminated; StatusMsg: "Registering forme host service"
Filename: "{app}\bin\forme-webd.exe"; Parameters: "--install-service --config ""{commonappdata}\forme\config\web.json"""; Flags: runhidden waituntilterminated; StatusMsg: "Registering forme web service"
Filename: "{app}\bin\forme-gatewayd.exe"; Parameters: "--install-service --config ""{commonappdata}\forme\config\core.json"""; Flags: runhidden waituntilterminated; StatusMsg: "Registering forme brain service"

[UninstallRun]
Filename: "{app}\bin\forme-webd.exe"; Parameters: "--uninstall-service --config ""{commonappdata}\forme\config\web.json"""; Flags: runhidden waituntilterminated
Filename: "{app}\bin\forme-gatewayd.exe"; Parameters: "--uninstall-service --config ""{commonappdata}\forme\config\core.json"""; Flags: runhidden waituntilterminated
Filename: "{app}\bin\forme-hostd.exe"; Parameters: "--uninstall-service --config ""{commonappdata}\forme\config\host.json"""; Flags: runhidden waituntilterminated

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  { Service registration remains typed in the host binaries. The installer
    never accepts an arbitrary executable or a model credential argument. }
  Result := '';
end;
