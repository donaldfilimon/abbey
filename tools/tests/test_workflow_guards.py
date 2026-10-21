from __future__ import annotations

from dataclasses import dataclass
import importlib
from pathlib import Path
from pathlib import PurePosixPath
import re
import stat
import unittest


def resolve_toml_reader(importer=importlib.import_module):
    """Select the supported TOML reader without weakening Cargo parsing.

    Python 3.11+ provides tomllib. Older managed runners may provide the
    backport directly; Xcode's Python 3.9 provides the same reader through its
    bundled pip. A minimal Python without any of those readers fails closed
    with an actionable diagnostic instead of silently parsing TOML as text.
    """
    attempted = ("tomllib", "tomli", "pip._vendor.tomli")
    for module_name in attempted:
        try:
            return importer(module_name)
        except ModuleNotFoundError:
            continue
    raise RuntimeError(
        "workflow guards require Python 3.11+ or a managed Python runtime "
        "providing tomli; no structural TOML reader is available"
    )


tomllib = resolve_toml_reader()

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "rust.yml"
CARGO_MANIFEST = ROOT / "Cargo.toml"
PUBLIC_CHECKOUT = ROOT / "tools" / "ci" / "checkout-public-revision.sh"
CROSS_GUARD = ROOT / "tools" / "ci" / "cross-compile-guard.sh"
CHECKOUT_ACTION = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
TOOLCHAIN_ACTION = (
    "dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772"
)


@dataclass(frozen=True)
class SiblingCheckout:
    root: str
    repository: str
    revision_env: str


SIBLING_CHECKOUTS = {
    "abi": SiblingCheckout("abi", "donaldfilimon/abi", "ABI_REVISION"),
    "wdbx": SiblingCheckout("wdbx", "donaldfilimon/wdbx", "WDBX_REVISION"),
}


class TomlReaderCompatibility(unittest.TestCase):
    def test_xcode_python_fallback_order_is_explicit(self) -> None:
        sentinel = object()
        attempted: list[str] = []

        def importer(name: str):
            attempted.append(name)
            if name == "pip._vendor.tomli":
                return sentinel
            raise ModuleNotFoundError(name)

        self.assertIs(resolve_toml_reader(importer), sentinel)
        self.assertEqual(attempted, ["tomllib", "tomli", "pip._vendor.tomli"])

    def test_missing_structural_reader_fails_closed(self) -> None:
        def missing(_name: str):
            raise ModuleNotFoundError

        with self.assertRaisesRegex(RuntimeError, "structural TOML reader"):
            resolve_toml_reader(missing)


def dependency_tables(manifest: dict[str, object]) -> list[dict[str, object]]:
    """Return Cargo dependency mappings that can contain path dependencies."""
    tables: list[dict[str, object]] = []
    dependency_keys = ("dependencies", "dev-dependencies", "build-dependencies")
    for key in dependency_keys:
        value = manifest.get(key)
        if isinstance(value, dict):
            tables.append(value)
    targets = manifest.get("target")
    if isinstance(targets, dict):
        for target in targets.values():
            if not isinstance(target, dict):
                continue
            for key in dependency_keys:
                value = target.get(key)
                if isinstance(value, dict):
                    tables.append(value)
    workspace = manifest.get("workspace")
    if isinstance(workspace, dict):
        for key in dependency_keys:
            value = workspace.get(key)
            if isinstance(value, dict):
                tables.append(value)
    return tables


def external_sibling_roots(manifest_text: str) -> tuple[str, ...]:
    """Derive immediate sibling checkout roots from Cargo path dependencies."""
    manifest = tomllib.loads(manifest_text)
    roots: set[str] = set()
    for table in dependency_tables(manifest):
        for dependency in table.values():
            if not isinstance(dependency, dict) or "path" not in dependency:
                continue
            raw_path = dependency["path"]
            if not isinstance(raw_path, str):
                raise AssertionError("Cargo dependency path must be a string")
            path = PurePosixPath(raw_path)
            parts = path.parts
            if path.is_absolute():
                raise AssertionError(
                    f"absolute Cargo dependency path is unsupported: {raw_path}"
                )
            if not parts or parts[0] != "..":
                continue
            if len(parts) < 2 or parts[1] in {"", ".", ".."}:
                raise AssertionError(
                    f"Cargo dependency must use one immediate sibling root: {raw_path}"
                )
            if ".." in parts[2:]:
                raise AssertionError(
                    f"Cargo dependency cannot traverse outside its sibling root: {raw_path}"
                )
            roots.add(parts[1])
    return tuple(sorted(roots))


