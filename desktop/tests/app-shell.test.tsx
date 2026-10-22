import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  App,
  ConnectionBanner,
  EmptyState,
  available,
  grantedFor,
} from "../src/App";
import { SURFACES, surfaceById } from "../src/surfaces";
import { ALL_APP_CAPABILITIES } from "../src/ipc/generated.samples";
import type { IpcError, RuntimeStatus } from "../src/ipc/generated";

/**
 * The application shell.
 *
 * The shell is what a reader sees before any data arrives, so its copy is
 * the promise the rest of the client has to keep: say whether the window is
 * still connecting, say when the runtime could not be reached, and name the
 * grant a surface is missing instead of showing an empty page.
 */

const status: RuntimeStatus = {
  protocol_version: 1,
  schema_version: 1,
  edition: "standard",
  state: "ready",
  version: "2.6.0",
  build_git: "0f1e2d3",
  build_target: "aarch64-apple-darwin",
  capabilities: { capabilities: [...ALL_APP_CAPABILITIES] },
};

const transportError: IpcError = {
  kind: "transport",
  message: "the daemon could not be reached",
  remedy: null,
};

test("the whole shell renders the navigation, the banner, and the footer", () => {
  const html = renderToStaticMarkup(<App />);
  // The window says it is still connecting, rather than showing an empty
  // page that looks like "nothing to see here".
  expect(html).toContain("Connecting to your Abbey runtime…");
  // Every surface is reachable from the sidebar.
  for (const surface of SURFACES) {
    expect(html).toContain(`aria-label="${surface.label}"`);
  }
  // And the shell's own furniture is there.
  expect(html).toContain("Quick navigation");
  expect(html).toContain("Abbey · Your work, thoughtfully connected");
});

test("the connecting banner offers nothing but the wait", () => {
  const html = renderToStaticMarkup(
    <ConnectionBanner loading error={null} onRetry={() => {}} />,
  );
  expect(html).toContain("Connecting to your Abbey runtime…");
  // There is no error and no retry while the window is still connecting.
  expect(html).not.toContain("Retry connection");
  expect(html).not.toContain("connection-error");
});

test("an unreachable runtime is reported, with the one action that can help", () => {
  const html = renderToStaticMarkup(
    <ConnectionBanner loading={false} error={transportError} onRetry={() => {}} />,
  );
  expect(html).toContain("The runtime couldn’t be reached. Check that Abbey is running, then retry.");
  // The failure itself is shown, not swallowed.
  expect(html).toContain("the daemon could not be reached");
  // And the reader is offered the retry.
  expect(html).toContain("Retry connection");
  expect(html).toContain("connection-error");
});

test("the empty state asks for a connection when there is none", () => {
  const html = renderToStaticMarkup(
    <EmptyState
      surface={surfaceById("runs")}
      loading={false}
      connected={false}
      onRetry={() => {}}
    />,
  );
  expect(html).toContain("Connect to your Abbey runtime to explore this view.");
  expect(html).toContain("Retry connection");
  // With no runtime there is no grant to name, so none is invented.
  expect(html).not.toContain("Required runtime grant");
});

test("the empty state names the grant a connected runtime withheld", () => {
  const html = renderToStaticMarkup(
    <EmptyState
      surface={surfaceById("runs")}
      loading={false}
      connected
      onRetry={() => {}}
    />,
  );
  expect(html).toContain("This view needs a capability your connected runtime hasn’t granted.");
  // The exact grant, so the reader knows what was refused.
  expect(html).toContain("Required runtime grant");
  expect(html).toContain("read_run");
  expect(html).toContain("Tasks");
});

test("the empty state says what it is still checking", () => {
  const html = renderToStaticMarkup(
    <EmptyState
      surface={surfaceById("memory")}
      loading
      connected={false}
      onRetry={() => {}}
    />,
  );
  expect(html).toContain("Checking what’s available in your runtime…");
  expect(html).toContain("Checking connection…");
  // A check in flight is not a retry the reader can hurry.
  expect(html).toMatch(/<button[^>]*disabled/);
});

test("the chat surface is answered by the conversation session, not the status", () => {
  const chat = surfaceById("chat");
  // The chat surface's capability is a v4 grant, which the protocol-v1
  // status never carries — so the ordinary rule can never open it.
  expect(chat.requires).toBe("read_conversations");
  expect(available(chat, status, [])).toBe(false);
  // The conversation session's own read authority is what opens it.
  expect(grantedFor("chat", chat, status, [], true)).toBe(true);
  expect(grantedFor("chat", chat, status, [], false)).toBe(false);
  // And a status that happens to carry something else does not open it.
  expect(grantedFor("chat", chat, status, ["read_models"], false)).toBe(false);
});

test("every other surface keeps the ordinary grant rule", () => {
  for (const surface of SURFACES) {
    if (surface.id === "chat") continue;
    expect(grantedFor(surface.id, surface, status, [], false)).toBe(
      available(surface, status, []),
    );
  }
});
