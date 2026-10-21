"""Guard the desktop release pipeline's cross-artifact consistency.

Five files describe one release: the safe edition's Tauri config, the
personal edition's overlay config, the release workflow, the updater feed
assembler, and the macOS bundling script. Nothing in the repository makes
them agree, so they drift silently and the failure mode is a published
update feed that points at an artifact the release never uploaded, or an
endpoint that serves a feed nobody assembles.

Every value below is read out of those files — product names, platform
keys, feed file names, accepted flags — so a rename in one place fails
this guard instead of shipping a mismatched feed. Stdlib only: PyYAML is
deliberately not a dependency of this repo's tooling, and check.sh runs
these tests on a bare runner.
"""

from __future__ import annotations

import base64
from dataclasses import dataclass
import json
from pathlib import Path
import re
import unittest
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[2]
DESKTOP = ROOT / "desktop"
SAFE_CONFIG = DESKTOP / "src-tauri" / "tauri.conf.json"
PERSONAL_CONFIG = DESKTOP / "src-tauri" / "tauri.personal.conf.json"
RELEASE_WORKFLOW = ROOT / ".github" / "workflows" / "desktop-release.yml"
FEED_ASSEMBLER = DESKTOP / "scripts" / "assemble-update-feed.sh"
RELEASE_SCRIPT = DESKTOP / "scripts" / "release-macos.sh"


@dataclass(frozen=True)
class AssemblerCall:
    """One feed-assembler invocation as the release workflow writes it."""

    command: str
    flags: dict[str, str]


def updater_plugin(config: dict) -> dict:
    """Return the `plugins.updater` mapping of one Tauri config."""
    plugins = config.get("plugins")
    if not isinstance(plugins, dict):
        raise AssertionError("config declares no plugins mapping")
    plugin = plugins.get("updater")
    if not isinstance(plugin, dict):
        raise AssertionError("config declares no plugins.updater mapping")
    return plugin


def updater_endpoints(config: dict) -> list[str]:
    """Return the updater endpoints of one Tauri config."""
    endpoints = updater_plugin(config).get("endpoints")
    if not isinstance(endpoints, list) or not endpoints:
        raise AssertionError("plugins.updater.endpoints must be a non-empty list")
    for endpoint in endpoints:
        if not isinstance(endpoint, str):
            raise AssertionError(f"updater endpoint is not a string: {endpoint!r}")
    return endpoints


def endpoint_filename(url: str) -> str:
    """Return the file name an updater endpoint serves."""
    return urlsplit(url).path.rsplit("/", 1)[-1]


def release_assets(text: str) -> list[str]:
    """Return the draft release's uploaded asset paths, in workflow order."""
    assets: list[str] = []
    inside = False
    for line in text.splitlines():
        if not inside:
            if re.match(r"^\s*assets=\(\s*$", line):
                inside = True
            continue
        stripped = line.strip()
        if stripped == ")":
            break
        match = re.fullmatch(r'"([^"]+)"', stripped)
        if match is None:
            raise AssertionError(f"unparsable release asset entry: {stripped!r}")
        assets.append(match.group(1))
    if not inside:
        raise AssertionError("the release workflow declares no assets array")
    return assets


FLAG_LINE = re.compile(
    r'^--(?P<name>[A-Za-z][A-Za-z0-9-]*)\s+'
    r'(?:"(?P<double>[^"]*)"|\'(?P<single>[^\']*)\'|(?P<bare>\S+))$'
)


def parse_assembler_call(tokens: list[str]) -> AssemblerCall:
    """Turn one invocation's tokens into a command plus its flag mapping."""
    command = tokens[0]
    flags: dict[str, str] = {}
    for token in tokens[1:]:
        match = FLAG_LINE.match(token)
        if match is None:
            raise AssertionError(f"unparsable assembler argument: {token!r}")
        value = match.group("double")
        if value is None:
            value = match.group("single")
        if value is None:
            value = match.group("bare")
        flags[f"--{match.group('name')}"] = value
    return AssemblerCall(command=command, flags=flags)


def assembler_invocations(text: str) -> list[AssemblerCall]:
    """Return the workflow's feed-assembler invocations, in order.

    The workflow shells out to the assembler with one flag per line, so
    the flags are parsed out of the invocation rather than hardcoded: a
    renamed script or a new flag is then visible to the guard.
    """
    lines = text.splitlines()
    calls: list[AssemblerCall] = []
    index = 0
    while index < len(lines):
        command = re.match(
            r"^\s*(\S*assemble-update-feed(?:\.sh)?)\s*\\?\s*$", lines[index]
        )
        if command is None:
            index += 1
            continue
        tokens = [command.group(1)]
        index += 1
        while index < len(lines):
            token = lines[index].strip()
            if not token.startswith("--"):
                break
            tokens.append(token.rstrip("\\").strip())
            index += 1
        calls.append(parse_assembler_call(tokens))
    return calls