def required_sibling_checkouts(manifest_text: str) -> tuple[SiblingCheckout, ...]:
    roots = external_sibling_roots(manifest_text)
    unknown = set(roots) - SIBLING_CHECKOUTS.keys()
    stale = SIBLING_CHECKOUTS.keys() - set(roots)
    if unknown:
        raise AssertionError(f"unmapped sibling checkout: {sorted(unknown)}")
    if stale:
        raise AssertionError(f"checkout mapping has no Cargo dependency: {sorted(stale)}")
    return tuple(SIBLING_CHECKOUTS[root] for root in roots)


def checkout_commands(body: str) -> tuple[tuple[str, str, str], ...]:
    pattern = re.compile(
        r"\./abbey/tools/ci/checkout-public-revision\.sh\s+"
        r"([a-z0-9_.-]+/[a-z0-9_.-]+)\s+\"\$\{([A-Z][A-Z0-9_]*)\}\"\s+"
        r"([a-z0-9_.-]+)"
    )
    return tuple(pattern.findall(body))


def workflow_env(text: str, name: str) -> str:
    match = re.search(rf"(?m)^  {re.escape(name)}:\s*([^\s#]+)\s*$", text)
    if match is None:
        raise AssertionError(f"workflow lacks environment value {name}")
    return match.group(1)


def job_blocks(text: str) -> dict[str, str]:
    """Split the workflow's `jobs:` mapping into {job_name: block_text}.

    Stdlib only: PyYAML is deliberately not a dependency of this repo's
    tooling, and check.sh runs these tests on a bare runner.
    """
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.rstrip() == "jobs:")
    except StopIteration as exc:  # pragma: no cover - guarded by test below
        raise AssertionError("workflow has no top-level `jobs:` mapping") from exc

    jobs: dict[str, list[str]] = {}
    current: str | None = None
    for line in lines[start + 1 :]:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            if current is not None:
                jobs[current].append(line)
            continue
        indent = len(line) - len(line.lstrip())
        if indent == 0:
            break  # left the jobs mapping
        header = re.match(r"^ {2}(?P<name>[A-Za-z0-9_-]+):\s*$", line)
        if header is not None and indent == 2:
            current = header.group("name")
            jobs[current] = []
            continue
        if current is not None:
            jobs[current].append(line)
    return {name: "\n".join(body) for name, body in jobs.items()}


def job_field(body: str, field: str) -> str:
    """Return one actual job-level field, including folded scalar content."""
    lines = body.splitlines()
    pattern = re.compile(rf"^ {{4}}{re.escape(field)}:\s*(.*)$")
    for index, line in enumerate(lines):
        match = pattern.match(line)
        if match is None:
            continue
        value = match.group(1).strip()
        if value not in (">", "|", ">-", "|-"):
            return value
        continuation = []
        for nested in lines[index + 1 :]:
            if nested.strip() and len(nested) - len(nested.lstrip()) <= 4:
                break
            if nested.strip() and not nested.lstrip().startswith("#"):
                continuation.append(nested.strip())
        return " ".join(continuation)
    raise AssertionError(f"job is missing field {field!r}")


