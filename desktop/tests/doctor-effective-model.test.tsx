import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { DoctorView } from "../src/views/DoctorView";
import type {
  BundleIdentity,
  ConnectionInfo,
  EffectiveModel,
  RuntimeStatus,
} from "../src/ipc/generated";

const status: RuntimeStatus = {
  protocol_version: 1,
  schema_version: 1,
  edition: "standard",
  state: "ready",
  version: "2.6.0",
  build_git: "0f1e2d3",
  build_target: "aarch64-apple-darwin",
  capabilities: { capabilities: ["read_status", "read_conversations"] },
};

const connection: ConnectionInfo = {
  source: "daemon",
  socket_path: "/private/tmp/abbey.sock",
  bearer_configured: true,
  bearer_source: "token_file",
  detail: "the daemon answered over the owner-only socket",
};

const identity: BundleIdentity = {
  edition: "standard",
  product_name: "Abbey",
  binary_name: "abbey",
  daemon_binary_name: "abbeyd",
  bundle_id: "com.abbey.standard",
  configured_bundle_id: "com.abbey.standard",
};

const resolved: EffectiveModel = {
  backend: "abi",
  raw: "local",
  resolved: "local",
  source: "runtime-route",
};

test("the doctor panel shows the effective model the runtime published", () => {
  const html = renderToStaticMarkup(
    <DoctorView
      status={status}
      connection={connection}
      identity={identity}
      effectiveModel={resolved}
    />,
  );
  expect(html).toContain("Effective model");
  expect(html).toContain("abi");
  expect(html).toContain("local");
  // The card names where the answer came from: the runtime's own route,
  // resolved with Abbey's shared rules — never the window's own environment.
  expect(html).toContain("runtime");
  expect(html).toContain("abbey doctor");
  expect(html).toContain("Read-only");
});

test("an unset live model is shown as unset, not as an empty cell", () => {
  const html = renderToStaticMarkup(
    <DoctorView
      status={status}
      connection={connection}
      identity={identity}
      effectiveModel={{
        backend: "cursor-agent",
        raw: "",
        resolved: "auto",
        source: "runtime-route",
      }}
    />,
  );
  expect(html).toContain("(unset)");
  expect(html).toContain("auto");
  expect(html).toContain("cursor-agent");
});

test("an unreadable effective model states why instead of guessing one", () => {
  const html = renderToStaticMarkup(
    <DoctorView
      status={status}
      connection={connection}
      identity={identity}
      effectiveError="no runtime route is published: this client executes no turns without a daemon"
    />,
  );
  expect(html).toContain("no runtime route is published");
  // No invented model, and no route either.
  expect(html).not.toContain("gemma4");
  expect(html).not.toContain("Resolved</dt>");
});

test("the doctor names the selected route as the authority for the answer", () => {
  // A turn runs on the *selected* route, so the Doctor has to answer for
  // that route. Reporting the runtime's first available route instead would
  // name a model the Chat surface will not use.
  const html = renderToStaticMarkup(
    <DoctorView
      status={status}
      connection={connection}
      identity={identity}
      routeId="fm.gemma-4-12b"
      effectiveModel={{
        backend: "fm",
        raw: "gemma-4-12b",
        resolved: "system",
        source: "runtime-route",
      }}
    />,
  );
  expect(html).toContain("the route the Chat surface has selected");
  expect(html).toContain("runtime-route");
  // The route itself is named, so the answer can be checked against the
  // Chat surface's selection rather than taken on trust.
  expect(html).toContain("fm.gemma-4-12b");
});

test("the doctor shows no route when none is selected", () => {
  // With no selection the answer is the runtime's default available route,
  // and the Doctor must not imply that some particular route was chosen.
  const html = renderToStaticMarkup(
    <DoctorView
      status={status}
      connection={connection}
      identity={identity}
      routeId={null}
      effectiveModel={{
        backend: "abi",
        raw: "local",
        resolved: "local",
        source: "runtime-route",
      }}
    />,
  );
  expect(html).toContain("because no route is selected");
  expect(html).not.toContain("fm.gemma-4-12b");
});

test("the doctor says the default route is the authority when none is selected", () => {
  const html = renderToStaticMarkup(
    <DoctorView
      status={status}
      connection={connection}
      identity={identity}
      routeId={null}
      effectiveModel={{
        backend: "abi",
        raw: "local",
        resolved: "local",
        source: "runtime-route",
      }}
    />,
  );
  expect(html).toContain("the default available route");
  expect(html).toContain("because no route is selected");
});
