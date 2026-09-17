param(
    # Hosted CI has no local reference corpus; copy detection is reported as SKIP instead of PASS.
    [switch]$AllowMissingCorpus
)

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
        @{ Id = "S1"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s1_final_run_is_event_sourced_and_idempotent" },
        @{ Id = "S2"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s2_denied_approval_suspends_then_aborts_without_execution" },
        @{ Id = "S3"; Package = "forme-capabilities"; Target = @("--test", "mcp_contract"); Test = "s3_stdio_discovery_allowlist_prepare_call_and_disable_are_governed" },
        @{ Id = "S4"; Package = "forme-capabilities"; Target = @("--test", "capability_contract"); Test = "s4_skill_metadata_is_default_and_only_selected_body_is_loaded" },
        @{ Id = "S5"; Package = "forme-capabilities"; Target = @("--test", "plugin_contract"); Test = "s5_manifest_contributions_are_governed_and_disabled_plugins_leave_the_toolset" },
        @{ Id = "S6"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s6_coordination_events_are_real_plan_outputs_with_the_decision_workspace_snapshot" },
        @{ Id = "S7"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s7_authorized_tick_persists_the_proactive_loop_and_rejection_never_executes" },
        @{ Id = "S8"; Package = "forme-cognition"; Target = @("--test", "temporal_contract"); Test = "s8_user_model_is_temporal_candidate_first_and_history_is_bootstrap_only" },
        @{ Id = "S9"; Package = "forme-cognition"; Target = @("--test", "cognition_contract"); Test = "s9_reflection_creates_low_confidence_candidate_without_mutating_stable_map" },
        @{ Id = "S10"; Package = "forme-eval"; Target = @("--lib"); Test = "tests::s10_failure_ledger_updates_digest_and_final_pass_does_not_hide_history" },
        @{ Id = "S11"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s11_subagent_is_spawned_by_harness_with_fresh_context_and_scoped_denial" },
        @{ Id = "S12"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s12_text_adapter_only_normalizes_and_defaults_untrusted" },
        @{ Id = "S13"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s13_external_session_is_bounded_and_terminates_on_round_limit" },
        @{ Id = "S14"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s14_sensitive_disclosure_is_refused_or_blurred_and_audited" },
        @{ Id = "S15"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s15_representation_never_claims_owner_identity_and_uncertainty_needs_approval" },
        @{ Id = "S16"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s16_device_requires_active_grant_and_revocation_blocks_new_events" },
        @{ Id = "S17"; Package = "forme-communication"; Target = @("--lib"); Test = "tests::s17_agent_session_has_hard_purpose_budget_and_termination" },
        @{ Id = "S18"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s18_competence_downgrade_is_evidence_backed_in_event_and_trace" },
        @{ Id = "S19"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s19_same_session_is_serial_and_tick_cannot_derail_foreground" },
        @{ Id = "S20"; Package = "forme-cognition"; Target = @("--test", "cognition_contract"); Test = "s20_retraction_traverses_all_derived_objects_without_deleting_history" },
        @{ Id = "S21"; Package = "forme-store"; Target = @("--test", "schema_replay"); Test = "s21_read_time_upcast_and_replay_leave_authoritative_history_unchanged" }
    )

    foreach ($scenario in $scenarios) {
        $arguments = @("test", "-p", $scenario.Package) + $scenario.Target + @($scenario.Test, "--", "--exact")
        Invoke-Checked -Label "$($scenario.Id) $($scenario.Test)" -Program "cargo" -Arguments $arguments
        Write-Host "[$($scenario.Id)] PASS"
    }

    Invoke-Checked -Label "S22 compliance fixtures" -Program "py" -Arguments @("-3", "-m", "unittest", "discover", "-s", "tools/tests", "-v")
    $doctorArguments = @("tools/compliance-doctor.sh")
    if ($AllowMissingCorpus) {
        $doctorArguments += "--allow-missing-corpus"
    }
    Invoke-Checked -Label "S22 real-tree compliance doctor" -Program "bash" -Arguments $doctorArguments
    Write-Host "[S22] PASS"

    Invoke-Checked -Label "format gate" -Program "cargo" -Arguments @("fmt", "--all", "--", "--check")
    Invoke-Checked -Label "workspace check" -Program "cargo" -Arguments @("check", "--workspace")
    Invoke-Checked -Label "workspace tests" -Program "cargo" -Arguments @("test", "--workspace", "--all-targets")
    Write-Host "M0 ACCEPTANCE: PASS (S1-S22 + compliance)"
}
finally {
    Pop-Location
}
