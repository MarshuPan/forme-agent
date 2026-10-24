# Elevated installer-only entry. No arbitrary names, accounts, paths or commands.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Prepare', 'Grant', 'Verify', 'Start')][string]$Mode,
    [Parameter(Mandatory)][string]$ProgramRoot,
    [Parameter(Mandatory)][string]$DataRoot
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'service-access.psm1') -Force

try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows is required' }
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Native installation requires the administrator installer'
    }
    Assert-ForMeInstallRoots $ProgramRoot $DataRoot
    # The updater is an on-demand maintenance service, not a fourth business daemon.
    # A leftover registration must also block fresh installation.
    $names = @('forme-host', 'forme-web', 'forme-brain', 'forme-release-updater')
    $services = @(Get-Service -ErrorAction Stop | Where-Object { $_.Name -in $names })
    if ($Mode -eq 'Prepare') {
        if ($services.Count -ne 0) { throw 'Existing services require explicit lifecycle recovery' }
        # Check every root before changing either; never take over a nonempty root.
        foreach ($path in @($ProgramRoot, $DataRoot)) {
            if (Test-Path -LiteralPath $path) {
                $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
                if (-not $item.PSIsContainer -or @(Get-ChildItem -LiteralPath $path -Force).Count -ne 0) {
                    throw 'Existing installation or data must be preserved for explicit recovery'
                }
                Assert-ForMeSecurity $path (New-ForMeSecurity -Directory $true)
            }
        }
        foreach ($path in @($ProgramRoot, $DataRoot)) {
            if (-not (Test-Path -LiteralPath $path)) {
                New-ForMeProtectedDirectory $path (New-ForMeSecurity -Directory $true)
            }
        }
    } else {
        if ($services.Count -ne 4 -or @($services | Where-Object Status -NE 'Stopped').Count -ne 0) {
            throw 'All business services and the maintenance updater must be registered and stopped before first-install access checks'
        }
        $web = [Security.Principal.NTAccount]::new('NT SERVICE\forme-web').Translate([Security.Principal.SecurityIdentifier]).Value
        $brain = [Security.Principal.NTAccount]::new('NT SERVICE\forme-brain').Translate([Security.Principal.SecurityIdentifier]).Value
        $plan = @(Get-ForMeAccessPlan $ProgramRoot $DataRoot $web $brain)
        Assert-ForMePreparedLayout $ProgramRoot $DataRoot
        # The initial brain and host state stay empty; no existing data is re-ACL'd.
        $dataItems = @(Get-ForMeTree $DataRoot)
        foreach ($item in $dataItems) {
            $entry = @($plan | Where-Object Path -IEQ $item.FullName)
            if ($entry.Count -ne 1 -or $entry[0].Directory -ne $item.PSIsContainer) {
                throw 'Unexpected data exists; retain it for explicit recovery'
            }
        }
        foreach ($entry in $plan) {
            if (-not (Test-Path -LiteralPath $entry.Path)) {
                if ($Mode -ne 'Grant' -or -not $entry.Directory -or
                    [IO.Path]::GetDirectoryName($entry.Path) -ine (Join-Path $DataRoot 'logs')) {
                    throw 'Prepared installation inventory is incomplete'
                }
                New-ForMeProtectedDirectory $entry.Path (New-ForMeSecurity -Directory $true)
            }
        }
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
    # Do not include credential bodies, native command output or private paths.
    Write-Error 'forme installation stopped; keep the protected installation for administrator diagnosis and explicit recovery' -ErrorAction Continue
    exit 1
}
