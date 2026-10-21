import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { RouteRow, workspaceLabel } from "../src/views/RoutesView";
import type { RouteAuditEntry } from "../src/ipc/generated";

/**
 * The routing audit row.
 *
 * The contract this view is built around: the application core redacts the
 * working directory into an opaque `ws-<digest>` before the entry crosses
 * the socket, because this window has no filesystem plugin and must not be
 * able to show a real path even if it wanted to. A row that rendered a path
 * would be a leak of the user's project layout into the UI.
 */

const entry: RouteAuditEntry = {
  recorded_at: "2026-10-21T09:00:00Z",
  workspace: "ws-0123456789ab",
  persona: "Abbey",
  role: "Max",
  model: "gemma4:26b-mlx",
  confidence_percent: 92,
  reason: "the persona and role matched the request",
};

test("a route row shows the decision the application core published", () => {
  const html = renderToStaticMarkup(<RouteRow entry={entry} />);
  expect(html).toContain("Abbey");
  expect(html).toContain("Max");
  expect(html).toContain("gemma4:26b-mlx");
  expect(html).toContain("92%");
  expect(html).toContain("2026-10-21T09:00:00Z");
  expect(html).toContain("the persona and role matched the request");
});

test("a route row shows the workspace as the opaque digest it was given", () => {
  const html = renderToStaticMarkup(<RouteRow entry={entry} />);
  expect(html).toContain("ws-0123456789ab");
  // The row says what the digest is, so a reader does not mistake it for a
  // truncated path.
  expect(html).toContain("Opaque digest of the working directory");
});

test("a route row never renders a filesystem path", () => {
  const html = renderToStaticMarkup(<RouteRow entry={entry} />);
  // The application core redacts paths, so the row has nothing to leak.
  // The assertion is that a path-shaped string never appears.
  expect(html).not.toContain("/Users/");
  expect(html).not.toContain("/home/");
  expect(html).not.toContain("C:\\");
});

test("an entry with no workspace says so instead of rendering a blank", () => {
  const withoutWorkspace: RouteAuditEntry = { ...entry, workspace: null };
  expect(workspaceLabel(withoutWorkspace)).toBe("unrecorded");
  const html = renderToStaticMarkup(<RouteRow entry={withoutWorkspace} />);
  expect(html).toContain("unrecorded");
  expect(html).not.toContain("undefined");
  expect(html).not.toContain("null");
});

test("a route row names the alternative and its fallback when the router considered one", () => {
  const withAlternative: RouteAuditEntry = {
    ...entry,
    alternate: "Aviva / Reviewer",
    fallback: "the confidence was below the threshold",
  };
  const html = renderToStaticMarkup(<RouteRow entry={withAlternative} />);
  expect(html).toContain("Considered instead");
  expect(html).toContain("Aviva / Reviewer");
  expect(html).toContain("the confidence was below the threshold");
});

test("a route row with no alternative does not claim one was considered", () => {
  const html = renderToStaticMarkup(<RouteRow entry={entry} />);
  expect(html).not.toContain("Considered instead");
});

test("a route row shows the stage and run correlation when the router recorded them", () => {
  const withTrace: RouteAuditEntry = {
    ...entry,
    stage: "second",
    correlation: "run-0001",
  };
  const html = renderToStaticMarkup(<RouteRow entry={withTrace} />);
  expect(html).toContain("stage second");
  expect(html).toContain("run run-0001");
});

test("a route row lists the tool markers the router attached", () => {
  const withTools: RouteAuditEntry = {
    ...entry,
    tools: ["media", "mcp"],
  };
  const html = renderToStaticMarkup(<RouteRow entry={withTools} />);
  expect(html).toContain("tools:");
  expect(html).toContain("media, mcp");
});

test("a route row with no tools does not render an empty tools line", () => {
  const html = renderToStaticMarkup(<RouteRow entry={entry} />);
  expect(html).not.toContain("tools:");
});
