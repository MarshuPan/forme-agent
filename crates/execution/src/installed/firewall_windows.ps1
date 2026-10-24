# Original read-only OS observation. Loaded only by the fixed Host adapter.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-ForMeFirewallShape {
    param($Value, [string[]]$Fields)
    if ($null -eq $Value -or $Value -isnot [pscustomobject] -or
        ((@($Value.PSObject.Properties.Name | Sort-Object) -join ',') -cne
         (@($Fields | Sort-Object) -join ','))) { throw 'Invalid firewall observation input' }
}

function Assert-ForMeFirewallBinding {
    param($Binding)
    if ($null -eq $Binding) { return }
    Assert-ForMeFirewallShape $Binding @('name', 'ip', 'port', 'digest')
    if ($Binding.name -isnot [string] -or $Binding.name -cnotmatch '^forme\.web\.lan\.[a-f0-9]{64}$' -or
        $Binding.digest -isnot [string] -or $Binding.digest -cnotmatch '^sha256:[a-f0-9]{64}$' -or
        $Binding.ip -isnot [string] -or
        ($Binding.port -isnot [int] -and $Binding.port -isnot [long]) -or
        $Binding.port -lt 1 -or $Binding.port -gt 65535) { throw 'Invalid scoped firewall binding' }
    $address = [Net.IPAddress]::Parse($Binding.ip)
    if ($address.ToString() -cne $Binding.ip -or [Net.IPAddress]::IsLoopback($address) -or
        $address.Equals([Net.IPAddress]::Any) -or $address.Equals([Net.IPAddress]::IPv6Any) -or
        $address.IsIPv6Multicast) { throw 'Invalid LAN address' }
    $bytes = $address.GetAddressBytes()
    if ($bytes.Length -eq 4 -and $bytes[0] -ge 224) { throw 'Invalid LAN address' }
}

function Assert-ForMeFirewallRequest {
    param($Request)
    Assert-ForMeFirewallShape $Request @('schema_version', 'mode', 'host_program', 'web_program',
        'host_config', 'web_config', 'previous', 'desired', 'request_digest')
    if (($Request.schema_version -isnot [int] -and $Request.schema_version -isnot [long]) -or
        $Request.schema_version -ne 1 -or $Request.mode -isnot [string] -or $Request.mode -cnotin @('check', 'verify') -or
        $Request.request_digest -isnot [string] -or
        $Request.request_digest -cnotmatch '^sha256:[a-f0-9]{64}$') { throw 'Invalid observation schema' }
    foreach ($name in @('host_program', 'web_program', 'host_config', 'web_config')) {
        $path = $Request.$name
        if ($path -isnot [string] -or $path -notmatch '^[A-Za-z]:\\' -or
            $path -match '[\x00-\x1f"<>|?*]' -or $path.Substring(2).Contains(':') -or
            [IO.Path]::GetFullPath($path) -ine $path) { throw 'Invalid fixed installation path' }
    }
    if ([IO.Path]::GetFileName($Request.host_program) -cne 'forme-hostd.exe' -or
        [IO.Path]::GetFileName($Request.web_program) -cne 'forme-webd.exe' -or
        [IO.Path]::GetFileName($Request.host_config) -cne 'host.json' -or
        [IO.Path]::GetFileName($Request.web_config) -cne 'web.json' -or
        [IO.Path]::GetDirectoryName($Request.host_program) -ine [IO.Path]::GetDirectoryName($Request.web_program) -or
        [IO.Path]::GetDirectoryName($Request.host_config) -ine [IO.Path]::GetDirectoryName($Request.web_config)) {
        throw 'Unbound installed network paths'
    }
    Assert-ForMeFirewallBinding $Request.previous
    Assert-ForMeFirewallBinding $Request.desired
    if ($Request.mode -eq 'check' -and ($null -ne $Request.previous -or $null -ne $Request.desired)) {
        throw 'Capability check cannot carry rule targets'
    }
}

