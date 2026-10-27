# Original finite NTFS policy for the three installed services. No shell endpoint.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-ForMePath {
    param([Parameter(Mandatory)][string]$Path)
    if ($Path -notmatch '^[A-Za-z]:\\' -or $Path.Length -gt 240 -or
        $Path.Substring(2) -match '[:/\x00-\x1f"<>|?*]' -or
        [IO.Path]::GetFullPath($Path).TrimEnd('\') -ine $Path.TrimEnd('\')) {
        throw 'Installation path is not a normalized local path'
    }
    foreach ($component in $Path.Substring(3).Split('\')) {
        if (-not $component -or $component -match '[. ]$' -or
            $component -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') {
            throw 'Installation path contains an alias or device name'
        }
    }
    $cursor = $Path
    while ($cursor) {
        $item = $null
        try { $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop }
        catch {
            if ($_.CategoryInfo.Category -ne 'ObjectNotFound') { throw }
        }
        if ($null -ne $item -and (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
                (-not $item.PSIsContainer -and $item.LinkType -eq 'HardLink'))) {
            throw 'Installation path contains filesystem indirection'
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
}

function Get-ForMeTree {
    param([Parameter(Mandatory)][string]$Root)
    Assert-ForMePath $Root
    $queue = [Collections.Generic.Queue[string]]::new()
    $queue.Enqueue($Root)
    $count = 0
    while ($queue.Count -gt 0) {
        $path = $queue.Dequeue()
        Assert-ForMePath $path
        $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        if (++$count -gt 4096) { throw 'Installation file inventory exceeds its bound' }
        $item
        if ($item.PSIsContainer) {
            foreach ($child in @(Get-ChildItem -LiteralPath $path -Force -ErrorAction Stop)) {
                $queue.Enqueue($child.FullName)
            }
        }
    }
}

function Assert-ForMeInstallRoots {
    param([string]$ProgramRoot, [string]$DataRoot)
    Assert-ForMePath $ProgramRoot
    Assert-ForMePath $DataRoot
    if ($ProgramRoot -ine (Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'ForMe') -or
        $DataRoot -ine (Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'ForMe')) {
        throw 'This first-install helper only accepts the package default roots'
    }
}

function New-ForMeSecurity {
    param([bool]$Directory, [hashtable]$Grants = @{})
    $security = if ($Directory) { [Security.AccessControl.DirectorySecurity]::new() }
                else { [Security.AccessControl.FileSecurity]::new() }
    $security.SetAccessRuleProtection($true, $false)
    $security.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
    $permissions = @{ 'S-1-5-18' = 'FullControl'; 'S-1-5-32-544' = 'FullControl' }
    foreach ($sid in $Grants.Keys) {
        if ($permissions.ContainsKey($sid)) { throw 'Service grant cannot replace an administrator grant' }
        $permissions[$sid] = $Grants[$sid]
    }
    foreach ($sid in $permissions.Keys) {
        $rights = if ($permissions[$sid] -eq 'Traverse') {
            [Security.AccessControl.FileSystemRights]'Traverse,ReadAttributes,ReadPermissions'
        } else { [Security.AccessControl.FileSystemRights]$permissions[$sid] }
        $inheritance = if ($Directory -and $permissions[$sid] -ne 'Traverse') {
            [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'
        } else { [Security.AccessControl.InheritanceFlags]::None }
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new($sid), $rights, $inheritance,
            [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)
        $security.AddAccessRule($rule)
    }
    return $security
}

function Get-ForMeRuleSignature {
    param([Security.AccessControl.FileSystemSecurity]$Security)
    return (@($Security.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) |
        ForEach-Object {
            '{0}|{1}|{2}|{3}|{4}|{5}' -f $_.IdentityReference.Value, [int]$_.AccessControlType,
                [int]$_.FileSystemRights, [int]$_.InheritanceFlags, [int]$_.PropagationFlags, $_.IsInherited
        } | Sort-Object) -join ';')
}

function Assert-ForMeSecurity {
    param([string]$Path, [Security.AccessControl.FileSystemSecurity]$Expected)
    Assert-ForMePath $Path
    $actual = Get-Acl -LiteralPath $Path -ErrorAction Stop
    if (-not $actual.AreAccessRulesProtected -or
        $actual.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne
            $Expected.GetOwner([Security.Principal.SecurityIdentifier]).Value -or
        (Get-ForMeRuleSignature $actual) -cne (Get-ForMeRuleSignature $Expected)) {
        throw 'Installation access verification failed; explicit repair is required'
    }
}

function New-ForMeProtectedDirectory {
    param([string]$Path, [Security.AccessControl.DirectorySecurity]$Security)
    Assert-ForMePath $Path
    if (Test-Path -LiteralPath $Path) { throw 'Protected directory must be new' }
    $parent = Get-Item -LiteralPath ([IO.Path]::GetDirectoryName($Path)) -Force -ErrorAction Stop
    if (-not $parent.PSIsContainer) { throw 'Protected directory parent must exist' }
    $directory = [IO.DirectoryInfo]::new($Path)
    # Framework exposes Create(DirectorySecurity); modern .NET exposes the public
    # FileSystemAclExtensions overload. Both set the DACL at creation, before data.
    if ($PSVersionTable.PSEdition -eq 'Desktop') { $directory.Create($Security) }
    else { [IO.FileSystemAclExtensions]::Create($directory, $Security) }
    Assert-ForMeSecurity $Path $Security
}

function Set-ForMeSecurity {
    param([string]$Path, [Security.AccessControl.FileSystemSecurity]$Security)
    Assert-ForMePath $Path
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if ($PSVersionTable.PSEdition -eq 'Desktop') { $item.SetAccessControl($Security) }
    elseif ($item.PSIsContainer) {
        [IO.FileSystemAclExtensions]::SetAccessControl([IO.DirectoryInfo]::new($Path), $Security)
    } else {
        [IO.FileSystemAclExtensions]::SetAccessControl([IO.FileInfo]::new($Path), $Security)
    }
}

function Get-ForMeAccessPlan {
    param([string]$ProgramRoot, [string]$DataRoot, [string]$WebSid, [string]$BrainSid)
    if ($WebSid -eq $BrainSid -or $WebSid -notmatch '^S-1-5-80-\d+-\d+-\d+-\d+-\d+$' -or
        $BrainSid -notmatch '^S-1-5-80-\d+-\d+-\d+-\d+-\d+$') {
        throw 'Distinct virtual service SIDs are required'
    }
    Assert-ForMePath $ProgramRoot
    Assert-ForMePath $DataRoot
    $bothTraverse = @{ $WebSid = 'Traverse'; $BrainSid = 'Traverse' }
    $bothRead = @{ $WebSid = 'ReadAndExecute'; $BrainSid = 'ReadAndExecute' }
    $rows = @(
        @($ProgramRoot, $true, $bothRead), @($DataRoot, $true, $bothTraverse),
        @((Join-Path $DataRoot 'config'), $true, $bothTraverse),
        @((Join-Path $DataRoot 'config\core'), $true, @{ $BrainSid = 'ReadAndExecute' }),
        @((Join-Path $DataRoot 'config\web-network'), $true, @{ $WebSid = 'ReadAndExecute' }),
        @((Join-Path $DataRoot 'config\web-material'), $true, @{ $WebSid = 'ReadAndExecute' }),
        @((Join-Path $DataRoot 'brain'), $true, @{ $BrainSid = 'Modify' }),
        @((Join-Path $DataRoot 'web-auth'), $true, @{ $WebSid = 'Modify' }),
        @((Join-Path $DataRoot 'host-state'), $true, @{}),
        @((Join-Path $DataRoot 'keys'), $true, $bothTraverse),
        @((Join-Path $DataRoot 'keys\model-credentials'), $true, @{ $BrainSid = 'ReadAndExecute' }),
        @((Join-Path $DataRoot 'logs'), $true, $bothTraverse),
        @((Join-Path $DataRoot 'logs\host'), $true, @{}),
        @((Join-Path $DataRoot 'logs\web'), $true, @{ $WebSid = 'Modify' }),
        @((Join-Path $DataRoot 'logs\brain'), $true, @{ $BrainSid = 'Modify' }),
        @((Join-Path $DataRoot 'backups'), $true, @{}),
        @((Join-Path $DataRoot 'workspaces'), $true, @{ $BrainSid = 'Modify' }),
        @((Join-Path $DataRoot 'workspaces\default'), $true, @{ $BrainSid = 'Modify' }),
        @((Join-Path $DataRoot 'ipc'), $true, @{})
    )
    foreach ($name in @('host.json', 'bootstrap.json', 'product-template.json', 'installation.json', 'worker.json')) {
        $rows += ,@((Join-Path $DataRoot "config\$name"), $false, @{})
    }
    $rows += ,@((Join-Path $DataRoot 'config\web.json'), $false, @{ $WebSid = 'Read' })
    $rows += ,@((Join-Path $DataRoot 'config\web-network\web-network.json'), $false, @{ $WebSid = 'Read' })
    $rows += ,@((Join-Path $DataRoot 'keys\bootstrap.key'), $false, @{})
    $rows += ,@((Join-Path $DataRoot 'keys\host-maintenance.token'), $false, @{})
    foreach ($name in @('core-owner.token', 'agent-execution.token')) {
        $rows += ,@((Join-Path $DataRoot "keys\$name"), $false, @{ $BrainSid = 'Read' })
    }
    $rows += ,@((Join-Path $DataRoot 'keys\core-web.token'), $false, @{ $WebSid = 'Read'; $BrainSid = 'Read' })
    foreach ($name in @('bootstrap-web.token', 'configuration.token', 'web-assertion.key')) {
        $rows += ,@((Join-Path $DataRoot "web-auth\$name"), $false, @{ $WebSid = 'Modify' })
    }
    foreach ($row in $rows) {
        [pscustomobject]@{ Path = $row[0]; Directory = $row[1]; Grants = $row[2] }
    }
}

function Get-ForMePlannedSecurity {
    param([IO.FileSystemInfo]$Item, [object[]]$Plan)
    $entry = $Plan | Where-Object {
        $Item.FullName -ieq $_.Path -or ($_.Directory -and
            $Item.FullName.StartsWith($_.Path + '\', [StringComparison]::OrdinalIgnoreCase))
    } | Sort-Object { $_.Path.Length } -Descending | Select-Object -First 1
    if ($null -eq $entry) { throw 'File is outside the finite installation plan' }
    return New-ForMeSecurity -Directory $Item.PSIsContainer -Grants $entry.Grants
}

function Read-ForMePreparedJson {
    param([string]$Path)
    Assert-ForMePath $Path
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($stream.Length -gt 262144) { throw 'Prepared metadata exceeds its bound' }
        $bytes = [byte[]]::new(262145)
        $count = 0
        while ($count -lt $bytes.Length) {
            $read = $stream.Read($bytes, $count, $bytes.Length - $count)
            if ($read -eq 0) { break }
            $count += $read
        }
        if ($count -gt 262144) { throw 'Prepared metadata exceeds its bound' }
        return ([Text.UTF8Encoding]::new($false, $true).GetString($bytes, 0, $count) | ConvertFrom-Json)
    } finally { $stream.Dispose() }
}

function Assert-ForMePreparedLayout {
    param([string]$ProgramRoot, [string]$DataRoot)
    Assert-ForMePath $ProgramRoot
    Assert-ForMePath $DataRoot
    $template = Read-ForMePreparedJson (Join-Path $DataRoot 'config\product-template.json')
    $manifest = Read-ForMePreparedJson (Join-Path $DataRoot 'config\installation.json')
    if ($manifest.schema_version -ne 1 -or $template.schema_version -ne 1 -or
        $template.install_ref -cne $manifest.install_id -or $manifest.target -cne 'windows_x86_64') {
        throw 'Prepared installation identity does not match this package'
    }
    $expected = @{
        core_config_path = 'config\core\core.json'
        web_network_config_path = 'config\web-network\web-network.json'
        web_material_directory = 'config\web-material'
        configuration_ack_path = 'brain\runtime\core-config-ack.json'
        core_session_fence_path = 'brain\runtime\core-auth.sqlite'
        model_credentials_directory = 'keys\model-credentials'
        brain_store_path = 'brain\forme.db'; artifact_root = 'brain\artifacts'
        core_owner_credential_path = 'keys\core-owner.token'; core_web_credential_path = 'keys\core-web.token'
    }
    foreach ($field in $expected.Keys) {
        if ($template.$field -ine (Join-Path $DataRoot $expected[$field])) {
            throw 'Legacy or changed configuration paths require explicit migration'
        }
    }
    $domains = @{ releases = $ProgramRoot; config = (Join-Path $DataRoot 'config') }
    foreach ($name in @('brain', 'web-auth', 'host-state', 'keys', 'logs', 'backups', 'workspaces', 'ipc')) {
        $domains[$name.Replace('-', '_')] = Join-Path $DataRoot $name
    }
    foreach ($field in $domains.get_Keys()) {
        if ($manifest.paths.$field -ine $domains[$field]) { throw 'Installation domain binding differs' }
    }
}

function Assert-ForMePreparedInventory {
    param([string]$DataRoot, [object[]]$Plan)
    # Initial preparation is a closed inventory, not an inheritance wildcard.
    # Validate the whole tree before the caller changes any of its permissions.
    foreach ($item in @(Get-ForMeTree $DataRoot)) {
        $entry = @($Plan | Where-Object Path -IEQ $item.FullName)
        if ($entry.Count -ne 1 -or $entry[0].Directory -ne $item.PSIsContainer) {
            throw 'Unexpected data exists; retain it for explicit recovery'
        }
    }
}

Export-ModuleMember -Function Assert-ForMePath, Get-ForMeTree, New-ForMeSecurity,
    Get-ForMeRuleSignature, Assert-ForMeSecurity, New-ForMeProtectedDirectory,
    Set-ForMeSecurity, Assert-ForMeInstallRoots, Get-ForMeAccessPlan, Get-ForMePlannedSecurity,
    Assert-ForMePreparedLayout, Assert-ForMePreparedInventory
