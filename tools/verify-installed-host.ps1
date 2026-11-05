param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
. (Join-Path $PSScriptRoot 'build-resources.ps1')
$isWindowsHost = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
Push-Location $repoRoot
try {
    function Invoke-NamedTest {
        param([string]$Package, [string[]]$Target, [string]$Test)
        $testArgs = @("test", "-p", $Package) + $Target + @($Test, "--", "--exact")
        $previous = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            Assert-FormeBuildSpace $repoRoot
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
        "legacy_report_requires_complete_ordered_same_binding_evidence",
        "legacy_rejected_and_unverifiable_reports_cannot_omit_or_repeat_issues",
        "legacy_binding_digest_covers_every_source_identity_and_checkpoint",
        "legacy_commands_are_closed_and_verification_has_independent_authorization_digest",
        "legacy_views_and_lists_preserve_owner_revision_order_and_receipt_boundaries",
        "legacy_wire_digest_golden_matches_the_browser_report_and_request_binding",
        "recovery_capabilities_accept_only_exact_supported_reader_updater_pairs"
    )) { Invoke-NamedTest "forme-protocol" @("--test", "installed_legacy_release_recovery_contract") $name }

    foreach ($name in @(
        "legacy_host_verifies_signed_files_and_commits_once_with_no_service_effects",
        "legacy_host_rejects_foreign_identity_missing_collector_and_forged_signatures_before_append",
        "legacy_host_rejects_each_missing_or_changed_artifact_despite_valid_signature",
        "legacy_host_rejects_signed_journal_self_attestation_before_append",
        "legacy_host_rejects_service_worker_configuration_and_review_drift",
        "legacy_host_interrupted_intent_is_query_only_until_original_verification"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::host::host_legacy_release_recovery::tests::$name" }

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
        "configuration_identity_reads_status_without_gaining_privileged_operations",
        "status_rejects_agent_and_foreign_configuration_proofs_before_observation"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_status_authorization") $name }

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
        "active_leases_cannot_share_job_directory_path_or_identity",
        "writeback_evidence_is_bound_to_terminal_job_and_replays_without_duplication"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::sandbox_lease::tests::$name" }

    foreach ($name in @(
        "release_data_identity_is_bound_to_owner_operation_and_real_schema_versions",
        "release_data_protocol_is_closed_and_rejects_rebinding_and_unknown_versions",
        "service_image_receipts_require_all_three_distinct_services_and_the_original_command"
    )) { Invoke-NamedTest "forme-protocol" @("--test", "installed_release_data_contract") $name }

    foreach ($name in @(
        "release_data_child_preserves_binding_and_consumes_backup_once",
        "release_recovery_inspection_uses_fixed_child_and_releases_without_backup",
        "release_data_hold_rejects_a_cleanly_exited_child_without_respawn_or_backup",
        "release_data_hold_rechecks_the_existing_child_and_cannot_survive_its_deadline",
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
        "release_recovery_preserves_failure_history_and_atomically_aborts_without_changing_active_release",
        "release_recovery_intent_survives_restart_queries_are_pure_and_only_explicit_verification_commits",
        "release_recovery_cas_race_and_cross_domain_keys_allow_exactly_one_decision",
        "release_recovery_rejects_missing_baseline_early_late_and_mismatched_evidence_without_appending",
        "release_recovery_schema_floor_preserves_v1_hashes_and_rejects_a_validly_rehashed_downgrade",
        "release_recovery_protocol_is_closed_and_binds_owner_revision_and_receipt"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::journal::release_recovery::tests::$name" }

    foreach ($name in @(
        "verified_legacy_recovery_is_atomic_and_replays_without_touching_active_release",
        "non_verified_or_drifted_reviews_are_rejected_without_append",
        "recovery_cas_and_cross_domain_collisions_allow_only_one_intent",
        "interrupted_recovery_requires_same_request_and_explicit_verification",
        "schema_three_is_not_read_by_a_v2_reader_and_unknown_fact_fails_closed",
        "legacy_authorization_waits_for_expired_takeover_and_cannot_outlive_review",
        "legacy_source_binding_is_checked_even_when_changed_report_digests_are_consistent"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::journal::legacy_recovery_review::tests::$name" }
    foreach ($name in @(
        "original_journal_evidence_reads_real_bytes_without_appending_or_moving_the_cursor",
        "original_journal_prefix_survives_its_own_intent_but_not_a_committed_decision",
        "original_journal_foreign_binding_and_expiry_reject_without_poisoning_valid_bytes",
        "original_journal_corruption_poisoning_prevents_any_further_decision_append",
        "original_journal_valid_truncation_is_detected_against_the_live_projection",
        "original_journal_rejects_an_intent_for_another_report_without_poisoning_the_writer"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::journal::legacy_recovery_review::tests::source_tests::$name" }

    foreach ($name in @(
        "offline_original_reports_actual_checkpoint_without_writing_or_exposing_tokens",
        "offline_original_refuses_live_writer_and_holds_its_own_lock_until_drop",
        "offline_original_custody_blocks_replacement_or_detects_changed_path_identity",
        "offline_original_never_creates_missing_or_initializes_empty_files",
        "offline_original_refuses_hardlink_aliases_and_foreign_identity_without_changes",
        "offline_original_replays_historical_absent_defaults_using_original_fact_bytes",
        "offline_original_rejects_corruption_unknown_schema_and_invalid_causality_without_repair"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::journal::legacy_recovery_review::tests::offline_source_tests::$name" }

    foreach ($name in @(
        "release_recovery_host_commits_once_then_old_release_replay_and_queries_are_effect_free",
        "release_recovery_rechecks_original_data_custody_before_committing_and_keeps_intent",
        "release_recovery_host_intent_fault_requires_explicit_verification_and_never_retries_on_replay",
        "release_recovery_host_rejects_missing_baseline_and_service_data_file_worker_drift"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_release_staging") "updater_cases::recovery_cases::$name" }

    foreach ($name in @(
        "release_recovery_worker_running_and_restart_unknown_never_prove_quiescence",
        "release_recovery_worker_observation_holds_admission_exclusion_through_its_callback"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_worker_contract") "recovery_cases::$name" }

    Invoke-NamedTest "forme-execution" @("--test", "installed_release_recovery_capabilities") "release_recovery_capability_mode_is_closed_and_never_loads_host_configuration"
    Invoke-NamedTest "forme-execution" @("--test", "installed_release_recovery_capabilities") "offline_source_inspection_never_falls_through_to_runtime_or_creates_missing_files"
    Invoke-NamedTest "forme-execution" @("--test", "installed_legacy_source_inspection") "offline_source_child_uses_fixed_original_and_rejects_writer_alias_or_manifest_drift"
    foreach ($name in @(
        "release_recovery_http_owner_csrf_reauthentication_and_closed_input_precede_host_io",
        "release_recovery_verification_is_separate_closed_owner_action_and_plain_maintenance_cannot_adjudicate"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") "web::http::tests::release_recovery::$name" }
    Invoke-NamedTest "forme-gateway" @("--lib") "web::http::tests::release_recovery::transport::release_recovery_http_forwards_only_session_owner_and_bound_request_after_password_then_reads_original"
    Invoke-NamedTest "forme-gateway" @("--lib") "web::http::tests::release_recovery::legacy::legacy_http_requires_owner_csrf_closed_input_and_bound_password_before_host_io"
    foreach ($name in @(
        "legacy_http_transports_only_bound_owner_commands_and_keeps_refresh_read_only",
        "legacy_http_rejects_cross_owner_rebound_or_unclosed_host_responses"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") "web::http::tests::release_recovery::legacy::transport::$name" }
    Invoke-NamedTest "forme-execution" @("--bin", "forme-hostd") "host::http::paths::tests::legacy_recovery_paths_are_configuration_only_without_a_management_wildcard"
    Invoke-NamedTest "forme-execution" @("--bin", "forme-hostd") "host::legacy_recovery_routes::tests::legacy_host_http_domains_reject_cross_credentials_and_unavailable_dependencies_without_facts"

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
            "host_process_proof_requires_running_own_process_with_the_exact_nonzero_pid",
            "registration_recheck_cannot_hide_a_changed_or_unqueryable_running_process"
        )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::registration::process::tests::$name" }
        foreach ($name in @(
            "installed::windows_appcontainer::tests::profile_identity_is_deterministic_and_plan_bound",
            "installed::windows_appcontainer::tests::job_object_name_is_closed_and_bound_to_the_appcontainer_identity",
            "installed::windows_appcontainer::tests::environment_block_is_deterministic_and_scoped",
            "installed::windows_appcontainer::tests::an_unowned_lease_blocks_new_admission_without_sandbox_side_effects",
            "installed::windows_appcontainer::tests::startup_recovery_revokes_an_unacknowledged_grant_and_is_idempotent",
            "installed::windows_appcontainer::tests::startup_recovery_preserves_unknown_overlay_after_revoking_access",
            "installed::windows_appcontainer_acl::tests::failed_file_opens_never_become_owned_handles",
            "installed::windows_appcontainer_acl::tests::real_acl_grant_and_revoke_match_the_lease_readback_contract",
            "installed::windows_appcontainer_acl::tests::low_labels_are_only_applied_to_a_private_test_copy",
            "installed::windows_appcontainer_native::tests::native_failure_handles_are_rejected_and_empty_capabilities_need_no_array",
            "installed::windows_appcontainer_native::tests::worker_integrity_requires_the_exact_low_mandatory_sid",
            "installed::windows_appcontainer_native::tests::empty_job_recovery_uses_accounting_without_synchronize_access",
            "installed::windows_agent_execution_native::tests::pipe_name_is_install_bound_and_closed",
            "installed::windows_agent_execution_native::tests::idle_native_pipe_client_expires_without_a_blocked_io_thread",
            "installed::windows_agent_execution_native::tests::native_pipe_client_reads_delayed_bytes_and_observes_disconnect"
        )) { Invoke-NamedTest "forme-execution" @("--lib") $name }
        foreach ($name in @(
            "copy_is_private_and_verified_writeback_handles_creates_edits_and_deletes",
            "owner_changes_reject_the_whole_writeback_without_an_intent",
            "exact_file_action_rejects_parameter_substitution_and_sibling_changes",
            "file_read_cannot_write_and_exact_delete_is_supported",
            "changed_binding_or_expired_permit_never_writes",
            "original_identity_replacement_is_a_conflict_even_with_identical_content",
            "snapshot_rejects_hardlinks_and_oversized_files",
            "original_handles_block_concurrent_write_and_ancestor_rename",
            "interrupted_writeback_preserves_intent_and_does_not_retry_partial_effects",
            "malformed_receipt_blocks_cleanup_instead_of_destroying_evidence",
            "missing_spec_network_or_foreign_cwd_cannot_create_a_copy"
        )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::windows_workspace_overlay::tests::$name" }
    }
    else {
        Invoke-NamedTest "forme-execution" @("--bin", "forme-hostd") "host::config::tests::broad_credential_permissions_are_refused"
    }

    foreach ($name in @(
        "worker_requires_agent_domain_and_real_isolation",
        "worker_binds_plan_scope_epoch_deadline_and_nonce_before_side_effect",
        "worker_admission_is_idempotent_and_restart_marks_exact_job_unknown",
        "worker_receipt_and_cancel_are_bound_to_the_original_job",
        "desktop_helper_dispatch_is_exact_and_terminal_receipt_is_idempotent",
        "desktop_grant_expiry_and_capability_mismatch_are_rejected_before_helper_call",
        "desktop_helper_loss_after_start_is_unknown_and_never_retried",
        "repeated_cancel_after_an_unsettled_cancel_never_calls_the_isolator_twice",
        "cancelled_observation_replays_the_exact_cancel_receipt",
        "concurrent_cancel_and_probe_cannot_duplicate_the_platform_effect",
        "cancellation_waits_for_admission_to_finish_before_touching_the_platform"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_worker_contract") $name }

    foreach ($name in @(
        "process_worker_uses_registered_program_and_reports_completion",
        "process_worker_preserves_failed_exit_as_failed_receipt",
        "process_worker_cancel_kills_running_child_without_retry",
        "result_ciphertext_survives_restart_but_wrong_missing_keys_and_tampering_never_read"
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
        "invalid_worker_requests_and_routes_have_no_admission_or_side_effect",
        "worker_broker_client_dispatches_typed_spec_and_reuses_original_job",
        "worker_broker_client_cannot_use_maintenance_listener",
        "worker_action_backend_crosses_real_host_and_delayed_transport_once",
        "worker_action_backend_lost_response_probes_the_real_original_job_only",
        "worker_action_backend_missing_real_result_has_no_completion_or_reexecution"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_host_worker_transport") $name }

    foreach ($name in @(
        "managed_worker_completes_and_reuses_original_job_without_redispatch",
        "dispatch_error_becomes_unknown_and_never_creates_a_replacement_job",
        "manifest_and_chunk_tampering_are_unknown",
        "fragmented_or_late_results_stop_without_output_completion_or_redispatch",
        "cancel_targets_the_original_worker_job",
        "transport_delay_empty_results_and_split_utf8_preserve_the_plan",
        "a_lost_dispatch_response_can_only_read_the_original_terminal_receipt",
        "token_cancellation_contacts_the_original_running_job",
        "invalid_limits_and_changed_plan_never_dispatch"
        "worker_file_mutations_never_claim_a_local_rollback_snapshot"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_worker_backend_contract") $name }

    foreach ($name in @(
        "typed_execution_spec_is_bound_to_job_and_permit",
        "typed_execution_spec_rejects_expiry_and_unbounded_payload",
        "dispatch_wire_accepts_legacy_request_without_optional_execution",
        "transport_delay_preserves_the_fixed_timeout_and_never_extends_the_deadline"
    )) { Invoke-NamedTest "forme-protocol" @("--test", "installed_worker_contract") $name }

    foreach ($name in @(
        "installed_file_entry_reads_writes_and_deletes_only_the_private_copy",
        "installed_file_entry_rejects_mapping_permit_and_scope_substitutions_before_writes",
        "installed_file_entry_checks_budget_encoding_and_link_custody_before_mutations",
        "installed_file_entry_result_creation_failure_never_modifies_the_original"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_file_worker_contract") $name }

    Invoke-NamedTest "forme-execution" @("--lib") "installed::worker_socket::tests::fragmented_tcp_bytes_cannot_extend_the_absolute_request_deadline"
    Invoke-NamedTest "forme-execution" @("--test", "installed_worker_contract") "result_handoff_orders_manifest_cleanup_receipt_and_never_retries_failed_cleanup"
    Invoke-NamedTest "forme-execution" @("--test", "installed_worker_contract") "cancel_result_cleanup_failure_stays_unknown_without_repeating_cleanup"
    Invoke-NamedTest "forme-execution" @("--test", "installed_worker_contract") "cancellation_seals_existing_source_without_fabricating_completion"
    if ($isWindowsHost) {
        Invoke-NamedTest "forme-execution" @("--lib") "installed::windows_appcontainer::results::tests::unsealed_or_changed_handoff_preserves_source_and_blocks_cleanup"
        Invoke-NamedTest "forme-execution" @("--lib") "installed::windows_agent_execution_native::tests::occupied_host_pipe_rejects_before_any_credential_or_job_byte"
    }

    foreach ($name in @(
        "managed_missing_or_invalid_worker_stays_blocked_without_local_fallback",
        "managed_worker_placement_is_composition_bound_stable_and_secret_free",
        "installed_file_roots_are_scope_bound_rechecked_and_part_of_composition"
    )) { Invoke-NamedTest "forme-harness" @("--test", "installed_worker_composition") $name }

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
        "stale_config_owner_and_expired_approval_do_not_stop_the_brain",
        "backup_verification_rejects_wrong_domain_owner_revision_and_lifecycle_fence_before_a_worker"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::host::host_restore_tests::$name" }

    foreach ($name in @(
        "installed_restore_worker_holds_both_real_databases_backs_up_and_fences_sessions",
        "installed_restore_worker_rejects_stale_revision_foreign_owner_digest_and_cross_operation_steps",
        "managed_work_admission_rechecks_each_claim_and_cannot_use_foreign_or_missing_release"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") "server::lifecycle_tests::installed_restore::$name" }

    foreach ($name in @(
        "offline_backup_verification_checks_real_encrypted_snapshot_without_loading_runtime",
        "offline_backup_verification_refuses_owner_revision_installation_and_path_substitution",
        "offline_backup_verification_refuses_lock_contention_and_corruption_without_repair"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") "server::lifecycle_tests::installed_backup_verification::$name" }
    Invoke-NamedTest "forme-gateway" @("--lib") "web::http::tests::backup_verification::offline_verification_requires_authenticated_owner_and_csrf_before_configuration_io"
    Invoke-NamedTest "forme-protocol" @("--lib") "installed_product::backup_verification::tests::offline_verification_binds_owner_revision_installation_and_opaque_object"

    foreach ($name in @(
        "restored_history_never_regains_model_action_resume_or_intention_authority",
        "repeated_restores_extend_fence_lineage_and_reject_tampering",
        "a_sealed_restore_candidate_cannot_be_committed_after_it_has_run_again",
        "online_restore_binds_the_approved_plan_and_final_post_sealing_manifest"
    )) { Invoke-NamedTest "forme-store" @("--test", "installed_offline_recovery") $name }

    foreach ($name in @(
        "active_programs_follow_only_the_journal_release_across_restart_without_rewriting_config",
        "active_programs_refuse_lost_staging_or_tampered_files_without_initial_image_fallback",
        "active_file_program_preserves_custom_profiles_and_rejects_replaced_reserved_arguments"
    )) { Invoke-NamedTest "forme-execution" @("--test", "installed_release_staging") "active_program_cases::$name" }

    foreach ($name in @(
        "new_host_generation_starts_a_stopped_brain_without_reexecuting_the_old_operation",
        "host_reopen_waits_for_ack_without_restarting_an_already_activated_running_brain",
        "host_reopen_preserves_unknown_start_and_never_creates_a_replacement_attempt",
        "explicit_stop_survives_config_save_and_host_reopen_until_explicit_start",
        "concurrent_startup_checks_admit_one_automatic_service_effect"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::product_config::activation_tests::startup_tests::$name" }

    foreach ($name in @(
        "readiness_precedes_startup_and_serving_can_answer_its_request",
        "failed_readiness_starts_neither_coordination_nor_serving",
        "serving_failure_still_joins_startup_before_returning",
        "startup_panic_is_a_host_failure_and_is_joined"
    )) { Invoke-NamedTest "forme-execution" @("--bin", "forme-hostd") "host::lifecycle::tests::$name" }

    foreach ($name in @(
        "v2_claim_preserves_v1_prefix_and_separates_commit_from_activation",
        "restart_at_each_stage_is_read_only_and_unactivated_data_requires_recovery",
        "verified_backup_uses_the_same_bound_commit_and_activation_lane",
        "selection_rejects_wrong_session_digest_epoch_owner_or_unverified_candidate",
        "inventory_rejects_stale_epoch_installation_drift_and_post_selection_refresh",
        "prepared_and_commit_are_bound_to_candidate_owner_target_and_worker_result",
        "abort_before_data_preserves_history_but_cannot_clear_prepared_data",
        "query_requires_service_and_exact_setup_binding_without_appending",
        "partial_unknown_and_downgraded_journals_are_preserved_and_fail_closed",
        "hash_valid_v1_fact_cannot_downgrade_a_v2_journal"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::bootstrap::tests::legacy_journal::$name" }

    foreach ($name in @(
        "offline_activation_restart_at_every_fact_never_redispatches_or_invents_confirmation",
        "activation_requires_original_account_and_exact_offline_confirmation_before_signing"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::bootstrap::tests::legacy_journal::activation::$name" }
    foreach ($name in @(
        "old_configuration_lists_only_checked_empty_target_and_preserves_original_selection",
        "unavailable_historical_discovery_cannot_downgrade_to_a_new_brain_catalog"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::bootstrap::tests::catalog_compat::$name" }
    foreach ($name in @(
        "bootstrap_data_binding_is_exact_idempotent_revision_zero_and_survives_restart_without_runtime_ack",
        "concurrent_first_adoptions_allow_one_owner_and_reject_foreign_target_before_publication"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::product_config::tests::bootstrap::$name" }
    foreach ($name in @(
        "bootstrap_v2_http_rejects_unbound_cookie_csrf_version_and_paths_before_host_io",
        "bootstrap_v2_http_recovers_only_the_exact_host_selection_after_a_lost_response",
        "bootstrap_v2_http_recovers_the_original_schema_one_new_choice_without_selecting_again",
        "bootstrap_v2_http_creates_owner_session_only_after_pinned_activation_without_starting_a_core"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") "web::http::tests::bootstrap_legacy::$name" }
    foreach ($name in @(
        "bootstrap_catalog_verifies_actual_owner_protection_replay_and_holds_source_fence",
        "bootstrap_wrong_owner_corrupt_database_and_unknown_schema_never_become_empty_brain",
        "bootstrap_owner_digest_without_authenticated_content_is_unverifiable",
        "bootstrap_aliases_and_busy_sources_remain_visible_and_unverifiable",
        "bootstrap_existing_fence_is_respected_without_recreating_or_rebinding_it",
        "bootstrap_epoch_digest_and_source_identity_are_bound_to_real_observation"
    )) { Invoke-NamedTest "forme-gateway" @("--lib") "server::lifecycle_tests::installed_bootstrap::$name" }
    Invoke-NamedTest "forme-store" @("--lib") "offline_validation::tests::crypto_deleted_envelopes_do_not_count_as_authenticated_owner_content"
    Assert-FormeBuildSpace $repoRoot
    & cargo build --locked -p forme-execution --bin forme-hostd -p forme-cli --bins
    if ($LASTEXITCODE -ne 0) { throw "bootstrap fixed-process binaries could not be built" }
    foreach ($name in @(
        "fixed_child_validates_candidates_without_creating_claim_brain_or_runtime",
        "real_host_catalog_owns_epoch_authentication_journal_and_preserves_new_brain_compatibility",
        "real_host_rechecks_source_before_selection_and_recovers_selection_without_activation",
        "adoption::real_host_adopts_encrypted_data_before_signing_without_core_or_account",
        "adoption::real_host_restores_verified_backup_preserving_source_owner_and_original_files",
        "adoption::real_host_restart_after_config_cas_requires_recovery_without_recopying_or_signing",
        "data_worker::bootstrap_worker_eof_keeps_original_intent_prepared_data_and_refuses_a_second_copy",
        "data_worker::bootstrap_worker_confirmation_rechecks_receipt_snapshot_and_exact_target_before_release"
    )) { Invoke-NamedTest "forme-cli" @("--test", "installed_bootstrap_candidates") $name }

    foreach ($name in @(
        "signed_repair_binds_full_distribution_and_preserves_business_images_and_state",
        "durable_receipt_survives_authorization_expiry_but_requires_commit_inside_window",
        "correctly_resigned_foreign_installation_owner_operation_release_and_checkpoint_are_rejected",
        "only_three_management_images_may_change_even_with_valid_independent_signatures",
        "every_receipt_binding_is_checked_even_when_resigned_by_the_trusted_authority",
        "independent_signature_domains_trust_roots_and_package_binding_cannot_be_substituted",
        "valid_signatures_cannot_claim_a_different_management_target",
        "closed_layout_closed_json_and_fixed_hashed_reference_reject_extra_inputs",
        "actual_images_configuration_services_and_exact_capabilities_are_required",
        "expired_inspection_and_unproved_real_platform_never_become_verified",
        "inspection_rechecks_actual_services_and_closed_artifact_layout_before_commit",
        "linked_distribution_or_installed_image_is_rejected_under_real_file_custody"
    )) { Invoke-NamedTest "forme-execution" @("--lib") "installed::management_repair::tests::$name" }
    if ($isWindowsHost) {
        Invoke-NamedTest "forme-execution" @("--lib") "installed::management_repair::tests::windows_inspection_denies_writing_or_replacing_original_files_until_guard_release"
    }

    Write-Host "installed-host-foundation and core-shutdown-components: PASS"
    Write-Host "IP01-IP24: NOT_RUN (this gate does not install services or certify a native package)"
}
finally { Pop-Location }