class WorkflowGuards(unittest.TestCase):
    def setUp(self) -> None:
        self.text = WORKFLOW.read_text(encoding="utf-8")
        self.jobs = job_blocks(self.text)
        self.checkouts = required_sibling_checkouts(
            CARGO_MANIFEST.read_text(encoding="utf-8")
        )

    def test_workflow_declares_jobs(self) -> None:
        self.assertTrue(self.jobs, "no jobs parsed out of rust.yml")

    def test_trusted_jobs_run_only_on_self_hosted_runners(self) -> None:
        for name in ("gate-macos",):
            body = self.jobs[name]
            self.assertIn(
                "self-hosted",
                job_field(body, "runs-on"),
                f"job {name!r} must use the owned self-hosted runner pool",
            )

    def test_every_self_hosted_job_requires_same_repository(self) -> None:
        for name in ("gate-macos",):
            body = self.jobs[name]
            self.assertIn(
                "github.repository == 'donaldfilimon/abbey'",
                job_field(body, "if"),
                f"self-hosted job {name!r} must refuse foreign repositories",
            )

    def test_every_self_hosted_job_is_gated_on_runner_availability(self) -> None:
        # A self-hosted job whose runner label is not registered cannot be
        # scheduled. Each such job must be skippable via a repository variable
        # so the workflow degrades to "skipped", never to an unschedulable job.
        #
        # Linux ARM64 (primary for ci-self-hosted-linux-proof) and Windows
        # evidence jobs are absent until runners are provisioned (removed
        # 2026-09-28). When re-added they must carry identical guard form.
        # macOS adjunct is the only current self-hosted job.
        expected = {
            "gate-macos": "vars.ABBEY_MACOS_ARM64_RUNNER == 'enabled'",
            # Prepared (not present): "gate-linux": "vars.ABBEY_LINUX_ARM64_RUNNER == 'enabled'",
        }
        self.assertEqual(set(self.jobs), set(expected))
        for name, guard in expected.items():
            body = self.jobs[name]
            self.assertIn(
                guard,
                job_field(body, "if"),
                f"self-hosted job {name!r} needs its exact availability guard",
            )

    def test_every_job_contains_required_release_inventory_and_cleanup_steps(self) -> None:
        required = {
            "Gate both Abbey feature sets",
            "RustSec dependency scan",
            "Warning-denied API docs and release binary",
            "Release install and provider inventory smoke",
            "ABI-backed local mesh proof",
            "Clean runner workspace",
        }
        for name in ("gate-macos",):
            body = self.jobs[name]
            steps = set(re.findall(r"^ {6}- name: (.+)$", body, re.MULTILINE))
            self.assertFalse(required - steps, f"job {name!r} is missing required steps")
            for isolated in (
                'export HOME="$smoke_root/home"',
                'export XDG_CONFIG_HOME="$smoke_root/home/.config"',
                'export XDG_STATE_HOME="$smoke_root/home/.local/state"',
                'export ABBEY_CONFIG="$smoke_root/home/.config/abbey/config.toml"',
                'export ABBEY_STATE_DIR="$smoke_root/state"',
            ):
                self.assertIn(isolated, body, f"job {name!r} lacks isolated smoke state")
            self.assertRegex(
                body,
                r"(?m)^ {6}- name: Clean runner workspace\n {8}if: always\(\)$",
            )
            for checkout in ("abbey", *(item.root for item in self.checkouts)):
                self.assertIn(
                    f'"$GITHUB_WORKSPACE/{checkout}"',
                    body,
                    f"self-hosted job {name!r} must clean the {checkout} checkout",
                )