def assembler_flags(script: str) -> set[str]:
    """Return the flags the assembler accepts, from its option dispatch."""
    match = re.search(r'case\s+"\$1"\s+in\s*\n\s*(--[^)\n]+)\)', script)
    if match is None:
        raise AssertionError("the assembler dispatches no option case")
    return {flag.strip() for flag in match.group(1).split("|")}


def assembler_usage_flags(script: str) -> set[str]:
    """Return the flags the assembler's usage text documents."""
    block = re.search(r"usage\(\)\s*\{(.*?)\n\}", script, re.S)
    if block is None:
        raise AssertionError("the assembler has no usage() function")
    return set(re.findall(r"--[A-Za-z][A-Za-z0-9-]*", block.group(1)))


def assembler_platform(script: str) -> str:
    """Return the single platform key the assembler accepts."""
    match = re.search(r'\[\s*"\$PLATFORM"\s*=\s*"([A-Za-z0-9._-]+)"\s*\]', script)
    if match is None:
        raise AssertionError("the assembler pins no platform key")
    return match.group(1)


def release_product_names(script: str) -> dict[str, str]:
    """Return the two editions' bundle product names from release-macos.sh."""
    default = re.search(r"PRINT=\$\{PRINT:-([^}]*)\}", script)
    personal = re.search(
        r'--personal\)\s*\n\s*EDITION_ARGS=[^\n]*\n\s*PRINT="([^"]+)"', script
    )
    if default is None:
        raise AssertionError("release-macos.sh declares no default product name")
    if personal is None:
        raise AssertionError("release-macos.sh declares no --personal product name")
    return {"safe": default.group(1), "personal": personal.group(1)}


def release_dmg_glob(script: str) -> str:
    """Return the DMG name pattern release-macos.sh discovers."""
    match = re.search(r'-name\s+"([^"]*\$\{PRINT\}[^"]*)"', script)
    if match is None:
        raise AssertionError("release-macos.sh does not discover its DMG by name")
    return match.group(1)


