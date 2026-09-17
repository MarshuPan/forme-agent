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

    $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE"
    }
    $packages = @($metadata.packages | Where-Object { $_.name -like "forme-*" })
    Assert-Condition ($packages.Count -eq 18) "workspace crate count changed from the frozen 18-crate graph"

    $actualEdges = @(
        $packages | ForEach-Object {
            $package = $_
            $package.dependencies |
                Where-Object { $null -eq $_.source -and $_.name -like "forme-*" } |
                ForEach-Object { "$($package.name)->$($_.name)" }
        } | Sort-Object -Unique
    )
    $expectedEdges = @(
        "forme-approval->forme-policy",
        "forme-approval->forme-protocol",
        "forme-capabilities->forme-policy",
        "forme-capabilities->forme-protocol",
        "forme-cli->forme-gateway",
        "forme-cli->forme-protocol",
        "forme-cognition->forme-memory",
        "forme-cognition->forme-protocol",
        "forme-cognition->forme-store",
        "forme-communication->forme-harness",
        "forme-communication->forme-policy",
        "forme-communication->forme-protocol",
        "forme-config->forme-protocol",
        "forme-context->forme-memory",
        "forme-context->forme-protocol",
        "forme-context->forme-store",
        "forme-coordination->forme-capabilities",
        "forme-coordination->forme-cognition",
        "forme-coordination->forme-protocol",
        "forme-eval->forme-protocol",
        "forme-eval->forme-store",
        "forme-execution->forme-policy",
        "forme-execution->forme-protocol",
        "forme-gateway->forme-communication",
        "forme-gateway->forme-harness",
        "forme-gateway->forme-protocol",
        "forme-harness->forme-approval",
        "forme-harness->forme-capabilities",
        "forme-harness->forme-cognition",
        "forme-harness->forme-context",
        "forme-harness->forme-coordination",
        "forme-harness->forme-eval",
        "forme-harness->forme-execution",
        "forme-harness->forme-loop",
        "forme-harness->forme-models",
        "forme-harness->forme-policy",
        "forme-harness->forme-protocol",
        "forme-harness->forme-store",
        "forme-loop->forme-capabilities",
        "forme-loop->forme-context",
        "forme-loop->forme-models",
        "forme-loop->forme-policy",
        "forme-loop->forme-protocol",
        "forme-memory->forme-protocol",
        "forme-memory->forme-store",
        "forme-models->forme-protocol",
        "forme-policy->forme-protocol",
        "forme-store->forme-protocol"
    ) | Sort-Object -Unique
    $edgeDiff = @(Compare-Object -ReferenceObject $expectedEdges -DifferenceObject $actualEdges)
    Assert-Condition ($edgeDiff.Count -eq 0) "internal crate dependency graph changed: $($edgeDiff | Out-String)"

    $execution = $packages | Where-Object { $_.name -ceq "forme-execution" } | Select-Object -First 1
    $expectedPins = @{
        "headless_chrome" = "=1.0.22"
        "portable-pty" = "=0.9.0"
        "url" = "=2.5.8"
        "ureq" = "=3.3.0"
        "enigo" = "=0.6.1"
        "xcap" = "=0.9.6"
    }
    foreach ($name in $expectedPins.Keys) {
        $dependency = @($execution.dependencies | Where-Object { $_.name -ceq $name })
        Assert-Condition ($dependency.Count -eq 1) "M2 dependency is missing or duplicated: $name"
        Assert-Condition ($dependency[0].req -ceq $expectedPins[$name]) "M2 dependency is not exact-pinned: $name"
    }

    foreach ($packageName in @("forme-capabilities", "forme-communication")) {
        $package = $packages | Where-Object { $_.name -ceq $packageName } | Select-Object -First 1
        $dependency = @($package.dependencies | Where-Object { $_.name -ceq "url" })
        Assert-Condition ($dependency.Count -eq 1) "M2-B URL parser dependency is missing or duplicated: $packageName"
        Assert-Condition ($dependency[0].req -ceq "=2.5.8") "M2-B URL parser dependency is not exact-pinned: $packageName"
    }

    Write-Host "[PASS] frozen-18-crate-graph"
    Write-Host "[PASS] no-new-internal-edges"
    Write-Host "[PASS] m2-direct-dependency-pins"
    Write-Host "m2-workspace-contract: PASS"
}
finally {
    Pop-Location
}
