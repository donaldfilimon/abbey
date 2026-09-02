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
        for name in ("gate-linux", "gate-macos"):
            body = self.jobs[name]
            self.assertIn(
                "self-hosted",
                job_field(body, "runs-on"),
                f"job {name!r} must use the owned self-hosted runner pool",
            )

    def test_every_self_hosted_job_requires_same_repository(self) -> None:
        for name in ("gate-linux", "gate-macos"):
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
        expected = {
            "gate-linux": "vars.ABBEY_LINUX_ARM64_RUNNER == 'enabled'",
            "gate-macos": "vars.ABBEY_MACOS_ARM64_RUNNER == 'enabled'",
        }
        self.assertEqual(set(self.jobs), {*expected, "gate-forks"})
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
        for name in ("gate-linux", "gate-macos"):
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
        for name in ("gate-linux", "gate-macos"):
            body = self.jobs[name]
            condition = job_field(body, "if")
            if "pull_request" not in condition:
                continue
            self.assertIn(
                "github.event.pull_request.head.repo.full_name == github.repository",
                condition,
                f"job {name!r} accepts pull_request without pinning the head repo",
            )

    def test_fork_job_uses_a_hosted_runner_and_foreign_head_guard(self) -> None:
        body = self.jobs["gate-forks"]
        self.assertEqual(job_field(body, "runs-on"), "ubuntu-latest")
        condition = job_field(body, "if")
        self.assertIn("github.event_name == 'pull_request'", condition)
        self.assertIn(
            "github.event.pull_request.head.repo.full_name != github.repository",
            condition,
        )
        self.assertNotIn("self-hosted", body)

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
        self.assertEqual(self.text.count(f"uses: {CHECKOUT_ACTION}"), 3)
        self.assertEqual(self.text.count("persist-credentials: false"), 3)
        self.assertEqual(self.text.count(f"uses: {TOOLCHAIN_ACTION}"), 6)
        self.assertIn("ABBEY_TOOLCHAIN: nightly-2026-09-01", self.text)
        self.assertIn("ABI_TOOLCHAIN: nightly-2026-08-20", self.text)
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

    def test_fork_job_runs_the_real_portable_gate(self) -> None:
        body = self.jobs["gate-forks"]
        steps = set(re.findall(r"^ {6}- name: (.+)$", body, re.MULTILINE))
        self.assertTrue(
            {
                "Check out Abbey",
                "Check out the verified ABI dependency",
                "Check out the public WDBX substrate",
                "Install pinned ABI toolchain",
                "Install pinned Abbey toolchain",
                "Build the real ABI binary",
                "Gate all portable Abbey modes",
            }.issubset(steps)
        )

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
