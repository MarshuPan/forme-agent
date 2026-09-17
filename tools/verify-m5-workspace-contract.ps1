$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

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

    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "verify-m4-workspace-contract.ps1")
    if ($LASTEXITCODE -ne 0) {
        throw "frozen M4 workspace contract failed with exit code $LASTEXITCODE"
    }

    $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE"
    }
    $packages = @($metadata.packages | Where-Object { $_.name -like "forme-*" })
    Assert-Condition ($packages.Count -eq 18) "workspace crate count changed from the frozen 18-crate graph"

    $capabilities = $packages | Where-Object { $_.name -ceq "forme-capabilities" } | Select-Object -First 1
    $harness = $packages | Where-Object { $_.name -ceq "forme-harness" } | Select-Object -First 1
    $eval = $packages | Where-Object { $_.name -ceq "forme-eval" } | Select-Object -First 1
    Assert-Condition ($null -ne $capabilities -and $null -ne $harness -and $null -ne $eval) "M5 owner crates are absent"

    $ed25519 = Assert-ExactDependency $capabilities "ed25519-dalek" "=2.2.0" "normal"
    Assert-Condition (-not $ed25519.uses_default_features) "ed25519-dalek default features must remain disabled"
    Assert-Condition (@($ed25519.features) -join "," -ceq "std") "ed25519-dalek feature set drifted from std"
    Assert-ExactDependency $harness "ed25519-dalek" "=2.2.0" "dev" | Out-Null

    $harnessBins = @($harness.targets | Where-Object { $_.kind -contains "bin" } | ForEach-Object name)
    $evalBins = @($eval.targets | Where-Object { $_.kind -contains "bin" } | ForEach-Object name)
    Assert-Condition ($harnessBins -contains "forme-m5-executord") "forme-m5-executord target is missing"
    Assert-Condition ($harnessBins -contains "forme-m5-registryd") "forme-m5-registryd target is missing"
    Assert-Condition ($evalBins -contains "forme-m5-artifact-verify") "forme-m5-artifact-verify target is missing"

    & cargo test -p forme-protocol --test m5_contract m5_taxonomy_is_additive_after_the_exact_m4_prefix -- --exact
    if ($LASTEXITCODE -ne 0) {
        throw "M5 97-kind exact-prefix contract failed with exit code $LASTEXITCODE"
    }

    Write-Host "[PASS] frozen-18-crate-graph-and-edges"
    Write-Host "[PASS] m5-exact-dependency-pins"
    Write-Host "[PASS] m5-97-kind-prefix"
    Write-Host "[PASS] m5-process-and-verifier-targets"
    Write-Host "m5-workspace-contract: PASS"
}
finally {
    Pop-Location
}