class ForkSafety(unittest.TestCase):
    def setUp(self) -> None:
        self.text = WORKFLOW.read_text(encoding="utf-8")
        self.jobs = job_blocks(self.text)
        self.checkouts = required_sibling_checkouts(
            CARGO_MANIFEST.read_text(encoding="utf-8")
        )

    def test_no_pull_request_target_trigger(self) -> None:
        # pull_request_target runs with repository secrets against fork code.
        self.assertNotIn("pull_request_target", self.text)

    def test_token_permissions_are_read_only(self) -> None:
        self.assertIsNotNone(
            re.search(r"^permissions:\n  contents: read\s*$", self.text, re.MULTILINE),
            "workflow must declare exactly `permissions:\\n  contents: read`",
        )

    def test_pull_request_jobs_require_a_same_repo_head(self) -> None:
        for name in ("gate-macos",):
            body = self.jobs[name]
            condition = job_field(body, "if")
            if "pull_request" not in condition:
                continue
            self.assertIn(
                "github.event.pull_request.head.repo.full_name == github.repository",
                condition,
                f"job {name!r} accepts pull_request without pinning the head repo",
            )

    def test_no_job_runs_for_a_foreign_pull_request_head(self) -> None:
        # Fork pull requests get no job: the self-hosted runner must never see
        # fork code, and GitHub-hosted jobs cannot start under the account's
        # billing lock (the hosted fork gate was removed on 2026-09-28).
        self.assertNotIn(
            "github.event.pull_request.head.repo.full_name != github.repository",
            self.text,
        )

    def test_public_sibling_checkouts_never_use_a_secret(self) -> None:
        self.assertNotIn("WDBX_CHECKOUT_TOKEN", self.text)
        self.assertNotRegex(self.text, r"(?m)^\s*token:\s*\$\{\{\s*secrets\.")
        for checkout in self.checkouts:
            self.assertNotIn(f"repository: {checkout.repository}", self.text)

    def test_every_job_checks_out_each_required_sibling_exactly_once(self) -> None:
        expected = {
            (item.repository, item.revision_env, item.root) for item in self.checkouts
        }
        for name, body in self.jobs.items():
            commands = checkout_commands(body)
            self.assertEqual(len(commands), len(set(commands)), name)
            self.assertEqual(set(commands), expected, name)

    def test_required_revision_environment_values_are_immutable(self) -> None:
        for checkout in self.checkouts:
            value = workflow_env(self.text, checkout.revision_env)
            self.assertRegex(value, r"^[0-9a-f]{40}$")

    def test_actions_and_toolchains_are_exactly_pinned(self) -> None:
        self.assertEqual(self.text.count(f"uses: {CHECKOUT_ACTION}"), 1)
        self.assertEqual(self.text.count("persist-credentials: false"), 1)
        self.assertEqual(self.text.count(f"uses: {TOOLCHAIN_ACTION}"), 2)
        self.assertIn("ABBEY_TOOLCHAIN: 1.99.1", self.text)
        self.assertIn("ABI_TOOLCHAIN: 1.99.1", self.text)
        self.assertIn("ABI_REVISION: 7b9519b6cfb25cc9bc9db2982e0a901318ba039b", self.text)
        self.assertIn("WDBX_REVISION: 5182bc3a60300e1dd152611025e774c4b906638b", self.text)
        self.assertNotRegex(
            self.text,
            r"(?m)^\s*(?:toolchain:\s*|rustup toolchain install )nightly\s*(?:$|--)",
        )

    def test_public_checkout_helper_is_allowlisted_and_noninteractive(self) -> None:
        helper = PUBLIC_CHECKOUT.read_text(encoding="utf-8")
        self.assertTrue(PUBLIC_CHECKOUT.stat().st_mode & stat.S_IXUSR)
        expected = {f"{item.repository}:{item.root}" for item in self.checkouts}
        allowlisted = set(
            re.findall(r"\b([a-z0-9_.-]+/[a-z0-9_.-]+:[a-z0-9_.-]+)\b", helper)
        )
        self.assertEqual(allowlisted, expected)
        self.assertIn("GIT_CONFIG_NOSYSTEM=1", helper)
        self.assertIn("GIT_CONFIG_GLOBAL=/dev/null", helper)
        self.assertIn("GIT_TERMINAL_PROMPT=0", helper)
        self.assertIn("-c credential.helper= fetch", helper)
        self.assertNotIn("github.token", helper)
        self.assertNotIn("secrets.", helper)

    def test_linux_windows_cross_compile_guard_script_is_prepared(self) -> None:
        """The Linux/Windows soft cross guard is prepared and executable.

        This is repo-side readiness for the blocked ci-self-hosted-linux-proof.
        The script is soft-only, skips uninstalled targets, and is called by
        check.sh. It does not provide the external Linux/Windows runtime
        evidence required to unblock.
        """
        self.assertTrue(CROSS_GUARD.exists())
        self.assertTrue(CROSS_GUARD.stat().st_mode & stat.S_IXUSR)
        text = CROSS_GUARD.read_text(encoding="utf-8")
        self.assertIn("Soft Linux/Windows cross-compile guard", text)
        self.assertIn("x86_64-pc-windows-gnu", text)
        self.assertIn("x86_64-unknown-linux-gnu", text)
        self.assertIn("ci-self-hosted-linux-proof", text)
        self.assertIn("Phase 2", text)
        self.assertIn("portable surfaces", text)

    def test_no_job_widens_token_permissions(self) -> None:
        # Job-level `permissions:` fully overrides the workflow-level default
        # for that job rather than intersecting with it, so a single job block
        # can silently widen the token beyond `contents: read`.
        for name, body in self.jobs.items():
            self.assertNotRegex(
                body,
                r"(?m)^ {4}permissions\s*:",
                f"job {name!r} must not override the workflow token permissions",
            )

    def test_every_job_level_permissions_syntax_is_detected(self) -> None:
        forbidden = (
            "    permissions: write-all",
            "    permissions: read-all",
            "    permissions: {contents: read, pull-requests: write}",
            "    permissions:\n      contents: write",
        )
        for body in forbidden:
            self.assertRegex(body, r"(?m)^ {4}permissions\s*:")


