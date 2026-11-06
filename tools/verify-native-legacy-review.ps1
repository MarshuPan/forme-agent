# Independent material/composition gate; no installed services or owner data.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Write-Host 'Native legacy review composition: NOT_RUN (Windows material custody required)'
    return
}
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'build-resources.ps1')
$prefix = 'installed::host::host_legacy_release_recovery::native::tests::'
$required = @(
    'native_source_context_survives_later_stages_and_replay_without_granting_eligibility',
    'native_managed_builder_mounts_provisioned_material_and_leaves_absence_unavailable',
    'native_materials_use_original_planned_time_and_independent_pinned_sources_without_writes',
    'native_absent_material_is_unavailable_and_never_provisions_trust',
    'native_closed_layout_rejects_missing_extra_oversized_and_linked_material',
    'native_report_and_source_signatures_have_separate_keys_domains_and_raw_pins',
    'native_resigning_does_not_override_source_identity_time_images_or_config_bindings',
    'native_closed_source_formats_reject_self_supplied_keys_flags_and_missing_fields',
    'native_combination_rebuilds_eight_evidences_and_preserves_journal_and_historical_null',
    'native_inspection_rejects_worker_data_or_custody_loss_without_reacquiring',
    'native_historical_sources_cannot_substitute_repair_source_or_current_native_checks',
    'native_material_custody_blocks_changes_or_rejects_them_and_expired_recheck_fails'
)
Push-Location $repoRoot
try {
    Assert-FormeBuildSpace $repoRoot
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $listed = @(& cargo test --locked -p forme-execution --lib $prefix -- --list 2>&1 | ForEach-Object { "$_" }); $listCode = $LASTEXITCODE }
    finally { $ErrorActionPreference = $previousPreference }
    if ($listCode -ne 0) { throw 'Native legacy review tests could not be enumerated' }
    foreach ($name in $required) {
        if ($listed -cnotcontains "${prefix}${name}: test") { throw "Missing native legacy review test: $name" }
    }
    & cargo test --locked -p forme-execution --lib $prefix
    if ($LASTEXITCODE -ne 0) { throw 'Native legacy review composition regression failed' }
    Write-Host 'Native legacy review composition: PASS (component evidence; installed history and IP remain separate)'
}
finally { Pop-Location }
