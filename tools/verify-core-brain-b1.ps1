$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# B1 is a module gate, not a substitute for C7-C12 or the final closure gate.
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $repoRoot
try {
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "verify-core-brain-workspace-contract.ps1")
    if ($LASTEXITCODE -ne 0) { throw "B1 workspace contract failed" }

    $targets = @(
        @{ Package = "forme-store"; Test = "v1_b1_tick_ledger"; Count = 8 },
        @{ Package = "forme-cognition"; Test = "v1_b1_tick_rules"; Count = 4 },
        @{ Package = "forme-cognition"; Test = "v1_b1_tick_recovery"; Count = 4 },
        @{ Package = "forme-harness"; Test = "v1_core_brain_b1_tick"; Count = 6 }
    )
    foreach ($target in $targets) {
        $previous = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $output = @(& cargo test -p $target.Package --test $target.Test 2>&1 | ForEach-Object { "$_" })
            $exitCode = $LASTEXITCODE
        }
        finally { $ErrorActionPreference = $previous }
        $output | ForEach-Object { Write-Host $_ }
        if ($exitCode -ne 0) { throw "B1 $($target.Test) failed with exit code $exitCode" }
        $expected = "^test result: ok\. $($target.Count) passed; 0 failed; 0 ignored;"
        if (@($output | Where-Object { $_ -match $expected }).Count -ne 1) {
            throw "B1 $($target.Test) did not run its complete scenario inventory"
        }
    }
    Write-Host "B1 (C7 module gate): PASS. B2-B6 and V1 final closure are not certified by this gate."
}
finally { Pop-Location }
