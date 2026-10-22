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
    if ($code -ne 0 -or @($output | Where-Object { $_ -match '^test result: ok\. 21 passed; 0 failed; 0 ignored;' }).Count -ne 1) {
        throw 'Product configuration state/activation/maintenance tests did not execute exactly 21 cases'
    }
    & cargo build --locked -p forme-execution --bin forme-hostd -p forme-cli --bin forme-hostctl -p forme-gateway --bin forme-webd --bin forme-gatewayd
    if ($LASTEXITCODE -ne 0) { throw 'Managed product binaries failed to build' }
    & node --test tools/tests/managed_configuration.test.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Managed configuration process test failed' }
    Write-Host 'Managed product configuration: PASS (durable state, service contract, actual Web/host/core process bridge)'
    Write-Host 'Native service-manager installation, live provider and IP01-IP24: NOT_RUN by this component gate'
}
finally { Pop-Location }
