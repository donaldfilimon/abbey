import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ToolsView } from "../src/views/ToolsView";
import {
  effectLabel,
  inventoryState,
  effectCounts,
} from "../src/views/toolInventory";
import { v3ToolPageFixture } from "../src/ipc/generated.samples";
import type { IpcError } from "../src/ipc/generated";

const transportError: IpcError = {
  kind: "transport",
  message: "the daemon could not be reached",
  remedy: null,
};

const rejectedError: IpcError = {
  kind: "rejected",
  message: "the tool inventory is daemon-only",
  remedy: null,
};

test("the safe-edition inventory lists every advertised tool with its effect class", () => {
  const html = renderToStaticMarkup(<ToolsView page={v3ToolPageFixture} />);
  for (const tool of v3ToolPageFixture.tools) {
    expect(html).toContain(tool.tool_id);
  }
  expect(html).toContain("Read-only");
  expect(html).toContain("Mutating — approval required");
  // The mutating tool is shown with the daemon's approval requirement and
  // with the fact that this window cannot supply one.
  expect(html).toContain("The daemon requires an exact-call approval before it runs");
  expect(html).toContain("no approve, deny, or invoke command");
});

test("the tools surface offers no way to run, approve, or cancel a tool", () => {
  const html = renderToStaticMarkup(<ToolsView page={v3ToolPageFixture} />);
  // Exactly one control: the read. There is no per-tool button, no invoke
  // control, and no approval control, because no such command exists.
  expect(html.match(/<button/g) ?? []).toHaveLength(1);
  expect(html).toContain("Read inventory");
  expect(html).not.toContain("Run tool");
  expect(html).not.toContain("Execute");
  expect(html).not.toContain("Approve");
  expect(html).not.toContain("Deny");
  expect(html).not.toContain("Cancel");
  // And the page says so in its own words.
  expect(html).toContain("Listing a tool is not calling it");
});

test("an unreadable inventory states why instead of showing an empty list", () => {
  const rejected = renderToStaticMarkup(<ToolsView error={rejectedError} />);
  expect(rejected).toContain("did not negotiate a tool inventory");
  expect(rejected).not.toContain("abbey_status");
  expect(rejected).not.toContain("abbey_memory_mark_obsolete");

  const transport = renderToStaticMarkup(<ToolsView error={transportError} />);
  expect(transport).toContain("could not be reached");
  expect(transport).not.toContain("abbey_status");
});

test("the unavailable states are distinguished, not collapsed into one message", () => {
  expect(inventoryState(transportError)).toBe("unreachable");
  expect(inventoryState(rejectedError)).toBe("not_negotiated");
  expect(
    inventoryState({
      kind: "transport",
      message: "Command app_tools_list not found",
      remedy: null,
    }),
  ).toBe("not_exposed");
});

test("the effect classes are labelled from the daemon's own vocabulary", () => {
  expect(effectLabel("read_only").label).toBe("Read-only");
  expect(effectLabel("mutating").label).toContain("approval required");
  expect(effectLabel("destructive").label).toBe("Destructive");
  const counts = effectCounts(v3ToolPageFixture.tools);
  expect(counts.read_only).toBe(3);
  expect(counts.mutating).toBe(1);
  expect(counts.destructive).toBe(0);
});
