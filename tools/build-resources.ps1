# Shared build-space checks; no cleanup is performed by verification gates.
function Invoke-FormeBuildResources {
    param(
        [ValidateSet('check', 'report')][string]$Command,
        [Parameter(Mandatory = $true)][string]$RepositoryRoot
    )
    if (Get-Command py -ErrorAction SilentlyContinue) {
        $resourcePython = 'py'
        $resourceArguments = @('-3')
    } else {
        $resourcePython = 'python3'
        $resourceArguments = @()
    }
    & $resourcePython @resourceArguments (Join-Path $RepositoryRoot 'tools/build_resources.py') $Command --root $RepositoryRoot
    if ($LASTEXITCODE -ne 0) { throw "Build resource $Command failed" }
}

function Assert-FormeBuildSpace {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)
    Invoke-FormeBuildResources -Command check -RepositoryRoot $RepositoryRoot
}