class CargoTopology(unittest.TestCase):
    def test_current_external_siblings_have_exact_checkout_mappings(self) -> None:
        checkouts = required_sibling_checkouts(CARGO_MANIFEST.read_text(encoding="utf-8"))
        self.assertEqual(tuple(item.root for item in checkouts), ("abi", "wdbx"))

    def test_unmapped_external_sibling_fails_closed(self) -> None:
        manifest = '[dependencies]\nexample = { path = "../other/crates/example" }\n'
        with self.assertRaisesRegex(AssertionError, "unmapped sibling checkout"):
            required_sibling_checkouts(manifest)

    def test_local_and_duplicate_paths_do_not_add_checkouts(self) -> None:
        manifest = """[dependencies]
local = { path = "crates/local" }
one = { path = "../abi/crates/one" }
two = { path = "../abi/crates/two" }
wdbx = { path = "../wdbx/crates/wdbx" }
"""
        self.assertEqual(external_sibling_roots(manifest), ("abi", "wdbx"))

    def test_non_immediate_external_path_fails_closed(self) -> None:
        manifest = '[dependencies]\nexample = { path = "../../other/example" }\n'
        with self.assertRaisesRegex(AssertionError, "immediate sibling root"):
            external_sibling_roots(manifest)

    def test_dependency_cannot_escape_after_a_valid_sibling_prefix(self) -> None:
        manifest = '[dependencies]\nexample = { path = "../abi/../other/example" }\n'
        with self.assertRaisesRegex(AssertionError, "cannot traverse"):
            external_sibling_roots(manifest)


DESKTOP_WORKFLOW = ROOT / ".github" / "workflows" / "desktop.yml"
DESKTOP_RELEASE_WORKFLOW = ROOT / ".github" / "workflows" / "desktop-release.yml"
BUN_PIN_STEP_ASSERTION = 'test "$(bun --version)" = "$expected"'


