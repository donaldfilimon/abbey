# Windows 11 ARM64 CLI cohorts only: no desktop/root-gate qualification.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Invoke-Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Native qualification command failed: $Command" }
}
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
    throw 'Native ARM64 OS and PowerShell process required'
}
$os = Get-CimInstance Win32_OperatingSystem
Write-Host "Windows: $($os.Caption), build $($os.BuildNumber)"
if ([int]$os.BuildNumber -lt 22000 -or $os.ProductType -ne 1) {
    throw 'Windows 11 client build >=22000 required'
}
# Preserve explicit provisioned Rust locations before redirecting profile state.
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
$env:RUSTUP_HOME = $rustupHome
$root = $env:ABBEY_WINDOWS_SCRATCH
if (-not $root -or -not (Test-Path -LiteralPath $root)) { throw 'Preflight scratch root required' }
$env:CARGO_HOME = Join-Path $root 'cargo'
# Clear inherited provider, execution, credentials and model overrides without values.
foreach ($variable in Get-ChildItem Env:) {
    if (($variable.Name -match 'TOKEN|KEY|PASSWORD|SECRET|CREDENTIAL|BEARER' -or
        $variable.Name -match '^(ABBEY|ABI|OLLAMA|FM|GROK|ANTHROPIC|OPENAI|CURSOR|CLAUDE|HF|HUGGING_FACE|AWS|AZURE|GOOGLE|GITHUB_TOKEN|GH_TOKEN)') -and
        $variable.Name -ne 'ABBEY_WINDOWS_SCRATCH') {
        Remove-Item "Env:$($variable.Name)"
    }
}
$env:HOME = Join-Path $root 'home'
$env:USERPROFILE = $env:HOME
$env:LOCALAPPDATA = Join-Path $root 'localappdata'
$env:APPDATA = Join-Path $root 'appdata'
$env:XDG_CONFIG_HOME = Join-Path $root 'config'
$env:XDG_STATE_HOME = Join-Path $root 'state'
$env:ABBEY_CONFIG = Join-Path $root 'safe/config.toml'
$env:ABBEY_STATE_DIR = Join-Path $root 'safe/state'
$env:ABBEY_PERSONAL_CONFIG = Join-Path $root 'personal/config.toml'
$env:ABBEY_PERSONAL_STATE_DIR = Join-Path $root 'personal/state'
foreach ($directory in @($env:CARGO_HOME, $env:HOME, $env:LOCALAPPDATA, $env:APPDATA, $env:XDG_CONFIG_HOME,
    $env:XDG_STATE_HOME, (Split-Path $env:ABBEY_CONFIG), $env:ABBEY_STATE_DIR,
    (Split-Path $env:ABBEY_PERSONAL_CONFIG), $env:ABBEY_PERSONAL_STATE_DIR)) {
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
}
$rust = Invoke-Checked 'rustc' @('+1.99.1', '-Vv')
$rustText = $rust -join "`n"
Write-Host $rustText
if ($rustText -notmatch '(?m)^host: aarch64-pc-windows-msvc\r?$' -or
    $rustText -notmatch '(?m)^release: 1.99.1\r?$') { throw 'Exact native Rust host required' }
