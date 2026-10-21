import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { LIMITS, SPACE_ID, MemoryView } from "../src/views/MemoryView";
import type { V3EntityRecord } from "../src/ipc/generated";

/**
 * The memory surface.
 *
 * The contract this view is built around: the daemon serves *sanitized
 * summaries* only. Payload, provenance, source metadata, and raw record
 * identifiers never cross the protocol-v3 socket, so the record type has
 * exactly three fields — an opaque id, a bounded label, and a state. The
 * view must therefore be unable to show more than it was given, and it
 * must not offer a mutation, because the obsolete/delete command is not on
 * the desktop's invoke surface at all.
 */

const record: V3EntityRecord = {
  id: "rec-0123456789ab",
  label: "the user's stated preference for concise answers",
  state: "available",
};

test("the memory view reads exactly one summary space", () => {
  // The daemon negotiates one space; the window names it rather than
  // accepting a space from the reader.
  expect(SPACE_ID).toBe("memory-v1-summary");
  const html = renderToStaticMarkup(<MemoryView />);
  expect(html).toContain(SPACE_ID);
});

test("the memory view offers a bounded set of page sizes", () => {
  expect([...LIMITS]).toEqual([8, 16, 32]);
  for (const limit of LIMITS) {
    expect(limit).toBeGreaterThan(0);
  }
});

test("the memory view offers no mutation control", () => {
  const html = renderToStaticMarkup(<MemoryView />);
  // Search and page-size are the only controls: one search button and the
  // page-size select. There is no obsolete, no delete, no edit — because
  // the command does not exist on this surface. The *prose* says the word
  // "obsolete" while explaining that, so the assertion is on the controls
  // rather than on the text.
  const buttons = html.match(/<button[^>]*>/g) ?? [];
  expect(buttons).toHaveLength(1);
  expect(html).toContain("Search");
  for (const label of ["Delete", "Edit", "Mark obsolete", "Remove"]) {
    expect(html).not.toContain(`>${label}<`);
  }
});

test("the memory view says the mutation is not on this surface", () => {
  const html = renderToStaticMarkup(<MemoryView />);
  expect(html).toContain("Obsolete/delete is not on the desktop invoke surface");
});

test("the memory view says the payload never crosses the contract", () => {
  const html = renderToStaticMarkup(<MemoryView />);
  // The view states the boundary it operates under, so a reader does not
  // think the summary is the whole record.
  expect(html).toContain("payload");
  expect(html).toContain("never cross this contract");
});

test("a record carries an opaque id, a bounded label, and a state", () => {
  // The type is the sanitization contract: three fields, and nothing that
  // could hold a path, a payload, or provenance.
  expect(Object.keys(record).sort()).toEqual(["id", "label", "state"]);
  expect(record.id).toBe("rec-0123456789ab");
  expect(record.state).toBe("available");
});