class DesktopWorkflowGuards(unittest.TestCase):
    """The desktop gate workflow repeats rust.yml's trust boundary on a file
    of its own.

    It must be a separate file, not a second job in rust.yml: that file's
    guards pin exactly one actions/checkout use, and the desktop checkout needs
    the same single-checkout shape. These checks keep the desktop file from
    escaping the boundary rust.yml's guards enforce: same trusted events, same
    runner pool and availability guard, same immutable sibling pins, same
    read-only token, and no repository-stored secrets in the release smoke.
    """

    def setUp(self) -> None:
        self.text = DESKTOP_WORKFLOW.read_text(encoding="utf-8")
        self.jobs = job_blocks(self.text)
        self.checkouts = required_sibling_checkouts(
            CARGO_MANIFEST.read_text(encoding="utf-8")
        )

    def test_declares_exactly_one_job(self) -> None:
        self.assertEqual(tuple(self.jobs), ("gate-desktop-macos",))

    def test_runs_on_the_owned_self_hosted_runner(self) -> None:
        body = self.jobs["gate-desktop-macos"]
        self.assertEqual(
            job_field(body, "runs-on"), "[self-hosted, macOS, ARM64, abbey]"
        )

    def test_gate_requires_the_same_trusted_event_and_runner_guard(self) -> None:
        body = self.jobs["gate-desktop-macos"]
        condition = job_field(body, "if")
        self.assertIn("vars.ABBEY_MACOS_ARM64_RUNNER == 'enabled'", condition)
        self.assertIn("github.repository == 'donaldfilimon/abbey'", condition)
        self.assertIn(
            "github.event.pull_request.head.repo.full_name == github.repository",
            condition,
        )

    def test_no_pull_request_target_trigger(self) -> None:
        self.assertNotIn("pull_request_target", self.text)

    def test_no_foreign_pull_request_head(self) -> None:
        self.assertNotIn(
            "github.event.pull_request.head.repo.full_name != github.repository",
            self.text,
        )

    def test_token_permissions_are_read_only(self) -> None:
        self.assertIsNotNone(
            re.search(r"^permissions:\n  contents: read\s*$", self.text, re.MULTILINE)
        )

    def test_no_job_widens_token_permissions(self) -> None:
        for name, body in self.jobs.items():
            self.assertNotRegex(
                body,
                r"(?m)^ {4}permissions\s*:",
                f"job {name!r} must not override the workflow token permissions",
            )

    def test_actions_are_exactly_pinned(self) -> None:
        self.assertEqual(self.text.count(f"uses: {CHECKOUT_ACTION}"), 1)
        self.assertEqual(self.text.count(f"uses: {TOOLCHAIN_ACTION}"), 1)
        self.assertEqual(self.text.count("persist-credentials: false"), 1)

    def test_checkouts_each_required_sibling_exactly_once(self) -> None:
        expected = {
            (item.repository, item.revision_env, item.root) for item in self.checkouts
        }
        commands = checkout_commands(self.text)
        self.assertEqual(len(commands), len(set(commands)))
        self.assertEqual(set(commands), expected)

    def test_sibling_pins_match_the_rust_gate_exactly(self) -> None:
        rust = WORKFLOW.read_text(encoding="utf-8")
        for name in ("ABBEY_TOOLCHAIN", "ABI_TOOLCHAIN", "ABI_REVISION", "WDBX_REVISION"):
            self.assertEqual(
                workflow_env(self.text, name), workflow_env(rust, name), name
            )

    def test_bun_is_an_exact_release_pin(self) -> None:
        self.assertEqual(workflow_env(self.text, "BUN_VERSION"), "1.4.4")
        body = self.jobs["gate-desktop-macos"]
        self.assertIn(
            "https://github.com/oven-sh/bun/releases/download/bun-v${expected}/bun-darwin-aarch64.zip",
            body,
        )
        self.assertIn(BUN_PIN_STEP_ASSERTION, body)

    def test_required_desktop_steps_exist(self) -> None:
        required = {
            "Check out the verified ABI dependency",
            "Check out the public WDBX substrate",
            "Desktop gate (typecheck, codegen drift, bun tests, both editions, daemon proof)",
            "Staged release smoke (ad-hoc signed .app + signed updater artifacts, no DMG)",
            "Clean runner workspace",
        }
        steps = set(re.findall(r"^ {6}- name: (.+)$", self.jobs["gate-desktop-macos"], re.MULTILINE))
        self.assertFalse(required - steps, f"missing desktop-gate steps: {sorted(required - steps)}")

    def test_staged_release_key_is_ephemeral_runner_state(self) -> None:
        body = self.jobs["gate-desktop-macos"]
        self.assertIn('key="${RUNNER_TEMP}/abbey-updater-ci/generated"', body)
        self.assertIn('release-macos.sh --skip-dmg --updater-key "$key"', body)
        self.assertNotIn(
            ".local/share/abbey-updater",
            self.text,
            "the CI runner must never read the owner-machine staging key",
        )

    def test_clean_runner_workspace_runs_always_and_cleans_all_checkouts(self) -> None:
        body = self.jobs["gate-desktop-macos"]
        self.assertRegex(body, r"(?m)^ {6}- name: Clean runner workspace\n {8}if: always\(\)$")
        self.assertIn("for checkout in abbey abi wdbx; do", body)
        self.assertIn('rm -rf "$GITHUB_WORKSPACE/${checkout}"', body)


