# Elevated installer-only entry. No arbitrary names, accounts, paths or commands.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Prepare', 'Grant', 'Verify', 'Start')][string]$Mode,
    [Parameter(Mandatory)][string]$ProgramRoot,
    [Parameter(Mandatory)][string]$DataRoot,
    [string]$DiagnosticPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$stage = 0
$diagnosticAllowed = $false

try {
    if ($DiagnosticPath) {
        $expected = Join-Path $PSScriptRoot ("access-$Mode.ini")
        if ([IO.Path]::GetFullPath($DiagnosticPath) -ine $expected) { throw 'Invalid installation diagnostic target' }
        $diagnosticAllowed = $true
    }
    Import-Module (Join-Path $PSScriptRoot 'service-access.psm1') -Force
    $stage = 1
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows is required' }
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Native installation requires the administrator installer'
    }
    $stage = 2
    Assert-ForMeInstallRoots $ProgramRoot $DataRoot
    # The updater is an on-demand maintenance service, not a fourth business daemon.
    # A leftover registration must also block fresh installation.
    $names = @('forme-host', 'forme-web', 'forme-brain', 'forme-release-updater')
    $stage = 3
    $services = @(Get-Service -ErrorAction Stop | Where-Object { $_.Name -in $names })
    if ($Mode -eq 'Prepare') {
        $stage = 4
        if ($services.Count -ne 0) { throw 'Existing services require explicit lifecycle recovery' }
        # Check every root before changing either; never take over a nonempty root.
        foreach ($path in @($ProgramRoot, $DataRoot)) {
            $stage = if ($path -eq $ProgramRoot) { 5 } else { 6 }
            if (Test-Path -LiteralPath $path) {
                $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
                if (-not $item.PSIsContainer -or @(Get-ChildItem -LiteralPath $path -Force).Count -ne 0) {
                    throw 'Existing installation or data must be preserved for explicit recovery'
                }
                Assert-ForMeSecurity $path (New-ForMeSecurity -Directory $true)
            }
        }
        foreach ($path in @($ProgramRoot, $DataRoot)) {
            $stage = if ($path -eq $ProgramRoot) { 7 } else { 8 }
            if (-not (Test-Path -LiteralPath $path)) {
                New-ForMeProtectedDirectory $path (New-ForMeSecurity -Directory $true)
            }
        }
    } else {
        $stage = 9
        if ($services.Count -ne 4 -or @($services | Where-Object Status -NE 'Stopped').Count -ne 0) {
            throw 'All business services and the maintenance updater must be registered and stopped before first-install access checks'
        }
        $stage = 10
        $web = [Security.Principal.NTAccount]::new('NT SERVICE\forme-web').Translate([Security.Principal.SecurityIdentifier]).Value
        $brain = [Security.Principal.NTAccount]::new('NT SERVICE\forme-brain').Translate([Security.Principal.SecurityIdentifier]).Value
        $plan = @(Get-ForMeAccessPlan $ProgramRoot $DataRoot $web $brain)
        $stage = 11
        Assert-ForMePreparedLayout $ProgramRoot $DataRoot
        # The initial brain and host state stay empty; no existing data is re-ACL'd.
        Assert-ForMePreparedInventory $DataRoot $plan
        foreach ($entry in $plan) {
            if (-not (Test-Path -LiteralPath $entry.Path)) {
                if ($Mode -ne 'Grant' -or -not $entry.Directory -or
                    [IO.Path]::GetDirectoryName($entry.Path) -ine (Join-Path $DataRoot 'logs')) {
                    throw 'Prepared installation inventory is incomplete'
                }
                New-ForMeProtectedDirectory $entry.Path (New-ForMeSecurity -Directory $true)
            }
        }
        $stage = 12
        $items = @((Get-ForMeTree $ProgramRoot)) + @((Get-ForMeTree $DataRoot))
        foreach ($item in $items) {
            $security = Get-ForMePlannedSecurity $item $plan
            if ($Mode -eq 'Grant') {
                Assert-ForMePath $item.FullName
                Set-ForMeSecurity $item.FullName $security
            }
            Assert-ForMeSecurity $item.FullName $security
        }
        if ($Mode -eq 'Start') {
            $stage = 13
            foreach ($name in @('forme-host', 'forme-web')) {
                $service = [ServiceProcess.ServiceController]::new($name)
                try {
                    $service.Start()
                    $service.WaitForStatus([ServiceProcess.ServiceControllerStatus]::Running, [TimeSpan]::FromSeconds(30))
                    $service.Refresh()
                    if ($service.Status -ne 'Running') { throw 'Service did not reach its observed OS state' }
                } finally { $service.Dispose() }
            }
            # OS Running is not runtime readiness or model/provider acceptance.
        }
    }
    Write-Output "forme first-install access: $Mode completed"
    exit 0
} catch {
    $failure = $_
    if ($diagnosticAllowed) {
        # Fixed sibling in Inno's protected extraction directory. Never log the
        # exception message, arguments, stack, environment or an arbitrary path.
        try {
            $fileId = switch ([IO.Path]::GetFileName($failure.InvocationInfo.ScriptName)) {
                'install-access.ps1' { 1 }
                'service-access.psm1' { 2 }
                default { 0 }
            }
            $fields = @('[access]', "stage=$stage", "source=$fileId",
                ('line=' + [int]$failure.InvocationInfo.ScriptLineNumber),
                ('hresult=' + [int]$failure.Exception.HResult),
                ('category=' + [int]$failure.CategoryInfo.Category))
            $bytes = [Text.Encoding]::ASCII.GetBytes(($fields -join "`r`n"))
            $stream = [IO.File]::Open($DiagnosticPath, [IO.FileMode]::CreateNew,
                [IO.FileAccess]::Write, [IO.FileShare]::Read)
            try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
            finally { $stream.Dispose() }
        } catch { } # Diagnostic failure cannot overwrite the original refusal.
    }
    # Do not include credential bodies, native command output or private paths.
    Write-Error 'forme installation stopped; keep the protected installation for administrator diagnosis and explicit recovery' -ErrorAction Continue
    exit 1
}
