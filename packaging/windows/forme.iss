; forme Windows installer input.
; The release pipeline must define AppVersion/AppId and sign the resulting EXE.
; This script installs a complete release but never embeds model keys.
#define ProductName "forme"
#ifndef ProductVersion
#define ProductVersion "0.0.1"
#endif
#define ProductPublisher "forme contributors"
#ifndef ReleaseRoot
#define ReleaseRoot "..\.."
#endif

[Setup]
AppId={{9F0D7A2B-5BF8-4A6F-9AC1-5A2D3F4E6B7C}
AppName={#ProductName}
AppVersion={#ProductVersion}
AppPublisher={#ProductPublisher}
DefaultDirName={autopf}\ForMe
DefaultGroupName=forme
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible
DisableProgramGroupPage=yes
DisableDirPage=yes
SetupMutex=Global\ForMeFirstInstall
OutputBaseFilename=forme-setup
UninstallDisplayIcon={app}\bin\forme-webd.exe
SetupLogging=yes
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Files]
Source: "{#ReleaseRoot}\bin\forme-hostd.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#ReleaseRoot}\bin\forme-webd.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#ReleaseRoot}\bin\forme-gatewayd.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#ReleaseRoot}\bin\forme-cli.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#ReleaseRoot}\bin\forme-hostctl.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#ReleaseRoot}\bin\forme-executord.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#ReleaseRoot}\share\forme\web\*"; DestDir: "{app}\share\forme\web"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#ReleaseRoot}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseRoot}\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseRoot}\bundle-inventory.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourcePath}\install-access.ps1"; Flags: dontcopy
Source: "{#SourcePath}\service-access.psm1"; Flags: dontcopy

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
Filename: "http://127.0.0.1:18765"; Description: "Open forme"; Flags: shellexec postinstall skipifsilent runasoriginaluser

[Code]
var
  InstallStagesCompleted: Boolean;

function GetCustomSetupExitCode(): Integer;
begin
  { A post-install exception can otherwise leave Setup's native result at zero.
    Only completion of ALL service/access steps is installation success. }
  if InstallStagesCompleted then Result := 0 else Result := 1;
end;

function AccessArguments(Mode: String): String;
begin
  Result := '-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' +
    ExpandConstant('{tmp}\install-access.ps1') + '" -Mode ' + Mode +
    ' -ProgramRoot "' + ExpandConstant('{app}') + '" -DataRoot "' +
    ExpandConstant('{commonappdata}\ForMe') + '" -DiagnosticPath "' +
    ExpandConstant('{tmp}\access-') + Mode + '.ini"';
end;

procedure LogAccessFailure(Mode: String);
var
  DiagnosticPath: String;
begin
  DiagnosticPath := ExpandConstant('{tmp}\access-') + Mode + '.ini';
  if not FileExists(DiagnosticPath) then Exit;
  { Parse only numeric metadata. Raw messages and extra INI keys never enter logs. }
  Log('forme access diagnostic ' + Mode +
    ': stage=' + IntToStr(StrToIntDef(GetIniString('access', 'stage', '0', DiagnosticPath), 0)) +
    ' source=' + IntToStr(StrToIntDef(GetIniString('access', 'source', '0', DiagnosticPath), 0)) +
    ' line=' + IntToStr(StrToIntDef(GetIniString('access', 'line', '0', DiagnosticPath), 0)) +
    ' hresult=' + IntToStr(StrToIntDef(GetIniString('access', 'hresult', '0', DiagnosticPath), 0)) +
    ' category=' + IntToStr(StrToIntDef(GetIniString('access', 'category', '0', DiagnosticPath), 0)));
end;

function RunChecked(ProgramPath: String; Arguments: String; Stage: String): Boolean;
var
  ExitCode: Integer;
begin
  Log('forme installer stage: ' + Stage);
  Result := Exec(ProgramPath, Arguments, '', SW_HIDE, ewWaitUntilTerminated, ExitCode);
  if Result then Log('forme installer child exit: ' + IntToStr(ExitCode))
  else Log('forme installer launch error: ' + IntToStr(ExitCode));
  Result := Result and (ExitCode = 0);
  if not Result then begin
    LogAccessFailure('Prepare');
    LogAccessFailure('Grant');
    LogAccessFailure('Verify');
    LogAccessFailure('Start');
  end;
end;

procedure RequireStep(ProgramPath: String; Arguments: String; Stage: String);
begin
  if not RunChecked(ProgramPath, Arguments, Stage) then
    RaiseException('forme installation stopped at ' + Stage +
      '. Retain the protected files for explicit administrator recovery.');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  ExtractTemporaryFile('install-access.ps1');
  ExtractTemporaryFile('service-access.psm1');
  { Protect empty roots BEFORE payload or secrets are written. Existing data
    and installed services require the separate lifecycle/recovery workflow. }
  if not RunChecked(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    AccessArguments('Prepare'), 'protect empty installation roots') then
    Result := 'Cannot prepare a fresh protected installation. Existing files and services were retained.';
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  PowerShellPath: String;
begin
  if CurStep <> ssPostInstall then Exit;
  PowerShellPath := ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe');
  RequireStep(ExpandConstant('{app}\bin\forme-hostctl.exe'),
    'prepare-installation --install-root "' + ExpandConstant('{app}') +
    '" --state-root "' + ExpandConstant('{commonappdata}\ForMe') +
    '" --config-root "' + ExpandConstant('{commonappdata}\ForMe\config') +
    '" --log-root "' + ExpandConstant('{commonappdata}\ForMe\logs') +
    '" --ipc-root "' + ExpandConstant('{commonappdata}\ForMe\ipc') +
    '" --target windows_x86_64 --bundle-inventory "' + ExpandConstant('{app}\bundle-inventory.json') + '"',
    'prepare installation');
  { Registration is typed in each daemon and never starts a service. }
  RequireStep(ExpandConstant('{app}\bin\forme-hostd.exe'),
    '--install-service --config "' + ExpandConstant('{commonappdata}\ForMe\config\host.json') + '"', 'register Host');
  RequireStep(ExpandConstant('{app}\bin\forme-webd.exe'),
    '--install-service --config "' + ExpandConstant('{commonappdata}\ForMe\config\web.json') + '"', 'register Web');
  RequireStep(ExpandConstant('{app}\bin\forme-gatewayd.exe'),
    '--install-service --config "' + ExpandConstant('{commonappdata}\ForMe\config\core\core.json') + '"', 'register Brain');
  RequireStep(PowerShellPath, AccessArguments('Grant'), 'grant fixed service access');
  RequireStep(PowerShellPath, AccessArguments('Verify'), 'verify installed access');
  RequireStep(PowerShellPath, AccessArguments('Start'), 'start Host and Web');
  { Brain is on-demand until owner claim and validated model configuration. }
  InstallStagesCompleted := True;
end;

function InitializeUninstall(): Boolean;
begin
  { Confirm scope before making the first change, and abort BEFORE program
    removal if any typed stop/unregister operation fails or is unknown. }
  Result := MsgBox('Remove forme services and program files? Your brain data, accounts, keys, backups and workspaces will be retained.',
    mbConfirmation, MB_YESNO) = IDYES;
  if not Result then Exit;
  Result := RunChecked(ExpandConstant('{app}\bin\forme-webd.exe'),
    '--uninstall-service --config "' + ExpandConstant('{commonappdata}\ForMe\config\web.json') + '"', 'remove Web service');
  if Result then
    Result := RunChecked(ExpandConstant('{app}\bin\forme-gatewayd.exe'),
      '--uninstall-service --config "' + ExpandConstant('{commonappdata}\ForMe\config\core\core.json') + '"', 'remove Brain service');
  if Result then
    Result := RunChecked(ExpandConstant('{app}\bin\forme-hostd.exe'),
      '--uninstall-service --config "' + ExpandConstant('{commonappdata}\ForMe\config\host.json') + '"', 'remove Host service');
  if not Result then
    MsgBox('Service removal did not complete. Program files and owner data were retained for explicit administrator recovery.', mbError, MB_OK);
end;
