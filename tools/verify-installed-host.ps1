param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $repoRoot
try {
    function Invoke-NamedTest {
        param([string]$Package, [string[]]$Target, [string]$Test)
        $testArgs = @("test", "-p", $Package) + $Target + @($Test, "--", "--exact")
        $previous = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $output = @(& cargo @testArgs 2>&1 | ForEach-Object { "$_" })
            $code = $LASTEXITCODE
        }
        finally { $ErrorActionPreference = $previous }
        if ($code -ne 0 -or @($output | Where-Object { $_ -match "^test result: ok\. 1 passed; 0 failed; 0 ignored;" }).Count -ne 1) {
            $output | ForEach-Object { Write-Host $_ }
            throw "host foundation test did not pass exactly once: $Test"
        }
        Write-Host "[PASS] $Test"
    }

    foreach ($name in @(
        "product_commands_are_closed_and_never_accept_shell_or_owner_reassignment",
        "uninstall_cannot_smuggle_owner_data_deletion_into_maintenance",
        "release_paths_reject_cross_platform_escapes_aliases_and_device_names",
        "release_ids_and_compatibility_ranges_are_validated_before_use",
        "manifest_paths_and_accounts_cannot_alias_privileged_domains",
        "lifecycle_cannot_skip_drain_backup_or_health_checks",
        "maintenance_authentication_can_renew_without_changing_bound_command_or_references",
        "productization_keeps_brain_taxonomy_unchanged"
    )) { Invoke-NamedTest "forme-protocol" @("--test", "installed_product_contract") $name }

    foreach ($name in @(
        "os_lock_blocks_a_second_writer_and_binding_survives_restart",
        "killed_writer_releases_os_lock_and_restart_never_repeats_the_observed_effect",
        "durable_idempotency_and_cas_reject_conflicts_without_writes",
        "concurrent_commands_cannot_both_advance_the_installation",
        "crash_after_intent_preserves_ambiguity_and_never_reexecutes_a_stage",
        "setup_cannot_start_core_and_failed_stage_cannot_be_overwritten_as_success",
        "recovery_required_retains_the_pending_stage_but_rejects_an_unreviewed_receipt",
        "successful_stage_chain_is_rebuilt_exactly_without_new_facts",
        "truncated_or_tampered_journal_fails_closed_and_is_never_repaired_silently",
        "pinned_signature_binds_every_binary_ui_target_and_closed_file_set",
        "unsigned_extra_files_and_schema_incompatible_rollback_are_rejected",
        "relative_host_paths_are_rejected_before_access",
        "host_broker_rejects_wrong_proof_and_stale_release_without_writing",
        "host_broker_plans_and_executes_a_native_action_exactly_once",
        "host_broker_fails_closed_for_unimplemented_maintenance_workers",
        "concurrent_host_execution_cannot_duplicate_the_native_call",
        "expired_host_plan_has_zero_native_calls_and_zero_revision_change",
        "idempotency_cannot_replace_server_bound_release_references",
        "broker_crash_after_native_effect_preserves_intent_and_forbids_reexecution"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_host_contract") $name }

    Invoke-NamedTest "forme-execution" @("--bin", "forme-hostd") "tests::ordinary_http11_is_parsed_and_ambiguous_or_browser_requests_are_rejected"
    Invoke-NamedTest "forme-execution" @("--test", "installed_host_transport") "http_broker_authenticates_and_reuses_the_original_read_only_operation"

    Invoke-NamedTest "forme-execution" @("--lib") "installed::service::tests::service_commands_have_only_builtin_targets_and_no_shell"
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        Invoke-NamedTest "forme-execution" @("--lib") "installed::service::tests::service_observation_never_turns_ambiguous_output_into_running"
    }
    else {
        Invoke-NamedTest "forme-execution" @("--test", "installed_host_contract") "symlinked_host_paths_are_rejected_before_open"
    }

    Write-Host "installed-host-foundation: PASS"
    Write-Host "IP01-IP24: NOT_RUN (this gate does not install services or certify a native package)"
}
finally { Pop-Location }
