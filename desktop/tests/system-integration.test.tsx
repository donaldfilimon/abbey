import { expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { SystemIntegrationPanel, describePermission } from "../src/components/SystemIntegration";
import type { SystemIntegration } from "../src/ipc/generated";

const MANIFEST = "src-tauri/capabilities/default.json";
const MANIFEST_PATH = new URL(`../${MANIFEST}`, import.meta.url);

function declaredPermissions(): string[] {
  const manifest = JSON.parse(readFileSync(MANIFEST_PATH, "utf8")) as {
    permissions: string[];
  };
  return manifest.permissions;
}

const facts: SystemIntegration = {
  declared_permissions: ["core:default", "updater:default"],
  execution_capable_permissions: [],
  login_startup: {
    state: "not_registered",
    detail:
      "This app registers no login item, launch agent, or startup service, and it must stay " +
      "unregistered: nothing it declares can install one.",
  },
  background_service: {
    state: "separate_process",
    detail:
      "This app installs and starts no background service and spawns no helper process. The " +
      "Abbey daemon (abbeyd) is a separate owner-only process that you start yourself; this " +
      "window is only a client of it.",
  },
  window_close: {
    state: "daemon_owned",
    detail:
      "Closing this window does not cancel daemon-owned work. Runs and their lifecycle events " +
      "live in the daemon's own durable store and continue there.",
  },
};

test("the system integration section states all four host facts", () => {
  const html = renderToStaticMarkup(<SystemIntegrationPanel integration={facts} />);
  expect(html).toContain("System integration");
  expect(html).toContain("Not registered");
  expect(html).toContain("Not installed or started by this app");
  expect(html).toContain("Daemon-owned work continues");
  expect(html).toContain("registers no login item");
  expect(html).toContain("installs and starts no background service");
  expect(html).toContain("does not cancel daemon-owned work");
  // The permission list is the build's own, not a hand-written claim.
  expect(html).toContain("core:default");
  expect(html).toContain("updater:default");
  expect(html).toContain("No declared permission can reach the filesystem, a shell, a process");
  // Nothing here is a control.
  expect(html).not.toContain("<button");
  expect(html).not.toContain("<input");
});

test("the shipped capability manifest is exactly the two reviewed permissions", () => {
  expect(declaredPermissions()).toEqual(["core:default", "updater:default"]);
});

test("every declared permission is explained, and no permission is unexplained", () => {
  for (const permission of declaredPermissions()) {
    expect(describePermission(permission)).not.toContain("does not explain");
  }
  // An unknown permission is reported honestly rather than described.
  expect(describePermission("fs:default")).toContain("does not explain");
});

test("the shipped client cannot register a login item, install a service, or spawn a process", () => {
  // The copy above is a claim about the build, so the build has to be checked:
  // no plugin that could register a login item or install a service, and no
  // process spawn anywhere in the desktop client.
  //
  // Comments are stripped first: the manifest *names* the plugins it refuses
  // to depend on, and a naive substring match would fail on the explanation
  // rather than on a real dependency.
  const manifest = readFileSync(
    new URL("../src-tauri/Cargo.toml", import.meta.url),
    "utf8",
  )
    .split("\n")
    .map((line) => line.split("#")[0] ?? "")
    .join("\n");
  for (const plugin of [
    "tauri-plugin-autostart",
    "tauri-plugin-single-instance",
    "tauri-plugin-shell",
    "tauri-plugin-fs",
    "tauri-plugin-process",
    "tauri-plugin-opener",
  ]) {
    expect(manifest).not.toContain(plugin);
  }
  const sources = readRustSources(new URL("../src-tauri/src/", import.meta.url));
  expect(sources).not.toContain("std::process::Command");
  expect(sources).not.toContain("Command::new");
});

/** Every Rust source under the desktop client, concatenated. */
function readRustSources(directory: URL): string {
  const entries = readdirSync(directory, { withFileTypes: true });
  return entries
    .map((entry) => {
      const path = new URL(`${entry.name}${entry.isDirectory() ? "/" : ""}`, directory);
      if (entry.isDirectory()) return readRustSources(path);
      if (!entry.name.endsWith(".rs")) return "";
      return readFileSync(path, "utf8");
    })
    .join("\n");
}
