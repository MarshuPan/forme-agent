# Disposable real-process evidence; never installs services or loads owner data.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'build-resources.ps1')
Push-Location $repoRoot
try {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        Assert-FormeBuildSpace $repoRoot
        $output = @(& cargo test --locked -p forme-execution --lib installed::product_config 2>&1 | ForEach-Object { "$_" })
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $previous }
    $output | ForEach-Object { Write-Host $_ }
    if ($code -ne 0 -or @($output | Where-Object { $_ -match '^test result: ok\. 80 passed; 0 failed; 0 ignored;' }).Count -ne 1) {
        throw 'Product configuration state/activation/maintenance/skills/network/settings/worker/startup tests did not execute exactly 80 cases'
    }
    Assert-FormeBuildSpace $repoRoot
    & cargo test --locked -p forme-execution --test installed_network_fence
    if ($LASTEXITCODE -ne 0) { throw 'Durable network transition admission failed' }
    Assert-FormeBuildSpace $repoRoot
    & cargo test --locked -p forme-harness --test installed_brain_preferences
    if ($LASTEXITCODE -ne 0) { throw 'Managed brain preference runtime evidence failed' }
    Assert-FormeBuildSpace $repoRoot
    & cargo test --locked -p forme-harness --test installed_managed_skills
    if ($LASTEXITCODE -ne 0) { throw 'Managed skill runtime evidence failed' }
    Assert-FormeBuildSpace $repoRoot
    & cargo build --locked -p forme-execution --bin forme-hostd --bin forme-executord -p forme-cli --bin forme-hostctl -p forme-gateway --bin forme-webd --bin forme-gatewayd
    if ($LASTEXITCODE -ne 0) { throw 'Managed product binaries failed to build' }
    # These fixtures deliberately use --development-config and disposable
    # paths. Do not skip real process coverage just because SCM is uninstalled.
    & node --test --test-concurrency=1 tools/tests/managed_configuration.test.mjs tools/tests/managed_network.test.mjs tools/tests/local_authority.test.mjs tools/tests/web_multi_process.test.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Managed configuration process test failed' }
    Write-Host 'Managed product configuration: PASS (durable state, service contract, actual Web/host/core process bridge)'
    Write-Host 'Native service-manager installation, live provider and IP01-IP24: NOT_RUN by this component gate'
}
finally { Pop-Location }
