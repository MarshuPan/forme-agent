# Component gate only. No installed-service/browser/live-provider claim.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $repoRoot
try {
    $expected = @(
        "identity_claim_requires_pinned_signature_and_exact_account_commitment",
        "identity_pending_claim_survives_restart_and_cannot_change_semantics",
        "identity_logout_revocation_survives_restart_and_secrets_are_not_stored",
        "identity_session_enforces_csrf_idle_absolute_and_clock_rollback",
        "identity_reauth_is_operation_bound_single_use_and_epoch_fenced",
        "identity_password_change_binds_new_hash_and_preserves_owner_across_restart",
        "identity_foreign_database_and_second_writer_are_rejected_without_repair",
        "identity_password_capacity_is_held_until_the_actual_job_finishes",
        "identity_password_bounds_do_not_trim_and_corrupt_kdf_parameters_fail_closed",
        "identity_expired_or_other_session_reauth_cannot_revoke",
        "identity_change_password_rejects_a_proof_for_another_prepared_hash",
        "identity_concurrent_authentication_keeps_one_live_session_without_false_clock_rollback",
        "identity_login_challenge_is_csrf_bound_expiring_and_consumed_once_across_restart",
        "identity_known_schema_upgrade_preserves_existing_account_and_session",
        "identity_setup_scope_is_distinct_and_candidate_binding_survives_restart",
        "identity_invalid_setup_proof_can_be_discarded_without_touching_claim_intent",
        "identity_password_change_key_reuses_only_its_bound_candidate",
        "identity_tampered_fence_outbox_requires_local_recovery",
        "product_operation_confirmation_consumes_only_exact_password_bound_proof",
        "revoked_session_cannot_confirm_product_operation_even_with_correct_password",
        "proposal_cases::proposal_recovery_rejects_corrupt_payload_and_mismatched_terminal_receipts",
        "proposal_cases::proposal_recovery_survives_installed_session_fence_and_requires_fresh_login",
        "proposal_cases::proposal_recovery_upgrades_v8_without_replacing_account_or_active_sessions",
        "proposal_recovery_capacity_fails_before_accepting_a_257th_operation",
        "proposal_recovery_is_exact_owner_bound_and_survives_identity_restart",
        "legacy_activation::identity_legacy_activation_requires_pinned_signature_candidate_owner_and_original_account",
        "legacy_activation::identity_legacy_startup_queries_only_original_activation_without_redispatch"
    )
    $ErrorActionPreference = "Continue"
    $output = @(& cargo test -p forme-gateway --lib web::identity::tests:: -- --test-threads=2 2>&1 | ForEach-Object { "$_" })
    $code = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($code -ne 0) {
        $output | ForEach-Object { Write-Host $_ }
        throw "Web identity component tests failed"
    }
    foreach ($name in $expected) {
        $pattern = '^test web::identity::tests::' + [regex]::Escape($name) + ' \.\.\. ok$'
        if (@($output | Where-Object { $_ -match $pattern }).Count -ne 1) {
            throw "Web identity test missing, ignored, duplicated, or failed: $name"
        }
    }
    $summary = '^test result: ok\. ' + $expected.Count + ' passed; 0 failed; 0 ignored;'
    if (@($output | Where-Object { $_ -match $summary }).Count -ne 1) {
        $output | ForEach-Object { Write-Host $_ }
        throw "Web identity component result count mismatch"
    }
    Write-Host "Web identity component gate: PASS ($($expected.Count) named tests)"
    Write-Host "IP01-IP24: NOT_RUN by this component gate"
}
finally { Pop-Location }
