#Requires -Version 7.2
<#
Native Windows only. Default mode runs hostile ZIP + cached-signature tests; it
never supplies a valid installer to install(). Requires Python 3 and OpenSSL
with Ed25519 support on PATH, and Developer Mode or symlink privileges.
InstallerAcceptance is an explicit operator mode for a disposable Windows VM.
The operator supplies a local acceptance script implementing the documented
safe/personal NSIS/MSI, restart/state and failed-update recovery procedure.
#>
[CmdletBinding()]
param(
    [ValidateSet('Tests', 'InstallerAcceptance')][string]$Mode = 'Tests',
    [string]$Python = 'python',
    [string]$AcceptanceScript,
    [switch]$DisposableVm
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'UNEXECUTED: native Windows is required' }
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Set-Location $Repo
$Run = Join-Path $Repo ('.work/updater-windows/' + (Get-Date -Format 'yyyyMMddTHHmmss') + '-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $Run
$env:CARGO_TARGET_DIR = Join-Path $Run 'target'
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:ABI_WDBX_PATH = ':memory:'
Remove-Item Env:ABI_WDBX_PERSIST -ErrorAction SilentlyContinue
Remove-Item Env:ABBEY_DESKTOP_UPDATE_ARTIFACT -ErrorAction SilentlyContinue
foreach ($name in @('RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER')) {
    Remove-Item "Env:$name" -ErrorAction SilentlyContinue
}
$env:ABBEY_UPDATER_QUAL_ROOT = Join-Path $Run 'temp'
$env:ABBEY_ZIP_PYTHON = (Get-Command $Python -CommandType Application).Source
$env:ABBEY_ZIP_OPENSSL = (Get-Command openssl -CommandType Application).Source
$null = New-Item -ItemType Directory -Path $env:ABBEY_UPDATER_QUAL_ROOT
$env:TEMP = $env:ABBEY_UPDATER_QUAL_ROOT
$env:TMP = $env:TEMP
function Source-Hashes {
    $hashes = [ordered]@{}
    $paths = @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'desktop/src-tauri/Cargo.toml', 'desktop/scripts/qualify-updater-windows.ps1')
    $paths += @(& git ls-files -- src tools .cargo build.rs desktop/src-tauri)
    $paths += @(Get-ChildItem desktop/src-tauri/tests -Recurse -File | ForEach-Object { [IO.Path]::GetRelativePath($Repo, $_.FullName) })
    foreach ($path in ($paths | Sort-Object -Unique)) { $hashes[$path] = (Get-FileHash -Algorithm SHA256 $path).Hash.ToLowerInvariant() }
    return $hashes
}
function Invoke-Recorded([string]$Label, [string]$Exe, [string[]]$Arguments) {
    $receipt = [ordered]@{
        command = @($Exe) + $Arguments; start = [DateTimeOffset]::UtcNow.ToString('o')
        source_before = Source-Hashes; stdin = 'closed'; mode = $Mode
        head = (& git rev-parse HEAD); native_os = [Runtime.InteropServices.RuntimeInformation]::OSDescription
        siblings = @{ abi = (& git -C ../abi rev-parse HEAD); wdbx = (& git -C ../wdbx rev-parse HEAD) }
        environment = [ordered]@{
            CARGO_TARGET_DIR = $env:CARGO_TARGET_DIR; CARGO_INCREMENTAL = $env:CARGO_INCREMENTAL
            CARGO_PROFILE_DEV_DEBUG = $env:CARGO_PROFILE_DEV_DEBUG; CARGO_PROFILE_TEST_DEBUG = $env:CARGO_PROFILE_TEST_DEBUG
            ABI_WDBX_PATH = $env:ABI_WDBX_PATH; ABI_WDBX_PERSIST = $null; RUSTC = $env:RUSTC
            TEMP = $env:TEMP; TMP = $env:TMP; ABBEY_ZIP_PYTHON = $env:ABBEY_ZIP_PYTHON; ABBEY_ZIP_OPENSSL = $env:ABBEY_ZIP_OPENSSL
        }
    }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = [Diagnostics.ProcessStartInfo]::new($Exe)
    $process.StartInfo.WorkingDirectory = $Repo
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.RedirectStandardInput = $true
    $process.StartInfo.RedirectStandardOutput = $true
    $process.StartInfo.RedirectStandardError = $true
    foreach ($arg in $Arguments) { $process.StartInfo.ArgumentList.Add($arg) }
    try {
        $null = $process.Start(); $process.StandardInput.Close()
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(1800000)) {
            $process.Kill($true); $process.WaitForExit(); $receipt.timeout = $true
        }
        $stdout.GetAwaiter().GetResult() | Set-Content (Join-Path $Run "$Label.stdout")
        $stderr.GetAwaiter().GetResult() | Set-Content (Join-Path $Run "$Label.stderr")
        $receipt.exit = $process.ExitCode
    } catch {
        $receipt.exit = 127; $receipt.error = $_.Exception.Message
    } finally {
        $receipt.end = [DateTimeOffset]::UtcNow.ToString('o'); $receipt.source_after = Source-Hashes
        $receipt | ConvertTo-Json -Depth 12 | Set-Content (Join-Path $Run "$Label.json")
        $process.Dispose()
    }
    if ($receipt.exit -ne 0 -or $receipt.timeout) { throw "$Label failed; receipts retained in $Run" }
}
try {
    $Rustup = (Get-Command rustup -CommandType Application).Source
    Invoke-Recorded 'which-cargo' $Rustup @('which', '--toolchain', '1.99.1', 'cargo')
    $Cargo = (Get-Content (Join-Path $Run 'which-cargo.stdout') -Raw).Trim()
    Invoke-Recorded 'which-rustc' $Rustup @('which', '--toolchain', '1.99.1', 'rustc')
    $Rustc = (Get-Content (Join-Path $Run 'which-rustc.stdout') -Raw).Trim()
    Invoke-Recorded 'compiler' $Rustc @('-vV')
    $Compiler = Get-Content (Join-Path $Run 'compiler.stdout') -Raw
    if ($Compiler -notmatch '(?m)^release: 1\.99\.1\r?$') { throw 'Rust 1.99.1 required' }
    if ((Get-Content rust-toolchain.toml -Raw) -notmatch 'channel\s*=\s*"1\.99\.1"') { throw 'repository compiler pin changed' }
    $Arch = switch ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
        'Arm64' { 'aarch64' }; 'X64' { 'x86_64' }; 'X86' { 'i686' }; default { throw 'unsupported native OS architecture' }
    }
    if ($Compiler -notmatch "(?m)^host: $Arch-pc-windows-msvc\r?`$") { throw 'native MSVC compiler matching OS architecture required' }
    $env:RUSTC = $Rustc
    Invoke-Recorded 'metadata' $Cargo @('metadata', '--locked', '--format-version', '1')
    $Metadata = Get-Content (Join-Path $Run 'metadata.stdout') -Raw | ConvertFrom-Json
    $Updater = @($Metadata.packages | Where-Object name -eq 'tauri-plugin-updater')
    if ($Updater.Count -ne 1 -or $Updater[0].source -ne 'git+https://github.com/tauri-apps/plugins-workspace?rev=ba98bfadc4c868c9a7e1a226c83af528e1f11b89#ba98bfadc4c868c9a7e1a226c83af528e1f11b89') { throw 'unexpected updater source' }
    $UpstreamRoot = Split-Path (Split-Path (Split-Path $Updater[0].manifest_path -Parent) -Parent) -Parent
    $Git = (Get-Command git -CommandType Application).Source
    Invoke-Recorded 'upstream-head' $Git @('-C', $UpstreamRoot, 'rev-parse', 'HEAD')
    if ((Get-Content (Join-Path $Run 'upstream-head.stdout') -Raw).Trim() -ne 'ba98bfadc4c868c9a7e1a226c83af528e1f11b89') { throw 'upstream checkout head differs' }
    Invoke-Recorded 'upstream-clean' $Git @('-C', $UpstreamRoot, 'diff', '--exit-code', 'HEAD', '--', 'plugins/updater')
    $Zip = @($Metadata.packages | Where-Object name -eq 'zip')
    if ($Zip.Count -ne 1 -or $Zip[0].version -ne '9.0.5') { throw 'unexpected ZIP graph' }
    if ($Mode -eq 'Tests') {
        Invoke-Recorded 'safe-windows-zip' $Cargo @('test', '--locked', '-p', 'abbey-desktop', '--test', 'updater_windows_zip', '--', '--test-threads=1')
        Invoke-Recorded 'personal-windows-zip' $Cargo @('test', '--locked', '-p', 'abbey-desktop', '--features', 'personal-edition', '--test', 'updater_windows_zip', '--', '--test-threads=1')
        Invoke-Recorded 'zip-dependency-contract' $Cargo @('test', '--locked', '-p', 'abbey-desktop', '--test', 'updater_zip_contract', '--', '--test-threads=1')
        Invoke-Recorded 'safe-cached' $Cargo @('test', '--locked', '-p', 'abbey-desktop', '--test', 'updater_signature', '--test', 'updater_cached_verification')
        Invoke-Recorded 'personal-cached' $Cargo @('test', '--locked', '-p', 'abbey-desktop', '--features', 'personal-edition', '--test', 'updater_signature', '--test', 'updater_cached_verification')
    } else {
        if (-not $DisposableVm -or -not $AcceptanceScript) { throw 'InstallerAcceptance requires -DisposableVm and -AcceptanceScript for the local VM procedure' }
        $Script = (Resolve-Path $AcceptanceScript).Path
        $null = Copy-Item $Script (Join-Path $Run 'operator-acceptance.ps1')
        # The operator adapter owns actual app UI/installer/restart automation.
        # Its exit is evidence only after reviewing the emitted acceptance.json.
        Invoke-Recorded 'operator-acceptance' (Get-Process -Id $PID).Path @('-NoProfile', '-File', $Script, '-EvidenceRoot', $Run)
        if (-not (Test-Path (Join-Path $Run 'acceptance.json'))) { throw 'operator must supply reviewed edition/installer/restart/state/recovery evidence in acceptance.json' }
    }
    Write-Host "Receipts retained in $Run; native installer acceptance remains separate from default tests."
} finally {
    Write-Host "Review source/lock hashes, stdout/stderr, native architecture and any UNEXECUTED setup failures in $Run."
}