function Assert-ForMeFirewallProtectedPath {
    param([string]$Path)
    $trustedInstaller = [Security.Principal.NTAccount]::new('NT SERVICE\TrustedInstaller').Translate(
        [Security.Principal.SecurityIdentifier]).Value
    $writers = @('S-1-5-18', 'S-1-5-32-544', $trustedInstaller)
    $mask = [Security.AccessControl.FileSystemRights]'Write,Delete,DeleteSubdirectoriesAndFiles,ChangePermissions,TakeOwnership'
    $cursor = $Path
    while ([IO.Path]::GetDirectoryName($cursor)) {
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
            (-not $item.PSIsContainer -and $item.LinkType -eq 'HardLink')) { throw 'Indirect native path' }
        $security = Get-Acl -LiteralPath $cursor -ErrorAction Stop
        if ($security.GetOwner([Security.Principal.SecurityIdentifier]).Value -notin $writers) {
            throw 'Native path owner is not privileged'
        }
        foreach ($ace in $security.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
            if ($ace.AccessControlType -eq 'Allow' -and ($ace.FileSystemRights -band $mask) -ne 0 -and
                $ace.IdentityReference.Value -notin $writers) { throw 'Native path has an untrusted writer' }
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
}

function Assert-ForMeFirewallEnvironment {
    param($Request)
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or
        [Environment]::SystemDirectory -ine 'C:\Windows\System32') { throw 'Native OS layout unavailable' }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $principal = [Security.Principal.WindowsPrincipal]::new($identity)
        if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            throw 'Installed administrator token unavailable'
        }
    } finally { $identity.Dispose() }
    foreach ($path in @($Request.host_program, $Request.web_program, $Request.host_config, $Request.web_config,
            'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe')) {
        Assert-ForMeFirewallProtectedPath $path
    }
    $module = 'C:\Windows\System32\WindowsPowerShell\v1.0\Modules\NetSecurity\NetSecurity.psd1'
    Assert-ForMeFirewallProtectedPath $module
    Import-Module -Name $module -ErrorAction Stop
}

# OS reads are isolated here so repository fixtures can test the policy without
# invoking any native firewall cmdlet. There are no native write commands.
function Get-ForMeNativeFirewallRule {
    param([string]$Name, [string]$Store)
    try { $rules = @(NetSecurity\Get-NetFirewallRule -Name $Name -PolicyStore $Store -ErrorAction Stop) }
    catch {
        if ($_.CategoryInfo.Category -eq 'ObjectNotFound') { return $null }
        throw
    }
    if ($rules.Count -eq 0) { return $null }
    if ($rules.Count -ne 1) { throw 'Non-unique native rule identity' }
    $rule = $rules[0]
    $port = @(NetSecurity\Get-NetFirewallPortFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop)
    $address = @(NetSecurity\Get-NetFirewallAddressFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop)
    $program = @(NetSecurity\Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop)
    $service = @(NetSecurity\Get-NetFirewallServiceFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop)
    $security = @(NetSecurity\Get-NetFirewallSecurityFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop)
    if ($port.Count -ne 1 -or $address.Count -ne 1 -or $program.Count -ne 1 -or
        $service.Count -ne 1 -or $security.Count -ne 1) { throw 'Ambiguous native rule filters' }
    return [pscustomobject]@{
        Name = $rule.Name; DisplayName = $rule.DisplayName; Group = $rule.Group; Description = $rule.Description
        Direction = [string]$rule.Direction; Action = [string]$rule.Action; Enabled = [string]$rule.Enabled
        Profile = [int]$rule.Profile; EdgeTraversalPolicy = [string]$rule.EdgeTraversalPolicy
        PrimaryStatus = [string]$rule.PrimaryStatus; EnforcementStatus = @($rule.EnforcementStatus | ForEach-Object { [string]$_ })
        Protocol = [string]$port[0].Protocol; LocalPort = @($port[0].LocalPort); RemotePort = @($port[0].RemotePort)
        LocalAddress = @($address[0].LocalAddress); RemoteAddress = @($address[0].RemoteAddress)
        Program = [string]$program[0].Program; Package = [string]$program[0].Package; Service = [string]$service[0].Service
        Authentication = [string]$security[0].Authentication; Encryption = [string]$security[0].Encryption
        OverrideBlockRules = [string]$security[0].OverrideBlockRules
    }
}

