# Real loopback component evidence, not native installation/browser acceptance.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $repoRoot
try {
    $expected = @(
        "web_unclaimed_ingress_is_reachable_without_a_brain_and_never_exposes_owner",
        "web_login_logout_and_reauth_revoke_enforce_live_sessions",
        "web_host_origin_forwarding_and_csrf_rejections_preserve_the_session",
        "web_password_change_revokes_old_cookies_without_changing_owner",
        "web_password_change_rejects_reusing_an_operation_key_for_another_password",
        "web_read_quota_is_separate_and_rate_rejections_have_retry_after",
        "web_configuration_rejects_plaintext_lan_and_malformed_secret_bodies_are_not_echoed",
        "web_cancelled_request_keeps_password_capacity_until_real_work_finishes",
        "web_idle_socket_capacity_is_bounded_and_recovers_when_a_connection_closes",
        "backup_verification::offline_verification_requires_authenticated_owner_and_csrf_before_configuration_io",
        "proposals::web_unknown_proposal_preserves_exact_command_and_only_explicit_verification_forwards"
    )
    $ErrorActionPreference = "Continue"
    $output = @(& cargo test -p forme-gateway --lib web::http::tests:: -- --test-threads=2 2>&1 | ForEach-Object { "$_" })
    $code = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($code -ne 0) {
        $output | ForEach-Object { Write-Host $_ }
        throw "Web ingress component tests failed"
    }
    foreach ($name in $expected) {
        $pattern = '^test web::http::tests::' + [regex]::Escape($name) + ' \.\.\. ok$'
        if (@($output | Where-Object { $_ -match $pattern }).Count -ne 1) {
            throw "Web ingress test missing, ignored, duplicated, or failed: $name"
        }
    }
    $summary = '^test result: ok\. ' + $expected.Count + ' passed; 0 failed; 0 ignored;'
    if (@($output | Where-Object { $_ -match $summary }).Count -ne 1) {
        throw "Web ingress component result count mismatch"
    }
    Write-Host "Web ingress component gate: PASS ($($expected.Count) named tests)"
    & (Join-Path $PSScriptRoot "verify-web-process.ps1")
    Write-Host "IP01-IP24: NOT_RUN by this component gate"
}
finally { Pop-Location }
