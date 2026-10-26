# Disposable real-process evidence; never installs services or loads owner data.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repoRoot
try {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = @(& cargo test --locked -p forme-execution --lib installed::product_config 2>&1 | ForEach-Object { "$_" })
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $previous }
    $output | ForEach-Object { Write-Host $_ }
    if ($code -ne 0 -or @($output | Where-Object { $_ -match '^test result: ok\. 65 passed; 0 failed; 0 ignored;' }).Count -ne 1) {
        throw 'Product configuration state/activation/maintenance/skills/network/settings tests did not execute exactly 65 cases'
    }
    & cargo test --locked -p forme-execution --test installed_network_fence
    if ($LASTEXITCODE -ne 0) { throw 'Durable network transition admission failed' }
    & cargo test --locked -p forme-harness --test installed_brain_preferences
    if ($LASTEXITCODE -ne 0) { throw 'Managed brain preference runtime evidence failed' }
    & cargo test --locked -p forme-harness --test installed_managed_skills
    if ($LASTEXITCODE -ne 0) { throw 'Managed skill runtime evidence failed' }
    & cargo build --locked -p forme-execution --bin forme-hostd -p forme-cli --bin forme-hostctl -p forme-gateway --bin forme-webd --bin forme-gatewayd
    if ($LASTEXITCODE -ne 0) { throw 'Managed product binaries failed to build' }
    $managedHostReady = $true
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        try {
            $account = New-Object System.Security.Principal.NTAccount('NT SERVICE\forme-brain')
            [void]$account.Translate([System.Security.Principal.SecurityIdentifier])
        } catch { $managedHostReady = $false }
    }
    if (-not $managedHostReady) {
        Write-Host 'Managed product configuration components: PASS (durable state, service contract, runtime binaries)'
        Write-Host 'Managed Host/Web/core process bridge: NOT_RUN (NT SERVICE\forme-brain is not installed on this machine)'
        Write-Host 'Native service-manager installation, live provider and IP01-IP24: NOT_RUN'
        return
    }
    & node --test tools/tests/managed_configuration.test.mjs tools/tests/managed_network.test.mjs tools/tests/local_authority.test.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Managed configuration process test failed' }
    Write-Host 'Managed product configuration: PASS (durable state, service contract, actual Web/host/core process bridge)'
    Write-Host 'Native service-manager installation, live provider and IP01-IP24: NOT_RUN by this component gate'
}
finally { Pop-Location }
