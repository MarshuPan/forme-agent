# Execute original read-only policy with repository-owned observations, not OS rules.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$policy = Join-Path $repo 'crates\execution\src\installed\firewall_windows.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($policy, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Native observation script has syntax errors' }
. $policy
$script:passed = 0
$script:observed = @{}
$script:reads = @()
function Get-ForMeNativeFirewallRule {
    param([string]$Name, [string]$Store)
    $script:reads += "$Name@$Store"
    return $script:observed["$Name@$Store"]
}
function Assert-That([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Expect-Refused([scriptblock]$Action) {
    $refused = $false
    try { & $Action } catch { $refused = $true }
    Assert-That $refused 'Expected scoped observation refusal'
}
function Test-Policy([string]$Label, [scriptblock]$Action) {
    $script:observed = @{}; $script:reads = @()
    & $Action
    $script:passed++
    Write-Host "[PASS] $Label"
}
function Binding([string]$Letter = 'a', [string]$Ip = '192.0.2.10') {
    return [pscustomobject]@{ name = ('forme.web.lan.' + ($Letter * 64)); ip = $Ip
        port = 8765; digest = ('sha256:' + ($Letter * 64)) }
}
function Request($Previous = $null, $Desired = (Binding)) {
    return [pscustomobject]@{ schema_version = 1; mode = 'verify'
        host_program = 'C:\Program Files\ForMe\bin\forme-hostd.exe'
        web_program = 'C:\Program Files\ForMe\bin\forme-webd.exe'
        host_config = 'C:\ProgramData\ForMe\config\host.json'
        web_config = 'C:\ProgramData\ForMe\config\web.json'
        previous = $Previous; desired = $Desired; request_digest = ('sha256:' + ('c' * 64)) }
}
function Rule($Binding) {
    return [pscustomobject]@{ Name = $Binding.name; DisplayName = $Binding.name; Group = 'forme.web.lan.v1'
        Description = $Binding.digest; Direction = 'Inbound'; Action = 'Allow'; Enabled = 'True'
        Profile = 3; EdgeTraversalPolicy = 'Block'; PrimaryStatus = 'OK'; EnforcementStatus = @('Full')
        Protocol = 'TCP'; LocalPort = @('8765'); RemotePort = @('Any'); LocalAddress = @($Binding.ip)
        RemoteAddress = @('LocalSubnet'); Program = 'C:\Program Files\ForMe\bin\forme-webd.exe'
        Package = 'Any'; Service = 'forme-web'; Authentication = 'NotRequired'; Encryption = 'NotRequired'
        OverrideBlockRules = 'False' }
}
function Observe($Binding) {
    foreach ($store in @('PersistentStore', 'ActiveStore')) { $script:observed["$($Binding.name)@$store"] = Rule $Binding }
}

Test-Policy 'program contains no firewall mutation or dynamic code commands' {
    $commands = $ast.FindAll({ param($node) $node -is [Management.Automation.Language.CommandAst] }, $true)
    foreach ($command in $commands) {
        $name = $command.GetCommandName()
        Assert-That ($null -ne $name) 'Dynamic command invocation is forbidden'
        Assert-That ($name -notmatch '(^|\\)(New|Set|Remove|Enable|Disable|Rename|Copy)-NetFirewall' -and
            $name -notin @('Invoke-Expression', 'Start-Process')) 'Native program contains a write or unbounded process command'
    }
}
Test-Policy 'exact target is independently read from persistent and effective stores' {
    $request = Request
    Observe $request.desired
    Test-ForMeFirewallBindings $request
    Assert-That ($script:reads.Count -eq 2) 'Both stores must be observed'
}
Test-Policy 'missing effective target does not become success from persistent configuration' {
    $request = Request
    $script:observed["$($request.desired.name)@PersistentStore"] = Rule $request.desired
    Expect-Refused { Test-ForMeFirewallBindings $request }
}
Test-Policy 'old product rule must already be absent from both stores' {
    $request = Request (Binding 'b') (Binding)
    Observe $request.desired
    $script:observed["$($request.previous.name)@ActiveStore"] = Rule $request.previous
    Expect-Refused { Test-ForMeFirewallBindings $request }
    $script:observed.Remove("$($request.previous.name)@ActiveStore")
    Test-ForMeFirewallBindings $request
    Assert-That ($script:reads -contains "$($request.previous.name)@PersistentStore") 'Previous persistent rule not checked'
}
Test-Policy 'LAN to loopback observes old removal without creating a replacement' {
    $request = Request (Binding) $null
    Test-ForMeFirewallBindings $request
    Assert-That ($script:reads.Count -eq 2) 'Previous binding not checked'
}
Test-Policy 'same-name range replacement accepts only the unique new address' {
    $previous = Binding 'a' '192.0.2.11'
    $request = Request $previous (Binding)
    Observe $previous
    Expect-Refused { Test-ForMeFirewallBindings $request }
    Observe $request.desired
    Test-ForMeFirewallBindings $request
}
foreach ($change in @(
    @('Group', 'foreign'), @('Description', ('sha256:' + ('e' * 64))), @('Direction', 'Outbound'),
    @('Action', 'Block'), @('Enabled', 'False'), @('Profile', 7), @('EdgeTraversalPolicy', 'Allow'),
    @('Protocol', 'UDP'), @('LocalPort', @('Any')), @('RemotePort', @('443')), @('LocalAddress', @('Any')),
    @('RemoteAddress', @('Any')), @('Program', 'Any'), @('Package', 'foreign'), @('Service', 'Any'),
    @('Authentication', 'Required'), @('OverrideBlockRules', 'True'), @('EnforcementStatus', @('FirewallOffInProfile'))
)) {
    Test-Policy ("mismatched filter refused: " + $change[0]) {
        $request = Request
        Observe $request.desired
        $script:observed["$($request.desired.name)@ActiveStore"].($change[0]) = $change[1]
        Expect-Refused { Test-ForMeFirewallBindings $request }
    }
}
Test-Policy 'closed input refuses extra fields, alternate programs and wildcard rule identities' {
    $request = Request
    $request | Add-Member -NotePropertyName shell -NotePropertyValue 'ignored'
    Expect-Refused { Assert-ForMeFirewallRequest $request }
    $request = Request
    $request.web_program = 'C:\other\forme-webd.exe'
    Expect-Refused { Assert-ForMeFirewallRequest $request }
    $request = Request
    $request.desired.name = '*'
    Expect-Refused { Assert-ForMeFirewallRequest $request }
}
Assert-That ($script:passed -eq 25) 'Not all observation policy cases ran'
Write-Host 'Firewall read-only policy: 25/25 PASS; native administrator rule effectiveness and IP09/IP24: NOT_RUN'
