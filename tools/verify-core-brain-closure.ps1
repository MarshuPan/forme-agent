param(
    # A wave gate runs the implemented wave's scenarios; "Final" requires every
    # wave and is the only form that can support a closure claim.
    [ValidateSet("A", "B", "C", "D", "Final")][string]$Wave = "Final"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $repoRoot
try {
    function Invoke-Checked {
        param(
            [Parameter(Mandatory = $true)][string]$Label,
            [Parameter(Mandatory = $true)][string]$Program,
            [Parameter(Mandatory = $true)][string[]]$Arguments
        )

        Write-Host "==> $Label"
        & $Program @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Label failed with exit code $LASTEXITCODE"
        }
    }

    # Runs one named test and proves it actually ran: a renamed or deleted
    # test must fail the gate instead of passing as "0 passed".
    function Invoke-Scenario {
        param([Parameter(Mandatory = $true)]$Scenario)

        $arguments = @("test", "-p", $Scenario.Package) + $Scenario.Target + @($Scenario.Test, "--", "--exact")
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

    $requestedWaves = if ($Wave -ceq "Final") { @("A", "B", "C", "D") } else { @($Wave) }
    foreach ($requested in $requestedWaves) {
        if (-not $waveScenarios.ContainsKey($requested)) {
            throw "V1 closure wave $requested has no implemented scenario inventory; the gate fails closed"
        }
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
    foreach ($requested in $requestedWaves) {
        foreach ($scenario in $waveScenarios[$requested]) {
            Invoke-Scenario $scenario
        }
        if ($requested -ceq "B") {
            Invoke-Checked -Label "C11 repository relevance artifact independent verification" -Program "cargo" -Arguments @(
                "run", "-p", "forme-eval", "--bin", "forme-core-brain-b-artifact-verify", "--",
                "docs/acceptance/core-brain-b-artifacts/relevance-comparison.json"
            )
        }
        Write-Host "[PASS] core-brain-wave-$requested"
    }
    Write-Host "core-brain-closure ($Wave): PASS"
}
finally {
    Pop-Location
}