function Assert-ForMeObservedFirewallRule {
    param($Rule, $Binding, [string]$Program, [bool]$Effective)
    if ($null -eq $Rule -or $Rule.Name -cne $Binding.name -or $Rule.DisplayName -cne $Binding.name -or
        $Rule.Group -cne 'forme.web.lan.v1' -or $Rule.Description -cne $Binding.digest -or
        $Rule.Direction -cne 'Inbound' -or $Rule.Action -cne 'Allow' -or $Rule.Enabled -cne 'True' -or
        $Rule.Profile -ne 3 -or $Rule.EdgeTraversalPolicy -cne 'Block' -or
        $Rule.Protocol -notin @('TCP', '6') -or $Rule.LocalPort.Count -ne 1 -or
        [string]$Rule.LocalPort[0] -cne [string]$Binding.port -or $Rule.RemotePort.Count -ne 1 -or
        $Rule.RemotePort[0] -cne 'Any' -or $Rule.LocalAddress.Count -ne 1 -or
        $Rule.LocalAddress[0] -cne $Binding.ip -or $Rule.RemoteAddress.Count -ne 1 -or
        $Rule.RemoteAddress[0] -cne 'LocalSubnet' -or $Rule.Program -ine $Program -or
        $Rule.Package -cne 'Any' -or $Rule.Service -cne 'forme-web' -or
        $Rule.Authentication -cne 'NotRequired' -or $Rule.Encryption -cne 'NotRequired' -or
        $Rule.OverrideBlockRules -cne 'False') { throw 'Native firewall rule differs from its exact product binding' }
    if ($Effective -and ($Rule.PrimaryStatus -cne 'OK' -or $Rule.EnforcementStatus.Count -ne 1 -or
            $Rule.EnforcementStatus[0] -cne 'Full')) { throw 'Native firewall rule is not fully effective' }
}

function Test-ForMeFirewallBindings {
    param($Request)
    Assert-ForMeFirewallRequest $Request
    if ($null -ne $Request.previous -and
        ($null -eq $Request.desired -or $Request.previous.name -cne $Request.desired.name)) {
        foreach ($store in @('PersistentStore', 'ActiveStore')) {
            if ($null -ne (Get-ForMeNativeFirewallRule $Request.previous.name $store)) {
                throw 'Administrator must remove the previous product rule before applying the network'
            }
        }
    }
    if ($null -ne $Request.desired) {
        foreach ($store in @('PersistentStore', 'ActiveStore')) {
            $rule = Get-ForMeNativeFirewallRule $Request.desired.name $store
            Assert-ForMeObservedFirewallRule $rule $Request.desired $Request.web_program ($store -eq 'ActiveStore')
        }
    }
}

function Invoke-ForMeFirewallInput {
    try {
        [Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        $buffer = [char[]]::new(16385)
        $length = 0
        while ($length -lt $buffer.Length) {
            $count = [Console]::In.Read($buffer, $length, ($buffer.Length - $length))
            if ($count -eq 0) { break }
            $length += $count
        }
        $text = [string]::new($buffer, 0, $length)
        if ($length -gt 16384 -or [Text.Encoding]::UTF8.GetByteCount($text) -gt 16384) { throw 'Input bound' }
        $request = $text | ConvertFrom-Json -ErrorAction Stop
        Assert-ForMeFirewallRequest $request
        Assert-ForMeFirewallEnvironment $request
        if ($request.mode -eq 'verify') { Test-ForMeFirewallBindings $request }
        $receipt = [ordered]@{ schema_version = 1; request_digest = $request.request_digest; completed = $true }
        [Console]::Out.WriteLine(($receipt | ConvertTo-Json -Compress))
    } catch {
        [Console]::Error.WriteLine('Native firewall verification unavailable; inspect the original operation')
        exit 1
    }
}
