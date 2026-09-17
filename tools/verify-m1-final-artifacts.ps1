param(
    [string]$ReportPath = "docs/acceptance/m1-real-model-golden-report.json",
    [string]$TracePath = "docs/acceptance/m1-real-model-golden-trace.json",
    [string]$AcceptancePath = "docs/acceptance/m1-acceptance-report.md",
    [string]$GoldenTasksPath = "evals/m1/golden-tasks.json"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Resolve-RepoPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }
    return Join-Path $repoRoot $Path
}

function Assert-Condition {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Read-JsonArtifact {
    param([Parameter(Mandatory = $true)][string]$Path)

    Assert-Condition (Test-Path -LiteralPath $Path -PathType Leaf) "required artifact is missing: $Path"
    $raw = Get-Content -LiteralPath $Path -Raw
    try {
        return [pscustomobject]@{
            Raw = $raw
            Value = $raw | ConvertFrom-Json
        }
    }
    catch {
        throw "artifact is not valid JSON: $Path"
    }
}

$reportFile = Resolve-RepoPath $ReportPath
$traceFile = Resolve-RepoPath $TracePath
$acceptanceFile = Resolve-RepoPath $AcceptancePath
$goldenTasksFile = Resolve-RepoPath $GoldenTasksPath

$reportArtifact = Read-JsonArtifact $reportFile
$traceArtifact = Read-JsonArtifact $traceFile
$goldenArtifact = Read-JsonArtifact $goldenTasksFile
$report = $reportArtifact.Value
$trace = $traceArtifact.Value
$events = @($trace.events)
$traceRefs = @($report.trace_refs | ForEach-Object { [string]$_ })
$eventIds = @($events | ForEach-Object { [string]$_.event_id })
$eventKinds = @($events | ForEach-Object { [string]$_.kind })

Assert-Condition ($report.schema_version -eq 1) "real-model report schema version is invalid"
Assert-Condition ($trace.schema_version -eq 1) "real-model trace schema version is invalid"
Assert-Condition ($report.outcome -ceq "Pass") "real-model report outcome is not Pass"
Assert-Condition ($trace.report_outcome -ceq "Pass") "trace manifest outcome is not Pass"
Assert-Condition ($trace.run_status -ceq "Complete") "trace manifest run is not Complete"
Assert-Condition (-not [string]::IsNullOrWhiteSpace([string]$report.eval_ref)) "eval_ref is missing"
Assert-Condition ([string]$report.eval_ref -like "eval:real:*") "eval_ref is not marked as a real-model evaluation"
Assert-Condition (-not [string]::IsNullOrWhiteSpace([string]$report.profile.model)) "model profile is missing"
Assert-Condition ([string]$report.profile.model -notmatch "(?i)scripted|fixture|fake") "model profile is not a configured real-model profile"
Assert-Condition ($report.run -ceq $trace.run) "report and trace run ids differ"
Assert-Condition ($report.eval_ref -ceq $trace.eval_ref) "report and trace eval refs differ"
Assert-Condition ($report.case_ref -ceq $trace.case_ref) "report and trace case refs differ"
Assert-Condition ($report.snapshot -ceq $trace.snapshot) "report and trace snapshots differ"
Assert-Condition ($report.snapshot -ceq $report.profile.replay_snapshot) "report replay snapshot is inconsistent"
Assert-Condition ($report.profile.model -ceq $trace.model_profile) "report and trace model profiles differ"
Assert-Condition ($events.Count -gt 0) "trace manifest has no events"
Assert-Condition ($events.Count -eq $traceRefs.Count) "trace ref count does not match event count"
Assert-Condition ($events.Count -eq [int]$trace.snapshot_upper_bound) "snapshot upper bound does not match event count"
Assert-Condition (($eventIds | Sort-Object -Unique).Count -eq $eventIds.Count) "trace event ids are not unique"
Assert-Condition (($traceRefs | Sort-Object -Unique).Count -eq $traceRefs.Count) "report trace refs are not unique"

for ($index = 0; $index -lt $events.Count; $index++) {
    $expectedSequence = $index + 1
    Assert-Condition ([int]$events[$index].stream_seq -eq $expectedSequence) "trace stream_seq is not contiguous at index $index"
    Assert-Condition ($traceRefs[$index] -ceq $eventIds[$index]) "trace ref does not match event id at stream_seq $expectedSequence"
}

$requiredKinds = @(
    "RunAccepted",
    "SessionBound",
    "DecisionTraceRecorded",
    "ModelCallStarted",
    "ModelCallDelta",
    "ModelCallFinished",
    "VerificationStarted",
    "VerificationFinished",
    "RunComplete"
)
foreach ($kind in $requiredKinds) {
    Assert-Condition ($eventKinds -ccontains $kind) "required real-model event is missing: $kind"
}
foreach ($kind in @("ActionStarted", "CandidatePromoted", "ActionOutcomeUnknown")) {
    Assert-Condition ($eventKinds -cnotcontains $kind) "forbidden final-only event is present: $kind"
}
Assert-Condition ([int64]$trace.model_usage.input_tokens -gt 0) "real-model input usage is missing"
Assert-Condition ([int64]$trace.model_usage.output_tokens -gt 0) "real-model output usage is missing"

$goldenCases = @($goldenArtifact.Value)
$matchingCases = @($goldenCases | Where-Object { $_.case_ref -ceq $report.case_ref })
Assert-Condition ($matchingCases.Count -eq 1) "report case_ref is not one repository-owned golden case"

$sensitiveFieldPattern = '"(?:api[_-]?key|authorization|credential|secret|endpoint|base_url)"\s*:'
$artifactText = $reportArtifact.Raw + "`n" + $traceArtifact.Raw
Assert-Condition ($artifactText -notmatch $sensitiveFieldPattern) "final artifacts contain a sensitive field"
foreach ($marker in @("FORME_MODEL_API_KEY", "Bearer ", "gateway.token", "http://", "https://")) {
    Assert-Condition (-not $artifactText.Contains($marker)) "final artifacts contain a sensitive value marker"
}

Assert-Condition (Test-Path -LiteralPath $acceptanceFile -PathType Leaf) "M1 final acceptance report is missing"
$acceptance = Get-Content -LiteralPath $acceptanceFile -Raw
Assert-Condition ($acceptance.Contains([string]$report.eval_ref)) "acceptance report does not reference eval_ref"
Assert-Condition ($acceptance.Contains([string]$report.run)) "acceptance report does not reference run id"
Assert-Condition ($acceptance.Contains("M1 最终验收 PASS")) "acceptance report does not record final PASS"

Write-Host "[PASS] real-model-report"
Write-Host "[PASS] portable-trace-manifest"
Write-Host "[PASS] artifact-secret-scan"
Write-Host "[PASS] final-acceptance-report"
Write-Host "m1-final-artifacts: PASS"
