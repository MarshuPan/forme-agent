param(
    [string]$ReportPath = "docs/acceptance/m2-a-browser-golden-report.json",
    [string]$TracePath = "docs/acceptance/m2-a-browser-golden-trace.json",
    [string]$AcceptancePath = "docs/acceptance/m2-a-acceptance-report.md"
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
$reportArtifact = Read-JsonArtifact $reportFile
$traceArtifact = Read-JsonArtifact $traceFile
$report = $reportArtifact.Value
$trace = $traceArtifact.Value
$events = @($trace.events)
$traceRefs = @($report.trace_refs | ForEach-Object { [string]$_ })
$eventIds = @($events | ForEach-Object { [string]$_.event_id })
$eventKinds = @($events | ForEach-Object { [string]$_.kind })

Assert-Condition ($report.schema_version -eq 1) "browser golden report schema is invalid"
Assert-Condition ($trace.schema_version -eq 1) "browser golden trace schema is invalid"
Assert-Condition ($report.outcome -ceq "Pass") "browser golden report outcome is not Pass"
Assert-Condition ($trace.report_outcome -ceq "Pass") "browser golden trace outcome is not Pass"
Assert-Condition ($trace.run_status -ceq "Complete") "browser golden run did not complete"
Assert-Condition ($report.eval_ref -ceq "eval:m2-a-real-browser-mutation") "browser eval_ref is unexpected"
Assert-Condition ($report.case_ref -ceq "case:m2-a-real-browser-mutation") "browser case_ref is unexpected"
Assert-Condition ($report.run -ceq $trace.run) "report and trace run ids differ"
Assert-Condition ($report.eval_ref -ceq $trace.eval_ref) "report and trace eval refs differ"
Assert-Condition ($report.case_ref -ceq $trace.case_ref) "report and trace case refs differ"
Assert-Condition ($report.snapshot -ceq $trace.snapshot) "report and trace snapshots differ"
Assert-Condition ($report.snapshot -ceq $report.profile.replay_snapshot) "report replay snapshot differs"
Assert-Condition ($report.ground_truth.driver_profile -ceq "headless-chrome-real-process") "real driver profile is missing"
Assert-Condition ($report.ground_truth.approval_scope -ceq "OneShot") "golden approval was not one-shot"
Assert-Condition ($report.ground_truth.effect -ceq "Committed") "golden effect was not committed"
Assert-Condition ([int]$report.ground_truth.mutation_count -eq 1) "server mutation ground truth is not exactly one"
Assert-Condition ([int]$trace.ground_truth.mutation_count -eq 1) "trace mutation ground truth is not exactly one"

Assert-Condition ($events.Count -gt 0) "browser golden trace has no events"
Assert-Condition ($events.Count -eq $traceRefs.Count) "report trace ref count differs from trace events"
Assert-Condition ($events.Count -eq [int]$trace.snapshot_upper_bound) "snapshot upper bound differs from event count"
Assert-Condition (($eventIds | Sort-Object -Unique).Count -eq $eventIds.Count) "trace event ids are not unique"
for ($index = 0; $index -lt $events.Count; $index++) {
    $expectedSequence = $index + 1
    Assert-Condition ([int]$events[$index].stream_seq -eq $expectedSequence) "stream_seq is not contiguous at index $index"
    Assert-Condition ($traceRefs[$index] -ceq $eventIds[$index]) "report trace ref differs at stream_seq $expectedSequence"
}

$requiredOrder = @(
    "ToolCallProposed",
    "ToolPolicyEvaluated",
    "ApprovalRequested",
    "RunWaiting",
    "ApprovalResolved",
    "RunResumed",
    "ToolPolicyEvaluated",
    "CompetenceGateEvaluated",
    "ActionPlanned",
    "ActionStarted",
    "ActionOutputDelta",
    "ActionCompleted",
    "CapabilityEvidenceRecorded",
    "VerificationStarted",
    "VerificationFinished",
    "RunComplete"
)
$cursor = 0
foreach ($kind in $requiredOrder) {
    $found = -1
    for ($index = $cursor; $index -lt $eventKinds.Count; $index++) {
        if ($eventKinds[$index] -ceq $kind) {
            $found = $index
            break
        }
    }
    Assert-Condition ($found -ge 0) "required ordered event is missing: $kind"
    $cursor = $found + 1
}

Assert-Condition ($trace.action.backend -ceq "Browser") "trace action backend is not Browser"
Assert-Condition ([string]$trace.action.plan_digest -match '^sha256:[0-9a-f]{64}$') "trace plan digest is invalid"
Assert-Condition (-not [string]::IsNullOrWhiteSpace([string]$trace.action.approval_ref)) "trace approval ref is missing"
Assert-Condition ($trace.action.receipt_effect -ceq "Committed") "trace receipt effect is not Committed"
Assert-Condition ($trace.action.receipt_trust -ceq "Untrusted") "trace receipt trust is not Untrusted"
$outputEvents = @($events | Where-Object { $_.kind -ceq "ActionOutputDelta" })
Assert-Condition ($outputEvents.Count -gt 0) "trace has no external output event"
Assert-Condition (@($outputEvents | Where-Object { $_.provenance_trust -cne "Untrusted" }).Count -eq 0) "external output provenance is not Untrusted"
Assert-Condition (@($outputEvents | Where-Object { $_.payload_trust -cne "Untrusted" }).Count -eq 0) "external output payload trust is not Untrusted"
foreach ($kind in @("ActionCompleted", "CapabilityEvidenceRecorded")) {
    $matching = @($events | Where-Object { $_.kind -ceq $kind })
    Assert-Condition ($matching.Count -gt 0) "required external ground-truth event is missing: $kind"
    Assert-Condition (@($matching | Where-Object { $_.provenance_trust -cne "Untrusted" }).Count -eq 0) "$kind provenance is not Untrusted"
}
foreach ($kind in @($trace.forbidden_events_absent)) {
    Assert-Condition ($eventKinds -cnotcontains [string]$kind) "forbidden event is present: $kind"
}

$artifactText = $reportArtifact.Raw + "`n" + $traceArtifact.Raw
$sensitiveFieldPattern = '"(?:api[_-]?key|authorization|credential|secret|executable|environment|target_url|allowed_origins|raw_text|dom|transcript|path)"\s*:'
Assert-Condition ($artifactText -notmatch $sensitiveFieldPattern) "browser golden artifacts contain a sensitive field"
foreach ($marker in @(
    "FORME_",
    "Bearer ",
    "http://",
    "https://",
    "127.0.0.1",
    "Program Files",
    "AppData",
    "opaque-pty-marker"
)) {
    Assert-Condition (-not $artifactText.Contains($marker)) "browser golden artifacts contain a sensitive marker: $marker"
}

Assert-Condition (Test-Path -LiteralPath $acceptanceFile -PathType Leaf) "M2-A acceptance report is missing"
$acceptance = Get-Content -LiteralPath $acceptanceFile -Raw
Assert-Condition ($acceptance.Contains([string]$report.eval_ref)) "M2-A acceptance report does not reference the golden eval"
Assert-Condition ($acceptance.Contains([string]$report.run)) "M2-A acceptance report does not reference the golden run"
Assert-Condition ($acceptance.Contains("M2-A 验收 PASS")) "M2-A acceptance report does not record PASS"

Write-Host "[PASS] m2-a-typed-eval"
Write-Host "[PASS] m2-a-portable-trace"
Write-Host "[PASS] m2-a-real-browser-ground-truth"
Write-Host "[PASS] m2-a-artifact-sensitive-scan"
Write-Host "m2-a-artifacts: PASS"
