$ErrorActionPreference = "Stop"

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

    $scenarios = @(
        @{ Id = "S23"; Package = "forme-gateway"; Target = @("--test", "gatewayd_e2e"); Test = "s23_http_web_surface_uses_real_gateway_harness_and_event_stream" },
        @{ Id = "S24"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s24_event_page_reconnect_is_contiguous_deterministic_and_read_only" },
        @{ Id = "S25"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s25_gateway_approval_is_one_shot_plan_bound_and_resumes_the_run" },
        @{ Id = "S25-cancel"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "cancel_interrupts_an_active_action_and_finishes_without_losing_state" },
        @{ Id = "S26"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s26_trace_view_resolves_failure_and_verification_without_writing_history" },
        @{ Id = "S27"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s27_candidate_review_compares_state_and_retraction_schedules_reevaluation" },
        @{ Id = "S28"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s28_manual_eval_is_repeatable_trace_bound_and_never_promotes_policy" },
        @{ Id = "S29"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s29_due_background_intention_creates_one_governed_schedule_run" },
        @{ Id = "S29-http"; Package = "forme-gateway"; Target = @("--test", "gatewayd_e2e"); Test = "jobs_api_persists_across_daemon_restart_and_cancel_prevents_a_run" },
        @{ Id = "S30-lease"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s30_restart_reclaims_safe_lease_and_duplicate_intent_never_repeats_delivery" },
        @{ Id = "S30-unknown"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s30_unknown_schedule_outcome_enters_manual_review_without_backend_retry" },
        @{ Id = "S31"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s31_foreground_precedes_schedule_and_budget_or_cancel_blocks_new_actions" },
        @{ Id = "S32-failure"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s32_failure_followup_uses_three_gates_attention_budget_and_reject_suppression" },
        @{ Id = "S32-verification"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s32_verification_fail_and_unverifiable_followups_read_result_evidence" },
        @{ Id = "S33"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s33_local_notification_is_plan_bound_and_high_risk_waits_for_approval" },
        @{ Id = "S34"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s34_automatic_compaction_preserves_lineage_and_done_contract" },
        @{ Id = "S34-lineage"; Package = "forme-context"; Target = @("--lib"); Test = "tests::s34_automatic_compaction_is_threshold_bound_and_keeps_governance_lineage" },
        @{ Id = "S35"; Package = "forme-capabilities"; Target = @("--test", "capability_contract"); Test = "s35_skill_search_is_bounded_explained_and_loads_only_the_selection" },
        @{ Id = "S36"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s36_mcp_refresh_search_schema_digest_and_execution_recheck_are_governed" },
        @{ Id = "S36-compat"; Package = "forme-protocol"; Target = @("--test", "control_contract"); Test = "m1_c_mcp_schema_digest_is_additive_and_old_parameters_remain_replayable" },
        @{ Id = "S37-plugin"; Package = "forme-capabilities"; Target = @("--test", "plugin_contract"); Test = "s37_plugin_reload_is_atomic_and_runtime_failure_is_isolated" },
        @{ Id = "S37-memory"; Package = "forme-memory"; Target = @("--lib"); Test = "tests::s37_scoped_memory_and_candidate_review_never_pollute_broader_stable_state" },
        @{ Id = "M1-C-golden"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "m1c_long_context_golden_task_passes_the_real_harness_path" },
        @{ Id = "M1-B-memory"; Package = "forme-memory"; Target = @("--lib"); Test = "tests::scheduled_intention_binding_and_lease_survive_rebuild" },
        @{ Id = "M1-B-digest"; Package = "forme-execution"; Target = @("--test", "execution_contract"); Test = "notification_target_scope_and_body_ref_are_bound_by_the_plan_digest" },
        @{ Id = "M1-C-doctor"; Package = "forme-config"; Target = @("--lib"); Test = "tests::doctor_checks_every_required_matrix_row_with_explanations" }
    )

    foreach ($scenario in $scenarios) {
        $arguments = @("test", "-p", $scenario.Package) + $scenario.Target + @($scenario.Test, "--", "--exact")
        Invoke-Checked -Label "$($scenario.Id) $($scenario.Test)" -Program "cargo" -Arguments $arguments
        Write-Host "[$($scenario.Id)] PASS"
    }

    Invoke-Checked -Label "M0 permanent regression gate" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m0.ps1")
    )
    Write-Host "M1 SCENARIO GATE: PASS (S23-S37 + S1-S22 + compliance)"
    Invoke-Checked -Label "M1 final artifact gate" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m1-final-artifacts.ps1")
    )
    Write-Host "M1 FINAL ACCEPTANCE: PASS (artifacts + S1-S37 + compliance)"
}
finally {
    Pop-Location
}
