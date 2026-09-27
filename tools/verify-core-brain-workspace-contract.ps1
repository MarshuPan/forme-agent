$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# V1 core-brain closure workspace contract (prd/23 §11): the frozen M5
# workspace contract still holds, the current taxonomy is the 99-kind closure
# set with the exact 97-kind M5 prefix, and closure-only dependency additions
# stay exact-pinned to packages already recorded for release.
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $repoRoot
try {
    function Assert-Condition {
        param(
            [Parameter(Mandatory = $true)][bool]$Condition,
            [Parameter(Mandatory = $true)][string]$Message
        )

        if (-not $Condition) {
            throw $Message
        }
    }

    function Assert-ExactDependency {
        param(
            [Parameter(Mandatory = $true)]$Package,
            [Parameter(Mandatory = $true)][string]$Name,
            [Parameter(Mandatory = $true)][string]$Requirement,
            [Parameter(Mandatory = $true)][ValidateSet("normal", "dev")][string]$Kind
        )

        $dependencies = @(
            $Package.dependencies | Where-Object {
                $_.name -ceq $Name -and (
                    ($Kind -ceq "normal" -and $null -eq $_.kind) -or
                    ($Kind -ceq "dev" -and $_.kind -ceq "dev")
                )
            }
        )
        Assert-Condition ($dependencies.Count -eq 1) "$($Package.name) dependency is missing or duplicated: $Name ($Kind)"
        Assert-Condition ($dependencies[0].req -ceq $Requirement) "$($Package.name) dependency is not exact-pinned: $Name"
        return $dependencies[0]
    }

    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "verify-m5-workspace-contract.ps1")
    if ($LASTEXITCODE -ne 0) {
        throw "frozen M5 workspace contract failed with exit code $LASTEXITCODE"
    }

    $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE"
    }
    $packages = @($metadata.packages | Where-Object { $_.name -like "forme-*" })
    Assert-Condition ($packages.Count -eq 18) "workspace crate count changed from the frozen 18-crate graph"

    $harness = $packages | Where-Object { $_.name -ceq "forme-harness" } | Select-Object -First 1
    $cli = $packages | Where-Object { $_.name -ceq "forme-cli" } | Select-Object -First 1
    $gateway = $packages | Where-Object { $_.name -ceq "forme-gateway" } | Select-Object -First 1
    Assert-Condition ($null -ne $harness -and $null -ne $cli -and $null -ne $gateway) "closure owner crates are absent"

    # Typed capability configuration and the CLI's gateway-entry contract
    # reuse packages that are already exact-pinned and recorded.
    Assert-ExactDependency $harness "serde" "=1.0.228" "normal" | Out-Null
    Assert-ExactDependency $cli "serde_json" "=1.0.150" "dev" | Out-Null
    $gatewayBins = @($gateway.targets | Where-Object { $_.kind -contains "bin" } | ForEach-Object name)
    $cliBins = @($cli.targets | Where-Object { $_.kind -contains "bin" } | ForEach-Object name)
    Assert-Condition ($gatewayBins -contains "forme-gatewayd") "forme-gatewayd target is missing"
    Assert-Condition ($cliBins -contains "forme-cli") "forme-cli target is missing"

    foreach ($test in @(
            "closure_taxonomy_preserves_the_exact_m5_prefix_and_adds_only_two_events",
            "session_bound_composition_manifest_is_optional_additive_and_byte_stable"
        )) {
        & cargo test -p forme-protocol --test v1_closure_contract $test -- --exact
        if ($LASTEXITCODE -ne 0) {
            throw "V1 closure protocol contract failed: $test"
        }
    }

    Write-Host "[PASS] frozen-m5-workspace-contract"
    Write-Host "[PASS] closure-99-kind-taxonomy-with-exact-m5-prefix"
    Write-Host "[PASS] closure-session-bound-additive-field"
    Write-Host "[PASS] closure-exact-dependency-pins"
    Write-Host "core-brain-workspace-contract: PASS"
}
finally {
    Pop-Location
}
