import { expect, test } from "bun:test";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  CommandPalette,
  isPaletteShortcut,
  paletteEntries,
} from "../src/components/CommandPalette";
import { SURFACES } from "../src/surfaces";

/**
 * The command palette.
 *
 * Bun has no DOM here, so these tests drive the palette through what it
 * exports and through the element tree it returns: the shortcut predicate the
 * shell uses, the filter the list is built from, the dialog's own keydown
 * handler, and the server-rendered list.
 */

type Keys = { key: string; metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean };
const key = (k: Keys) => ({ metaKey: false, ctrlKey: false, altKey: false, ...k });

function paletteTree(close: () => void): ReactElement<Record<string, unknown>> {
  let tree: ReactElement<Record<string, unknown>> | null = null;
  function Probe() {
    tree = CommandPalette({ close, navigate: () => {} }) as ReactElement<
      Record<string, unknown>
    >;
    return null;
  }
  renderToStaticMarkup(<Probe />);
  if (!tree) throw new Error("the palette rendered nothing");
  return tree;
}

function options(html: string): Map<string, string> {
  return new Map(
    [...html.matchAll(/<div role="option"[^>]*id="command-([a-z]+)"[^>]*>([\s\S]*?)<\/div>/g)].map(
      (m) => [m[1] ?? "", (m[2] ?? "").replace(/<[^>]*>/g, " ")],
    ),
  );
}

test("opens on its shortcut", () => {
  expect(isPaletteShortcut(key({ key: "k", metaKey: true }))).toBe(true);
  expect(isPaletteShortcut(key({ key: "K", ctrlKey: true }))).toBe(true);
  expect(isPaletteShortcut(key({ key: "k" }))).toBe(false);
  expect(isPaletteShortcut(key({ key: "k", metaKey: true, altKey: true }))).toBe(false);
  expect(isPaletteShortcut(key({ key: "b", metaKey: true }))).toBe(false);
});

test("filters by typed text", () => {
  expect(paletteEntries("").map((s) => s.id)).toEqual(SURFACES.map((s) => s.id));
  expect(paletteEntries("MEMORY").map((s) => s.id)).toEqual(["memory"]);
  // The blurb is searched too, not only the label.
  expect(paletteEntries("lora").map((s) => s.id)).toEqual(["training"]);
  expect(paletteEntries("no such page anywhere")).toEqual([]);
});

test("closes on Escape", () => {
  let closed = 0;
  let prevented = 0;
  const dialog = paletteTree(() => (closed += 1)).props.children as ReactElement<{
    onKeyDown: (event: unknown) => void;
  }>;
  dialog.props.onKeyDown({
    key: "Escape",
    shiftKey: false,
    target: null,
    preventDefault: () => (prevented += 1),
  });
  expect(closed).toBe(1);
  expect(prevented).toBeGreaterThanOrEqual(1);
});

test("never lists an unavailable surface as runnable", () => {
  const listed = options(renderToStaticMarkup(<CommandPalette close={() => {}} navigate={() => {}} />));
  expect(listed.size).toBe(SURFACES.length);
  for (const surface of SURFACES) {
    const text = listed.get(surface.id) ?? "";
    if (surface.unavailable) {
      expect(text).not.toContain("Go to page");
      expect(text).toContain("Unavailable");
    } else {
      expect(text).toContain("Go to page");
    }
  }
});