class DesktopReleaseWorkflowGuards(unittest.TestCase):
    """The desktop release workflow is the one deliberate permissions
    exception, so it gets its own guard class.

    Creating the draft release is the job's purpose and needs
    `contents: write`; every other boundary from desktop.yml still holds,
    plus release-specific rules: tag/dispatch triggers only (never a
    fork-reachable event of any kind), secrets asserted non-empty
    fail-closed before any build, updater key material only under
    RUNNER_TEMP, the desktop gate before any release build, both editions'
    feeds assembled by the shipped assembler, and a draft — never a direct
    publish — release.
    """

    def setUp(self) -> None:
        self.text = DESKTOP_RELEASE_WORKFLOW.read_text(encoding="utf-8")
        self.jobs = job_blocks(self.text)
        self.checkouts = required_sibling_checkouts(
            CARGO_MANIFEST.read_text(encoding="utf-8")
        )
        self.job = self.jobs["release-macos"]

    def test_declares_exactly_one_job(self) -> None:
        self.assertEqual(tuple(self.jobs), ("release-macos",))

    def test_runs_only_for_release_tags_and_manual_dispatch(self) -> None:
        self.assertIn('tags: ["abbey-desktop-v*"]', self.text)
        self.assertIn("workflow_dispatch", self.text)

    def test_no_fork_reachable_event_of_any_kind(self) -> None:
        # Release secrets on the self-hosted runner must never be reachable
        # from foreign code: this file declares no fork-triggerable event at
        # all, and even its comments must not suggest one was added. A bare
        # substring check covers both trigger keys and event filters.
        self.assertNotIn("pull_request", self.text)

    def test_runs_on_the_owned_self_hosted_runner(self) -> None:
        self.assertEqual(
            job_field(self.job, "runs-on"), "[self-hosted, macOS, ARM64, abbey]"
        )

    def test_gate_requires_the_runner_and_repository_guard(self) -> None:
        condition = job_field(self.job, "if")
        self.assertIn("vars.ABBEY_MACOS_ARM64_RUNNER == 'enabled'", condition)
        self.assertIn("github.repository == 'donaldfilimon/abbey'", condition)

    def test_release_token_is_contents_write_as_the_documented_exemption(self) -> None:
        # The one deliberate exemption to the read-only token rule: creating
        # the draft release requires contents: write on the default token,
        # and this file has no fork-reachable trigger, so the elevated token
        # can never act on foreign code. The plain read-only block must NOT
        # be present — an unnoticed downgrade would silently break release
        # creation, while an unnoticed write elsewhere must fail this guard.
        self.assertIsNotNone(
            re.search(r"^permissions:\n  contents: write\s*$", self.text, re.MULTILINE),
            "release workflow must declare exactly `permissions:\n  contents: write` "
            "(the single release-creation exemption)",
        )
        self.assertIsNone(
            re.search(r"^permissions:\n  contents: read\s*$", self.text, re.MULTILINE),
            "the release workflow is not the read-only gate; do not copy its block",
        )

    def test_no_job_level_permissions_override(self) -> None:
        # Job-level `permissions:` fully overrides the workflow-level value,
        # so a job block silently widening or narrowing it must fail here.
        for name, body in self.jobs.items():
            self.assertNotRegex(
                body,
                r"(?m)^ {4}permissions\s*:",
                f"job {name!r} must not override the workflow token permissions",
            )

    def test_actions_are_exactly_pinned(self) -> None:
        self.assertEqual(self.text.count(f"uses: {CHECKOUT_ACTION}"), 1)
        self.assertEqual(self.text.count(f"uses: {TOOLCHAIN_ACTION}"), 1)
        self.assertEqual(self.text.count("persist-credentials: false"), 1)

    def test_checkouts_each_required_sibling_exactly_once(self) -> None:
        expected = {
            (item.repository, item.revision_env, item.root) for item in self.checkouts
        }
        commands = checkout_commands(self.text)
        self.assertEqual(len(commands), len(set(commands)))
        self.assertEqual(set(commands), expected)

    def test_sibling_pins_match_the_rust_gate_exactly(self) -> None:
        rust = WORKFLOW.read_text(encoding="utf-8")
        for name in ("ABBEY_TOOLCHAIN", "ABI_TOOLCHAIN", "ABI_REVISION", "WDBX_REVISION"):
            self.assertEqual(
                workflow_env(self.text, name), workflow_env(rust, name), name
            )

    def test_bun_is_an_exact_release_pin(self) -> None:
        self.assertEqual(workflow_env(self.text, "BUN_VERSION"), "1.4.4")

    def test_release_secrets_are_referenced_in_canonical_shape(self) -> None:
        # Every secret reference must look like an Actions secret name; a
        # lowercase or odd-shaped reference would silently resolve to an
        # empty string on GitHub.
        references = re.findall(r"secrets\.[A-Za-z0-9_]+", self.text)
        self.assertTrue(references, "release workflow must reference secrets")
        for reference in references:
            self.assertRegex(
                reference,
                r"^secrets\.[A-Z][A-Z0-9_]*$",
                f"non-canonical secret reference: {reference}",
            )
        self.assertEqual(
            len(references), len(set(references)), "duplicated secret reference"
        )

    def test_every_required_release_secret_is_fail_closed_checked(self) -> None:
        # The job is only a specification until the owner fills the secrets:
        # the guard step must name each secret and fail with an actionable
        # missing-secret message before any build runs.
        required = (
            "secrets.APPLE_SIGNING_IDENTITY",
            "secrets.APPLE_API_KEY",
            "secrets.APPLE_API_KEY_ID",
            "secrets.APPLE_API_ISSUER",
            "secrets.ABBEY_UPDATER_SIGNING_KEY",
            "secrets.ABBEY_UPDATER_SIGNING_KEY_PASSWORD",
            "secrets.GITHUB_TOKEN",
        )
        for reference in required:
            self.assertIn(reference, self.text, f"missing secret reference {reference}")
        for name in (
            "APPLE_SIGNING_IDENTITY",
            "APPLE_API_KEY_ID",
            "APPLE_API_ISSUER",
            "ABBEY_UPDATER_SIGNING_KEY",
            "ABBEY_UPDATER_SIGNING_KEY_PASSWORD",
        ):
            self.assertIn(name, self.job, f"job never names required secret {name}")
        self.assertIn("missing secret(s) for the desktop release", self.job)
        self.assertIn("check_secret APPLE_SIGNING_IDENTITY", self.job)
        self.assertIn("check_secret APPLE_API_KEY_B64", self.job)
        self.assertIn("check_secret APPLE_API_KEY_ID", self.job)
        self.assertIn("check_secret APPLE_API_ISSUER", self.job)
        self.assertIn("check_secret ABBEY_UPDATER_SIGNING_KEY", self.job)

    def test_secret_material_is_mode_600_under_runner_temp(self) -> None:
        # Key material may exist only as mode-600 files under RUNNER_TEMP:
        # no owner-machine staging key and no repository-tracked path.
        self.assertIn('materialized="${RUNNER_TEMP}/abbey-release-secrets"', self.job)
        self.assertIn('updater_key="${materialized}/abbey-updater.key"', self.job)
        self.assertIn('chmod 600 "$key_file"', self.job)
        self.assertIn('chmod 600 "$updater_key"', self.job)
        self.assertNotIn(
            ".local/share/abbey-updater",
            self.text,
            "the release runner must never read the owner-machine staging key",
        )
        self.assertIn("values never echoed", self.job)

    def test_updater_key_reaches_the_build_only_from_runner_temp(self) -> None:
        self.assertIn('--updater-key "${UPDATER_KEY_FILE}"', self.job)
        self.assertIn('echo "UPDATER_KEY_FILE=${updater_key}"', self.job)

    def test_desktop_gate_runs_before_any_release_build(self) -> None:
        self.assertLess(
            self.job.index("./check.sh"),
            self.job.index("release-macos.sh"),
            "the desktop gate must run before any release build",
        )

    def test_both_editions_are_built_and_dmg_ships(self) -> None:
        # Release builds are full: real DMGs included, never --skip-dmg.
        self.assertIn("./scripts/release-macos.sh --updater-key", self.job)
        self.assertIn("./scripts/release-macos.sh --personal --updater-key", self.job)
        self.assertNotIn("--skip-dmg", self.job)

    def test_version_is_derived_from_the_tag_and_verified_against_the_bundle(self) -> None:
        self.assertIn('version="${GITHUB_REF_NAME#abbey-desktop-v}"', self.job)
        self.assertIn("CFBundleShortVersionString", self.job)
        self.assertIn("/usr/libexec/PlistBuddy", self.job)

    def test_feed_assembler_drives_both_editions(self) -> None:
        self.assertGreaterEqual(self.job.count("assemble-update-feed"), 2)
        self.assertIn("latest.json", self.job)
        self.assertIn("latest-personal.json", self.job)
        self.assertIn("Abbey%20Personal.app.tar.gz", self.job)
        self.assertIn("--platform darwin-aarch64", self.job)

    def test_draft_release_is_created_with_gh_and_both_editions_assets(self) -> None:
        # gh must be provisioned on the runner; no package-manager fallback
        # is allowed on the release runner.
        self.assertIn("command -v gh", self.job)
        self.assertIn("gh release create", self.job)
        self.assertIn("--draft", self.job, "releases must be drafts until reviewed")
        self.assertIn('--title "Abbey Desktop ${version}"', self.job)
        self.assertIn("GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}", self.text)
        for asset in (
            "dmg/Abbey_${version}_aarch64.dmg",
            "dmg/Abbey Personal_${version}_aarch64.dmg",
            "macos/Abbey.app.tar.gz",
            "macos/Abbey Personal.app.tar.gz",
            "SHA256SUMS-abbey.txt",
            "SHA256SUMS-abbey-personal.txt",
        ):
            self.assertIn(asset, self.job, f"draft release must upload {asset}")

    def test_clean_runner_workspace_runs_always_and_cleans_all_checkouts(self) -> None:
        self.assertRegex(
            self.job, r"(?m)^ {6}- name: Clean runner workspace\n {8}if: always\(\)$"
        )
        self.assertIn("for checkout in abbey abi wdbx; do", self.job)
        self.assertIn('rm -rf "$GITHUB_WORKSPACE/${checkout}"', self.job)

