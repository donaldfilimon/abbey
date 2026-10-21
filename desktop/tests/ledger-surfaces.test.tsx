import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ClaimBadge, ClaimRow } from "../src/views/ClaimsView";
import { ErrorCard } from "../src/views/ErrorCard";
import type { ClaimRecord, ClaimStatus, IpcError } from "../src/ipc/generated";

/**
 * The ledger surfaces and the error card.
 *
 * These are the components every other view falls back on: an error that
 * renders wrong is an error every view renders wrong, and a claim row that
 * drops its "use instead" pointer is a row that misleads.
 */

const claim: ClaimRecord = {
  name: "backend-ollama",
  status: "current",
  note: "preferred default, never required",
  instead: null,
};

const proposedClaim: ClaimRecord = {
  name: "models-production-local-weights",
  status: "proposed",
  note: "Max/Gemma names currently remain role bindings, not bundled weights",
  instead: "abbey backend ollama",
};

test("a claim row shows its name, status, and note", () => {
  const html = renderToStaticMarkup(<ClaimRow claim={claim} />);
  expect(html).toContain("backend-ollama");
  expect(html).toContain("preferred default, never required");
  expect(html).toContain("current");
});

test("a claim row names what to use instead when the ledger says so", () => {
  const html = renderToStaticMarkup(<ClaimRow claim={proposedClaim} />);
  expect(html).toContain("Use instead");
  expect(html).toContain("abbey backend ollama");
  expect(html).toContain("proposed");
});

test("a claim row with no alternative does not invent one", () => {
  const html = renderToStaticMarkup(<ClaimRow claim={claim} />);
  expect(html).not.toContain("Use instead");
});

test("every claim status has a readable label", () => {
  // The badge maps the wire status to words. A status the map does not
  // cover would render as an empty badge, so a reader would see a status
  // word they cannot read.
  const labels: Record<ClaimStatus, string> = {
    current: "current",
    partial: "partial",
    proposed: "proposed",
    blocked: "blocked",
    out_of_scope: "out of scope",
    failed: "failed",
    revoked: "revoked",
    superseded: "superseded",
    expired: "expired",
  };
  for (const [status, label] of Object.entries(labels) as [ClaimStatus, string][]) {
    const html = renderToStaticMarkup(<ClaimBadge status={status} />);
    expect(html).toContain(label);
    expect(html).toContain(status);
  }
});

test("the error card names the kind and shows the message", () => {
  const error: IpcError = {
    kind: "transport",
    message: "the daemon could not be reached",
    remedy: null,
  };
  const html = renderToStaticMarkup(<ErrorCard error={error} />);
  expect(html).toContain("Cannot reach Abbey");
  expect(html).toContain("the daemon could not be reached");
  expect(html).not.toContain("undefined");
});

test("the error card shows a remedy when the daemon offered one", () => {
  const error: IpcError = {
    kind: "configuration",
    message: "no bearer is configured",
    remedy: "set ABBEYD_BEARER_TOKEN and restart the window",
  };
  const html = renderToStaticMarkup(<ErrorCard error={error} />);
  expect(html).toContain("Daemon configuration error");
  expect(html).toContain("set ABBEYD_BEARER_TOKEN and restart the window");
});

test("the error card omits the remedy line when there is no remedy", () => {
  const error: IpcError = {
    kind: "rejected",
    message: "the tool inventory is daemon-only",
    remedy: null,
  };
  const html = renderToStaticMarkup(<ErrorCard error={error} />);
  expect(html).toContain("Request rejected");
  // One message paragraph, not two.
  expect(html.match(/class="claim-note"/g) ?? []).toHaveLength(1);
});

test("every error kind has a readable headline", () => {
  const kinds: IpcError["kind"][] = [
    "configuration",
    "transport",
    "protocol",
    "not_found",
    "rejected",
    "unsupported_platform",
  ];
  for (const kind of kinds) {
    const html = renderToStaticMarkup(
      <ErrorCard error={{ kind, message: "detail", remedy: null }} />,
    );
    expect(html).not.toContain("undefined");
    expect(html).not.toContain("null");
    expect(html).toContain("detail");
  }
});
