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
        "productization_keeps_brain_taxonomy_unchanged",
        "owner_maintenance_exposes_only_bound_release_lifecycle_actions",
        "release_lifecycle_action_rejects_unbound_backup_or_release_values"
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

    foreach ($name in @(
        "stage_publishes_exact_files_once_and_replays_without_the_source",
        "tampered_inbox_at_plan_has_no_fact_and_after_plan_keeps_unknown_intent",
        "existing_destination_extra_files_and_wrong_digest_are_rejected_without_publication",
        "concurrent_execution_publishes_a_single_immutable_record",
        "uninstall_scope_retains_every_data_domain_and_never_uninstalls",
        "wrong_installation_worker_cannot_attach",
        "interrupted_stage_directory_is_preserved_and_cannot_be_reused",
        "restart_rejects_misbound_stop_receipt_before_attempting_start",
        "verified_staged_release_uses_committed_facts_without_inbox_or_mutation",
        "verified_staged_release_refuses_uncommitted_or_misbound_installation",
        "verified_staged_release_rechecks_receipt_and_exact_content_before_switch",
        "staged_compatibility_requires_observed_database_schema_and_real_migrator",
        "typed_release_worker_commits_upgrade_and_replays_active_release",
        "failed_release_worker_enters_recovery_and_never_replays_the_effect",
        "initial_release_binding_is_usable_without_reopening_the_broker",
        "failed_admission_release_preserves_commit_and_does_not_replay_lifecycle",
        "release_plan_without_worker_has_no_new_journal_fact",
        "release_plan_rejects_stale_identity_digest_and_target_before_planning",
        "updater_manifest_must_match_the_initial_installation_verification",
        "release_worker_with_matching_id_but_foreign_manifest_is_rejected_before_plan",
        "release_rechecks_target_after_backup_before_any_switch_effect",
        "service_switch_cases::service_image_switch_preflights_all_three_and_durably_consumes_the_original_intent",
        "service_switch_cases::failed_service_preflight_has_zero_effects_and_zero_claims",
        "service_switch_cases::lost_native_switch_receipt_survives_restart_without_repeating_partial_effects",
        "service_switch_cases::stale_expired_or_changed_configuration_plans_never_reach_native_effects"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_release_staging") $name }

    foreach ($name in @(
        "historical_journal_hashes_preserve_absent_and_explicit_null_fields",
        "active_release_projection_replays_after_a_complete_switch",
        "active_release_change_rejects_stale_target_and_non_lifecycle_operations",
        "active_release_change_requires_switching_intent"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::journal::tests::$name" }

    foreach ($name in @(
        "incomplete_acl_intent_survives_restart_as_pending_cleanup",
        "cleanup_requires_tree_termination_and_exact_acl_revocation",
        "cleanup_receipt_and_projection_survive_reopen",
        "second_writer_and_torn_journal_fail_closed",
        "hash_chain_tampering_is_rejected",
        "appcontainer_identity_and_cleanup_directory_are_bound_to_lease",
        "active_leases_cannot_share_job_directory_path_or_identity"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::sandbox_lease::tests::$name" }

    foreach ($name in @(
        "release_data_identity_is_bound_to_owner_operation_and_real_schema_versions",
        "release_data_protocol_is_closed_and_rejects_rebinding_and_unknown_versions",
        "service_image_receipts_require_all_three_distinct_services_and_the_original_command"
    )) { Invoke-NamedTest "forme-protocol" @("--test", "installed_release_data_contract") $name }

    foreach ($name in @(
        "release_data_child_preserves_binding_and_consumes_backup_once",
        "release_data_child_deadline_kills_waiting_process_and_does_not_retry_unknown_backup",
        "release_data_child_rejects_bad_owner_oversize_nonzero_exit_and_replaced_installation"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_release_data_transport") $name }

    foreach ($name in @(
        "updater_cases::deferred_release_admission_is_exact_once_and_restart_cannot_recreate_dispatch_authority",
        "updater_cases::updater_consumes_marker_before_stops_and_never_competes_for_the_live_host_writer",
        "updater_cases::altered_handoff_or_tampered_target_produces_zero_native_stops",
        "updater_cases::updater_transfers_real_writers_and_child_pipe_then_leaves_target_checking_closed"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_release_staging") $name }

    foreach ($name in @(
        "release_data_observes_real_schema_holds_store_and_creates_verified_backup_once",
        "release_data_rejects_wrong_owner_revision_missing_keys_and_cross_operation_frames"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") "server::lifecycle_tests::installed_release::$name" }

    foreach ($name in @(
        "server::lifecycle_tests::installed_release::live_probe::live_release_router_enforces_http_boundaries_and_independent_rate_without_brain_effects",
        "server::lifecycle_tests::installed_release::live_probe::ordinary_runtime_has_no_release_readiness_endpoint_authority"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") $name }

    foreach ($name in @(
        "installed::release_probe::tests::real_probe_socket_rejects_forgery_replay_and_wrong_binding_without_disclosing_bearer",
        "installed::release_probe::tests::non_loopback_or_expired_probe_has_no_connection_attempt"
    )) { Invoke-NamedTest "forme-execution" @("--lib") $name }

    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        Invoke-NamedTest "forme-execution" @("--lib") "installed::registration::release::tests::image_change_preserves_identity_startup_dependency_group_and_display"
    }

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
        "managed_host_load_is_closed_bounded_and_side_effect_free",
        "managed_host_needs_private_distinct_file_credentials_and_cannot_enable_process_workers",
        "host_configuration_cannot_alias_itself_or_the_fallback_worker_journal",
        "managed_bootstrap_reference_is_preflighted_without_opening_its_authority",
        "release_staging_roots_are_checked_before_credentials_and_journal",
        "administrative_arguments_need_an_explicit_config_and_never_start_development"
    )) { Invoke-NamedTest "forme-execution" @("--bin", "forme-hostd") "host::config::tests::$name" }
    foreach ($name in @(
        "managed_host_uses_file_config_and_keeps_both_credential_domains_separate",
        "invalid_managed_host_config_fails_without_legacy_fallback_or_journal",
        "managed_host_subconfig_errors_have_zero_management_or_worker_facts"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_host_managed_transport") $name }
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        foreach ($name in @(
            "native_registration_rejects_foreign_command_account_or_startup",
            "native_registration_has_fixed_identity_and_finite_recovery"
        )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::registration::tests::$name" }
        foreach ($name in @(
            "installed::windows_appcontainer::tests::profile_identity_is_deterministic_and_plan_bound",
            "installed::windows_appcontainer::tests::job_object_name_is_closed_and_bound_to_the_appcontainer_identity",
            "installed::windows_appcontainer::tests::environment_block_is_deterministic_and_scoped",
            "installed::windows_appcontainer::tests::an_unowned_lease_blocks_new_admission_without_sandbox_side_effects",
            "installed::windows_appcontainer::tests::startup_recovery_revokes_an_unacknowledged_grant_and_is_idempotent",
            "installed::windows_appcontainer_acl::tests::failed_file_opens_never_become_owned_handles",
            "installed::windows_appcontainer_acl::tests::real_acl_grant_and_revoke_match_the_lease_readback_contract",
            "installed::windows_appcontainer_native::tests::native_failure_handles_are_rejected_and_empty_capabilities_need_no_array",
            "installed::windows_appcontainer_native::tests::worker_integrity_requires_the_exact_low_mandatory_sid",
            "installed::windows_appcontainer_native::tests::empty_job_recovery_uses_accounting_without_synchronize_access"
        )) { Invoke-NamedTest "forme-execution" @("--lib") $name }
    }
    else {
        Invoke-NamedTest "forme-execution" @("--bin", "forme-hostd") "host::config::tests::broad_credential_permissions_are_refused"
    }

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
        "hostd_accepts_delayed_fragmented_worker_requests_without_redispatch",
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

    foreach ($name in @(
        "restore_stages_precede_effects_and_commit_releases_work_only_after_exact_readiness",
        "every_interrupted_restore_keeps_original_operation_and_never_retries_side_effects",
        "stale_config_owner_and_expired_approval_do_not_stop_the_brain"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::host::host_restore_tests::$name" }

    foreach ($name in @(
        "installed_restore_worker_holds_both_real_databases_backs_up_and_fences_sessions",
        "installed_restore_worker_rejects_stale_revision_foreign_owner_digest_and_cross_operation_steps",
        "managed_work_admission_rechecks_each_claim_and_cannot_use_foreign_or_missing_release"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") "server::lifecycle_tests::installed_restore::$name" }

    foreach ($name in @(
        "restored_history_never_regains_model_action_resume_or_intention_authority",
        "repeated_restores_extend_fence_lineage_and_reject_tampering",
        "a_sealed_restore_candidate_cannot_be_committed_after_it_has_run_again",
        "online_restore_binds_the_approved_plan_and_final_post_sealing_manifest"
    )) { Invoke-NamedTest "forme-store" @("--test", "installed_offline_recovery") $name }

    Write-Host "installed-host-foundation and core-shutdown-components: PASS"
    Write-Host "IP01-IP24: NOT_RUN (this gate does not install services or certify a native package)"
}
finally { Pop-Location }
