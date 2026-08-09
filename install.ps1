# Install Abbey CLI/TUI on Windows (PowerShell).
# Usage: powershell -ExecutionPolicy Bypass -File .\install.ps1
#
# Extra cargo features, e.g. to build and install the separately named personal
# edition (src/edition.rs) — same variable install.sh reads:
#   $env:ABBEY_CARGO_FEATURES = "personal-edition"
#   powershell -ExecutionPolicy Bypass -File .\install.ps1

$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

if ($env:ABBEY_CARGO_FEATURES) {
    Write-Host "== cargo build --release --locked --features $env:ABBEY_CARGO_FEATURES =="
    cargo build --release --locked --features $env:ABBEY_CARGO_FEATURES
} else {
    Write-Host "== cargo build --release --locked =="
    cargo build --release --locked
}

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "target" }
# Cargo's [[bin]] target name, deliberately NOT an edition name: both editions
# compile the same `abbey` target, so the build *output* path is identical and
# only the *installed* names below differ. Deriving this from the edition would
# make a personal-edition install fail with "missing release binary".
$cargoBinName = "abbey"
$bin = Join-Path $targetDir "release\$($cargoBinName).exe"
if (-not (Test-Path $bin)) {
    throw "missing release binary: $bin"
}

# Install under the *compiled edition's* names, exactly as install.sh does. A
# personal-edition build must never overwrite the safe edition's binary or
# completion, so the names come from the binary itself (src/edition.rs) rather
# than from a literal repeated here. There is deliberately no fallback literal:
# guessing "abbey" when the probe fails is precisely the clobber this prevents.
$editionBin = (& $bin edition --name | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $editionBin) {
    throw "could not read the compiled edition binary name from $bin"
}
$editionDaemon = (& $bin edition --daemon-name | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $editionDaemon) {
    throw "could not read the compiled edition daemon name from $bin"
}
$binFileName = "$($editionBin).exe"
$daemonFileName = "$($editionDaemon).exe"
$completionFileName = "_$($editionBin).ps1"

$destDir = if ($env:LOCALAPPDATA) {
    Join-Path $env:LOCALAPPDATA "abbey\bin"
} else {
    Join-Path $env:USERPROFILE ".local\bin"
}
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$dest = Join-Path $destDir $binFileName
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

# The authenticated daemon is Unix-socket-only until a named-pipe transport
# lands, so nothing is installed for it here. Its edition-scoped name is still
# reported so this script and install.sh agree on one identity source.
Write-Host "not installed: $daemonFileName (the authenticated daemon is Unix-socket-only)"

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
    $completion = Join-Path $compDir $completionFileName
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
    Write-Host "wrote $completion"
}
