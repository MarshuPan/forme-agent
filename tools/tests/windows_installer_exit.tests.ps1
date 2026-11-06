# Native Inno fixture: exercise the real exit/status helpers without installing
# ForMe, changing ACLs, registering services, or requesting administrator rights.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Toolchain)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$config = Get-Content -LiteralPath $Toolchain -Raw | ConvertFrom-Json
$compiler = @($config.tools | Where-Object id -CEQ 'iscc')
if ($compiler.Count -ne 1 -or $compiler[0].sha256 -cne
    'f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc' -or
    (Get-FileHash -LiteralPath $compiler[0].path -Algorithm SHA256).Hash -ine $compiler[0].sha256) {
    throw 'This native regression requires the reviewed Inno 6.6.1 compiler'
}
$template = [IO.File]::ReadAllText((Join-Path $repo 'packaging\windows\forme.iss'))
$helpers = foreach ($name in @('LogAccessFailure', 'GetCustomSetupExitCode', 'RunChecked', 'RequireStep')) {
    $match = [regex]::Match($template, '(?ms)^(?:function|procedure) ' + $name + '\([^\r\n]*\).*?^end;')
    if (-not $match.Success) { throw "Installer helper is missing: $name" }
    $match.Value
}
$tempRoot = [IO.Path]::GetTempPath().TrimEnd('\')
$scratch = Join-Path $tempRoot ('forme-installer-exit-' + [Guid]::NewGuid().ToString('N'))
$null = [IO.Directory]::CreateDirectory($scratch)
try {
    foreach ($case in @('success', 'failure')) {
        $childExit = if ($case -eq 'failure') { 7 } else { 0 }
        $fixture = @"
[Setup]
AppName=ForMe exit regression fixture
AppVersion=0.0.1
CreateAppDir=no
Uninstallable=no
PrivilegesRequired=lowest
OutputDir=$scratch
OutputBaseFilename=fixture-$case
Compression=none
CloseApplications=no
RestartApplications=no
[Code]
var
  InstallStagesCompleted: Boolean;
$($helpers -join "`r`n")
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep <> ssPostInstall then Exit;
  SaveStringToFile(ExpandConstant('{tmp}\access-Prepare.ini'),
    '[access]' + #13#10 + 'stage=17' + #13#10 + 'source=2' + #13#10 +
    'line=42' + #13#10 + 'hresult=-1' + #13#10 + 'category=not-a-number' + #13#10 +
    'message=FORME_PRIVATE_DIAGNOSTIC_CANARY', False);
  RequireStep(ExpandConstant('{sys}\cmd.exe'), '/D /C exit $childExit', 'controlled fixture step');
  InstallStagesCompleted := True;
end;
"@
        $source = Join-Path $scratch "fixture-$case.iss"
        [IO.File]::WriteAllText($source, $fixture, [Text.UTF8Encoding]::new($false))
        $compileOutput = @(& $compiler[0].path '/Q' $source 2>&1)
        if ($LASTEXITCODE -ne 0) { throw ('Native fixture compilation failed: ' + ($compileOutput -join ' ')) }
        $executable = Join-Path $scratch "fixture-$case.exe"
        $process = Start-Process -FilePath $executable -WindowStyle Hidden -PassThru -ArgumentList @(
            '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/LOG=`"$scratch\$case.log`"")
        try {
            if (-not $process.WaitForExit(30000)) { throw 'Native fixture did not exit within its bound' }
            $code = $process.ExitCode
        } finally { $process.Dispose() }
        $log = [IO.File]::ReadAllText((Join-Path $scratch "$case.log"))
        if (-not $log.Contains('forme installer stage: controlled fixture step')) {
            throw 'The fixture did not execute the real installer helper'
        }
        if (($case -eq 'success' -and $code -ne 0) -or ($case -eq 'failure' -and $code -eq 0)) {
            throw "Incorrect native result for $case fixture: $code"
        }
        if ($case -eq 'failure' -and -not $log.Contains('forme installation stopped at controlled fixture step')) {
            throw 'Failure did not originate from the real post-install step'
        }
        if ($log.Contains('FORME_PRIVATE_DIAGNOSTIC_CANARY') -or $log.Contains('not-a-number')) {
            throw 'Native error log exposed an untrusted diagnostic value'
        }
        if ($case -eq 'failure' -and -not $log.Contains('forme access diagnostic Prepare: stage=17 source=2 line=42 hresult=-1 category=0')) {
            throw 'Native failure did not preserve bounded numeric diagnostic metadata'
        }
        Write-Host "[PASS] native Inno $case fixture: exit $code"
    }
    Write-Host 'Native installer exit regression: 2/2 PASS; no ForMe services or installed data changed'
} finally {
    if ([IO.Path]::GetDirectoryName($scratch) -ine $tempRoot -or
        -not [IO.Path]::GetFileName($scratch).StartsWith('forme-installer-exit-') -or
        ((Get-Item -LiteralPath $scratch -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Unsafe native fixture cleanup root'
    }
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
