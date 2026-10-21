# Disposable process fixture. Never load .forme, owner data, or real credentials.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$temporaryParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$root = Join-Path $temporaryParent ("forme-web-process-" + [Guid]::NewGuid().ToString("N"))
$process = $null
$client = $null
Push-Location $repoRoot
try {
    & cargo build -p forme-gateway --bin forme-webd
    if ($LASTEXITCODE -ne 0) { throw "Web process build failed" }
    New-Item -ItemType Directory -Path $root | Out-Null
    $auth = Join-Path $root "auth"
    New-Item -ItemType Directory -Path $auth | Out-Null
    $portProbe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $portProbe.Start()
    $port = $portProbe.LocalEndpoint.Port
    $portProbe.Stop()
    $configPath = Join-Path $root "web.json"
    # Public half of the repository-owned [127;32] fixture, not a release key.
    $public = @(178,169,66,255,76,152,113,139,237,118,226,85,152,127,109,89,177,167,45,59,44,210,81,0,3,230,23,10,198,58,159,251)
    @{
        schema_version=1; install_ref="installation:process-fixture"
        identity_directory=$auth; claim_verifying_key=$public; bind="127.0.0.1:$port"
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $configPath -Encoding Ascii
    $suffix = if ($env:OS -eq "Windows_NT") { ".exe" } else { "" }
    $binary = Join-Path $repoRoot "target/debug/forme-webd$suffix"
    Add-Type -AssemblyName System.Net.Http
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseProxy = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(5)
    for ($attempt = 0; $attempt -lt 2; $attempt++) {
        $start = [System.Diagnostics.ProcessStartInfo]::new()
        $start.FileName = $binary
        $start.Arguments = '--config "' + $configPath + '"'
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.EnvironmentVariables.Clear()
        if ($env:SystemRoot) { $start.EnvironmentVariables.Add("SystemRoot", $env:SystemRoot) }
        $start.EnvironmentVariables.Add("TEMP", $root)
        $start.EnvironmentVariables.Add("TMP", $root)
        $process = [System.Diagnostics.Process]::new()
        $process.StartInfo = $start
        if (-not $process.Start()) { throw "Web fixture process did not start" }
        $line = $process.StandardOutput.ReadLineAsync()
        if (-not $line.Wait(30000) -or $line.Result -cne "FORME_WEB_URL=http://127.0.0.1:$port") {
            throw "Web fixture did not announce the configured address"
        }
        $result = $client.GetStringAsync("http://127.0.0.1:$port/api/v1/bootstrap/status").GetAwaiter().GetResult() | ConvertFrom-Json
        $names = @($result.PSObject.Properties.Name | Sort-Object)
        if (($names -join ',') -cne 'claimed,protocol_version,schema_version,setup_needed' -or
            $result.claimed -ne $false -or $result.setup_needed -ne $true -or
            $result.schema_version -ne 1 -or $result.protocol_version -ne 1) {
            throw "Anonymous bootstrap projection differs from the closed contract"
        }
        $process.Kill()
        if (-not $process.WaitForExit(5000)) { throw "Web fixture process failed to stop" }
        $process.Dispose()
        $process = $null
    }
    if (-not (Test-Path -LiteralPath (Join-Path $auth "identity.sqlite"))) {
        throw "Authentication state was not persisted"
    }
    if (Test-Path -LiteralPath (Join-Path $root "brain.sqlite")) {
        throw "Independent Web fixture unexpectedly created a brain"
    }
    Write-Host "Web process smoke: PASS (empty environment, fixed loopback, kill/restart, independent auth storage)"
    Write-Host "Native service, browser, setup claim, and IP01-IP24: NOT_RUN"
}
finally {
    if ($null -ne $process) {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit(5000) | Out-Null }
        $process.Dispose()
    }
    if ($null -ne $client) { $client.Dispose() }
    $resolved = [System.IO.Path]::GetFullPath($root)
    if ((Split-Path -Parent $resolved) -ceq $temporaryParent.TrimEnd([char]92,[char]47) -and
        (Split-Path -Leaf $resolved).StartsWith("forme-web-process-", [StringComparison]::Ordinal) -and
        (Test-Path -LiteralPath $resolved)) {
        $attributes = (Get-Item -LiteralPath $resolved -Force).Attributes
        if (($attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Refusing to remove a linked Web fixture root"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    Pop-Location
}
