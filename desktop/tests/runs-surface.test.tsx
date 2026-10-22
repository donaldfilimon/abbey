import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { EventRow, RunsView, eventLabel } from "../src/views/RunsView";
import { UnavailableView } from "../src/views/UnavailableView";
import { surfaceById } from "../src/surfaces";
import type { RunLifecycleEvent } from "../src/ipc/generated";

/**
 * The run lifecycle row.
 *
 * A run page exists to answer "what happened, and why". A failed run that
 * renders only the word "failed" does not: the daemon recorded a failure
 * *code* and a *message*, and a reader who cannot see them cannot tell a
 * provider refusal from a timeout from a cancelled turn.
 */

const failed: RunLifecycleEvent = {
  type: "failed",
  payload: {
    failure: {
      code: "provider_refused",
      message: "the provider refused the request",
      retryable: false,
    },
  },
};

const cancelled: RunLifecycleEvent = {
  type: "cancelled",
  payload: { reason: "requested" },
};

const interrupted: RunLifecycleEvent = {
  type: "interrupted",
  payload: { reason: "manager_shutdown" },
};

test("a failed run shows the failure code and the daemon's own message", () => {
  const html = renderToStaticMarkup(
    <EventRow sequence={7} recorded_at="2026-10-21T09:00:00Z" event={failed} />,
  );
  expect(html).toContain("failed (provider_refused)");
  expect(html).toContain("the provider refused the request");
  expect(html).toContain("#7");
  expect(html).toContain("2026-10-21T09:00:00Z");
});

test("a cancelled run shows why it was cancelled", () => {
  expect(eventLabel(cancelled)).toBe("cancelled (requested)");
  const html = renderToStaticMarkup(
    <EventRow sequence={2} recorded_at="2026-10-21T09:00:00Z" event={cancelled} />,
  );
  expect(html).toContain("cancelled (requested)");
});

test("an interrupted run shows why it was interrupted", () => {
  expect(eventLabel(interrupted)).toBe("interrupted (manager_shutdown)");
});

test("an ordinary lifecycle event shows its kind and nothing more", () => {
  for (const type of ["queued", "starting", "running", "succeeded"] as const) {
    expect(eventLabel({ type })).toBe(type);
  }
  const html = renderToStaticMarkup(
    <EventRow sequence={1} recorded_at="2026-10-21T09:00:00Z" event={{ type: "succeeded" }} />,
  );
  expect(html).toContain("succeeded");
  // A successful event has no failure detail, so it must not render a
  // failure line.
  expect(html.match(/class="claim-note"/g) ?? []).toHaveLength(0);
});

test("a cancel request is not the same as a cancellation", () => {
  // The request and the outcome are different events, and the row must not
  // collapse them: a cancel that never took effect is a different story
  // from a cancelled run.
  expect(eventLabel({ type: "cancel_requested" })).toBe("cancel_requested");
  expect(eventLabel(cancelled)).toBe("cancelled (requested)");
});

test("every lifecycle event renders without an empty or undefined label", () => {
  const events: RunLifecycleEvent[] = [
    { type: "queued" },
    { type: "starting" },
    { type: "running" },
    { type: "cancel_requested" },
    { type: "succeeded" },
    failed,
    cancelled,
    interrupted,
  ];
  for (const event of events) {
    const html = renderToStaticMarkup(
      <EventRow sequence={1} recorded_at="2026-10-21T09:00:00Z" event={event} />,
    );
    expect(html).not.toContain("undefined");
    expect(html).not.toContain("null");
    expect(html).toContain("#1");
  }
});

/**
 * The whole Runs page.
 *
 * The surface is live — it reads a durable run and its lifecycle events —
 * but it cannot delegate a task. That refusal used to be a hard-coded
 * badge on this page, in a voice no other surface used. It is now declared
 * once in the surface catalogue and rendered through the shared
 * unavailable mechanism, with the live ledger beside it.
 */

test("the whole view explains that task delegation is unavailable", () => {
  const html = renderToStaticMarkup(<RunsView />);
  // The refusal, in the shared voice: the same headline and the same
  // "why" affordance the unavailable pages use.
  expect(html).toContain("Not available");
  expect(html).toContain("Unavailable");
  expect(html).toContain("Why this isn’t available yet");
  // And the specific contract-level reason.
  expect(html).toContain("Task delegation is not on the desktop invoke surface");
  // It names the thing that is missing rather than a bare badge.
  expect(html).toContain("Task delegation");
});

test("the whole view cites the live ledger for the withheld capability", () => {
  const html = renderToStaticMarkup(<RunsView />);
  // The filter is the one the catalogue declares, so the ledger rows below
  // are the evidence for the refusal.
  const withheld = surfaceById("runs").withheld;
  expect(withheld).toBeDefined();
  expect(withheld?.ledgerFilter).toBe("task");
  expect(html).toContain("task");
  expect(html).toContain("What the ledger says");
});

test("the whole view still offers the live run lookup it actually has", () => {
  const html = renderToStaticMarkup(<RunsView />);
  // The unavailable part is the task delegation, not the run trace: the
  // page must not lose its real surface because it refuses a different one.
  expect(html).toContain("Runs &amp; trace");
  expect(html).toContain("app_run_status");
  expect(html).toContain("app_run_events");
  expect(html).toContain("Look up");
});

test("the runs view and the unavailable pages share one unavailable mechanism", () => {
  const runs = renderToStaticMarkup(<RunsView />);
  const automations = renderToStaticMarkup(
    <UnavailableView surface={surfaceById("automations")} />,
  );
  for (const html of [runs, automations]) {
    expect(html).toContain("Not available");
    expect(html).toContain("Why this isn’t available yet");
    expect(html).toContain("can’t start or change work");
  }
  // The bespoke card this page used to carry is gone: its copy was in a
  // voice the rest of the desktop does not use.
  expect(runs).not.toContain("No task records or actions are available here");
});
