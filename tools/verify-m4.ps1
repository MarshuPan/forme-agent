$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$artifactRoot = Join-Path $repoRoot "docs\acceptance\m4-artifacts"
$m3ReleaseRoot = Join-Path $repoRoot "docs\acceptance\m3-release-audit-artifacts"
$m4ReleaseRoot = Join-Path $repoRoot "docs\acceptance\m4-release-audit-artifacts"
$temporaryArtifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m4-artifacts-" + [Guid]::NewGuid().ToString("N")
)
$temporaryReleaseRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m4-release-audit-" + [Guid]::NewGuid().ToString("N")
)
$previousM4ArtifactRoot = $env:FORME_M4_ARTIFACT_DIR
$previousM2ArtifactRoot = $env:FORME_M2_GOLDEN_ARTIFACT_DIR
$previousM3AArtifactRoot = $env:FORME_M3_A_ARTIFACT_DIR
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

    function Assert-TemporaryPath {
        param(
            [Parameter(Mandatory = $true)][string]$Path,
            [Parameter(Mandatory = $true)][string]$Label
        )

        $tempParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        $resolved = [System.IO.Path]::GetFullPath($Path)
        if (-not $resolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase)) {
            throw "$Label escaped the operating-system temp directory"
        }
        return $resolved
    }

    Invoke-Checked -Label "Permanent M0-M3 regression gate (S1-S69)" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m3.ps1"),
        "-RegressionOnly"
    )

    $scenarios = @(
        @{ Id = "M4-93-kind"; Package = "forme-protocol"; Target = @("--test", "event_payload_contract"); Test = "taxonomy_matches_the_fixed_contract_snapshot" },
        @{ Id = "M4-protocol-prefix"; Package = "forme-protocol"; Target = @("--test", "m4_contract"); Test = "m4_event_taxonomy_is_additive_after_the_frozen_m3_prefix" },
        @{ Id = "S71-S72-wire"; Package = "forme-protocol"; Target = @("--test", "m4_contract"); Test = "remote_wire_round_trip_is_closed_plan_bound_and_tamper_evident" },
        @{ Id = "M4-runtime-state"; Package = "forme-protocol"; Target = @("--test", "m4_contract"); Test = "remote_recovery_and_retention_state_are_versioned_and_digest_bound" },
        @{ Id = "S73-wire"; Package = "forme-protocol"; Target = @("--test", "m4_contract"); Test = "remote_operation_rejects_recursive_remote_and_host_paths" },
        @{ Id = "S78-S79-protocol"; Package = "forme-protocol"; Target = @("--test", "m4_contract"); Test = "owner_control_and_retention_are_nonce_expiry_and_digest_bound" },
        @{ Id = "S80-protocol"; Package = "forme-protocol"; Target = @("--test", "m4_contract"); Test = "placement_score_cannot_revive_a_filtered_executor" },
        @{ Id = "S81-protocol"; Package = "forme-protocol"; Target = @("--test", "m4_contract"); Test = "handoff_requires_a_passing_durable_checkpoint_and_a_new_run" },
        @{ Id = "S70-S71-S77-store"; Package = "forme-store"; Target = @("--test", "m4_federation_contract"); Test = "s70_s71_s77_peer_epoch_lease_and_dispatch_claim_are_single_writer" },
        @{ Id = "S75-S76-store"; Package = "forme-store"; Target = @("--test", "m4_federation_contract"); Test = "s75_s76_replication_is_filtered_contiguous_atomic_and_idempotent" },
        @{ Id = "S76-replicad"; Package = "forme-store"; Target = @("--test", "m4_replicad_process_contract"); Test = "s76_replicad_process_is_atomic_restart_idempotent_and_rejects_reorder_or_tamper" },
        @{ Id = "S72-S74-TLS"; Package = "forme-execution"; Target = @("--test", "m4_tls_contract"); Test = "s72_s74_real_mutual_tls_dispatch_is_durable_and_exactly_once_at_the_driver" },
        @{ Id = "S71-S73-executor-admission"; Package = "forme-execution"; Target = @("--test", "m4_tls_contract"); Test = "s71_s73_stale_fence_and_unscoped_credentials_stop_before_the_inner_driver" },
        @{ Id = "S70-harness"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s70_enrollment_binds_real_snapshot_and_stale_cas_or_revoke_fails_closed" },
        @{ Id = "S71-S72-harness"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s71_s72_remote_action_dispatches_once_then_recovers_the_original_receipt" },
        @{ Id = "S71-drift"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s71_plan_schema_grant_and_epoch_drift_stop_before_remote_driver" },
        @{ Id = "S73-harness"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s73_mismatched_ground_truth_records_failure_before_unknown_and_never_passes_capability" },
        @{ Id = "S73-untrusted-content"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s73_authenticated_remote_injection_and_secret_echo_never_enter_authority_facts" },
        @{ Id = "S72-S78-S79-S82-restart"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s72_s78_s79_s82_authority_restart_preserves_recovery_and_replay_ledgers" },
        @{ Id = "S78-S82-harness"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s78_s82_retention_is_honest_and_device_signals_have_one_authority_decision" },
        @{ Id = "S79-harness"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s79_cross_device_owner_control_is_dual_bound_one_shot_and_fences_late_receipts" },
        @{ Id = "S74-gateway"; Package = "forme-gateway"; Target = @("--lib"); Test = "tests::s74_gateway_remote_action_requires_owner_auth_and_routes_only_through_harness" },
        @{ Id = "S79-gateway"; Package = "forme-gateway"; Target = @("--lib"); Test = "tests::s79_owner_client_requires_peer_channel_and_independent_owner_before_harness_control" },
        @{ Id = "S75-S78-S82-gateway"; Package = "forme-gateway"; Target = @("--lib"); Test = "tests::s75_s78_s82_peer_ingress_is_channel_bound_before_harness" },
        @{ Id = "S81-harness"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s81_handoff_requires_a_verified_checkpoint_current_snapshot_and_new_segment" },
        @{ Id = "S80-harness"; Package = "forme-harness"; Target = @("--test", "m4_federation_contract"); Test = "s80_authority_filters_candidates_and_records_selection_before_action_plan" },
        @{ Id = "S82-scheduler"; Package = "forme-harness"; Target = @("--test", "harness_contract"); Test = "s82_two_owner_devices_share_one_authority_scheduler_budget_and_cancel" },
        @{ Id = "S80-filter"; Package = "forme-coordination"; Target = @("--test", "m4_c_coordination_contract"); Test = "s80_filters_authority_before_ranking_and_score_never_authorizes" },
        @{ Id = "S80-empty"; Package = "forme-coordination"; Target = @("--test", "m4_c_coordination_contract"); Test = "s80_no_candidate_returns_a_governed_no_placement_decision" },
        @{ Id = "S74-S83-artifact"; Package = "forme-eval"; Target = @("--test", "m4_artifact_contract"); Test = "s74_s83_five_file_artifact_set_is_content_addressed_and_offline_verifiable" },
        @{ Id = "S84-artifact-set"; Package = "forme-eval"; Target = @("--test", "m4_artifact_contract"); Test = "s84_artifact_gate_rejects_tamper_unknown_fields_extra_and_missing_files" },
        @{ Id = "S84-artifact-secret"; Package = "forme-eval"; Target = @("--test", "m4_artifact_contract"); Test = "s84_artifact_gate_rejects_secrets_host_paths_and_endpoints" },
        @{ Id = "S84-binding"; Package = "forme-eval"; Target = @("--test", "m4_artifact_contract"); Test = "s84_plan_lease_fence_cursor_and_trace_mismatches_fail_closed" },
        @{ Id = "S84-doctor"; Package = "forme-config"; Target = @("--lib"); Test = "tests::doctor_checks_every_required_matrix_row_with_explanations" }
    )

    foreach ($scenario in $scenarios) {
        $arguments = @("test", "-p", $scenario.Package) + $scenario.Target + @(
            $scenario.Test,
            "--",
            "--exact"
        )
        Invoke-Checked -Label "$($scenario.Id) scenario contract" -Program "cargo" -Arguments $arguments
        Write-Host "[$($scenario.Id)] PASS"
    }

    Invoke-Checked -Label "Frozen graph and M4 exact dependency pins" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m4-workspace-contract.ps1")
    )

    Invoke-Checked -Label "Build independent M4 executor process" -Program "cargo" -Arguments @(
        "build", "-p", "forme-execution", "--bin", "forme-executord"
    )
    Invoke-Checked -Label "Build independent M4 replica process" -Program "cargo" -Arguments @(
        "build", "-p", "forme-store", "--bin", "forme-replicad"
    )

    $temporaryArtifactResolved = Assert-TemporaryPath $temporaryArtifactRoot "temporary M4 artifact root"
    $env:FORME_M4_ARTIFACT_DIR = $temporaryArtifactResolved
    Invoke-Checked -Label "S74/S83 three-process mutual-TLS golden" -Program "cargo" -Arguments @(
        "test",
        "-p",
        "forme-harness",
        "--test",
        "m4_federation_contract",
        "s74_s83_three_process_tls_unknown_recovery_replication_revoke_and_artifacts",
        "--",
        "--ignored",
        "--exact",
        "--nocapture"
    )
    Invoke-Checked -Label "Generated M4 artifact independent verification" -Program "cargo" -Arguments @(
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m4-artifact-verify",
        "--",
        $temporaryArtifactResolved
    )
    Remove-Item Env:FORME_M4_ARTIFACT_DIR -ErrorAction SilentlyContinue
    Invoke-Checked -Label "Repository-owned M4 artifact independent verification" -Program "cargo" -Arguments @(
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m4-artifact-verify",
        "--",
        $artifactRoot
    )

    Remove-Item Env:FORME_M2_GOLDEN_ARTIFACT_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:FORME_M3_A_ARTIFACT_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:FORME_M3_C_ARTIFACT_DIR -ErrorAction SilentlyContinue
    Invoke-Checked -Label "S38 governed real Browser golden" -Program "cargo" -Arguments @(
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
    Invoke-Checked -Label "S68 governed evolution Browser golden" -Program "cargo" -Arguments @(
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

    Invoke-Checked -Label "M4 workspace formatting" -Program "cargo" -Arguments @(
        "fmt", "--all", "--", "--check"
    )
    Invoke-Checked -Label "M4 workspace check" -Program "cargo" -Arguments @(
        "check", "--workspace", "--all-targets"
    )
    Invoke-Checked -Label "M4 strict Clippy" -Program "cargo" -Arguments @(
        "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"
    )
    Invoke-Checked -Label "M4 complete workspace tests" -Program "cargo" -Arguments @(
        "test", "--workspace", "--all-targets"
    )
    Invoke-Checked -Label "M4 compliance fixture tests" -Program "py" -Arguments @(
        "-3", "-m", "unittest", "discover", "-s", "tools/tests", "-v"
    )
    Invoke-Checked -Label "M4 originality and compliance doctor" -Program "bash" -Arguments @(
        "tools/compliance-doctor.sh"
    )
    Invoke-Checked -Label "M4 Git whitespace gate" -Program "git" -Arguments @(
        "diff", "--check"
    )

    Invoke-Checked -Label "Historical M3 release receipt independent verification" -Program "py" -Arguments @(
        "-3", "tools/release_audit.py", "--verify", $m3ReleaseRoot
    )
    $temporaryReleaseResolved = Assert-TemporaryPath $temporaryReleaseRoot "temporary M4 release audit root"
    Invoke-Checked -Label "S84 current M4 release and federation audit" -Program "py" -Arguments @(
        "-3", "tools/m4_release_audit.py", "--root", $repoRoot, "--output", $temporaryReleaseResolved
    )
    Invoke-Checked -Label "Generated M4 release receipt independent verification" -Program "py" -Arguments @(
        "-3", "tools/m4_release_audit.py", "--verify", $temporaryReleaseResolved
    )
    Invoke-Checked -Label "Repository-owned M4 release receipt independent verification" -Program "py" -Arguments @(
        "-3", "tools/m4_release_audit.py", "--verify", $m4ReleaseRoot
    )
    Invoke-Checked -Label "Repository M4 release receipt matches current clean tree" -Program "py" -Arguments @(
        "-3", "tools/m4_release_audit.py", "--compare", $temporaryReleaseResolved, $m4ReleaseRoot
    )

    Write-Host "M4 FINAL ACCEPTANCE: PASS (S1-S84 + 93 EventKinds + 18 crates + three-process federation golden + release compliance)"
}
finally {
    if ($null -eq $previousM4ArtifactRoot) {
        Remove-Item Env:FORME_M4_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M4_ARTIFACT_DIR = $previousM4ArtifactRoot
    }
    if ($null -eq $previousM2ArtifactRoot) {
        Remove-Item Env:FORME_M2_GOLDEN_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M2_GOLDEN_ARTIFACT_DIR = $previousM2ArtifactRoot
    }
    if ($null -eq $previousM3AArtifactRoot) {
        Remove-Item Env:FORME_M3_A_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M3_A_ARTIFACT_DIR = $previousM3AArtifactRoot
    }
    if ($null -eq $previousM3CArtifactRoot) {
        Remove-Item Env:FORME_M3_C_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M3_C_ARTIFACT_DIR = $previousM3CArtifactRoot
    }

    $tempParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    foreach ($path in @($temporaryArtifactRoot, $temporaryReleaseRoot)) {
        $resolved = [System.IO.Path]::GetFullPath($path)
        if ($resolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -and
            (Test-Path -LiteralPath $resolved)) {
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    }
    Pop-Location
}
