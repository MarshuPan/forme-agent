# Named contract gate. Real loopback/runtime fixtures; no native IP certification.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repoRoot
try {
    $names = @(
        'web::core::credential_tests::bridge_credentials_are_bounded_regular_files_and_never_echo_private_input',
        'server::managed_config::tests::managed_config_uses_explicit_credentials_and_reopens_the_same_production_brain',
        'server::managed_config::tests::managed_configuration_fails_closed_for_missing_secrets_versions_and_aliases',
        'server::managed_config::tests::managed_config_rejects_unknown_fields_relative_paths_and_oversize_files',
        'server::web_bridge::session_fence::tests::bridge_nonce_and_session_revocation_survive_restart',
        'server::web_bridge::session_fence::tests::bridge_fence_rejects_foreign_store_second_writer_expiry_and_epoch_regression',
        'server::web_bridge::session_fence::tests::bridge_nonce_consumption_is_atomic_for_competing_requests',
        'server::lifecycle_tests::web_workbench::web_workbench_runs_through_the_brain_once_and_logout_fences_core_proofs'
    )
    foreach ($name in $names) {
        $previous = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $output = @(& cargo test --locked -p forme-gateway --lib $name -- --exact 2>&1 | ForEach-Object { "$_" })
            $code = $LASTEXITCODE
        } finally { $ErrorActionPreference = $previous }
        if ($code -ne 0 -or @($output | Where-Object { $_ -match '^test result: ok\. 1 passed; 0 failed; 0 ignored;' }).Count -ne 1 -or
            @($output | Where-Object { $_ -ceq "test $name ... ok" }).Count -ne 1) {
            $output | ForEach-Object { Write-Host $_ }
            throw "Managed Web contract did not pass exactly once: $name"
        }
        Write-Host "[PASS] $name"
    }
    Write-Host "Web workbench components: PASS ($($names.Count) named cases; counted model fixture)"
    Write-Host 'Native installer, real provider, claimed-session browser and IP01-IP24: NOT_RUN by this gate'
} finally { Pop-Location }
