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

    Invoke-NamedTest "forme-execution" @("--bin", "forme-hostd") "tests::service_mode_never_accepts_the_development_process_switch"

    foreach ($name in @(
        "worker_requires_agent_domain_and_real_isolation",
        "worker_binds_plan_scope_epoch_deadline_and_nonce_before_side_effect",
        "worker_admission_is_idempotent_and_restart_marks_exact_job_unknown",
        "worker_receipt_and_cancel_are_bound_to_the_original_job",
        "repeated_cancel_after_an_unsettled_cancel_never_calls_the_isolator_twice",
        "cancelled_observation_replays_the_exact_cancel_receipt",
        "concurrent_cancel_and_probe_cannot_duplicate_the_platform_effect",
        "cancellation_waits_for_admission_to_finish_before_touching_the_platform"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_worker_contract") $name }

    foreach ($name in @(
        "process_worker_uses_registered_program_and_reports_completion",
        "process_worker_preserves_failed_exit_as_failed_receipt",
        "process_worker_cancel_kills_running_child_without_retry"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_worker_process_contract") $name }

    foreach ($name in @(
        "managed_worker_config_rejects_schema_scope_and_profile_ambiguity",
        "managed_worker_config_rejects_writable_path_overlap_and_protected_files",
        "managed_worker_config_load_is_bounded_closed_and_outside_worker_paths"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_worker_config_contract") $name }
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        Invoke-NamedTest "forme-execution" @("--test", "installed_worker_config_contract") "managed_worker_config_rejects_linked_files_and_workspaces"
    }

    foreach ($name in @(
        "hostd_dispatches_registered_worker_and_keeps_maintenance_domain_out",
        "hostd_cancels_the_original_worker_job_without_a_second_dispatch",
        "hostd_default_config_cannot_enable_an_unsandboxed_worker",
        "concurrent_http_dispatches_run_the_original_worker_once",
        "invalid_worker_requests_and_routes_have_no_admission_or_side_effect"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_host_worker_transport") $name }

    foreach ($name in @(
        "lifecycle::tests::closing_admission_preserves_existing_work_and_rejects_later_claims",
        "lifecycle::tests::drain_configuration_rejects_unversioned_or_unbounded_waits",
        "server::lifecycle_tests::controlled_shutdown_waits_for_the_real_model_run_and_releases_the_listener",
        "server::lifecycle_tests::shutdown_timeout_preserves_inflight_facts_without_fabricating_a_terminal",
        "server::lifecycle_tests::cancelled_request_keeps_its_blocking_admission_until_the_operation_finishes",
        "server::lifecycle_tests::stop_during_startup_never_announces_ready_or_claims_background_work",
        "server::lifecycle_tests::shutdown_preserves_the_exact_pending_approval_across_restart_without_an_effect",
        "server::lifecycle_tests::shutdown_stops_background_claims_and_leaves_later_intentions_pending"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") $name }

    Invoke-NamedTest "forme-harness" @("--test", "harness_contract") "default_approval_restart_rejects_foreign_or_unreproducible_contexts_without_effects"

    Write-Host "installed-host-foundation and core-shutdown-components: PASS"
    Write-Host "IP01-IP24: NOT_RUN (this gate does not install services or certify a native package)"
}
finally { Pop-Location }
