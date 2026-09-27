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
        Write-Host "[PASS] core-brain-wave-$requested"
    }
    Write-Host "core-brain-closure ($Wave): PASS"
}
finally {
    Pop-Location
}
