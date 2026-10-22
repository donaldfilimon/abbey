import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  UpdateCheck,
  UpdateStatus,
  type UpdateCandidate,
} from "../src/components/UpdateCheck";

/**
 * The updater trigger.
 *
 * This is the only place in the client that can start an update, and the
 * property worth proving is that it is *user-initiated*: nothing checks on
 * its own, nothing downloads on its own, and the surface always says which
 * endpoint the artifact is verified against.
 */

const update: UpdateCandidate = {
  version: "2.7.0",
  body: "Fixes the update trigger.",
  downloadAndInstall: async () => {},
};

function buttons(html: string): string[] {
  return [...html.matchAll(/<button[^>]*>([\s\S]*?)<\/button>/g)].map((match) =>
    (match[1] ?? "").replace(/<[^>]*>/g, "").trim(),
  );
}

test("the surface names the pinned source and the explicit-action policy", () => {
  const html = renderToStaticMarkup(
    <UpdateStatus phase={{ kind: "idle" }} onCheck={() => {}} onInstall={() => {}} />,
  );
  expect(html).toContain("Updates");
  expect(html).toContain("plugins.updater.endpoints");
  expect(html).toContain("pinned Ed25519 public key");
  expect(html).toContain("Checking and installing are always explicit actions here");
});

test("nothing is checked until the reader asks", () => {
  let calls = 0;
  const html = renderToStaticMarkup(
    <UpdateCheck
      check={() => {
        calls += 1;
        return Promise.resolve(null);
      }}
    />,
  );
  // Rendering the surface must not reach the endpoint.
  expect(calls).toBe(0);
  expect(html).toContain("Not checked since launch.");
  // The one control that starts a check.
  expect(buttons(html)).toEqual(["Check for updates"]);
});

test("the idle surface has exactly one control", () => {
  const html = renderToStaticMarkup(
    <UpdateStatus phase={{ kind: "idle" }} onCheck={() => {}} onInstall={() => {}} />,
  );
  // There is no second trigger and no automatic action of any kind.
  expect(buttons(html)).toEqual(["Check for updates"]);
  expect(html).not.toContain("Download");
});

test("a current installation says so", () => {
  const html = renderToStaticMarkup(
    <UpdateStatus phase={{ kind: "current" }} onCheck={() => {}} onInstall={() => {}} />,
  );
  expect(html).toContain("Abbey is up to date.");
  expect(buttons(html)).toEqual(["Check for updates"]);
});

test("an available update offers exactly one install control", () => {
  const html = renderToStaticMarkup(
    <UpdateStatus
      phase={{ kind: "available", update }}
      onCheck={() => {}}
      onInstall={() => {}}
    />,
  );
  // The version and the release notes are the server's own.
  expect(html).toContain("Abbey 2.7.0 is available");
  expect(html).toContain("Fixes the update trigger.");
  // And the only way forward is an explicit press.
  expect(buttons(html)).toEqual(["Download 2.7.0 and install"]);
});

test("a failed check reports the failure instead of claiming an update", () => {
  const html = renderToStaticMarkup(
    <UpdateStatus
      phase={{ kind: "failed", message: "the endpoint did not answer" }}
      onCheck={() => {}}
      onInstall={() => {}}
    />,
  );
  expect(html).toContain("Update check failed: the endpoint did not answer");
  expect(html).not.toContain("is available");
  expect(buttons(html)).toEqual(["Check for updates"]);
});

test("a check in flight cannot be started again", () => {
  const html = renderToStaticMarkup(
    <UpdateStatus phase={{ kind: "checking" }} onCheck={() => {}} onInstall={() => {}} />,
  );
  expect(html).toContain("Checking the pinned release endpoint…");
  expect(html).toMatch(/<button[^>]*disabled/);
  expect(buttons(html)).toEqual(["Check for updates"]);
});

test("a download in progress reports the bytes it has verified", () => {
  const html = renderToStaticMarkup(
    <UpdateStatus
      phase={{ kind: "downloading", bytes: 5 * 1048576 }}
      onCheck={() => {}}
      onInstall={() => {}}
    />,
  );
  expect(html).toContain("Downloading and verifying (5.0 MiB)");
  expect(html).toMatch(/<button[^>]*disabled/);
});

test("an installed update says to quit and reopen", () => {
  const html = renderToStaticMarkup(
    <UpdateStatus phase={{ kind: "installed" }} onCheck={() => {}} onInstall={() => {}} />,
  );
  // There is no process plugin, so this window cannot relaunch itself and
  // must not pretend to.
  expect(html).toContain("Quit and reopen Abbey to finish the update.");
  expect(html).not.toContain("relaunch");
});
