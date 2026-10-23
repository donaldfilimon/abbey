# Source guard only on non-Windows hosts. Provisioned workspace ancestors must be trusted.
param([ValidateSet('Preflight', 'Cleanup')][string]$Mode = 'Preflight')
$ErrorActionPreference = 'Stop'

function Assert-NoReparse([string]$Path) {
    try { $item = Get-Item -Force -LiteralPath $Path }
    catch [System.Management.Automation.ItemNotFoundException] { return }
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'Reparse point refused; trusted runner administration must remove it'
    }
    if ($item.PSIsContainer) {
        foreach ($entry in [IO.Directory]::GetFileSystemEntries($item.FullName)) {
            Assert-NoReparse $entry
        }
    }
}
function Assert-Ancestors([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        $item = Get-Item -Force -LiteralPath $current
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Workspace ancestor reparse point refused'
        }
        $current = [IO.Path]::GetDirectoryName($current)
    }
}
# Delete individually only after a complete no-link scan. Runner must be exclusive:
# no concurrent untrusted writer may race the scan/removal (ephemeral recommended).
function Remove-OwnedTree([string]$Path) {
    try { $item = Get-Item -Force -LiteralPath $Path }
    catch [System.Management.Automation.ItemNotFoundException] { return }
    if ($item.PSIsContainer) {
        foreach ($entry in [IO.Directory]::GetFileSystemEntries($item.FullName)) {
            Remove-OwnedTree $entry
        }
    }
    Remove-Item -Force -LiteralPath $Path
}
foreach ($value in @($env:GITHUB_WORKSPACE, $env:RUNNER_TEMP)) {
    if (-not $value -or -not [IO.Path]::IsPathFullyQualified($value)) {
        throw 'Absolute provisioned workspace and temporary roots required'
    }
    Assert-Ancestors $value
}
if ($env:GITHUB_RUN_ID -notmatch '^\d+$' -or $env:GITHUB_RUN_ATTEMPT -notmatch '^\d+$') {
    throw 'Numeric run identity required'
}
$workspace = [IO.Path]::GetFullPath($env:GITHUB_WORKSPACE)
$scratch = Join-Path ([IO.Path]::GetFullPath($env:RUNNER_TEMP)) "abbey-windows-$($env:GITHUB_RUN_ID)-$($env:GITHUB_RUN_ATTEMPT)"
$owned = @('abbey', 'abi', 'wdbx') | ForEach-Object { Join-Path $workspace $_ }
$owned += $scratch
foreach ($path in $owned) { Assert-NoReparse $path }
if ($Mode -eq 'Cleanup') {
    # Script may be removed with Abbey; all definitions are already loaded.
    Set-Location $env:RUNNER_TEMP
    foreach ($path in $owned) { Remove-OwnedTree $path }
} else {
    if (Test-Path -LiteralPath $scratch) { throw 'Scratch root already exists' }
    New-Item -ItemType Directory -Path $scratch | Out-Null
    "ABBEY_WINDOWS_SCRATCH=$scratch" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
}
