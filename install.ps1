# Install Abbey CLI/TUI on Windows (PowerShell).
# Usage: powershell -ExecutionPolicy Bypass -File .\install.ps1

$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

Write-Host "== cargo build --release --locked =="
cargo build --release --locked

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "target" }
$bin = Join-Path $targetDir "release\abbey.exe"
if (-not (Test-Path $bin)) {
    throw "missing release binary: $bin"
}

$destDir = if ($env:LOCALAPPDATA) {
    Join-Path $env:LOCALAPPDATA "abbey\bin"
} else {
    Join-Path $env:USERPROFILE ".local\bin"
}
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$dest = Join-Path $destDir "abbey.exe"
$staged = Join-Path $destDir (".abbey-" + [System.IO.Path]::GetRandomFileName() + ".exe")
Copy-Item -Force $bin $staged
try {
    & $staged --version | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "staged Abbey binary failed its version probe"
    }
    Move-Item -Force $staged $dest
} finally {
    if (Test-Path $staged) {
        Remove-Item -Force $staged
    }
}

$version = & $dest --version
if ($LASTEXITCODE -ne 0) {
    throw "installed Abbey binary failed its version probe"
}
Write-Host "installed: $dest ($version)"

# Offer PATH hint when install dir is not already on PATH.
$pathEntries = $env:PATH -split ';'
if ($pathEntries -notcontains $destDir) {
    Write-Host ""
    Write-Host "Add to user PATH (current session):"
    Write-Host "  `$env:PATH = `"$destDir;`$env:PATH`""
    Write-Host "Persist:"
    Write-Host "  [Environment]::SetEnvironmentVariable('PATH', `"$destDir;`" + [Environment]::GetEnvironmentVariable('PATH','User'), 'User')"
}

$compDir = Join-Path $env:USERPROFILE "Documents\PowerShell\Completions"
if (Test-Path (Split-Path $compDir -Parent)) {
    New-Item -ItemType Directory -Force -Path $compDir | Out-Null
    $completion = Join-Path $compDir "_abbey.ps1"
    $stagedCompletion = Join-Path $compDir (".abbey-completion-" + [System.IO.Path]::GetRandomFileName())
    try {
        & $dest completion powershell | Out-File -Encoding utf8 $stagedCompletion
        if ($LASTEXITCODE -ne 0) {
            throw "PowerShell completion generation failed; existing file preserved"
        }
        Move-Item -Force $stagedCompletion $completion
    } finally {
        if (Test-Path $stagedCompletion) {
            Remove-Item -Force $stagedCompletion
        }
    }
    Write-Host "wrote $compDir\_abbey.ps1"
}
