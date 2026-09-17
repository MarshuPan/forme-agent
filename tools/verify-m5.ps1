$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$m4ArtifactRoot = Join-Path $repoRoot "docs\acceptance\m4-artifacts"
$m4ReleaseRoot = Join-Path $repoRoot "docs\acceptance\m4-release-audit-artifacts"
$m5ArtifactRoot = Join-Path $repoRoot "docs\acceptance\m5-artifacts"
$m5ReleaseRoot = Join-Path $repoRoot "docs\acceptance\m5-release-audit-artifacts"
$temporaryM4ArtifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m5-m4-regression-artifacts-" + [Guid]::NewGuid().ToString("N")
)
$temporaryM5ArtifactRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m5-artifacts-" + [Guid]::NewGuid().ToString("N")
)
$temporaryReleaseRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "forme-m5-release-audit-" + [Guid]::NewGuid().ToString("N")
)
$previousM4ArtifactRoot = $env:FORME_M4_ARTIFACT_DIR
$previousM5ArtifactRoot = $env:FORME_M5_ARTIFACT_DIR
$previousM5Executor = $env:FORME_M5_EXECUTORD_BIN
$previousM5Registry = $env:FORME_M5_REGISTRYD_BIN

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

    $m4Targets = @(
        @{ Label = "M4 protocol contracts (S70-S81)"; Args = @("test", "-p", "forme-protocol", "--test", "m4_contract") },
        @{ Label = "M4 store contracts (S70-S77)"; Args = @("test", "-p", "forme-store", "--test", "m4_federation_contract") },
        @{ Label = "M4 replica process contracts (S76)"; Args = @("test", "-p", "forme-store", "--test", "m4_replicad_process_contract") },
        @{ Label = "M4 mutual-TLS contracts (S71-S74)"; Args = @("test", "-p", "forme-execution", "--test", "m4_tls_contract") },
        @{ Label = "M4 Harness contracts (S70-S82)"; Args = @("test", "-p", "forme-harness", "--test", "m4_federation_contract") },
        @{ Label = "M4 coordination contracts (S80)"; Args = @("test", "-p", "forme-coordination", "--test", "m4_c_coordination_contract") },
        @{ Label = "M4 artifact contracts (S74-S84)"; Args = @("test", "-p", "forme-eval", "--test", "m4_artifact_contract") },
        @{ Label = "M4 Gateway contracts (S74-S82)"; Args = @("test", "-p", "forme-gateway", "--lib") }
    )
    foreach ($target in $m4Targets) {
        Invoke-Checked -Label $target.Label -Program "cargo" -Arguments $target.Args
    }

    Invoke-Checked -Label "Build M4 executor process" -Program "cargo" -Arguments @(
        "build", "-p", "forme-execution", "--bin", "forme-executord"
    )
    Invoke-Checked -Label "Build M4 replica process" -Program "cargo" -Arguments @(
        "build", "-p", "forme-store", "--bin", "forme-replicad"
    )
    $temporaryM4Resolved = Assert-TemporaryPath $temporaryM4ArtifactRoot "temporary M4 artifact root"
    $env:FORME_M4_ARTIFACT_DIR = $temporaryM4Resolved
    Invoke-Checked -Label "M4 three-process federation golden (S74/S83)" -Program "cargo" -Arguments @(
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
    Invoke-Checked -Label "Generated M4 artifact verification" -Program "cargo" -Arguments @(
        "run", "-p", "forme-eval", "--bin", "forme-m4-artifact-verify", "--", $temporaryM4Resolved
    )
    Invoke-Checked -Label "Repository M4 artifact verification" -Program "cargo" -Arguments @(
        "run", "-p", "forme-eval", "--bin", "forme-m4-artifact-verify", "--", $m4ArtifactRoot
    )
    Remove-Item Env:FORME_M4_ARTIFACT_DIR -ErrorAction SilentlyContinue

    $m5Targets = @(
        @{ Label = "M5 protocol contracts (S85-S97)"; Args = @("test", "-p", "forme-protocol", "--test", "m5_contract") },
        @{ Label = "M5 store contracts (S85-S97)"; Args = @("test", "-p", "forme-store", "--test", "m5_ecosystem_contract") },
        @{ Label = "M5 supply-chain and registry contracts (S87-S94)"; Args = @("test", "-p", "forme-capabilities", "--test", "m5_ecosystem_contract") },
        @{ Label = "M5 lifecycle and catalog contracts (S85-S94/S97)"; Args = @("test", "-p", "forme-harness", "--test", "m5_ecosystem_contract") },
        @{ Label = "M5 distribution contracts (S95-S97)"; Args = @("test", "-p", "forme-harness", "--test", "m5_distribution_contract") },
        @{ Label = "M5 artifact and threat contracts (S98-S99)"; Args = @("test", "-p", "forme-eval", "--test", "m5_artifact_contract") },
        @{ Label = "M5 release-audit Python contracts (S99)"; Program = "py"; Args = @("-3", "-m", "unittest", "tools.tests.test_m5_release_audit", "-v") }
    )
    foreach ($target in $m5Targets) {
        $program = if ($target.ContainsKey("Program")) { $target.Program } else { "cargo" }
        Invoke-Checked -Label $target.Label -Program $program -Arguments $target.Args
    }

    Invoke-Checked -Label "Frozen graph, pins, taxonomy, and M5 process targets" -Program "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        (Join-Path $PSScriptRoot "verify-m5-workspace-contract.ps1")
    )
    Invoke-Checked -Label "Build independent M5 registry and executor processes" -Program "cargo" -Arguments @(
        "build",
        "-p",
        "forme-harness",
        "--bin",
        "forme-m5-registryd",
        "--bin",
        "forme-m5-executord"
    )

    $temporaryM5Resolved = Assert-TemporaryPath $temporaryM5ArtifactRoot "temporary M5 artifact root"
    $env:FORME_M5_ARTIFACT_DIR = $temporaryM5Resolved
    $env:FORME_M5_EXECUTORD_BIN = (Resolve-Path (Join-Path $repoRoot "target\debug\forme-m5-executord.exe")).Path
    $env:FORME_M5_REGISTRYD_BIN = (Resolve-Path (Join-Path $repoRoot "target\debug\forme-m5-registryd.exe")).Path
    Invoke-Checked -Label "M5 authority/registry/executor golden and artifact generation (S98)" -Program "cargo" -Arguments @(
        "test",
        "-p",
        "forme-harness",
        "--test",
        "m5_distribution_contract",
        "s98_three_process_registry_authority_executor_golden_is_governed_end_to_end",
        "--",
        "--ignored",
        "--exact",
        "--nocapture"
    )
    Invoke-Checked -Label "Generated M5 artifact independent verification" -Program "cargo" -Arguments @(
        "run", "-p", "forme-eval", "--bin", "forme-m5-artifact-verify", "--", $temporaryM5Resolved
    )
    Invoke-Checked -Label "Repository-owned M5 artifact independent verification" -Program "cargo" -Arguments @(
        "run", "-p", "forme-eval", "--bin", "forme-m5-artifact-verify", "--", $m5ArtifactRoot
    )
    $generatedM5Digest = (& cargo run -q -p forme-eval --bin forme-m5-artifact-verify -- $temporaryM5Resolved).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "generated M5 artifact digest could not be read"
    }
    $repositoryM5Digest = (& cargo run -q -p forme-eval --bin forme-m5-artifact-verify -- $m5ArtifactRoot).Trim()
    if ($LASTEXITCODE -ne 0 -or $generatedM5Digest -cne $repositoryM5Digest) {
        throw "repository M5 artifact set does not match the regenerated real golden"
    }
    Remove-Item Env:FORME_M5_ARTIFACT_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:FORME_M5_EXECUTORD_BIN -ErrorAction SilentlyContinue
    Remove-Item Env:FORME_M5_REGISTRYD_BIN -ErrorAction SilentlyContinue

    Invoke-Checked -Label "M5 workspace formatting" -Program "cargo" -Arguments @(
        "fmt", "--all", "--", "--check"
    )
    Invoke-Checked -Label "M5 workspace check" -Program "cargo" -Arguments @(
        "check", "--workspace", "--all-targets"
    )
    Invoke-Checked -Label "M5 strict Clippy" -Program "cargo" -Arguments @(
        "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"
    )
    Invoke-Checked -Label "M5 complete workspace tests" -Program "cargo" -Arguments @(
        "test", "--workspace", "--all-targets"
    )
    Invoke-Checked -Label "M5 complete Python gates" -Program "py" -Arguments @(
        "-3", "-m", "unittest", "discover", "-s", "tools/tests", "-v"
    )
    Invoke-Checked -Label "M5 originality and compliance doctor" -Program "bash" -Arguments @(
        "tools/compliance-doctor.sh"
    )
    Invoke-Checked -Label "M5 Git whitespace gate" -Program "git" -Arguments @(
        "diff", "--check"
    )

    Invoke-Checked -Label "Historical M4 release receipt independent verification" -Program "py" -Arguments @(
        "-3", "tools/m4_release_audit.py", "--verify", $m4ReleaseRoot
    )
    $temporaryReleaseResolved = Assert-TemporaryPath $temporaryReleaseRoot "temporary M5 release audit root"
    Invoke-Checked -Label "S99 current M5 supply-chain release audit" -Program "py" -Arguments @(
        "-3", "tools/m5_release_audit.py", "--root", $repoRoot, "--output", $temporaryReleaseResolved
    )
    Invoke-Checked -Label "Generated M5 release receipt independent verification" -Program "py" -Arguments @(
        "-3", "tools/m5_release_audit.py", "--verify", $temporaryReleaseResolved
    )
    Invoke-Checked -Label "Repository-owned M5 release receipt independent verification" -Program "py" -Arguments @(
        "-3", "tools/m5_release_audit.py", "--verify", $m5ReleaseRoot
    )
    Invoke-Checked -Label "Repository M5 release receipt matches current clean tree" -Program "py" -Arguments @(
        "-3", "tools/m5_release_audit.py", "--compare", $temporaryReleaseResolved, $m5ReleaseRoot
    )

    Write-Host "M5 FINAL ACCEPTANCE: PASS (S1-S99 + 97 EventKinds + 18 crates + governed ecosystem golden + release compliance)"
}
finally {
    if ($null -eq $previousM4ArtifactRoot) {
        Remove-Item Env:FORME_M4_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M4_ARTIFACT_DIR = $previousM4ArtifactRoot
    }
    if ($null -eq $previousM5ArtifactRoot) {
        Remove-Item Env:FORME_M5_ARTIFACT_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M5_ARTIFACT_DIR = $previousM5ArtifactRoot
    }
    if ($null -eq $previousM5Executor) {
        Remove-Item Env:FORME_M5_EXECUTORD_BIN -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M5_EXECUTORD_BIN = $previousM5Executor
    }
    if ($null -eq $previousM5Registry) {
        Remove-Item Env:FORME_M5_REGISTRYD_BIN -ErrorAction SilentlyContinue
    }
    else {
        $env:FORME_M5_REGISTRYD_BIN = $previousM5Registry
    }

    $tempParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    foreach ($path in @($temporaryM4ArtifactRoot, $temporaryM5ArtifactRoot, $temporaryReleaseRoot)) {
        $resolved = [System.IO.Path]::GetFullPath($path)
        if ($resolved.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -and
            (Test-Path -LiteralPath $resolved)) {
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    }
    Pop-Location
}
