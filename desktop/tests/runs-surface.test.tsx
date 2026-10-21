import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { EventRow, eventLabel } from "../src/views/RunsView";
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
