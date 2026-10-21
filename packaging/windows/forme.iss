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
DefaultDirName={autopf}\ForMe
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
Name: "{commonappdata}\ForMe\config"
Name: "{commonappdata}\ForMe\brain"
Name: "{commonappdata}\ForMe\web-auth"
Name: "{commonappdata}\ForMe\host-state"
Name: "{commonappdata}\ForMe\keys"
Name: "{commonappdata}\ForMe\logs"
Name: "{commonappdata}\ForMe\backups"
Name: "{commonappdata}\ForMe\workspaces"

[Run]
Filename: "{app}\bin\forme-hostctl.exe"; Parameters: "prepare-installation --install-root ""{app}"" --state-root ""{commonappdata}\ForMe"" --config-root ""{commonappdata}\ForMe\config"" --log-root ""{commonappdata}\ForMe\logs"" --ipc-root ""{commonappdata}\ForMe\ipc"" --target windows_x86_64 --bundle-inventory ""{app}\bundle-inventory.json"""; Flags: runhidden waituntilterminated; StatusMsg: "Preparing forme installation"
Filename: "{app}\bin\forme-hostd.exe"; Parameters: "--install-service --config ""{commonappdata}\ForMe\config\host.json"""; Flags: runhidden waituntilterminated; StatusMsg: "Registering forme host service"
Filename: "{app}\bin\forme-webd.exe"; Parameters: "--install-service --config ""{commonappdata}\ForMe\config\web.json"""; Flags: runhidden waituntilterminated; StatusMsg: "Registering forme web service"

[UninstallRun]
Filename: "{app}\bin\forme-webd.exe"; Parameters: "--uninstall-service --config ""{commonappdata}\ForMe\config\web.json"""; Flags: runhidden waituntilterminated
Filename: "{app}\bin\forme-gatewayd.exe"; Parameters: "--uninstall-service --config ""{commonappdata}\ForMe\config\core.json"""; Flags: runhidden waituntilterminated
Filename: "{app}\bin\forme-hostd.exe"; Parameters: "--uninstall-service --config ""{commonappdata}\ForMe\config\host.json"""; Flags: runhidden waituntilterminated

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  { Service registration remains typed in the host binaries. The installer
    never accepts an arbitrary executable or a model credential argument. }
  Result := '';
end;
