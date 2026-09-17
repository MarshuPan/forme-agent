param(
    [switch]$RegressionOnly
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$artifactRoot = Join-Path $repoRoot "docs\acceptance\m3-a-artifacts"
$m3BArtifactRoot = Join-Path $repoRoot "docs\acceptance\m3-b-artifacts"
$m3CArtifactRoot = Join-Path $repoRoot "docs\acceptance\m3-c-artifacts"
$releaseAuditArtifactRoot = Join-Path $repoRoot "docs\acceptance\m3-release-audit-artifacts"
$temporaryArtifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m3-a-artifacts-" + [Guid]::NewGuid().ToString("N")
)
$temporaryM3BArtifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m3-b-artifacts-" + [Guid]::NewGuid().ToString("N")
)
$temporaryM3CArtifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m3-c-artifacts-" + [Guid]::NewGuid().ToString("N")
)
$temporaryReleaseAuditRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m3-release-audit-" + [Guid]::NewGuid().ToString("N")
)
$previousArtifactRoot = $env:FORME_M3_A_ARTIFACT_DIR
$previousM3BArtifactRoot = $env:FORME_M3_B_ARTIFACT_DIR
$previousM3CArtifactRoot = $env:FORME_M3_C_ARTIFACT_DIR

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

    function Assert-Condition {
        param(
            [Parameter(Mandatory = $true)][bool]$Condition,
            [Parameter(Mandatory = $true)][string]$Message
        )

        if (-not $Condition) {
            throw $Message
        }
    }

    Invoke-Checked -Label "M2 permanent regression gate (S1-S52)" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m2.ps1")
    )

    $scenarios = @(
        @{ Id = "M3-A-protocol"; Package = "forme-protocol"; Target = @("--test", "m3_a_contract"); Test = $null },
        @{ Id = "M3-A-89-kind"; Package = "forme-protocol"; Target = @("--test", "event_payload_contract"); Test = "taxonomy_matches_the_fixed_contract_snapshot" },
        @{ Id = "S53-exact"; Package = "forme-eval"; Target = @("--test", "m3_a_eval_contract"); Test = "s53_portable_exact_replay_is_deterministic_read_only_and_effect_free" },
        @{ Id = "S53-negative"; Package = "forme-eval"; Target = @("--test", "m3_a_eval_contract"); Test = "s53_portable_replay_rejects_partial_secret_and_private_path_inputs" },
        @{ Id = "S53-audit"; Package = "forme-harness"; Target = @("--test", "m3_a_harness_contract"); Test = "s53_exact_replay_writes_a_separate_effect_free_audit_run" },
        @{ Id = "S54"; Package = "forme-harness"; Target = @("--test", "m3_a_harness_contract"); Test = "s54_simulation_denies_before_backend_planning_or_execution" },
        @{ Id = "S55"; Package = "forme-eval"; Target = @("--test", "m3_a_eval_contract"); Test = "s55_ground_truth_and_hard_invariants_dominate_self_score_and_cost" },
        @{ Id = "S56-governor"; Package = "forme-cognition"; Target = @("--test", "m3_a_governor_contract"); Test = "s56_governor_keeps_promotion_owner_and_rollback_decisions_distinct" },
        @{ Id = "S56-untrusted"; Package = "forme-cognition"; Target = @("--test", "m3_a_governor_contract"); Test = "s56_untrusted_or_stale_baseline_cannot_promote" },
        @{ Id = "S56-cautious"; Package = "forme-harness"; Target = @("--test", "m3_a_harness_contract"); Test = "s56_cautious_activation_is_automatic_but_never_authorizes" },
        @{ Id = "S56-stable-gate"; Package = "forme-store"; Target = @("--test", "m3_a_evolution_contract"); Test = "s56_strategy_promotion_requires_a_recorded_passing_evaluation" },
        @{ Id = "S56-S57-harness"; Package = "forme-harness"; Target = @("--test", "m3_a_harness_contract"); Test = "s56_s57_candidate_promotion_activation_run_pinning_and_rollback_are_separate" },
        @{ Id = "S57-CAS"; Package = "forme-store"; Target = @("--test", "m3_a_evolution_contract"); Test = "s57_promotion_activation_snapshot_cas_and_rollback_remain_separate" },
        @{ Id = "S57-owner"; Package = "forme-store"; Target = @("--test", "m3_a_evolution_contract"); Test = "s57_active_control_cannot_bypass_cas_or_owner_impact_gate" },
        @{ Id = "S57-legacy"; Package = "forme-store"; Target = @("--test", "m3_a_evolution_contract"); Test = "s57_legacy_store_has_no_synthetic_active_history" },
        @{ Id = "M3-A-artifact-contract"; Package = "forme-eval"; Target = @("--test", "m3_a_eval_contract"); Test = "m3_artifacts_are_typed_content_addressed_secret_free_and_offline_verifiable" },
        @{ Id = "M3-A-gateway"; Package = "forme-gateway"; Target = @("--lib"); Test = "tests::m3_evolution_controls_require_authenticated_owner_and_expose_pause_to_cli" },
        @{ Id = "M3-A-gateway-http"; Package = "forme-gateway"; Target = @("--test", "gatewayd_e2e"); Test = "s23_http_web_surface_uses_real_gateway_harness_and_event_stream" },
        @{ Id = "M3-A-cli"; Package = "forme-cli"; Target = @("--lib"); Test = "tests::m3_evolution_cli_uses_the_owner_gated_gateway_surface" },
        @{ Id = "M3-A-doctor"; Package = "forme-config"; Target = @("--lib"); Test = "tests::doctor_checks_every_required_matrix_row_with_explanations" },
        @{ Id = "M3-B-protocol"; Package = "forme-protocol"; Target = @("--test", "m3_b_contract"); Test = $null },
        @{ Id = "S58-registry"; Package = "forme-loop"; Target = @("--test", "m3_b_loop_contract"); Test = "s58_loop_registry_is_immutable_seeded_and_only_narrows_runtime_budget" },
        @{ Id = "S58-pinning"; Package = "forme-loop"; Target = @("--test", "m3_b_loop_contract"); Test = "s58_run_binds_one_loop_version_and_cannot_hot_swap" },
        @{ Id = "S59-authority"; Package = "forme-coordination"; Target = @("--test", "m3_b_coordination_contract"); Test = "s59_coordination_strategy_selects_existing_roles_without_expanding_child_authority" },
        @{ Id = "S59-fitness"; Package = "forme-coordination"; Target = @("--test", "m3_b_coordination_contract"); Test = "s59_lower_cost_never_outvotes_incomplete_quality_or_over_delegation" },
        @{ Id = "S60-filter"; Package = "forme-capabilities"; Target = @("--test", "m3_b_selection_contract"); Test = "s60_filters_authority_and_managed_state_before_strategy_ranking" },
        @{ Id = "S60-evidence"; Package = "forme-capabilities"; Target = @("--test", "m3_b_selection_contract"); Test = "s60_provider_declaration_never_raises_the_result_evidence_ceiling" },
        @{ Id = "S60-immutable"; Package = "forme-capabilities"; Target = @("--test", "m3_b_selection_contract"); Test = "s60_selection_registry_keeps_version_content_immutable" },
        @{ Id = "S61-scaffold"; Package = "forme-models"; Target = @("--test", "m3_b_adaptation_contract"); Test = "s61_weak_and_strong_profiles_change_scaffolding_but_not_governance" },
        @{ Id = "S61-evidence"; Package = "forme-models"; Target = @("--test", "m3_b_adaptation_contract"); Test = "s61_provider_strength_cannot_replace_measured_outcome_evidence" },
        @{ Id = "S61-immutable"; Package = "forme-models"; Target = @("--test", "m3_b_adaptation_contract"); Test = "s61_model_adaptation_registry_rejects_same_version_different_digest" },
        @{ Id = "S58-S61-harness"; Package = "forme-harness"; Target = @("--test", "m3_b_harness_contract"); Test = "s58_s61_active_domain_specs_bind_one_run_and_preserve_governance_events" },
        @{ Id = "S58-S61-incompatible"; Package = "forme-harness"; Target = @("--test", "m3_b_harness_contract"); Test = "s58_s61_incompatible_active_content_fails_before_session_bound" },
        @{ Id = "S62-long-horizon"; Package = "forme-harness"; Target = @("--test", "m3_b_harness_contract"); Test = "s62_long_horizon_checkpoints_yield_to_foreground_pin_versions_and_stop_cleanly" },
        @{ Id = "S62-artifact"; Package = "forme-eval"; Target = @("--test", "m3_b_artifact_contract"); Test = "s62_long_horizon_artifact_is_content_addressed_and_offline_verifiable" },
        @{ Id = "S62-artifact-negative"; Package = "forme-eval"; Target = @("--test", "m3_b_artifact_contract"); Test = "s62_artifact_rejects_tamper_extra_entries_secrets_and_private_paths" },
        @{ Id = "M3-C-protocol"; Package = "forme-protocol"; Target = @("--test", "m3_c_contract"); Test = $null },
        @{ Id = "S63-conflict"; Package = "forme-memory"; Target = @("--test", "m3_c_strategy_memory_contract"); Test = "s63_conflict_retraction_and_decay_produce_governed_recommendations" },
        @{ Id = "S63-untrusted"; Package = "forme-memory"; Target = @("--test", "m3_c_strategy_memory_contract"); Test = "s63_untrusted_content_never_creates_lineage_or_maintenance_actions" },
        @{ Id = "S63-history"; Package = "forme-memory"; Target = @("--test", "m3_c_strategy_memory_contract"); Test = "s63_append_only_resolution_keeps_history_and_removes_active_v2" },
        @{ Id = "S63-invalid"; Package = "forme-memory"; Target = @("--test", "m3_c_strategy_memory_contract"); Test = "s63_malformed_control_payloads_and_candidate_envelopes_fail_closed" },
        @{ Id = "M3-C-registry"; Package = "forme-cognition"; Target = @("--test", "m3_c_evolution_contract"); Test = "m3_c_registry_requires_all_seeds_and_keeps_version_content_immutable" },
        @{ Id = "S64"; Package = "forme-cognition"; Target = @("--test", "m3_c_evolution_contract"); Test = "s64_agent_self_uses_result_evidence_and_self_assessment_only_lowers" },
        @{ Id = "S65"; Package = "forme-cognition"; Target = @("--test", "m3_c_evolution_contract"); Test = "s65_partnership_requires_process_timepoints_and_authenticated_owner_correction" },
        @{ Id = "S66"; Package = "forme-cognition"; Target = @("--test", "m3_c_evolution_contract"); Test = "s66_trust_success_stops_at_owner_proposal_and_failure_downgrades" },
        @{ Id = "S67-proactivity"; Package = "forme-cognition"; Target = @("--test", "m3_c_evolution_contract"); Test = "s67_proactivity_reduces_regret_without_missing_commitment_or_bypassing_attention" },
        @{ Id = "S67-communication"; Package = "forme-cognition"; Target = @("--test", "m3_c_evolution_contract"); Test = "s67_communication_strategy_never_expands_recipient_disclosure_or_outward_authority" },
        @{ Id = "S68-artifact"; Package = "forme-eval"; Target = @("--test", "m3_c_artifact_contract"); Test = "s68_repository_golden_is_content_addressed_and_offline_verifiable" },
        @{ Id = "S68-lineage-negative"; Package = "forme-eval"; Target = @("--test", "m3_c_artifact_contract"); Test = "s68_missing_phase_hidden_regression_and_final_v2_are_rejected" },
        @{ Id = "S68-portability-negative"; Package = "forme-eval"; Target = @("--test", "m3_c_artifact_contract"); Test = "s68_secret_and_private_path_are_rejected" },
        @{ Id = "S68-tamper-negative"; Package = "forme-eval"; Target = @("--test", "m3_c_artifact_contract"); Test = "s68_tamper_and_extra_artifact_are_rejected" }
    )

    foreach ($scenario in $scenarios) {
        $arguments = @("test", "-p", $scenario.Package) + $scenario.Target
        if ($null -ne $scenario.Test) {
            $arguments += @($scenario.Test, "--", "--exact")
        }
        Invoke-Checked -Label "$($scenario.Id) scenario contract" -Program "cargo" -Arguments $arguments
        Write-Host "[$($scenario.Id)] PASS"
    }

    Invoke-Checked -Label "Frozen 18-crate graph and exact dependency pins" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m2-workspace-contract.ps1")
    )

    $tempParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    $tempResolved = [System.IO.Path]::GetFullPath($temporaryArtifactRoot)
    Assert-Condition ($tempResolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase)) (
        "temporary M3 artifact root escaped the operating-system temp directory"
    )
    $env:FORME_M3_A_ARTIFACT_DIR = $tempResolved
    Invoke-Checked -Label "M3-A real browser typed artifact generation" -Program "cargo" -Arguments @(
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
    Invoke-Checked -Label "Generated M3-A artifact independent verification" -Program "cargo" -Arguments @(
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m3-artifact-verify",
        "--",
        $tempResolved
    )
    Remove-Item Env:FORME_M3_A_ARTIFACT_DIR -ErrorAction SilentlyContinue
    Invoke-Checked -Label "Repository-owned M3-A artifact independent verification" -Program "cargo" -Arguments @(
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m3-artifact-verify",
        "--",
        $artifactRoot
    )

    $temporaryM3BResolved = [System.IO.Path]::GetFullPath($temporaryM3BArtifactRoot)
    Assert-Condition ($temporaryM3BResolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase)) (
        "temporary M3-B artifact root escaped the operating-system temp directory"
    )
    $env:FORME_M3_B_ARTIFACT_DIR = $temporaryM3BResolved
    Invoke-Checked -Label "M3-B long-horizon typed artifact generation" -Program "cargo" -Arguments @(
        "test",
        "-p",
        "forme-harness",
        "--test",
        "m3_b_harness_contract",
        "s62_long_horizon_checkpoints_yield_to_foreground_pin_versions_and_stop_cleanly",
        "--",
        "--exact"
    )
    Invoke-Checked -Label "Generated M3-B artifact independent verification" -Program "cargo" -Arguments @(
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m3-b-artifact-verify",
        "--",
        $temporaryM3BResolved
    )
    Remove-Item Env:FORME_M3_B_ARTIFACT_DIR -ErrorAction SilentlyContinue
    Invoke-Checked -Label "Repository-owned M3-B artifact independent verification" -Program "cargo" -Arguments @(
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m3-b-artifact-verify",
        "--",
        $m3BArtifactRoot
    )

    $temporaryM3CResolved = [System.IO.Path]::GetFullPath($temporaryM3CArtifactRoot)
    Assert-Condition ($temporaryM3CResolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase)) (
        "temporary M3-C artifact root escaped the operating-system temp directory"
    )
    $env:FORME_M3_C_ARTIFACT_DIR = $temporaryM3CResolved
    Invoke-Checked -Label "M3-C governed real browser evolution artifact generation" -Program "cargo" -Arguments @(
        "test",
        "-p",
        "forme-harness",
        "--test",
        "m2_a_browser_golden",
        "s68_m3_candidate_live_v2_regression_rollback_and_live_v1_are_governed",
        "--",
        "--ignored",
        "--exact"
    )
    Invoke-Checked -Label "Generated M3-C artifact independent verification" -Program "cargo" -Arguments @(
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m3-c-artifact-verify",
        "--",
        $temporaryM3CResolved
    )
    Remove-Item Env:FORME_M3_C_ARTIFACT_DIR -ErrorAction SilentlyContinue
    Invoke-Checked -Label "Repository-owned M3-C artifact independent verification" -Program "cargo" -Arguments @(
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m3-c-artifact-verify",
        "--",
        $m3CArtifactRoot
    )

    Invoke-Checked -Label "M3 workspace formatting" -Program "cargo" -Arguments @(
        "fmt",
        "--all",
        "--",
        "--check"
    )
    Invoke-Checked -Label "M3 workspace check" -Program "cargo" -Arguments @(
        "check",
        "--workspace",
        "--all-targets"
    )
    Invoke-Checked -Label "M3 strict Clippy" -Program "cargo" -Arguments @(
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings"
    )
    Invoke-Checked -Label "M3 complete workspace tests" -Program "cargo" -Arguments @(
        "test",
        "--workspace",
        "--all-targets"
    )
    Invoke-Checked -Label "Compliance doctor fixture tests" -Program "py" -Arguments @(
        "-3",
        "-m",
        "unittest",
        "discover",
        "-s",
        "tools/tests",
        "-v"
    )
    Invoke-Checked -Label "Originality and release compliance doctor" -Program "bash" -Arguments @(
        "tools/compliance-doctor.sh"
    )
    Invoke-Checked -Label "Git whitespace gate" -Program "git" -Arguments @(
        "diff",
        "--check"
    )

    if ($RegressionOnly) {
        Write-Host "M3 PERMANENT REGRESSION: PASS (S1-S69 + historical artifacts; current-tree receipt deferred to the active milestone)"
    }
    else {
        $temporaryReleaseAuditResolved = [System.IO.Path]::GetFullPath($temporaryReleaseAuditRoot)
        Assert-Condition ($temporaryReleaseAuditResolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase)) (
            "temporary release audit root escaped the operating-system temp directory"
        )
        Invoke-Checked -Label "S69 clean release tree and vulnerability audit" -Program "py" -Arguments @(
            "-3",
            "tools/release_audit.py",
            "--root",
            $repoRoot,
            "--output",
            $temporaryReleaseAuditResolved
        )
        Invoke-Checked -Label "Generated release audit artifact independent verification" -Program "py" -Arguments @(
            "-3",
            "tools/release_audit.py",
            "--verify",
            $temporaryReleaseAuditResolved
        )
        Invoke-Checked -Label "Repository-owned release audit artifact independent verification" -Program "py" -Arguments @(
            "-3",
            "tools/release_audit.py",
            "--verify",
            $releaseAuditArtifactRoot
        )
        Invoke-Checked -Label "Repository release audit matches current clean tree" -Program "py" -Arguments @(
            "-3",
            "tools/release_audit.py",
            "--compare",
            $temporaryReleaseAuditResolved,
            $releaseAuditArtifactRoot
        )
        Write-Host "M3 FINAL ACCEPTANCE: PASS (S1-S69 + 89 EventKinds + 18 crates + governed evolution golden + release compliance)"
    }
}
finally {
    if ($null -eq $previousArtifactRoot) {
        Remove-Item Env:FORME_M3_A_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M3_A_ARTIFACT_DIR = $previousArtifactRoot
    }
    if ($null -eq $previousM3BArtifactRoot) {
        Remove-Item Env:FORME_M3_B_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M3_B_ARTIFACT_DIR = $previousM3BArtifactRoot
    }
    if ($null -eq $previousM3CArtifactRoot) {
        Remove-Item Env:FORME_M3_C_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M3_C_ARTIFACT_DIR = $previousM3CArtifactRoot
    }
    $tempParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    $tempResolved = [System.IO.Path]::GetFullPath($temporaryArtifactRoot)
    if ($tempResolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $tempResolved)) {
        Remove-Item -LiteralPath $tempResolved -Recurse -Force
    }
    $temporaryM3BResolved = [System.IO.Path]::GetFullPath($temporaryM3BArtifactRoot)
    if ($temporaryM3BResolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $temporaryM3BResolved)) {
        Remove-Item -LiteralPath $temporaryM3BResolved -Recurse -Force
    }
    $temporaryM3CResolved = [System.IO.Path]::GetFullPath($temporaryM3CArtifactRoot)
    if ($temporaryM3CResolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $temporaryM3CResolved)) {
        Remove-Item -LiteralPath $temporaryM3CResolved -Recurse -Force
    }
    $temporaryReleaseAuditResolved = [System.IO.Path]::GetFullPath($temporaryReleaseAuditRoot)
    if ($temporaryReleaseAuditResolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $temporaryReleaseAuditResolved)) {
        Remove-Item -LiteralPath $temporaryReleaseAuditResolved -Recurse -Force
    }
    Pop-Location
}
