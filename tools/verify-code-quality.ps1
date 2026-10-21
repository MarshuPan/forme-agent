# The same blocking source-quality gate runs locally and on CI. No owner data.
[CmdletBinding()]
param([switch]$AllowMissingCorpus)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Invoke-QualityStep {
    param([string]$Label, [string]$Program, [string[]]$Arguments)
    Write-Host "Quality: $Label"
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Quality step failed: $Label (exit $LASTEXITCODE)" }
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
    Invoke-QualityStep 'structure checker regressions' $python ($pythonArguments + @('-m', 'unittest', 'tools.tests.test_code_quality'))
    Invoke-QualityStep 'crate boundaries and size ratchet' $python ($pythonArguments + @('tools/code-quality.py'))
    Invoke-QualityStep 'Rust formatting' 'cargo' @('fmt', '--all', '--', '--check')
    Invoke-QualityStep 'all Rust targets' 'cargo' @('check', '--locked', '--workspace', '--all-targets')
    Invoke-QualityStep 'complete Rust regression' 'cargo' @('test', '--locked', '--workspace', '--all-targets')
    Invoke-QualityStep 'strict Clippy' 'cargo' @('clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings')
    Invoke-QualityStep 'locked frontend dependencies (no install scripts)' 'npm' @('ci', '--prefix', 'frontend', '--ignore-scripts', '--no-fund')
    Invoke-QualityStep 'frontend dependency vulnerability audit' 'npm' @('audit', '--prefix', 'frontend', '--audit-level=moderate')
    Invoke-QualityStep 'frontend types, regressions and reproducible embedded assets' 'npm' @('--prefix', 'frontend', 'run', 'verify')
    Invoke-QualityStep 'bundle integrity and preservation regressions' 'node' @('--test', 'tools/tests/product_bundle.test.mjs')
    $complianceArguments = @('tools/compliance-doctor.sh')
    if ($AllowMissingCorpus) { $complianceArguments += '--allow-missing-corpus' }
    Invoke-QualityStep 'originality and license' 'bash' $complianceArguments
    Invoke-QualityStep 'diff whitespace' 'git' @('diff', '--check')
    Write-Host 'Code quality: PASS (source checks; native installation and product IP acceptance are separate)'
}
finally { Pop-Location }
