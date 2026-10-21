import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { UnavailableView } from "../src/views/UnavailableView";
import { SURFACES, surfaceById } from "../src/surfaces";

/**
 * The unavailable surface.
 *
 * This is the view a reader lands on when they click something Abbey does
 * not do. Its whole job is honesty: it must say *why* the thing is missing,
 * must not pretend a capability exists, and must point at the ledger rather
 * than assert a status of its own. A view that guessed a status would be
 * worse than no view.
 */

function unavailableSurface(id: string) {
  const surface = SURFACES.find((candidate) => candidate.id === id);
  if (surface === undefined) throw new Error(`no surface ${id}`);
  if (surface.unavailable === undefined) throw new Error(`${id} is available`);
  return surface;
}

test("an unavailable surface says the contract is not exposed, and why", () => {
  const html = renderToStaticMarkup(
    <UnavailableView surface={unavailableSurface("automations")} />,
  );
  expect(html).toContain("Automations");
  // The reason, in the view's own words — not a bare "unavailable".
  expect(html).toContain("does not expose this contract yet");
  // And the specific reason it is not on the contract.
  expect(html).toContain("Automation scheduling and execution are not exposed");
  // The ledger filter it points at, so the reader can look it up.
  expect(html).toContain("automation");
});

test("a capability that is not implemented anywhere says exactly that", () => {
  const html = renderToStaticMarkup(
    <UnavailableView surface={unavailableSurface("training")} />,
  );
  expect(html).toContain("not implemented anywhere in Abbey yet");
  // The distinction matters: "not on this contract" is a statement about the
  // desktop, "not implemented" is a statement about Abbey.
  expect(html).not.toContain("does not expose this contract yet");
  expect(html).toContain("lora");
});

test("an unavailable surface offers no way to start or change work", () => {
  const html = renderToStaticMarkup(
    <UnavailableView surface={unavailableSurface("cluster")} />,
  );
  // No control at all: the surface cannot start, submit, or change anything.
  expect(html.match(/<button/g) ?? []).toHaveLength(0);
  expect(html).not.toContain("Run");
  expect(html).not.toContain("Create");
  expect(html).not.toContain("Start");
  // And the page says so rather than leaving the reader to infer it.
  expect(html).toContain("can’t start or change work");
});

test("an unavailable surface distinguishes the desktop's limit from Abbey's", () => {
  const notOnContract = renderToStaticMarkup(
    <UnavailableView surface={unavailableSurface("automations")} />,
  );
  // The apostrophe is HTML-escaped in the rendered markup, so the
  // assertion is written against the rendered form.
  expect(notOnContract).toContain(
    "limitation of the desktop&#x27;s read-only invoke surface",
  );
  expect(notOnContract).toContain("not a statement");

  const notImplemented = renderToStaticMarkup(
    <UnavailableView surface={unavailableSurface("training")} />,
  );
  expect(notImplemented).toContain("the authority on status");
  // The not-implemented view does not blame the desktop's contract.
  expect(notImplemented).not.toContain(
    "limitation of the desktop&#x27;s read-only invoke surface",
  );
});

test("every surface marked unavailable renders without a bridge and says why", () => {
  for (const surface of SURFACES) {
    if (surface.unavailable === undefined) continue;
    const html = renderToStaticMarkup(<UnavailableView surface={surface} />);
    expect(html).toContain(surface.label);
    expect(html).toContain(surface.unavailable.detail);
    // Every unavailable surface names the ledger filter it will show.
    expect(html).toContain(surface.unavailable.ledgerFilter);
    // And it never renders an empty or undefined headline.
    expect(html).not.toContain("undefined");
  }
});

test("an available surface is not renderable as an unavailable one", () => {
  // The component refuses an available surface rather than showing an
  // honest-looking "unavailable" page for something that is available.
  const doctor = surfaceById("doctor");
  expect(doctor.unavailable).toBeUndefined();
  expect(() =>
    renderToStaticMarkup(
      <UnavailableView surface={{ ...doctor, unavailable: undefined }} />,
    ),
  ).toThrow();
});