Invoke-Checked 'cargo' @('+1.99.1', '-V')
foreach ($checkout in @('abbey', 'abi', 'wdbx')) {
    Invoke-Checked 'git' @('-C', (Join-Path $env:GITHUB_WORKSPACE $checkout), 'rev-parse', 'HEAD')
}
# ABI-backed CLI tests use the actual native sibling binary, never a shim.
$env:CARGO_TARGET_DIR = Join-Path $root 'target-abi'
Set-Location (Join-Path $env:GITHUB_WORKSPACE 'abi')
Invoke-Checked 'cargo' @('+1.99.1', 'build', '--locked', '-p', 'abi-cli', '--target', 'aarch64-pc-windows-msvc')
$env:ABBEY_ABI_BIN = Join-Path $env:CARGO_TARGET_DIR 'aarch64-pc-windows-msvc/debug/abi.exe'
Set-Location (Join-Path $env:GITHUB_WORKSPACE 'abbey')
Invoke-Checked 'cargo' @('+1.99.1', 'fmt', '--package', 'abbey', '--', '--check')
function Assert-ArmBinary([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        if ($reader.ReadUInt16() -ne 0x5A4D) { throw 'Missing DOS header' }
        $stream.Position = 0x3c
        $offset = $reader.ReadInt32()
        if ($offset -lt 64 -or $offset -gt ($stream.Length - 6)) { throw 'Invalid PE offset' }
        $stream.Position = $offset
        if ($reader.ReadUInt32() -ne 0x00004550 -or $reader.ReadUInt16() -ne 0xAA64) {
            throw 'Produced binary must have ARM64 PE machine type'
        }
    } finally { $stream.Dispose() }
}
$binaries = @{}
foreach ($features in @('', 'wdbx', 'personal-edition', 'wdbx,personal-edition')) {
    $cohort = if ($features) { $features.Replace(',', '-') } else { 'safe' }
    $env:CARGO_TARGET_DIR = Join-Path $root "target-$cohort"
    $common = @('--locked', '-p', 'abbey', '--target', 'aarch64-pc-windows-msvc')
    if ($features) { $common += @('--features', $features) }
    Invoke-Checked 'cargo' (@('+1.99.1', 'check') + $common)
    Invoke-Checked 'cargo' (@('+1.99.1', 'clippy') + $common + @('--all-targets', '--', '-D', 'warnings'))
    Invoke-Checked 'cargo' (@('+1.99.1', 'build') + $common + @('--bins'))
    Invoke-Checked 'cargo' (@('+1.99.1', 'test') + $common)
    $binary = Join-Path $env:CARGO_TARGET_DIR 'aarch64-pc-windows-msvc/debug/abbey.exe'
    Assert-ArmBinary $binary
    $binaries[$cohort] = $binary
    Invoke-Checked $binary @('--version')
    $edition = (Invoke-Checked $binary @('edition', '--name') | Out-String).Trim()
    $expected = if ($features -match 'personal-edition') { 'abbey-personal' } else { 'abbey' }
    if ($edition -ne $expected) { throw 'Produced edition identity mismatch' }
}
# Retained binaries coexist in their independent cohort target directories.
# Read-only identity commands must resolve the edition's own roots and preserve
# every seeded byte, including the other edition's config and state sentinel.
function Invoke-Coexistence([string]$SafeBinary, [string]$PersonalBinary) {
    $fixtures = @(
        @{ Binary = $SafeBinary; Name = 'abbey'; Config = $env:ABBEY_CONFIG; State = $env:ABBEY_STATE_DIR },
        @{ Binary = $PersonalBinary; Name = 'abbey-personal'; Config = $env:ABBEY_PERSONAL_CONFIG; State = $env:ABBEY_PERSONAL_STATE_DIR }
    )
    if ($fixtures[0].Config -eq $fixtures[1].Config -or $fixtures[0].State -eq $fixtures[1].State) {
        throw 'Coexistence roots must differ'
    }
    $before = @{}
    foreach ($fixture in $fixtures) {
        # TOML comments are valid config; these are deliberately distinct bytes.
        [IO.File]::WriteAllText($fixture.Config, "# coexistence $($fixture.Name)`n")
        $sentinel = Join-Path $fixture.State 'coexistence-sentinel.txt'
        [IO.File]::WriteAllText($sentinel, "state owned by $($fixture.Name)`n")
        foreach ($path in @($fixture.Config, $sentinel)) {
            $before[$path] = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash
        }
    }
    foreach ($fixture in $fixtures) {
        $name = (Invoke-Checked $fixture.Binary @('edition', '--name') | Out-String).Trim()
        if ($name -ne $fixture.Name) { throw 'Coexistence edition identity mismatch' }
        $report = (Invoke-Checked $fixture.Binary @('edition') | Out-String)
        $configLine = '(?m)^config:\s*' + [regex]::Escape($fixture.Config) + '\r?$'
        $stateLine = '(?m)^state root:\s*' + [regex]::Escape($fixture.State) + '\r?$'
        if ($report -notmatch $configLine -or $report -notmatch $stateLine) {
            throw 'Coexistence resolved config/state roots mismatch'
        }
        foreach ($path in $before.Keys) {
            if ((Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash -ne $before[$path]) {
                throw 'Coexistence fixture changed seeded data'
            }
        }
    }
}
Invoke-Coexistence $binaries['safe'] $binaries['personal-edition']
Invoke-Coexistence $binaries['wdbx'] $binaries['wdbx-personal-edition']
Write-Host 'CLI coexistence fixtures complete; installation/service acceptance remains separate.'
Write-Host 'CLI cohorts complete; desktop, installer and daemon acceptance remain separate.'
