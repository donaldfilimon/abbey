import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  ModelActionButtons,
  ModelConfirmation,
  ModelOperationCard,
  ModelRecoveryButton,
  ModelsView,
} from "../src/views/ModelsView";
import type {
  V3EntityPage,
  V3EntityRecord,
  V3ModelDetails,
} from "../src/ipc/generated";
import type { ModelAttempt } from "../src/models/operations";

/**
 * The Models surface.
 *
 * Three things on this page are safety properties, not decoration:
 *
 *   1. the disabled-button matrix — an action needs its own grant, an
 *      accepted licence, and a model that is not already loaded;
 *   2. the confirmation gate — nothing is sent until the reader has been
 *      told exactly what will happen and has confirmed it;
 *   3. the recovery control for an operation whose outcome is unknown —
 *      the action may already have happened, so it is read, never re-sent.
 */

const record: V3EntityRecord = {
  id: "model-abc123",
  label: "Abbey fixture model",
  state: "available",
};

const page: V3EntityPage = { after: 0, through: 1, records: [record] };

const details: V3ModelDetails = {
  model_id: "model-abc123",
  revision: "rev-1",
  architecture: "fixture-architecture",
  configured_device: "cpu",
  license_sha256: "a".repeat(64),
  license_accepted: true,
  loaded: false,
};

const uncertain: ModelAttempt = {
  kind: "download",
  action: {
    model_id: "model-abc123",
    revision: "rev-1",
    operation_id: "desktop-operation-1",
  },
  phase: "uncertain",
  status: null,
  note: "The response was interrupted. The action may have happened.",
};

/** The rendered `<button>` elements, in order. */
function buttons(html: string): string[] {
  return [...html.matchAll(/<button[^>]*>([\s\S]*?)<\/button>/g)].map((match) =>
    (match[1] ?? "").replace(/<[^>]*>/g, "").trim(),
  );
}

/** The rendered `<button>` elements that carry the `disabled` attribute. */
function disabledButtons(html: string): string[] {
  return [...html.matchAll(/<button([^>]*)>([\s\S]*?)<\/button>/g)]
    .filter((match) => (match[1] ?? "").includes("disabled"))
    .map((match) => (match[2] ?? "").replace(/<[^>]*>/g, "").trim());
}

test("the whole view renders the inventory it was given", () => {
  const html = renderToStaticMarkup(
    <ModelsView page={page} grants={[]} selected={record} details={details} />,
  );
  expect(html).toContain("Models");
  expect(html).toContain(record.label);
  expect(html).toContain(record.id);
  // The exact details are the runtime's own snapshot.
  expect(html).toContain("rev-1");
  expect(html).toContain("fixture-architecture");
  expect(html).toContain(details.license_sha256);
});

test("the whole view states that inventory presence is not inference", () => {
  const html = renderToStaticMarkup(
    <ModelsView page={page} grants={[]} selected={record} details={details} />,
  );
  expect(html).toContain("does not verify artifacts or demonstrate inference");
  expect(html).toContain("Loading a model does not mean inference has run");
});

test("an inventory-only runtime offers no lifecycle control", () => {
  const html = renderToStaticMarkup(
    <ModelsView page={page} grants={[]} selected={record} details={details} />,
  );
  // The runtime granted inventory access only.
  expect(html).toContain("This runtime granted inventory access only");
  expect(html).toContain("Model actions are unavailable");
  expect(disabledButtons(html)).toContain("Download exact revision");
  expect(disabledButtons(html)).toContain("Load exact revision");
  expect(disabledButtons(html)).toContain("Unload exact revision");
});

test("the button matrix needs the grant, the licence, and an unloaded model", () => {
  // Everything granted, licence accepted, model not loaded: download and
  // load are offered; unload is not, because there is nothing loaded.
  const offered = renderToStaticMarkup(
    <ModelActionButtons
      details={details}
      canDownload
      canManage
      blocked={false}
      detailLoading={false}
      onConfirm={() => {}}
      onRefresh={() => {}}
    />,
  );
  expect(buttons(offered)).toEqual([
    "Download exact revision",
    "Load exact revision",
    "Unload exact revision",
    "Refresh details",
  ]);
  expect(disabledButtons(offered)).toEqual(["Unload exact revision"]);

  // The licence is not accepted: download and load are refused, and the
  // page says why.
  const unlicensed = renderToStaticMarkup(
    <ModelsView
      page={page}
      grants={["download_models", "manage_models"]}
      selected={record}
      details={{ ...details, license_accepted: false }}
    />,
  );
  expect(unlicensed).toContain("Download and load require the runtime’s exact license acceptance");
  expect(disabledButtons(unlicensed)).toContain("Download exact revision");
  expect(disabledButtons(unlicensed)).toContain("Load exact revision");

  // The model is already loaded: download and load are refused, unload is
  // the only lifecycle action left.
  const loaded = renderToStaticMarkup(
    <ModelActionButtons
      details={{ ...details, loaded: true }}
      canDownload
      canManage
      blocked={false}
      detailLoading={false}
      onConfirm={() => {}}
      onRefresh={() => {}}
    />,
  );
  expect(disabledButtons(loaded)).toEqual([
    "Download exact revision",
    "Load exact revision",
  ]);
});

