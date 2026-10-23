"""Windows source guards only; these do not execute or parse PowerShell."""
from pathlib import Path
import re
import unittest
from test_workflow_guards import job_blocks, job_field, checkout_commands

ROOT = Path(__file__).resolve().parents[2]


def guard(workflow, qualification, workspace):
    job = job_blocks(workflow).get('qualify-windows-arm64')
    assert job is not None, 'Windows qualification job missing'
    assert job_field(job, 'runs-on') == '[self-hosted, Windows, ARM64, abbey]'
    condition = job_field(job, 'if')
    assert condition == ("vars.ABBEY_WINDOWS_ARM64_RUNNER == 'enabled' && "
        "github.repository == 'donaldfilimon/abbey' && "
        "(github.event_name == 'push' || github.event_name == 'workflow_dispatch' || "
        "(github.event_name == 'pull_request' && "
        "github.event.pull_request.head.repo.full_name == github.repository))")
    assert 'secrets.' not in job and not re.search(r'^    permissions:', job, re.M)
    assert 'persist-credentials: false' in job
    assert job.index('Refuse reparse points before checkout') < job.index('uses: actions/checkout@')
    assert '[IO.FileAttributes]::ReparsePoint' in job
    assert 'GetFileSystemEntries' in job
    assert 'shell: bash' in job
    assert 'export GITHUB_WORKSPACE="$(cygpath -u "$GITHUB_WORKSPACE")"' in job
    assert 'test "$(cygpath -w "$GITHUB_WORKSPACE")" = "$native_workspace"' in job
    assert set(checkout_commands(job)) == {('donaldfilimon/abi', 'ABI_REVISION', 'abi'), ('donaldfilimon/wdbx', 'WDBX_REVISION', 'wdbx')}
    assert '-Mode Preflight' in job
    assert job.index('-Mode Preflight') < job.index('checkout-public-revision.sh')
    assert re.search(r'name: Clean runner workspace\n        if: always\(\)', job)
    assert '-Mode Cleanup' in job
    for value in ('OSArchitecture', 'ProcessArchitecture', "-ne 'Arm64'", '22000', 'Win32_OperatingSystem', 'aarch64-pc-windows-msvc', 'release: 1.99.1', '0xAA64', 'Invoke-Checked', '$LASTEXITCODE -ne 0', '--locked', "'wdbx'", "'personal-edition'", "'wdbx,personal-edition'"):
        assert value in qualification, value
    for value in ('HOME', 'USERPROFILE', 'LOCALAPPDATA', 'APPDATA', 'XDG_CONFIG_HOME', 'XDG_STATE_HOME', 'ABBEY_CONFIG', 'ABBEY_STATE_DIR', 'ABBEY_PERSONAL_CONFIG', 'ABBEY_PERSONAL_STATE_DIR', 'RUSTUP_HOME', 'CARGO_HOME'):
        assert value in qualification, value
    for value in ("$binaries[$cohort] = $binary", "Invoke-Coexistence $binaries['safe'] $binaries['personal-edition']", "Invoke-Coexistence $binaries['wdbx'] $binaries['wdbx-personal-edition']", "Invoke-Checked $fixture.Binary @('edition')", "$fixture.Config", "$fixture.State", "Get-FileHash -Algorithm SHA256", "$before[$path]", "if ((Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash -ne $before[$path])", "if ($report -notmatch $configLine -or $report -notmatch $stateLine)", "if ($name -ne $fixture.Name)", "Coexistence fixture changed seeded data"):
        assert value in qualification, value
    assert 'Remove-Item "Env:' in qualification
    assert not re.search(r'^\s*(cargo|rustc|git)\s', qualification, re.M), 'unchecked native invocation'
    for value in ('ReparsePoint', 'GetFileSystemEntries', 'Assert-NoReparse', "@('abbey', 'abi', 'wdbx')", 'ABBEY_WINDOWS_SCRATCH', 'GITHUB_RUN_ATTEMPT', 'GITHUB_RUN_ID'):
        assert value in workspace, value
    assert '-Recurse' not in workspace, 'must enumerate and refuse links before removal'


class WindowsSourceGuards(unittest.TestCase):
    def setUp(self):
        self.sources = [(ROOT / p).read_text() if (ROOT / p).exists() else '' for p in
            ('.github/workflows/rust.yml', 'tools/ci/qualify-windows.ps1', 'tools/ci/windows-workspace.ps1')]

    def test_authored_source_contract(self):
        guard(*self.sources)

    def test_mutations_fail_closed(self):
        mutations = [(0, "vars.ABBEY_WINDOWS_ARM64_RUNNER == 'enabled'", 'true'),
            (0, 'self-hosted, Windows, ARM64, abbey', 'self-hosted, Windows, abbey'),
            (0, 'head.repo.full_name == github.repository', 'head.repo.full_name != github.repository'),
            (0, 'if: always()', 'if: success()'),
            (0, '-Mode Preflight', '-Mode Other'),
            (0, 'persist-credentials: false', 'persist-credentials: true'),
            (0, 'donaldfilimon/abi', 'other/abi'),
            (0, '    timeout-minutes: 60', '    permissions: write-all'),
            (0, '        shell: bash', '        shell: pwsh'),
            (1, 'ProcessArchitecture', 'IgnoredArchitecture'),
            (1, '$LASTEXITCODE -ne 0', '$false'),
            (1, '0xAA64', '0x8664'),
            (1, "Invoke-Coexistence $binaries['safe'] $binaries['personal-edition']", '# removed invocation'),
            (1, 'if ((Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash -ne $before[$path])', 'if ($false)'),
            (1, 'if ($report -notmatch $configLine -or $report -notmatch $stateLine)', 'if ($false)'),
            (1, 'if ($name -ne $fixture.Name)', 'if ($false)'),
            (1, '$fixture.Config', '$ignored.Config'),
            (1, '$fixture.State', '$ignored.State'),
            (2, 'ReparsePoint', 'Directory')]
        guard(*self.sources)
        for index, old, new in mutations:
            with self.subTest(old=old):
                changed = self.sources.copy()
                # Limit workflow mutations to Windows; Unix guards remain independent.
                if index == 0:
                    prefix, body = changed[0].split('  qualify-windows-arm64:', 1)
                    changed[0] = prefix + '  qualify-windows-arm64:' + body.replace(old, new)
                else:
                    changed[index] = changed[index].replace(old, new)
                with self.assertRaises(AssertionError):
                    guard(*changed)
