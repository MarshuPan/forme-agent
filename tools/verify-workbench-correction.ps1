# Exact owner feedback through the production brain and disposable processes.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repoRoot
try {
    & cargo test --locked -p forme-store --test owner_correction_atomic
    if ($LASTEXITCODE -ne 0) { throw 'Atomic correction rollback/CAS evidence failed' }
    & cargo test --locked -p forme-harness --test installed_cognition_correction
    if ($LASTEXITCODE -ne 0) { throw 'Production correction and erasure evidence failed' }
    & cargo test --locked -p forme-gateway --lib server::lifecycle_tests::web_workbench::web_workbench_runs_through_the_brain_once_and_logout_fences_core_proofs -- --exact
    if ($LASTEXITCODE -ne 0) { throw 'Authenticated Web correction evidence failed' }
    & cargo build --locked -p forme-execution --bin forme-hostd -p forme-cli --bin forme-hostctl -p forme-gateway --bin forme-webd --bin forme-gatewayd
    if ($LASTEXITCODE -ne 0) { throw 'Managed product binaries failed to build' }
    & node --test tools/tests/cognition_correction.test.mjs
    if ($LASTEXITCODE -ne 0) { throw 'Correction process and restart evidence failed' }
    Write-Host 'Workbench correction: PASS (atomic feedback, invalidation, candidate, erasure, authenticated bridge and process restart)'
    Write-Host 'Native installation, live provider and complete IP20/IP24: NOT_RUN by this component gate'
}
finally { Pop-Location }