test("an unresolved operation blocks every new action", () => {
  const html = renderToStaticMarkup(
    <ModelActionButtons
      details={details}
      canDownload
      canManage
      blocked
      detailLoading={false}
      onConfirm={() => {}}
      onRefresh={() => {}}
    />,
  );
  // Nothing new may start while the previous outcome is unknown.
  expect(disabledButtons(html)).toEqual([
    "Download exact revision",
    "Load exact revision",
    "Unload exact revision",
    "Refresh details",
  ]);
});

test("the confirmation gate names the exact action it would send", () => {
  const html = renderToStaticMarkup(
    <ModelConfirmation
      effect="download"
      details={details}
      blocked={false}
      onCancel={() => {}}
      onConfirm={() => {}}
    />,
  );
  // The reader is told what would happen before it happens.
  expect(html).toContain("Confirm download");
  expect(html).toContain("Download artifacts for");
  expect(html).toContain(details.model_id);
  expect(html).toContain("rev-1");
  expect(html).toContain("cpu");
  // And what it would not do.
  expect(html).toContain("does not run inference");
  // Two controls: cancel, and the one that sends it.
  expect(buttons(html)).toEqual(["Cancel", "Confirm download"]);
  expect(disabledButtons(html)).toEqual([]);
});

test("the confirmation gate is closed while an operation is unresolved", () => {
  const html = renderToStaticMarkup(
    <ModelConfirmation
      effect="load"
      details={details}
      blocked
      onCancel={() => {}}
      onConfirm={() => {}}
    />,
  );
  expect(html).toContain("Confirm load");
  expect(html).toContain("Load into the runtime");
  expect(disabledButtons(html)).toEqual(["Confirm load"]);
});

test("the gate offers a cancel that never sends the action", () => {
  let confirmed = 0;
  let cancelled = 0;
  const html = renderToStaticMarkup(
    <ModelConfirmation
      effect="unload"
      details={details}
      blocked={false}
      onCancel={() => { cancelled += 1; }}
      onConfirm={() => { confirmed += 1; }}
    />,
  );
  expect(html).toContain("Unload from the runtime");
  expect(buttons(html)).toEqual(["Cancel", "Confirm unload"]);
  // Rendering the gate sends nothing by itself.
  expect(confirmed).toBe(0);
  expect(cancelled).toBe(0);
});

test("an unknown outcome is stated as unknown, and recovery only reads status", () => {
  const html = renderToStaticMarkup(
    <ModelOperationCard
      attempt={uncertain}
      recovery={false}
      onRecover={() => {}}
    />,
  );
  // Never as a success or a failure the runtime did not report.
  expect(html).toContain("Outcome unknown");
  expect(html).not.toContain("succeeded");
  expect(html).toContain("The response was interrupted");
  // The recovery control reads the retained operation; it does not resend.
  expect(buttons(html)).toEqual(["Check existing operation status"]);
});

test("the recovery control is disabled while a status check is in flight", () => {
  const html = renderToStaticMarkup(
    <ModelRecoveryButton recovery onRecover={() => {}} />,
  );
  expect(html).toContain("Checking status…");
  expect(disabledButtons(html)).toEqual(["Checking status…"]);
});

test("a terminal operation shows the runtime's own state, not a recovery", () => {
  const html = renderToStaticMarkup(
    <ModelOperationCard
      attempt={{
        kind: "load",
        action: uncertain.action,
        phase: "terminal",
        status: {
          operation_id: uncertain.action.operation_id,
          resource_id: uncertain.action.model_id,
          state: "succeeded",
          progress_basis_points: 10000,
        },
        note: null,
      }}
      recovery={false}
      onRecover={() => {}}
    />,
  );
  expect(html).toContain("succeeded");
  expect(html).toContain("100.0%");
  // A terminal operation needs no recovery: the outcome is known.
  expect(buttons(html)).toEqual([]);
});
