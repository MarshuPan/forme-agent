# The same blocking source-quality gate runs locally and on CI. No owner data.
[CmdletBinding()]
param([switch]$AllowMissingCorpus, [string]$WindowsInstallerToolchain)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'build-resources.ps1')

function Invoke-QualityStep {
    param([string]$Label, [string]$Program, [string[]]$Arguments)
    Write-Host "Quality: $Label"
    if ($Program -eq 'cargo') { Assert-FormeBuildSpace $repoRoot }
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Quality step failed: $Label (exit $LASTEXITCODE)" }
    if ($Program -eq 'cargo' -and $Arguments[0] -ne 'fmt') { Invoke-FormeBuildResources report $repoRoot }
}

Push-Location $repoRoot
try {
    if (Get-Command py -ErrorAction SilentlyContinue) {
        $python = 'py'
        $pythonArguments = @('-3')
    } else {
        $python = 'python3'
        $pythonArguments = @()
    }
    Invoke-QualityStep 'structure, cache safety and exact-byte checkout regressions' $python ($pythonArguments + @('-m', 'unittest', 'tools.tests.test_code_quality', 'tools.tests.test_build_resources', 'tools.tests.test_web_asset_checkout'))
    Invoke-QualityStep 'crate boundaries and size ratchet' $python ($pythonArguments + @('tools/code-quality.py'))
    Invoke-QualityStep 'Rust formatting' 'cargo' @('fmt', '--all', '--', '--check')
    Invoke-QualityStep 'all Rust targets' 'cargo' @('check', '--locked', '--workspace', '--all-targets')
    # Cross-crate process fixtures resolve the ordinary workspace executables;
    # cargo test does not build another package's non-test binary for them.
    Invoke-QualityStep 'current workspace binaries for process regressions' 'cargo' @('build', '--locked', '--workspace', '--bins')
    Invoke-QualityStep 'complete Rust regression' 'cargo' @('test', '--locked', '--workspace', '--all-targets')
    Invoke-QualityStep 'strict Clippy' 'cargo' @('clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings')
    Invoke-QualityStep 'locked frontend dependencies (no install scripts)' 'npm' @('ci', '--prefix', 'frontend', '--ignore-scripts', '--no-fund')
    Invoke-QualityStep 'frontend dependency vulnerability audit' 'npm' @('audit', '--prefix', 'frontend', '--audit-level=moderate')
    Invoke-QualityStep 'frontend types, regressions and reproducible embedded assets' 'npm' @('--prefix', 'frontend', 'run', 'verify')
    Invoke-QualityStep 'bundle integrity and preservation regressions' 'node' @('--test', 'tools/tests/product_bundle.test.mjs')
    Invoke-QualityStep 'native package input boundaries and POSIX control flow' 'node' @('--test', 'tools/tests/native_package.test.mjs', 'tools/tests/posix_packaging.test.mjs')
    Invoke-QualityStep 'current offline release verifier' 'cargo' @('build', '--locked', '-p', 'forme-execution', '--bin', 'forme-hostd')
    $oldReleaseVerifier = $env:FORME_RELEASE_VERIFIER
    try {
        $verifierName = if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) { 'forme-hostd.exe' } else { 'forme-hostd' }
        $env:FORME_RELEASE_VERIFIER = (Resolve-Path (Join-Path $repoRoot "target/debug/$verifierName")).Path
        Invoke-QualityStep 'offline signature and actual Rust verification' 'node' @('--test', 'tools/tests/signed_release.test.mjs')
    }
    finally { $env:FORME_RELEASE_VERIFIER = $oldReleaseVerifier }
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        Invoke-QualityStep 'current installation preparation binary' 'cargo' @('build', '--locked', '-p', 'forme-cli', '--bin', 'forme-hostctl')
        Invoke-QualityStep 'native NTFS access descriptors and safe installation refusal' 'powershell.exe' @('-NoProfile', '-NonInteractive', '-File', 'tools/tests/windows_service_access.tests.ps1')
        Invoke-QualityStep 'read-only firewall policy (fixture observations, no native rule changes)' 'powershell.exe' @('-NoProfile', '-NonInteractive', '-File', 'tools/tests/windows_firewall_policy.tests.ps1')
        if ($WindowsInstallerToolchain) {
            Invoke-QualityStep 'native installer success/failure exits (no ForMe install or elevation)' 'powershell.exe' @('-NoProfile', '-NonInteractive', '-File', 'tools/tests/windows_installer_exit.tests.ps1', '-Toolchain', $WindowsInstallerToolchain)
        } else {
            Write-Host 'Native Inno exit fixture: NOT_RUN (supply -WindowsInstallerToolchain with the reviewed compiler config)'
        }
    }
    $complianceArguments = @('tools/compliance-doctor.sh')
    if ($AllowMissingCorpus) { $complianceArguments += '--allow-missing-corpus' }
    Invoke-QualityStep 'originality and license' 'bash' $complianceArguments
    Invoke-QualityStep 'diff whitespace' 'git' @('diff', '--check')
    Write-Host 'Code quality: PASS (source checks; native installation and product IP acceptance are separate)'
}
finally { Pop-Location }