class DesktopReleaseConsistency(unittest.TestCase):
    """The parts of one release must describe the same release.

    Each test states one agreement between two files: an endpoint and the
    feed it serves, a pinned key and the edition that inherits it, an
    uploaded asset and the URL a feed points at, a product name and the
    artifact derived from it, a flag and the CLI that accepts it, and a
    platform key and the architecture the release builds for. The values
    are read out of the artifacts themselves, so a rename in one place
    fails here instead of publishing a mismatched feed.
    """

    def setUp(self) -> None:
        self.safe_config = json.loads(SAFE_CONFIG.read_text(encoding="utf-8"))
        self.personal_config = json.loads(PERSONAL_CONFIG.read_text(encoding="utf-8"))
        self.workflow_text = RELEASE_WORKFLOW.read_text(encoding="utf-8")
        self.assembler_text = FEED_ASSEMBLER.read_text(encoding="utf-8")
        self.release_text = RELEASE_SCRIPT.read_text(encoding="utf-8")
        self.asset_paths = release_assets(self.workflow_text)
        self.asset_names = [path.rsplit("/", 1)[-1] for path in self.asset_paths]
        self.invocations = assembler_invocations(self.workflow_text)
        self.accepted_flags = assembler_flags(self.assembler_text)
        self.usage_flags = assembler_usage_flags(self.assembler_text)
        self.platform = assembler_platform(self.assembler_text)
        self.product_names = release_product_names(self.release_text)
        self.dmg_glob = release_dmg_glob(self.release_text)

    def test_safe_endpoint_serves_the_safe_feed_file(self) -> None:
        endpoints = updater_endpoints(self.safe_config)
        self.assertEqual(
            len(endpoints),
            1,
            f"the safe edition must have exactly one updater endpoint: {endpoints}",
        )
        self.assertTrue(
            endpoints[0].endswith("/latest.json"),
            f"the safe updater endpoint must serve the safe feed file "
            f"latest.json, not: {endpoints[0]}",
        )

    def test_personal_endpoint_serves_the_personal_feed_file(self) -> None:
        endpoints = updater_endpoints(self.personal_config)
        self.assertEqual(
            len(endpoints),
            1,
            f"the personal edition must have exactly one updater endpoint: {endpoints}",
        )
        self.assertTrue(
            endpoints[0].endswith("/latest-personal.json"),
            f"the personal updater endpoint must serve the personal feed file "
            f"latest-personal.json, not: {endpoints[0]}",
        )

    def test_personal_overlay_inherits_the_pinned_key(self) -> None:
        overlay = updater_plugin(self.personal_config)
        self.assertNotIn(
            "pubkey",
            overlay,
            "the personal overlay must inherit the base config's pinned pubkey, "
            "not pin a second key",
        )
        self.assertIn(
            overlay.get("requireSignedVersion", True),
            (True,),
            "the personal overlay must not disable requireSignedVersion",
        )

    def test_base_config_requires_signed_versions_with_a_pinned_key(self) -> None:
        plugin = updater_plugin(self.safe_config)
        self.assertIs(
            plugin.get("requireSignedVersion"),
            True,
            "the base config must require signed versions",
        )
        pubkey = plugin.get("pubkey")
        self.assertIsInstance(pubkey, str, "the base config must pin an updater pubkey")
        self.assertTrue(pubkey.strip(), "the pinned updater pubkey must not be empty")

    def test_pinned_key_decodes_to_a_minisign_public_key(self) -> None:
        pubkey = updater_plugin(self.safe_config)["pubkey"].strip()
        try:
            decoded = base64.b64decode(pubkey, validate=True)
        except ValueError as exc:  # binascii.Error is a ValueError
            self.fail(f"the pinned updater pubkey is not valid base64: {exc}")
        self.assertEqual(
            base64.b64encode(decoded).decode("ascii"),
            pubkey,
            "the pinned updater pubkey does not round-trip through base64",
        )
        try:
            text = decoded.decode("utf-8")
        except UnicodeDecodeError as exc:
            self.fail(f"the pinned updater pubkey does not decode to UTF-8 text: {exc}")
        self.assertIn(
            "\n",
            text,
            "the pinned updater pubkey decoded to a single line: it is truncated",
        )
        lines = [line for line in text.splitlines() if line.strip()]
        self.assertGreaterEqual(
            len(lines),
            2,
            f"the pinned updater pubkey is truncated: {lines!r}",
        )
        self.assertTrue(
            lines[0].startswith("untrusted comment: minisign public key"),
            f"the pinned updater pubkey is not a minisign public key file: {lines[0]!r}",
        )
        self.assertTrue(
            lines[1].startswith("RW"),
            f"the pinned updater pubkey has no minisign public key payload: {lines[1]!r}",
        )

    def test_release_uploads_both_feed_files(self) -> None:
        uploaded = sorted(name for name in self.asset_names if name.endswith(".json"))
        self.assertEqual(
            uploaded,
            sorted(["latest.json", "latest-personal.json"]),
            "the draft release must upload exactly the two per-edition feed files",
        )

    def test_feed_urls_point_at_uploaded_assets(self) -> None:
        for call in self.invocations:
            url = call.flags.get("--url", "")
            segment = unquote(urlsplit(url).path.rsplit("/", 1)[-1])
            self.assertIn(
                segment,
                self.asset_names,
                f"the feed URL {url} points at {segment!r}, which the release "
                f"never uploads",
            )

    def test_feed_outputs_are_the_files_the_endpoints_serve(self) -> None:
        outputs = [
            call.flags.get("--out", "").rsplit("/", 1)[-1] for call in self.invocations
        ]
        for label, config in (
            ("safe", self.safe_config),
            ("personal", self.personal_config),
        ):
            endpoint = endpoint_filename(updater_endpoints(config)[0])
            self.assertIn(
                endpoint,
                outputs,
                f"no feed is assembled for the {label} endpoint's file {endpoint}",
            )
            self.assertIn(
                endpoint,
                self.asset_names,
                f"the {label} feed file {endpoint} is never uploaded",
            )
        self.assertEqual(
            len(set(outputs)),
            len(outputs),
            f"both editions must assemble a distinct feed file, got {outputs}",
        )

    def test_workflow_asset_names_follow_the_release_product_names(self) -> None:
        known = set(self.product_names.values())
        names = set(self.asset_names)
        for edition, product in self.product_names.items():
            self.assertIn(
                f"{product}.app.tar.gz",
                names,
                f"the {edition} edition's updater artifact {product}.app.tar.gz "
                f"is not uploaded",
            )
            self.assertIn(
                f"{product}.app.tar.gz.sig",
                names,
                f"the {edition} edition's updater signature "
                f"{product}.app.tar.gz.sig is not uploaded",
            )
            dmgs = sorted(
                name
                for name in names
                if name.startswith(f"{product}_") and name.endswith(".dmg")
            )
            self.assertEqual(
                len(dmgs),
                1,
                f"expected exactly one {edition} DMG derived from {product!r}, "
                f"found {dmgs}",
            )
        for name in names:
            if not name.endswith(".app.tar.gz"):
                continue
            product = name[: -len(".app.tar.gz")]
            self.assertIn(
                product,
                known,
                f"the uploaded artifact {name} uses the product name {product!r}, "
                f"which release-macos.sh does not build",
            )

    def test_release_script_product_names_match_the_edition_configs(self) -> None:
        self.assertEqual(
            self.safe_config.get("productName"),
            self.product_names["safe"],
            "the safe edition's productName and release-macos.sh's default "
            "PRINT must be the same name",
        )
        self.assertEqual(
            self.personal_config.get("productName"),
            self.product_names["personal"],
            "the personal overlay's productName and release-macos.sh's "
            "--personal PRINT must be the same name",
        )

    def test_workflow_flags_are_accepted_by_the_assembler(self) -> None:
        self.assertEqual(
            self.usage_flags,
            self.accepted_flags,
            "the assembler's usage text and its option dispatch disagree",
        )
        for call in self.invocations:
            for flag in call.flags:
                self.assertIn(
                    flag,
                    self.accepted_flags,
                    f"the workflow passes {flag}, which the assembler does not "
                    f"accept",
                )

    def test_every_feed_invocation_carries_the_required_flags(self) -> None:
        self.assertEqual(
            len(self.invocations),
            2,
            "the workflow must assemble one feed per edition",
        )
        for call in self.invocations:
            for flag in ("--artifact", "--sig", "--url", "--out", "--version"):
                self.assertIn(
                    flag,
                    call.flags,
                    f"a feed invocation is missing {flag}",
                )
                self.assertTrue(
                    call.flags[flag].strip(),
                    f"{flag} must carry a value in every feed invocation",
                )

    def test_platform_key_matches_the_architecture_the_release_builds(self) -> None:
        os_name, _, arch = self.platform.partition("-")
        self.assertEqual(
            os_name,
            "darwin",
            f"the feed platform key must name the macOS platform: {self.platform}",
        )
        self.assertTrue(
            arch, f"the feed platform key must name an architecture: {self.platform}"
        )
        # release-macos.sh discovers its DMG by product name only
        # ("${PRINT}_*.dmg"), so the architecture in the uploaded name is
        # the bundler's stamp: it must be the architecture the feed's
        # platform key names.
        self.assertRegex(
            self.dmg_glob,
            r"^\$\{PRINT\}_[^/]*\.dmg$",
            "release-macos.sh must discover its DMG by product name",
        )
        dmg_assets = sorted(name for name in self.asset_names if name.endswith(".dmg"))
        self.assertEqual(
            len(dmg_assets),
            2,
            f"expected one DMG per edition, found {dmg_assets}",
        )
        for name in dmg_assets:
            match = re.fullmatch(
                r"(?P<product>.+)_\$\{version\}_(?P<arch>[^.]+)\.dmg", name
            )
            self.assertIsNotNone(
                match,
                f"the DMG asset {name} does not follow "
                f"<product>_<version>_<arch>.dmg",
            )
            self.assertEqual(
                match.group("arch"),
                arch,
                f"the DMG asset {name} does not match the feed platform key "
                f"{self.platform}",
            )
        for call in self.invocations:
            self.assertEqual(
                call.flags.get("--platform"),
                self.platform,
                "the workflow must pass the assembler's only accepted platform",
            )

    def test_endpoints_are_https_without_placeholders(self) -> None:
        for label, config in (
            ("safe", self.safe_config),
            ("personal", self.personal_config),
        ):
            for endpoint in updater_endpoints(config):
                self.assertTrue(
                    endpoint.startswith("https://"),
                    f"the {label} updater endpoint must be https: {endpoint}",
                )
                self.assertNotIn(
                    "*",
                    endpoint,
                    f"the {label} updater endpoint must not contain a wildcard: "
                    f"{endpoint}",
                )
                self.assertIsNone(
                    re.search(r"[<>]|\$\{|\{\{", endpoint),
                    f"the {label} updater endpoint must not contain a placeholder "
                    f"token: {endpoint}",
                )

    def test_workflow_invokes_the_shipped_assembler(self) -> None:
        for call in self.invocations:
            # The workflow runs with `working-directory: abbey/desktop`, so the
            # invoked path is relative to the desktop root, not to scripts/.
            invoked = re.sub(r"^\./", "", call.command)
            self.assertTrue(
                (DESKTOP / invoked).exists(),
                f"the release workflow invokes {call.command!r}, which does not "
                f"exist in desktop/scripts/ — the shipped assembler is "
                f"{FEED_ASSEMBLER.relative_to(DESKTOP)}",
            )


if __name__ == "__main__":
    unittest.main()
