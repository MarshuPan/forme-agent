# Execute the real gate control flow with inert command spies. This is an
# orchestration fixture, never evidence that Cargo, goldens or audits passed.
param(
    [Parameter(Mandatory = $true)][string]$RepositoryRoot,
    [Parameter(Mandatory = $true)][ValidateSet('m0', 'm1', 'm2', 'm3', 'm5')][string]$Gate,
    [switch]$DeferSharedChecks,
    [switch]$RegressionOnly,
    [string]$FailureStep = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$global:FormeGateSpyCalls = [System.Collections.Generic.List[object]]::new()
$global:FormeGateSpyFailure = $FailureStep
$global:FormeGateSpyRoot = [System.IO.Path]::GetFullPath($RepositoryRoot)
$global:LASTEXITCODE = 0

function global:Add-FormeGateSpyCall {
    param([string]$Program, [string[]]$Arguments)
    $global:FormeGateSpyCalls.Add([ordered]@{ program = $Program; arguments = @($Arguments) })
    $step = ''
    if ($Program -eq 'cargo') {
        if ($Arguments -contains '--workspace' -or $Arguments[0] -ceq 'fmt') { $step = 'cargo:' + $Arguments[0] }
        elseif ($Arguments -contains 'm2_a_browser_golden') { $step = 'browser-golden' }
        elseif ($Arguments -contains 'forme-m5-artifact-verify') { $step = 'artifact-verifier' }
    }
    elseif ($Program -eq 'py' -and $Arguments -contains 'discover') { $step = 'python:discover' }
    elseif ($Program -eq 'bash') { $step = 'compliance' }
    $global:LASTEXITCODE = if ($step -ne '' -and $step -ceq $global:FormeGateSpyFailure) { 29 } else { 0 }
    return $global:LASTEXITCODE
}

function global:cargo {
    $result = Add-FormeGateSpyCall -Program 'cargo' -Arguments @($args)
    if ($result -eq 0) { 'sha256:fixture-only-not-an-artifact' }
}
function global:py { $null = Add-FormeGateSpyCall -Program 'py' -Arguments @($args) }
function global:bash { $null = Add-FormeGateSpyCall -Program 'bash' -Arguments @($args) }
function global:git { $null = Add-FormeGateSpyCall -Program 'git' -Arguments @($args) }

function global:powershell {
    $gateArguments = @($args | ForEach-Object { [string]$_ })
    $null = Add-FormeGateSpyCall -Program 'powershell' -Arguments $gateArguments
    $at = [Array]::IndexOf($gateArguments, '-File')
    if ($at -lt 0 -or $at + 1 -ge $gateArguments.Length) { throw 'Fixture child gate has no file' }
    $file = [System.IO.Path]::GetFullPath($gateArguments[$at + 1])
    $name = [System.IO.Path]::GetFileName($file)
    $expected = Join-Path (Join-Path $global:FormeGateSpyRoot 'tools') $name
    if ($file -cne $expected) { throw 'Fixture child escaped the repository tools directory' }
    $children = @('verify-m0.ps1', 'verify-m1.ps1', 'verify-m2.ps1', 'verify-m3.ps1')
    $inert = @('verify-m1-final-artifacts.ps1', 'verify-m2-workspace-contract.ps1',
        'verify-m2-a-artifacts.ps1', 'verify-m5-workspace-contract.ps1')
    if ($children -contains $name) {
        $tail = if ($at + 2 -lt $gateArguments.Length) { @($gateArguments[($at + 2)..($gateArguments.Length - 1)]) } else { @() }
        $childParameters = @{}
        foreach ($option in $tail) {
            if ($option -notin @('-DeferSharedChecks', '-RegressionOnly')) { throw 'Unexpected child argument in fixture' }
            $childParameters[$option.TrimStart('-')] = $true
        }
        & $file @childParameters
    }
    elseif ($inert -notcontains $name) { throw 'Unexpected external script in fixture' }
    $global:LASTEXITCODE = 0
}

$fixtureExit = 0
try {
    $parameters = @{}
    if ($DeferSharedChecks) { $parameters.DeferSharedChecks = $true }
    if ($RegressionOnly) { $parameters.RegressionOnly = $true }
    & (Join-Path (Join-Path $RepositoryRoot 'tools') "verify-$Gate.ps1") @parameters
}
catch {
    $fixtureExit = 1
    Write-Output ('FIXTURE_FAILURE: ' + $_.Exception.Message)
}
finally {
    Write-Output ('FORME_GATE_SPY=' + (ConvertTo-Json -InputObject @($global:FormeGateSpyCalls.ToArray()) -Depth 8 -Compress))
}
exit $fixtureExit
