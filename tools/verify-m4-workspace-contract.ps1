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

    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "verify-m2-workspace-contract.ps1")
    if ($LASTEXITCODE -ne 0) {
        throw "frozen M2 workspace contract failed with exit code $LASTEXITCODE"
    }

    $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE"
    }
    $packages = @($metadata.packages | Where-Object { $_.name -like "forme-*" })
    Assert-Condition ($packages.Count -eq 18) "workspace crate count changed from the frozen 18-crate graph"

    $execution = $packages | Where-Object { $_.name -ceq "forme-execution" } | Select-Object -First 1
    $harness = $packages | Where-Object { $_.name -ceq "forme-harness" } | Select-Object -First 1
    $store = $packages | Where-Object { $_.name -ceq "forme-store" } | Select-Object -First 1
    Assert-Condition ($null -ne $execution -and $null -ne $harness -and $null -ne $store) "M4 owner crates are absent"

    Assert-ExactDependency $execution "httparse" "=1.10.1" "normal" | Out-Null
    $rustls = Assert-ExactDependency $execution "rustls" "=0.23.45" "normal"
    Assert-Condition (-not $rustls.uses_default_features) "rustls default features must remain disabled"
    Assert-Condition (@($rustls.features | Sort-Object) -join "," -ceq "ring,std") "rustls feature set drifted from ring,std"
    Assert-ExactDependency $execution "rustls-pki-types" "=1.15.0" "normal" | Out-Null
    Assert-ExactDependency $execution "rcgen" "=0.14.8" "dev" | Out-Null
    Assert-ExactDependency $harness "rcgen" "=0.14.8" "dev" | Out-Null

    $executionBins = @($execution.targets | Where-Object { $_.kind -contains "bin" } | ForEach-Object name)
    $storeBins = @($store.targets | Where-Object { $_.kind -contains "bin" } | ForEach-Object name)
    Assert-Condition ($executionBins -contains "forme-executord") "forme-executord target is missing"
    Assert-Condition ($storeBins -contains "forme-replicad") "forme-replicad target is missing"

    Write-Host "[PASS] frozen-18-crate-graph-and-edges"
    Write-Host "[PASS] m4-exact-dependency-pins"
    Write-Host "[PASS] m4-process-targets"
    Write-Host "m4-workspace-contract: PASS"
}
finally {
    Pop-Location
}
