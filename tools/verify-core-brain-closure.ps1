param(
    # A wave gate runs the implemented wave's scenarios; "Final" requires every
    # wave and is the only form that can support a closure claim.
    [ValidateSet("A", "B", "C", "D", "Final")][string]$Wave = "Final",
    # Runtime evidence alone never closes C24, wave D, or Final.
    [switch]$RuntimeOnly,
    # Read-only machine inventory, also used by the independent release audit.
    [switch]$ListScenarios
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$priorCArtifactRoot = $env:FORME_CORE_C_ARTIFACT_DIR
$priorTargetDirectory = $env:CARGO_TARGET_DIR
$priorCliBinary = $env:FORME_CLI_BIN
$priorGatewayBinary = $env:FORME_GATEWAYD_BIN
$temporaryCArtifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("forme-core-c-gate-" + [Guid]::NewGuid().ToString("N"))
$temporaryReleaseRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("forme-core-release-gate-" + [Guid]::NewGuid().ToString("N"))
$gateExitCode = 1
Push-Location $repoRoot
try {
    if ($RuntimeOnly -and $Wave -cne "D") {
        throw "-RuntimeOnly is supported only with -Wave D; it cannot close D or Final"
    }

    function Invoke-Checked {
        param(
            [Parameter(Mandatory = $true)][string]$Label,
            [Parameter(Mandatory = $true)][string]$Program,
            [Parameter(Mandatory = $true)][string[]]$Arguments
        )

        Write-Host "==> $Label"
        & $Program @Arguments
        if ($LASTEXITCODE -ne 0) {
            $script:gateExitCode = if ($LASTEXITCODE -eq 2) { 2 } else { 1 }
            throw "$Label failed with exit code $LASTEXITCODE"
        }
    }

    # Runs one named test and proves it actually ran: a renamed or deleted
    # test must fail the gate instead of passing as "0 passed".
    function Invoke-Scenario {
        param([Parameter(Mandatory = $true)]$Scenario)

        $arguments = @("test", "-p", $Scenario.Package) + $Scenario.Target + @($Scenario.Test, "--", "--exact")
        if ($Scenario.ContainsKey("IncludeIgnored") -and $Scenario.IncludeIgnored) {
            $arguments += "--include-ignored"
        }
        Write-Host "==> [$($Scenario.Id)] $($Scenario.Package) $($Scenario.Test)"
        $previous = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $output = @(& cargo @arguments 2>&1 | ForEach-Object { "$_" })
            $exitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $previous
        }
        $output | ForEach-Object { Write-Host $_ }
        if ($exitCode -ne 0) {
            throw "[$($Scenario.Id)] failed with exit code $exitCode"
        }
        $ran = @($output | Where-Object { $_ -match "^test result: ok\. 1 passed; 0 failed" })
        if ($ran.Count -ne 1) {
            throw "[$($Scenario.Id)] did not run exactly one named test: $($Scenario.Test)"
        }
        Write-Host "[$($Scenario.Id)] PASS"
    }

    $harnessA = @("--test", "v1_core_brain_a")
    $harnessRuns = @("--test", "v1_core_brain_a_runs")
    $waveScenarios = @{
        "A" = @(
            @{ Id = "C1-composition"; Package = "forme-harness"; Target = $harnessA; Test = "production_builder_composes_one_non_degraded_brain_and_formal_context" },
            @{ Id = "C1-single-writer"; Package = "forme-harness"; Target = $harnessA; Test = "production_builder_fences_wrong_owner_and_second_live_writer" },
            @{ Id = "C1-lease-heartbeat"; Package = "forme-harness"; Target = $harnessA; Test = "idle_runtime_heartbeat_keeps_the_sole_writer_lease_and_drop_releases_it" },
            @{ Id = "C1-crash-unknown"; Package = "forme-harness"; Target = $harnessRuns; Test = "c1_startup_recovery_turns_a_crashed_action_into_one_unknown_without_driver_calls" },
            @{ Id = "C1-store-authority"; Package = "forme-store"; Target = @("--test", "v1_brain_authority"); Test = "wrong_owner_and_live_second_writer_fail_closed_without_mutation" },
            @{ Id = "C1-store-takeover"; Package = "forme-store"; Target = @("--test", "v1_brain_authority"); Test = "installed_guard_renews_the_exact_writer_and_fences_a_later_takeover" },
            @{ Id = "C2-restart-refs"; Package = "forme-harness"; Target = $harnessA; Test = "production_brain_authority_and_manifest_survive_restart_without_synthetic_facts" },
            @{ Id = "C2-brain-continuity"; Package = "forme-harness"; Target = $harnessRuns; Test = "c2_owner_brain_state_continues_across_restart_without_synthetic_facts" },
            @{ Id = "C2-command-idempotency"; Package = "forme-harness"; Target = $harnessRuns; Test = "c2_run_command_idempotency_is_durable_concurrent_and_conflict_safe" },
            @{ Id = "C2-store-deterministic"; Package = "forme-store"; Target = @("--test", "v1_brain_authority"); Test = "authority_binding_is_persistent_deterministic_and_event_free" },
            @{ Id = "C2-store-legacy"; Package = "forme-store"; Target = @("--test", "v1_brain_authority"); Test = "legacy_store_upgrade_adds_empty_authority_catalog_without_synthetic_facts" },
            @{ Id = "C2-store-domain-conflict"; Package = "forme-store"; Target = @("--test", "event_log"); Test = "independent_store_handles_reject_concurrent_conflicting_domain_claims" },
            @{ Id = "C3-ground-truth"; Package = "forme-harness"; Target = $harnessRuns; Test = "c3_formal_context_carries_history_rules_and_brain_slices_to_the_model" },
            @{ Id = "C3-history-scope"; Package = "forme-context"; Target = @("--lib"); Test = "hydration::tests::loads_only_same_session_and_workspace_history_with_original_provenance" },
            @{ Id = "C3-skill-untrusted"; Package = "forme-context"; Target = @("--lib"); Test = "hydration::tests::keeps_only_selected_skill_body_and_preserves_untrusted_slices_as_data" },
            @{ Id = "C3-slice-conflict"; Package = "forme-context"; Target = @("--lib"); Test = "hydration::tests::identical_slice_ids_deduplicate_but_conflicts_fail_closed" },
            @{ Id = "C3-empty-fails-closed"; Package = "forme-context"; Target = @("--lib"); Test = "hydration::tests::empty_formal_context_fails_closed_instead_of_calling_a_model" },
            @{ Id = "C3-bounded-recall"; Package = "forme-context"; Target = @("--lib"); Test = "hydration::tests::brain_recall_is_bounded_newest_first_scoped_and_keeps_untrusted_data" },
            @{ Id = "C4-formal-providers"; Package = "forme-harness"; Target = $harnessA; Test = "formal_environment_registers_owner_configured_providers_through_one_lifecycle" },
            @{ Id = "C4-document-fails-closed"; Package = "forme-harness"; Target = $harnessA; Test = "formal_capability_document_fails_closed_without_echoing_private_paths" },
            @{ Id = "C4-owner-binding"; Package = "forme-harness"; Target = $harnessA; Test = "providers_without_owner_binding_or_environment_stay_invisible" },
            @{ Id = "C4-optional-backends"; Package = "forme-harness"; Target = $harnessA; Test = "optional_backends_degrade_without_entering_the_visible_toolset" },
            @{ Id = "C4-app-api-unavailable"; Package = "forme-harness"; Target = $harnessA; Test = "app_api_missing_credential_or_schema_is_not_exposed" },
            @{ Id = "C4-plugin-catalog"; Package = "forme-harness"; Target = $harnessA; Test = "plugin_skill_is_dynamic_but_unbound_plugin_tool_is_never_executable" },
            @{ Id = "C4-mcp-allowlist"; Package = "forme-harness"; Target = $harnessA; Test = "production_mcp_catalog_exposes_only_allowlisted_discovery" },
            @{ Id = "C4-app-api-routing"; Package = "forme-harness"; Target = $harnessA; Test = "multiple_app_api_connectors_route_by_plan_and_public_artifacts_hide_secrets" },
            @{ Id = "C4-budget-mismatch"; Package = "forme-harness"; Target = $harnessA; Test = "provider_budget_mismatch_is_explicit_and_never_enters_the_toolset" },
            @{ Id = "C4-child-app-api"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "child_route_never_inherits_app_api_connector_authority" },
            @{ Id = "C5-scheduler-pass"; Package = "forme-harness"; Target = $harnessRuns; Test = "c5_scheduler_pass_runs_through_the_same_runtime_as_owner_turns" },
            @{ Id = "C5-cli-gateway"; Package = "forme-cli"; Target = @("--test", "cli_gateway_entry"); Test = "cli_defaults_to_the_gateway_daemon_and_shares_its_single_runtime" },
            @{ Id = "C5-cli-no-gateway"; Package = "forme-cli"; Target = @("--test", "cli_gateway_entry"); Test = "cli_without_a_configured_gateway_fails_closed_with_guidance" },
            @{ Id = "C5-cli-embedded"; Package = "forme-cli"; Target = @("--test", "cli_e2e"); Test = "cli_question_reaches_http_model_and_prints_the_answer" },
            @{ Id = "C6-config-failures"; Package = "forme-harness"; Target = $harnessRuns; Test = "c6_required_configuration_fails_closed_and_optional_providers_degrade" },
            @{ Id = "C6-approval-restart"; Package = "forme-harness"; Target = $harnessA; Test = "production_runtime_approval_consumption_remains_closed_after_restart" },
            @{ Id = "C6-final-recheck"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "final_policy_floor_is_rechecked_before_driver_dispatch" },
            @{ Id = "C6-plan-drift"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "plan_drift_is_denied_before_driver_dispatch" },
            @{ Id = "C6-grant-binding"; Package = "forme-approval"; Target = @("--test", "approval_contract"); Test = "grant_binds_approver_digest_policy_schema_nonce_and_expiry" },
            @{ Id = "C6-one-shot"; Package = "forme-approval"; Target = @("--test", "approval_contract"); Test = "one_shot_is_consumed_once_and_post_approval_plan_change_is_rejected" },
            @{ Id = "C6-durable-claim"; Package = "forme-store"; Target = @("--test", "approval_consumption"); Test = "claim_is_restart_durable_and_exactly_once" },
            @{ Id = "C6-claim-conflict"; Package = "forme-store"; Target = @("--test", "approval_consumption"); Test = "same_nonce_with_changed_semantics_is_rejected_without_a_second_row" },
            @{ Id = "C6-model-registry"; Package = "forme-models"; Target = @("--lib"); Test = "registry::tests::json_config_is_bounded_closed_and_keeps_credentials_indirect" },
            @{ Id = "C6-model-incompatible"; Package = "forme-models"; Target = @("--lib"); Test = "registry::tests::tool_incompatibility_is_a_blocking_selection_error" },
            @{ Id = "C6-model-degrade"; Package = "forme-models"; Target = @("--lib"); Test = "registry::tests::optional_profile_binding_failure_degrades_but_default_fails_closed" }
        )
    }

    $bGroups = @(
        @{ Id = "C7-stable-replay"; Package = "forme-cognition"; Target = "v1_b0_stable_replay"; Tests = @(
            "promotions_replay_field_for_field_across_every_stable_family",
            "restart_keeps_reproduction_time_so_nothing_turns_stale",
            "a_target_that_is_not_the_recorded_object_writes_nothing",
            "an_owner_downgrade_replays_on_the_recorded_object",
            "a_historical_promotion_without_a_record_keeps_its_original_replay"
        ) },
        @{ Id = "C7-tick-ledger"; Package = "forme-store"; Target = "v1_b1_tick_ledger"; Tests = @(
            "a_window_is_fulfilled_once_and_duplicate_signals_never_rerun_it",
            "a_takeover_reads_the_recorded_range_and_fences_the_lost_attempt",
            "abandoned_windows_retry_and_consumption_stays_inside_the_range",
            "finishing_binds_the_recorded_range_attempt_expiry_and_schema",
            "windows_are_sequential_and_honour_the_cadence",
            "tick_claims_survive_restart_and_require_the_owner_fence",
            "the_consumed_cursor_grows_by_run_and_never_moves_back",
            "run_heads_snapshot_every_run_in_one_read"
        ) },
        @{ Id = "C7-tick-rules"; Package = "forme-cognition"; Target = "v1_b1_tick_rules"; Tests = @(
            "signals_represent_each_failure_once_and_put_unknown_outcomes_first",
            "failures_beyond_the_observation_bound_still_reach_reflection",
            "reflections_are_deterministic_low_confidence_blind_spots",
            "a_report_lists_its_phases_in_order_and_consumes_by_deferral"
        ) },
        @{ Id = "C7-tick-recovery"; Package = "forme-cognition"; Target = "v1_b1_tick_recovery"; Tests = @(
            "maintenance_recovers_after_invalidation_before_candidate_without_duplicate_facts",
            "an_old_snapshot_never_invalidates_a_newly_reproduced_object",
            "maintenance_and_owner_promotion_commit_in_one_semantic_order",
            "reflection_recovers_its_candidate_and_rejects_changed_recorded_facts"
        ) },
        @{ Id = "C7-formal-tick"; Package = "forme-harness"; Target = "v1_core_brain_b1_tick"; Tests = @(
            "c7_one_window_observes_then_reflects_then_maintains_over_a_fixed_range",
            "c7_duplicate_signals_and_restarts_never_rerun_a_window",
            "c7_a_foreground_session_defers_heavy_phases_and_keeps_their_inputs",
            "c7_a_crashed_window_retries_its_recorded_range_and_never_repeats_a_proposal",
            "c7_a_recovered_window_keeps_its_cognition_snapshot_and_judgment_time",
            "c7_an_unknown_action_outcome_is_surfaced_and_never_retried"
        ) },
        @{ Id = "C8-learning-contract"; Package = "forme-protocol"; Target = "v1_learning_contract"; Tests = @(
            "learning_additions_keep_the_legacy_payload_bytes_and_taxonomy"
        ) },
        @{ Id = "C8-learning-rules"; Package = "forme-cognition"; Target = "v1_b2_learning_rules"; Tests = @(
            "known_scoped_knowledge_closes_only_its_declared_gap",
            "legacy_payloads_omit_new_fields_and_untrusted_or_forged_learning_is_rejected"
        ) },
        @{ Id = "C8-formal-learning"; Package = "forme-harness"; Target = "v1_core_brain_b_learning"; Tests = @(
            "c8_goal_gap_questions_and_owner_answers_are_durable_candidates_only",
            "c8_rejection_and_defer_survive_restart_and_never_repeat_the_origin",
            "c8_low_value_quiet_unrelated_and_unauthenticated_learning_are_zero_emit_or_zero_write",
            "c8_map_answers_stay_candidates_and_pending_questions_use_formal_scoped_context"
        ) },
        @{ Id = "C9-attention"; Package = "forme-cognition"; Target = "v1_b3_attention"; Tests = @(
            "c9_owner_projection_filters_before_capacity_and_is_order_independent",
            "c9_attention_digest_binds_full_cursor_time_config_and_rejects_conflicting_positions",
            "c9_all_six_item_kinds_remove_terminal_work_and_exclude_future_intentions"
        ) },
        @{ Id = "C9-formal-context"; Package = "forme-harness"; Target = "v1_core_brain_b_workspace"; Tests = @(
            "c9_owner_attention_and_formal_context_rebuild_across_workspace_restart"
        ) },
        @{ Id = "C8-C9-gateway-restart"; Package = "forme-gateway"; Target = "gatewayd_e2e"; Tests = @(
            "c8_c9_learning_http_control_is_authenticated_scoped_durable_and_candidate_only"
        ) },
        @{ Id = "C10-activation"; Package = "forme-memory"; Target = "v1_b4_activation"; Tests = @(
            "c10_five_shapes_converge_decay_and_obey_all_activation_bounds",
            "c10_no_seed_has_no_snapshot_and_untrusted_or_out_of_scope_cannot_spread",
            "c10_typed_maintenance_cas_replays_every_delta_and_rejects_poison"
        ) },
        @{ Id = "C10-C11-formal-relevance"; Package = "forme-harness"; Target = "v1_core_brain_b_activation"; Tests = @(
            "c10_formal_tick_uses_explicit_bounded_diffusion_and_restart_does_not_self_trigger",
            "c11_common_snapshot_provider_baseline_records_honest_metrics_and_negative_result",
            "c11_formal_tick_can_use_direct_model_without_diffusion_or_hidden_profile"
        ) },
        @{ Id = "C12-maintenance"; Package = "forme-harness"; Target = "v1_core_brain_b_maintenance"; Tests = @(
            "c12_formal_maintenance_covers_age_unseen_conflict_pruning_and_transitive_replay",
            "c12_old_graph_cannot_decay_new_knowledge_and_window_receipts_are_idempotent",
            "c12_healthy_prefix_cannot_hide_later_maintenance_issues"
        ) }
    )
    $waveScenarios["B"] = @(
        foreach ($group in $bGroups) {
            foreach ($test in $group.Tests) {
                @{ Id = $group.Id; Package = $group.Package; Target = @("--test", $group.Target); Test = $test }
            }
        }
    )

    $cGroups = @(
        @{ Id = "C13-charter"; Package = "forme-harness"; Target = "v1_core_brain_c_charter"; Tests = @(
            "c13_charter_is_owner_bound_atomic_versioned_replayable_and_in_formal_context",
            "c13_concurrent_charter_cas_has_one_winner_and_identical_commands_are_durable"
        ) },
        @{ Id = "C13-C18-formal-runtime"; Package = "forme-harness"; Target = "v1_core_brain_c_orchestration"; Tests = @(
            "c13_changed_charter_overrides_a_waiting_child_approval_before_the_driver",
            "c13_charter_references_and_live_resource_tightening_fail_without_admission",
            "c13_direct_project_cannot_borrow_the_owner_brains_unassigned_file_capability",
            "c14_all_eight_patterns_reach_the_formal_runtime_with_correct_child_counts",
            "c15_clarification_blocks_admission_and_planning_retries_keep_the_original_segment",
            "c15_unmatched_signature_executes_a_low_confidence_custom_review_route_without_activation",
            "c16_checkpoint_is_required_and_cancellation_cannot_resurrect_failed_nodes",
            "c16_concurrent_execute_has_one_driver_and_live_parent_cancel_reaches_children",
            "c16_crashed_child_action_is_unknown_even_when_startup_restores_a_waiting_handle",
            "c16_dependency_failure_modes_and_retry_limits_fold_without_partial_state",
            "c16_failure_retry_requires_new_intent_and_only_reclaims_unused_units",
            "c16_formal_dag_runs_in_distinct_real_directories_with_dependency_context",
            "c16_formal_direct_route_is_idempotent_verified_and_rebuilds_after_restart",
            "c16_parent_cancel_cascades_pending_work_and_preserves_real_conflicts",
            "c16_parent_file_targets_and_cross_directory_aliases_cannot_escape_child_roots",
            "c16_real_child_file_tools_wait_resume_and_keep_sibling_directories_isolated",
            "c16_store_cas_reserves_parent_budget_once_and_rejects_forged_success",
            "c16_unknown_claim_survives_restart_holds_budget_and_never_calls_a_driver_again",
            "c16_waiting_child_approval_survives_restart_without_repeating_model_or_effect",
            "c17_late_evidence_stays_out_of_the_current_review_baseline_and_is_seen_next_time",
            "c17_no_change_cadence_deadline_and_failed_outcome_use_the_checkpoint_snapshot",
            "c17_tick_plans_next_segment_after_checkpoint_and_never_executes_or_hot_swaps",
            "c18_success_and_failure_form_episodes_lineage_and_only_strategy_candidates"
        ) },
        @{ Id = "C14-C15-library"; Package = "forme-coordination"; Target = "coordination_contract"; Tests = @(
            "c14_eight_seed_patterns_have_operating_guides_and_executable_topologies",
            "c15_signature_scores_are_explainable_hard_filtered_and_custom_is_conservative"
        ) },
        @{ Id = "C16-protocol"; Package = "forme-protocol"; Target = "v1_orchestration_contract"; Tests = @(
            "c16_optional_orchestration_keeps_legacy_decision_bytes_and_taxonomy",
            "c16_orchestration_digest_schema_and_bounds_fail_closed"
        ) },
        @{ Id = "C13-C16-gateway"; Package = "forme-gateway"; Target = "gatewayd_e2e"; Tests = @(
            "c13_c16_projects_http_control_is_owner_authenticated_bounded_and_restart_durable"
        ) },
        @{ Id = "C13-C18-portable-evidence"; Package = "forme-eval"; Target = "v1_core_brain_c_artifacts"; Tests = @(
            "c13_c18_six_portable_cases_replay_the_online_ledger_and_keep_actual_profiles",
            "c16_portable_witness_gaps_missing_payloads_digest_and_false_budget_are_rejected",
            "c16_portable_resealed_secret_private_path_model_and_schema_tampering_fail_closed",
            "c16_portable_cli_verifies_a_real_file_and_rejects_unknown_fields"
        ) }
    )
    $waveScenarios["C"] = @(
        foreach ($group in $cGroups) {
            foreach ($test in $group.Tests) {
                @{ Id = $group.Id; Package = $group.Package; Target = @("--test", $group.Target); Test = $test }
            }
        }
        @{ Id = "C16-final-output"; Package = "forme-harness"; Target = @("--lib"); Test = "brain_orchestration::tests::c16_final_verification_ignores_old_answers_and_unfinished_calls" }
    )

    $dGroups = @(
        @{ Id = "C19-pinned-adaptation"; Package = "forme-harness"; Target = @("--test", "m3_b_harness_contract"); Tests = @(
            "c19_selected_provider_cannot_inherit_another_models_measured_adaptation"
        ) },
        @{ Id = "C19-C23-formal-product"; Package = "forme-harness"; Target = @("--test", "v1_core_brain_d_product"); Tests = @(
            "c19_actual_attempts_bind_distinct_providers_and_only_explicit_rejections_fallback",
            "c19_unknown_timeout_and_incompatible_profiles_never_switch_models",
            "c20_dashboard_is_workspace_scoped_replayable_and_counts_actual_fallbacks",
            "c20_learning_scope_window_and_backlog_follow_the_original_proposal",
            "c21_owner_restore_binds_exact_backup_plan_and_one_shot_without_switching_authority",
            "c23_real_runtime_erases_derived_graph_and_cognition_evicts_cached_output_and_restarts",
            "c23_pending_cleanup_fences_foreground_ticks_and_cached_reads_until_resume"
        ) },
        @{ Id = "C22-C23-data-protection"; Package = "forme-store"; Target = @("--test", "v1_data_protection"); Tests = @(
            "c22_events_projections_idempotency_backup_and_restart_contain_no_plaintext",
            "c22_missing_resolver_wrong_owner_and_historical_plaintext_fail_closed",
            "c22_protected_sync_bootstrap_reopens_with_bound_metadata_and_no_plaintext",
            "c23_scoped_cleanup_removes_real_content_caches_keys_and_survives_replay",
            "c23_wrong_owner_scope_stale_and_changed_plan_have_zero_changes",
            "c23_interrupted_key_destruction_has_no_success_and_resumes_its_immutable_plan",
            "c23_retain_keeps_content_and_predeletion_backup_cannot_restore_destroyed_keys",
            "c23_prefixed_evidence_and_transitive_aggregate_suffix_are_all_invalidated"
        ) },
        @{ Id = "C22-legacy-migration"; Package = "forme-store"; Target = @("--test", "v1_legacy_migration"); Tests = @(
            "c22_legacy_migration_encrypts_real_store_and_free_pages_and_restarts",
            "c22_migration_requires_owner_source_authority_and_exact_approved_plan",
            "c22_migration_rechecks_source_after_approval_and_never_reuses_a_target",
            "c22_migration_rejects_live_source_lease_and_target_sidecars",
            "c22_migration_fails_closed_for_unknown_schema_and_unhandled_content",
            "c22_failed_migration_retains_source_and_incomplete_target_then_recovers_to_new_path",
            "c22_migrated_ciphertext_still_requires_the_real_owner_key_and_authentication",
            "c22_migration_covers_every_auxiliary_blob_with_row_binding_history_and_lineage",
            "c22_legacy_migration_checks_real_replay_and_checksums_before_any_target_write",
            "c22_legacy_migration_encrypts_strategy_catalog_and_distinct_activation_history_bindings"
        ) },
        @{ Id = "C22-migration-operator"; Package = "forme-store"; Target = @("--test", "v1_data_migrate_operator"); Tests = @(
            "c22_operator_plan_apply_preserves_source_and_restarts_protected_store",
            "c22_operator_requires_environment_owner_bound_to_source_authority",
            "c22_operator_rejects_changed_digest_bindings_expiry_and_stale_source_without_writes",
            "c22_operator_rejects_rebound_paths_and_existing_target_or_key_vault",
            "c22_operator_plan_input_is_required_bounded_and_strict",
            "c22_operator_requires_cold_source_and_stopped_authority",
            "c22_operator_apply_is_one_shot_and_does_not_overwrite_completed_target"
        ) },
        @{ Id = "C24-closed-evidence"; Package = "forme-eval"; Target = @("--test", "v1_core_brain_d_artifacts"); Tests = @(
            "c24_closed_six_cases_and_unavailable_never_claim_live_success",
            "c24_full_stream_provider_usage_and_workflow_evidence_cannot_be_replaced_by_text",
            "c24_resealed_private_data_and_unknown_schema_are_rejected",
            "c24_offline_verifier_requires_live_pass_and_rejects_unknown_fields"
        ) },
        @{ Id = "C20-C23-gateway-data"; Package = "forme-gateway"; Target = @("--test", "v1_data_controls"); Tests = @(
            "c21_c23_every_data_route_requires_owner_bearer_origin_and_csrf_without_mutation",
            "c21_exact_restore_verifies_real_files_audit_and_durable_one_shot_without_authority_switch",
            "c23_exact_lifecycle_shreds_real_subject_keys_preserves_other_subject_and_replays_receipt",
            "c21_restore_rejects_forged_nonce_digest_owner_expiry_and_recomputed_unissued_plan",
            "c23_lifecycle_rejects_scope_subject_object_version_and_confirmation_forgery",
            "c23_stale_lifecycle_version_is_zero_write_and_fresh_exact_plan_can_proceed",
            "c23_subject_growth_after_preview_requires_new_cleanup_before_consuming_approval",
            "c21_c23_data_dtos_reject_unknown_fields_schema_spoofing_and_cross_operation_plans",
            "c21_opaque_paths_existing_files_and_stale_targets_reject_without_writes",
            "c21_tampered_backup_bytes_manifest_and_changed_source_block_restore_without_mutation",
            "c20_owner_dashboard_filters_workspace_time_and_replays_identically_after_restart"
        ) },
        @{ Id = "C22-C23-content"; Package = "forme-store"; Target = @("--lib"); Tests = @(
            "content::tests::encrypt_decrypt_binds_owner_scope_object_and_schema",
            "content::tests::wrong_key_missing_key_and_tamper_fail_closed",
            "content::tests::destroying_key_makes_existing_envelope_unreadable",
            "content::tests::rebinding_and_recomputing_public_digest_cannot_forge_authentication"
        ) },
        @{ Id = "C21-backup"; Package = "forme-store"; Target = @("--lib"); Tests = @(
            "backup::tests::cold_backup_restore_and_tamper_verification_are_fail_closed"
        ) },
        @{ Id = "C22-at-rest"; Package = "forme-store"; Target = @("--lib"); Tests = @(
            "at_rest::auxiliary_tests::c22_all_auxiliary_blob_writers_authenticate_insert_update_and_row_binding"
        ) },
        @{ Id = "C19-model-selection"; Package = "forme-models"; Target = @("--lib"); Tests = @(
            "registry::tests::registry_rejects_duplicates_and_selects_deterministically",
            "registry::tests::tool_incompatibility_is_a_blocking_selection_error",
            "registry::tests::fallback_is_explicit_group_bounded_and_disabled_for_high_risk",
            "registry::tests::json_config_is_bounded_closed_and_keeps_credentials_indirect",
            "registry::tests::optional_profile_binding_failure_degrades_but_default_fails_closed",
            "tests::endpoint_error_text_cannot_forge_retryability_or_escape_to_diagnostics"
        ) },
        @{ Id = "C20-dashboard"; Package = "forme-eval"; Target = @("--lib"); Tests = @(
            "dashboard::tests::projection_is_replayable_and_marks_unverifiable_separately",
            "dashboard::tests::model_window_uses_start_cohort_and_prior_profiles_without_future_finishes"
        ) },
        @{ Id = "C22-C23-protected-artifacts"; Package = "forme-harness"; Target = @("--lib"); Tests = @(
            "production::protected_artifact_tests::c22_artifact_driver_encrypts_bytes_and_c23_source_cleanup_revokes_reads"
        ) },
        @{ Id = "C24-runner"; Package = "forme-harness"; Target = @("--bin", "forme-core-brain-d-golden"); Tests = @(
            "product_golden_runner::tests::six_workflows_preserve_full_witnesses_files_restart_learning_and_failed_route",
            "product_golden_runner::tests::failed_calls_keep_actual_profile_and_unknown_cost_without_fake_events",
            "product_golden_runner::tests::wrong_model_answer_and_private_response_never_become_success",
            "product_golden_runner::tests::live_opt_in_and_bounded_repeats_are_required",
            "product_golden_runner::tests::production_classifier_never_upgrades_controlled_endpoints_but_allows_private_providers"
        ) },
        @{ Id = "C24-runner-cli"; Package = "forme-harness"; Target = @("--test", "v1_core_brain_d_runner"); Tests = @(
            "runner_missing_model_credentials_emits_unavailable_exit2_and_refuses_overwrite"
        ) }
    )
    $waveScenarios["D"] = @(
        foreach ($group in $dGroups) {
            foreach ($test in $group.Tests) {
                @{ Id = $group.Id; Package = $group.Package; Target = $group.Target; Test = $test }
            }
        }
        @{ Id = "C20-C23-cli-data"; Package = "forme-gateway"; Target = @("--test", "v1_data_controls"); Test = "c20_c21_c23_cli_data_and_brain_use_authenticated_real_daemon_without_local_writer"; IncludeIgnored = $true }
    )

    $requestedWaves = if ($Wave -ceq "Final") { @("A", "B", "C", "D") } else { @($Wave) }
    foreach ($requested in $requestedWaves) {
        if (-not $waveScenarios.ContainsKey($requested)) {
            throw "V1 closure wave $requested has no implemented scenario inventory; the gate fails closed"
        }
    }

    if ($ListScenarios) {
        $inventory = @(
            foreach ($requested in $requestedWaves) {
                foreach ($scenario in $waveScenarios[$requested]) {
                    [ordered]@{
                        wave = $requested
                        id = $scenario.Id
                        package = $scenario.Package
                        target = @($scenario.Target)
                        test = $scenario.Test
                        include_ignored = ($scenario.ContainsKey("IncludeIgnored") -and [bool]$scenario.IncludeIgnored)
                    }
                }
            }
        )
        ConvertTo-Json -InputObject ([ordered]@{ schema_version = 1; scenarios = $inventory }) -Depth 6
        return
    }

    $localTarget = Join-Path $repoRoot "target"
    if (($Wave -ceq "Final" -or ($Wave -ceq "D" -and -not $RuntimeOnly)) -and
        -not [string]::IsNullOrEmpty($priorTargetDirectory) -and
        [System.IO.Path]::GetFullPath($priorTargetDirectory) -ine [System.IO.Path]::GetFullPath($localTarget)) {
        throw "release gates reject shared CARGO_TARGET_DIR; use this checkout's target/debug"
    }
    if (-not [string]::IsNullOrEmpty($env:CARGO_BUILD_TARGET)) {
        throw "closure gates require native checkout-local target/debug, without CARGO_BUILD_TARGET"
    }
    $env:CARGO_TARGET_DIR = $localTarget

    if ($Wave -ceq "Final") {
        $status = @(& git status --porcelain=v1 --untracked-files=all)
        if ($LASTEXITCODE -ne 0 -or $status.Count -ne 0) {
            throw "Final requires a clean tracked, staged, and untracked release tree"
        }
        Invoke-Checked "Final workspace formatting" "cargo" @("fmt", "--all", "--", "--check")
        Invoke-Checked "Final workspace check" "cargo" @("check", "--workspace", "--all-targets")
        Invoke-Checked "Final complete workspace tests" "cargo" @("test", "--workspace", "--all-targets")
        Invoke-Checked "Final strict Clippy" "cargo" @("clippy", "--workspace", "--all-targets", "--", "-D", "warnings")
        Invoke-Checked "Final originality and compliance" "bash" @("tools/compliance-doctor.sh")
        Invoke-Checked "Permanent S1-S99 regression" "powershell" @(
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $PSScriptRoot "verify-m5.ps1")
        )
    }

    Invoke-Checked -Label "V1 closure workspace contract" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-core-brain-workspace-contract.ps1")
    )
    # The CLI entry scenarios drive the real daemon binary.
    Invoke-Checked -Label "Build the local gateway daemon" -Program "cargo" -Arguments @(
        "build", "-p", "forme-gateway", "--bin", "forme-gatewayd"
    )
    Invoke-Checked -Label "Build the local CLI for explicit ignored entry tests" -Program "cargo" -Arguments @(
        "build", "-p", "forme-cli", "--bin", "forme-cli"
    )
    $env:FORME_CLI_BIN = (Resolve-Path (Join-Path $localTarget "debug/forme-cli.exe")).Path
    $env:FORME_GATEWAYD_BIN = (Resolve-Path (Join-Path $localTarget "debug/forme-gatewayd.exe")).Path
    foreach ($requested in $requestedWaves) {
        if ($requested -ceq "C") {
            $env:FORME_CORE_C_ARTIFACT_DIR = $temporaryCArtifactRoot
        }
        foreach ($scenario in $waveScenarios[$requested]) {
            Invoke-Scenario $scenario
        }
        if ($requested -ceq "C") {
            foreach ($name in @("patterns", "conflict", "recovery", "isolation", "supervision", "learning")) {
                foreach ($root in @($temporaryCArtifactRoot, (Join-Path $repoRoot "docs/acceptance/core-brain-c-artifacts"))) {
                    Invoke-Checked -Label "C13-C18 $name artifact independent verification" -Program "cargo" -Arguments @(
                        "run", "-p", "forme-eval", "--bin", "forme-core-brain-c-artifact-verify", "--", (Join-Path $root "$name.json")
                    )
                }
            }
        }
        if ($requested -ceq "B") {
            Invoke-Checked -Label "C11 repository relevance artifact independent verification" -Program "cargo" -Arguments @(
                "run", "-p", "forme-eval", "--bin", "forme-core-brain-b-artifact-verify", "--",
                "docs/acceptance/core-brain-b-artifacts/relevance-comparison.json"
            )
        }
        if ($requested -ceq "D") {
            if ($RuntimeOnly) {
                Write-Host "[PASS] core-brain-wave-D-runtime (C24 live evidence not evaluated; D/Final remain open)"
                continue
            }
            Invoke-Checked -Label "C24 six-task live evidence and source commit (--require-pass)" -Program "py" -Arguments @(
                "-3", (Join-Path $PSScriptRoot "core_brain_release_audit.py"), "--root", $repoRoot,
                "--verify-product", (Join-Path $repoRoot "docs/acceptance/core-brain-d-artifacts/product-golden.json")
            )
        }
        Write-Host "[PASS] core-brain-wave-$requested"
    }
    if ($RuntimeOnly) {
        Write-Host "core-brain-runtime (D): PASS (runtime only; not wave D or Final acceptance)"
        return
    }
    if ($Wave -ceq "Final") {
        $auditScript = Join-Path $PSScriptRoot "core_brain_release_audit.py"
        $repositoryReleaseRoot = Join-Path $repoRoot "docs/acceptance/core-brain-closure-release-audit-artifacts"
        Invoke-Checked "Current clean-tree core-brain release audit" "py" @(
            "-3", $auditScript, "--root", $repoRoot, "--output", $temporaryReleaseRoot
        )
        Invoke-Checked "Generated core-brain release receipt" "py" @("-3", $auditScript, "--verify", $temporaryReleaseRoot)
        Invoke-Checked "Repository core-brain release receipt" "py" @("-3", $auditScript, "--verify", $repositoryReleaseRoot)
        Invoke-Checked "Core-brain release receipt matches current clean tree" "py" @(
            "-3", $auditScript, "--compare", $temporaryReleaseRoot, $repositoryReleaseRoot
        )
    }
    Write-Host "core-brain-closure ($Wave): PASS"
}
catch {
    Write-Host "core-brain-closure ($Wave): FAIL CLOSED: $($_.Exception.Message)"
    exit $gateExitCode
}
finally {
    foreach ($entry in @(
        @{ Name = "FORME_CLI_BIN"; Previous = $priorCliBinary },
        @{ Name = "FORME_GATEWAYD_BIN"; Previous = $priorGatewayBinary }
    )) {
        if ($null -eq $entry.Previous) {
            Remove-Item -LiteralPath ("Env:" + $entry.Name) -ErrorAction SilentlyContinue
        } else {
            Set-Item -LiteralPath ("Env:" + $entry.Name) -Value $entry.Previous
        }
    }
    if ($null -eq $priorTargetDirectory) {
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    } else {
        $env:CARGO_TARGET_DIR = $priorTargetDirectory
    }
    if ($null -eq $priorCArtifactRoot) {
        Remove-Item Env:FORME_CORE_C_ARTIFACT_DIR -ErrorAction SilentlyContinue
    } else {
        $env:FORME_CORE_C_ARTIFACT_DIR = $priorCArtifactRoot
    }
    $tempParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    foreach ($path in @($temporaryCArtifactRoot, $temporaryReleaseRoot)) {
        $resolved = [System.IO.Path]::GetFullPath($path)
        if ($resolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $resolved)) {
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    }
    Pop-Location
}
