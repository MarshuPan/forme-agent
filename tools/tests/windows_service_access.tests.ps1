# Repository-owned descriptor and disposable NTFS tests. Never registers services.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'This component gate needs native Windows NTFS; it must not pass by skipping'
}
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Import-Module (Join-Path $repo 'packaging\windows\service-access.psm1') -Force
$script:passed = 0
function Assert-That([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Assert-Refused([scriptblock]$Action) {
    $refused = $false
    try { $null = & $Action } catch { $refused = $true }
    Assert-That $refused 'Expected a refusal'
}
function Test-Access([string]$Name, [scriptblock]$Action) {
    & $Action
    $script:passed++
    Write-Host "[PASS] $Name"
}
$web = 'S-1-5-80-11-22-33-44-55'
$brain = 'S-1-5-80-66-77-88-99-100'
$tempRoot = [IO.Path]::GetTempPath().TrimEnd('\')
$scratch = Join-Path $tempRoot ('forme-access-test-' + [Guid]::NewGuid().ToString('N'))
$null = [IO.Directory]::CreateDirectory($scratch)
$currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value

function Test-Security([bool]$Directory, [hashtable]$Grants = @{}) {
    $testGrants = @{} + $Grants
    $testGrants[$currentSid] = 'FullControl'
    $security = New-ForMeSecurity -Directory $Directory -Grants $testGrants
    $security.SetOwner([Security.Principal.SecurityIdentifier]::new($currentSid))
    return $security
}
function Service-Rules($Security, [string]$Sid) {
    return @($Security.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) |
        Where-Object { $_.IdentityReference.Value -eq $Sid })
}
try {
    $program = Join-Path $scratch 'program'
    $data = Join-Path $scratch 'data'
    $plan = @(Get-ForMeAccessPlan $program $data $web $brain)
    Test-Access 'closed role matrix separates management, Web, Brain and absent workers' {
        Assert-That ($plan.Count -eq 29) 'Unexpected matrix size'
        foreach ($row in $plan) {
            $security = New-ForMeSecurity -Directory $row.Directory -Grants $row.Grants
            Assert-That $security.AreAccessRulesProtected 'Inherited broad grants must be removed'
            $sids = @($security.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) |
                ForEach-Object { $_.IdentityReference.Value })
            Assert-That (@($sids | Where-Object { $_ -notin @('S-1-5-18', 'S-1-5-32-544', $web, $brain) }).Count -eq 0) 'Unexpected principal'
        }
        foreach ($relative in @('host-state', 'backups', 'ipc', 'keys\bootstrap.key', 'keys\host-maintenance.token')) {
            $row = $plan | Where-Object Path -EQ (Join-Path $data $relative)
            Assert-That ($row.Grants.Count -eq 0) 'Management must remain private'
        }
        foreach ($pair in @(@('brain', $brain), @('web-auth', $web), @('workspaces', $brain))) {
            $row = $plan | Where-Object Path -EQ (Join-Path $data $pair[0])
            Assert-That ($row.Grants.Count -eq 1 -and $row.Grants[$pair[1]] -eq 'Modify') 'Wrong data writer'
        }
        $model = $plan | Where-Object Path -EQ (Join-Path $data 'keys\model-credentials')
        Assert-That ($model.Grants.Count -eq 1 -and $model.Grants[$brain] -eq 'ReadAndExecute') 'Models are Brain read-only'
        $owner = $plan | Where-Object Path -EQ (Join-Path $data 'keys\core-owner.token')
        Assert-That (-not $owner.Grants.ContainsKey($web)) 'Web cannot impersonate CLI owner'
        $bridge = $plan | Where-Object Path -EQ (Join-Path $data 'keys\core-web.token')
        Assert-That ($bridge.Grants[$web] -eq 'Read' -and $bridge.Grants[$brain] -eq 'Read') 'Bridge must be shared read-only'
    }
    Test-Access 'parent traversal does not inherit access to neighboring credentials' {
        foreach ($relative in @('', 'config', 'keys', 'logs')) {
            $path = if ($relative) { Join-Path $data $relative } else { $data }
            $row = $plan | Where-Object Path -EQ $path
            $security = New-ForMeSecurity -Directory $true -Grants $row.Grants
            foreach ($sid in @($web, $brain)) {
                $rules = @(Service-Rules $security $sid)
                Assert-That ($rules.Count -eq 1 -and $rules[0].InheritanceFlags -eq 'None') 'Traversal propagated'
                Assert-That (($rules[0].FileSystemRights -band [Security.AccessControl.FileSystemRights]::WriteData) -eq 0) 'Traversal became write'
                Assert-That (($rules[0].FileSystemRights -band [Security.AccessControl.FileSystemRights]::ReadData) -eq 0) 'Traversal became directory listing'
            }
        }
        Assert-Refused { Get-ForMeAccessPlan $program $data $web $web }
        Assert-Refused { Get-ForMeAccessPlan $program $data 'S-1-5-18' $brain }
        Assert-Refused { New-ForMeSecurity -Directory $true -Grants @{ 'S-1-5-18' = 'Read' } }
    }
    Test-Access 'atomic directory creation and exact ACL verification detect tampering' {
        $directory = Join-Path $scratch 'protected'
        $security = Test-Security $true @{ $brain = 'ReadAndExecute' }
        New-ForMeProtectedDirectory $directory $security
        Assert-ForMeSecurity $directory $security
        Assert-Refused { New-ForMeProtectedDirectory $directory $security }
        $altered = Test-Security $true @{ $brain = 'ReadAndExecute' }
        $altered.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new($web), 'Modify', 'Allow'))
        Set-ForMeSecurity $directory $altered
        Assert-Refused { Assert-ForMeSecurity $directory $security }
        Set-ForMeSecurity $directory $security
        Assert-ForMeSecurity $directory $security
        $unprotected = Test-Security $true @{ $brain = 'ReadAndExecute' }
        $unprotected.SetAccessRuleProtection($false, $false)
        Set-ForMeSecurity $directory $unprotected
        Assert-Refused { Assert-ForMeSecurity $directory $security }
        Set-ForMeSecurity $directory $security
    }
    Test-Access 'new revisions and replacement files inherit Brain read-only access' {
        $directory = Join-Path $scratch 'published'
        $security = Test-Security $true @{ $brain = 'ReadAndExecute' }
        New-ForMeProtectedDirectory $directory $security
        $active = Join-Path $directory 'core.json'
        [IO.File]::WriteAllText($active, 'revision-one')
        $stage = Join-Path $directory 'staged.json'
        [IO.File]::WriteAllText($stage, 'revision-two')
        # A staged sibling receives the same inheritance before publication.
        foreach ($path in @($active, $stage)) {
            $rules = @(Service-Rules (Get-Acl -LiteralPath $path) $brain)
            Assert-That ($rules.Count -eq 1 -and $rules[0].IsInherited) 'Read grant did not inherit'
            Assert-That (($rules[0].FileSystemRights -band [Security.AccessControl.FileSystemRights]::WriteData) -eq 0) 'Brain can modify published config'
            Assert-That (@(Service-Rules (Get-Acl -LiteralPath $path) $web).Count -eq 0) 'Web can read published config'
        }
        if ($PSVersionTable.PSEdition -eq 'Desktop') { [IO.File]::Replace($stage, $active, (Join-Path $directory 'previous.json')) }
        else { [IO.File]::Move($stage, $active, $true) }
        Assert-That ([IO.File]::ReadAllText($active) -eq 'revision-two') 'Replacement did not happen'
        $rules = @(Service-Rules (Get-Acl -LiteralPath $active) $brain)
        Assert-That ($rules.Count -eq 1 -and ($rules[0].FileSystemRights -band [Security.AccessControl.FileSystemRights]::WriteData) -eq 0) 'Replacement widened access'
        $revision = Join-Path $directory 'credential-revision'
        $null = [IO.Directory]::CreateDirectory($revision)
        $credential = Join-Path $revision 'fixture.secret'
        [IO.File]::WriteAllText($credential, 'repository-fixture-only')
        $rules = @(Service-Rules (Get-Acl -LiteralPath $credential) $brain)
        Assert-That ($rules.Count -eq 1 -and $rules[0].IsInherited) 'Nested revision lost read access'
    }
    Test-Access 'unsafe paths, junctions and hard links are refused without touching targets' {
        foreach ($unsafe in @('relative', ($scratch + '\..\escape'), ($scratch + '\alias.'),
            ($scratch + '\key:stream'), ($scratch + '\NUL'), ('\\?\' + $scratch))) {
            Assert-Refused { Assert-ForMePath $unsafe }
        }
        $outside = Join-Path $scratch 'outside'
        $null = [IO.Directory]::CreateDirectory($outside)
        $marker = Join-Path $outside 'marker'
        [IO.File]::WriteAllText($marker, 'preserve')
        $junction = Join-Path $scratch 'junction'
        $null = New-Item -ItemType Junction -Path $junction -Target $outside
        Assert-Refused { Get-ForMeTree $junction }
        $hardlink = Join-Path $scratch 'hardlink'
        $null = New-Item -ItemType HardLink -Path $hardlink -Target $marker
        Assert-Refused { Assert-ForMePath $hardlink }
        Assert-That ([IO.File]::ReadAllText($marker) -eq 'preserve') 'Target was modified'
        # Remove only the fixture links, never recurse through either target.
        [IO.File]::Delete($hardlink)
        [IO.Directory]::Delete($junction)
    }
    Test-Access 'first-install entry rejects arbitrary roots with zero fixture writes' {
        $parseTokens = $null; $parseErrors = $null
        $null = [Management.Automation.Language.Parser]::ParseFile(
            (Join-Path $repo 'packaging\windows\install-access.ps1'), [ref]$parseTokens, [ref]$parseErrors)
        Assert-That ($parseErrors.Count -eq 0) 'Installer entry contains a syntax error'
        Assert-Refused { Assert-ForMeInstallRoots $program $data }
        Assert-Refused { Assert-ForMeInstallRoots $data $program }
        $marker = Join-Path $scratch 'preserve.txt'
        [IO.File]::WriteAllText($marker, 'owner-fixture')
        $before = @(Get-ChildItem -LiteralPath $scratch -Recurse -Force | ForEach-Object FullName)
        $shell = (Get-Process -Id $PID).Path
        $arguments = @('-NoLogo', '-NoProfile', '-NonInteractive', '-File',
            (Join-Path $repo 'packaging\windows\install-access.ps1'), '-Mode', 'Prepare',
            '-ProgramRoot', $program, '-DataRoot', $data)
        $previous = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try { $output = @(& $shell @arguments 2>&1); $code = $LASTEXITCODE }
        finally { $ErrorActionPreference = $previous }
        Assert-That ($code -ne 0) 'Arbitrary-root provisioning unexpectedly succeeded'
        Assert-That (($output -join ' ') -match 'forme installation stopped') 'Refusal was not an installer refusal'
        $after = @(Get-ChildItem -LiteralPath $scratch -Recurse -Force | ForEach-Object FullName)
        Assert-That (($before -join '|') -ceq ($after -join '|')) 'Refused operation changed files'
        Assert-That ([IO.File]::ReadAllText($marker) -eq 'owner-fixture') 'Existing fixture changed'
    }
    Test-Access 'real hostctl preparation matches native layout and preserves rejected legacy input' {
        $hostctl = Join-Path $repo 'target\debug\forme-hostctl.exe'
        $arguments = @('prepare-installation', '--install-root', $program, '--state-root', $data,
            '--config-root', (Join-Path $data 'config'), '--log-root', (Join-Path $data 'logs'),
            '--ipc-root', (Join-Path $data 'ipc'), '--target', 'windows_x86_64',
            '--release-id', 'access-fixture', '--release-digest', ('sha256:' + ('a' * 64)))
        $output = @(& $hostctl @arguments)
        Assert-That ($LASTEXITCODE -eq 0) 'Installation preparation failed'
        Assert-ForMePreparedLayout $program $data
        $templatePath = Join-Path $data 'config\product-template.json'
        $original = [IO.File]::ReadAllText($templatePath)
        $keyPath = Join-Path $data 'keys\bootstrap.key'
        $keyHash = (Get-FileHash -LiteralPath $keyPath).Hash
        foreach ($change in @('legacy', 'foreign', 'oversized')) {
            $template = $original | ConvertFrom-Json
            if ($change -eq 'legacy') { $template.core_config_path = Join-Path $data 'config\core.json' }
            if ($change -eq 'foreign') { $template.install_ref = 'installation:foreign' }
            $changed = if ($change -eq 'oversized') { ' ' * 262145 } else { $template | ConvertTo-Json -Depth 8 }
            [IO.File]::WriteAllText($templatePath, $changed)
            Assert-Refused { Assert-ForMePreparedLayout $program $data }
            Assert-That ([IO.File]::ReadAllText($templatePath) -ceq $changed) 'Rejected metadata was rewritten'
        }
        Assert-That ((Get-FileHash -LiteralPath $keyPath).Hash -eq $keyHash) 'Key was replaced'
        Assert-That (@(Get-ChildItem -LiteralPath (Join-Path $data 'brain') -Force).Count -eq 0) 'New brain was prematurely initialized'
    }
    Assert-That ($script:passed -eq 7) 'Component cases did not all run'
    Write-Host 'Windows access component: 7/7 PASS; native services, service-token denial and IP01-IP24: NOT_RUN'
} finally {
    # Only this independently created fixture is disposable; validate exact scope.
    Assert-That ([IO.Path]::GetDirectoryName($scratch) -ieq $tempRoot) 'Unsafe fixture cleanup root'
    Assert-That ([IO.Path]::GetFileName($scratch).StartsWith('forme-access-test-')) 'Unsafe fixture name'
    $null = @(Get-ForMeTree $scratch)
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
