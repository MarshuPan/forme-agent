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

    Invoke-Checked -Label "M1 permanent regression gate (S1-S37)" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m1.ps1")
    )

    $scenarios = @(
        @{ Id = "M2-A-protocol"; Package = "forme-protocol"; Target = @("--test", "m2_a_contract"); Test = $null },
        @{ Id = "S38"; Package = "forme-execution"; Target = @("--test", "m2_a_backend_contract"); Test = "s38_browser_backend_emits_untrusted_typed_receipt_and_binds_plan" },
        @{ Id = "S39"; Package = "forme-execution"; Target = @("--test", "m2_a_backend_contract"); Test = "s39_computer_backend_rejects_out_of_bounds_before_driver_effect" },
        @{ Id = "S40"; Package = "forme-execution"; Target = @("--test", "m2_a_backend_contract"); Test = "s40_pty_backend_uses_real_pty_minimal_env_and_secret_ref" },
        @{ Id = "S41-default"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s41_s42_external_floor_is_plan_bound_and_harness_stamps_untrusted" },
        @{ Id = "S41-L4"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s41_explicit_narrow_l4_requires_result_evidence_and_skips_per_action_approval" },
        @{ Id = "S41-rollback-truth"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s41_intent_cannot_claim_a_reversible_boundary_the_backend_did_not_declare" },
        @{ Id = "S41-L5"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s41_l5_rejects_session_grant_without_calling_backend" },
        @{ Id = "S41-recovery"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s41_external_started_without_terminal_recovers_unknown_and_never_retries_backend" },
        @{ Id = "S42-policy"; Package = "forme-policy"; Target = @("--test", "policy_contract"); Test = "s38_s40_external_parameter_recheck_is_fail_closed" },
        @{ Id = "S42-model-envelope"; Package = "forme-models"; Target = @("--lib"); Test = "tests::s42_untrusted_model_message_preserves_source_trust_and_data_treatment" },
        @{ Id = "S42-loop-roundtrip"; Package = "forme-loop"; Target = @("--lib"); Test = "tests::s42_untrusted_input_and_tool_result_stay_data_in_each_model_request" },
        @{ Id = "S42-source-floor"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s42_model_cannot_spoof_user_turn_source_from_untrusted_communication" },
        @{ Id = "M2-B-protocol"; Package = "forme-protocol"; Target = @("--test", "m2_b_contract"); Test = $null },
        @{ Id = "S43-lifecycle"; Package = "forme-capabilities"; Target = @("--test", "app_api_connector_contract"); Test = "s43_connector_lifecycle_indexes_only_active_identity_and_prepares_typed_actions" },
        @{ Id = "S43-revocation"; Package = "forme-capabilities"; Target = @("--test", "app_api_connector_contract"); Test = "s43_schema_drift_and_revocation_fail_the_execution_time_recheck" },
        @{ Id = "S43-policy"; Package = "forme-policy"; Target = @("--test", "m2_b_policy_contract"); Test = "s43_app_api_execution_recheck_is_total_and_fails_closed" },
        @{ Id = "S43-real"; Package = "forme-execution"; Target = @("--test", "m2_b_app_api_contract"); Test = "s43_real_project_connector_reads_and_mutates_with_untrusted_receipts" },
        @{ Id = "S43-unknown"; Package = "forme-execution"; Target = @("--test", "m2_b_app_api_contract"); Test = "s43_rate_limit_and_unknown_mutation_never_repeat_the_side_effect" },
        @{ Id = "S44-bounds"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s44_real_delivery_reserves_bounded_session_budget_before_transport" },
        @{ Id = "S44-owner-grant"; Package = "forme-gateway"; Target = @("--lib"); Test = "tests::s44_external_session_requires_owner_auth_and_records_owner_grant_provenance" },
        @{ Id = "S44-S45-real"; Package = "forme-gateway"; Target = @("--test", "m2_b_communication_golden"); Test = "s44_s45_real_loopback_delivery_stays_inside_disclosure_and_harness_governance" },
        @{ Id = "S45-representation"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s45_disclosure_and_representation_are_bound_before_delivery_intent" },
        @{ Id = "S45-one-shot"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s45_disclosure_request_ref_is_one_shot_within_a_session" },
        @{ Id = "S45-harness"; Package = "forme-harness"; Target = @("--test", "m2_b_disclosure_contract"); Test = "s45_harness_requires_exact_allowed_disclosure_bound_into_the_action_plan" },
        @{ Id = "S46-adapter"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s46_device_observation_enforces_identity_foreground_retention_and_revocation" },
        @{ Id = "S46-gateway"; Package = "forme-gateway"; Target = @("--lib"); Test = "tests::s46_gateway_stamps_device_observation_untrusted_and_persists_owner_revocation" },
        @{ Id = "S46-store-fts"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s46_no_raw_device_text_reaches_events_transcript_or_fts" },
        @{ Id = "S47-session"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s47_external_agent_session_uses_the_external_membrane_and_hard_termination" },
        @{ Id = "S47-gateway"; Package = "forme-gateway"; Target = @("--lib"); Test = "tests::s47_gateway_requires_mutual_channel_and_stamps_external_agent_untrusted" },
        @{ Id = "M2-C-protocol"; Package = "forme-protocol"; Target = @("--test", "m2_c_contract"); Test = $null },
        @{ Id = "S48"; Package = "forme-coordination"; Target = @("--test", "m2_c_contract"); Test = "s48_resource_graph_is_event_derived_deterministic_and_never_authorizes_by_score" },
        @{ Id = "S49"; Package = "forme-memory"; Target = @("--test", "m2_c_contract"); Test = "s49_long_term_goal_lineage_rebuilds_and_yields_replans_or_stops_before_action" },
        @{ Id = "S50"; Package = "forme-memory"; Target = @("--test", "m2_c_contract"); Test = "s50_capability_growth_is_result_led_candidate_only_and_owner_grants_narrowly" },
        @{ Id = "S51"; Package = "forme-capabilities"; Target = @("--test", "managed_plugin_contract"); Test = "s51_managed_plugin_policy_verifies_then_switches_atomically_and_revokes_without_ghosts" },
        @{ Id = "S52-memory"; Package = "forme-memory"; Target = @("--test", "m2_c_contract"); Test = "s52_hot_cold_projection_is_rebuildable_scoped_retained_and_secret_free" },
        @{ Id = "S52-CAS"; Package = "forme-store"; Target = @("--test", "m2_c_sync_contract"); Test = "s52_expected_append_uses_atomic_compare_and_zero_write_conflicts" },
        @{ Id = "S52-batch"; Package = "forme-store"; Target = @("--test", "m2_c_sync_contract"); Test = "s52_sync_batch_is_atomic_idempotent_single_peer_and_cursor_bound" },
        @{ Id = "S52-peer"; Package = "forme-store"; Target = @("--test", "m2_c_sync_contract"); Test = "s52_sync_peer_must_be_preconfigured_and_cannot_be_claimed_by_first_request" },
        @{ Id = "S52-export"; Package = "forme-store"; Target = @("--test", "m2_c_sync_contract"); Test = "s52_export_redacts_secrets_rejects_redacted_import_and_resumes_from_cursor" },
        @{ Id = "M2-C-doctor"; Package = "forme-config"; Target = @("--lib"); Test = "tests::doctor_checks_every_required_matrix_row_with_explanations" }
    )

    foreach ($scenario in $scenarios) {
        $arguments = @("test", "-p", $scenario.Package) + $scenario.Target
        if ($null -ne $scenario.Test) {
            $arguments += @($scenario.Test, "--", "--exact")
        }
        Invoke-Checked -Label "$($scenario.Id) scenario contract" -Program "cargo" -Arguments $arguments
        Write-Host "[$($scenario.Id)] PASS"
    }

    Invoke-Checked -Label "S38 real Chrome/Edge loopback mutation golden" -Program "cargo" -Arguments @(
        "test",
        "-p",
        "forme-harness",
        "--test",
        "m2_a_browser_golden",
        "s38_real_browser_golden_runs_harness_approval_and_observes_server_mutation",
        "--",
        "--ignored",
        "--exact"
    )
    Invoke-Checked -Label "M2 crate graph and exact dependency pins" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m2-workspace-contract.ps1")
    )
    Invoke-Checked -Label "M2-A offline artifact gate" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m2-a-artifacts.ps1")
    )
    Invoke-Checked -Label "M2 strict Clippy" -Program "cargo" -Arguments @(
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings"
    )
    Write-Host "M2 FINAL ACCEPTANCE: PASS (S1-S52 + real browser/API golden + artifacts + strict clippy)"
}
finally {
    Pop-Location
}
