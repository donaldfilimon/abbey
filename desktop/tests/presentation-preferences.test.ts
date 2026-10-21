import { test, expect } from "bun:test";
import {
  DEFAULT_PREFERENCES,
  parsePreferences,
  resolveMaterial,
} from "../src/presentation/preferences";
import type {
  Appearance,
  Density,
  Material,
  PresentationPlatform,
} from "../src/presentation/preferences";

test("missing, malformed, and non-object storage falls back without throwing", () => {
  for (const stored of [
    undefined, null, false, 1, {}, [], "", "{", "not JSON", "null", "[]",
    '[{"version":1}]', '"text"', "true", "42",
  ]) {
    expect(() => parsePreferences(stored)).not.toThrow();
    expect(parsePreferences(stored)).toEqual(DEFAULT_PREFERENCES);
  }
});

test("unknown or missing versions do not adopt stored preferences", () => {
  for (const version of [undefined, null, 0, 2, "1", true]) {
    expect(parsePreferences(JSON.stringify({
      version, appearance: "dark", material: "glass", density: "compact", collapsed: true,
    }))).toEqual(DEFAULT_PREFERENCES);
  }
});

test("legacy v1 preserves appearance and collapsed with new field defaults", () => {
  expect(parsePreferences(JSON.stringify({ version: 1, appearance: "dark", collapsed: true })))
    .toEqual({ appearance: "dark", material: "auto", density: "comfortable", collapsed: true });
  expect(parsePreferences('{"version":1}')).toEqual(DEFAULT_PREFERENCES);
});

test("invalid enums and non-boolean collapsed are defaulted independently", () => {
  for (const invalid of [null, false, 7, [], {}, "DARK", "invalid"]) {
    expect(parsePreferences(JSON.stringify({
      version: 1, appearance: invalid, material: invalid, density: invalid, collapsed: invalid,
    }))).toEqual(DEFAULT_PREFERENCES);
  }
  expect(parsePreferences(JSON.stringify({
    version: 1, appearance: "light", material: "invalid", density: "compact", collapsed: "true",
  }))).toEqual({ appearance: "light", material: "auto", density: "compact", collapsed: false });
});

test("all preference combinations round-trip through version 1 JSON", () => {
  const appearances: Appearance[] = ["system", "light", "dark"];
  const materials: Material[] = ["auto", "glass", "solid"];
  const densities: Density[] = ["comfortable", "compact"];
  for (const appearance of appearances) {
    for (const material of materials) {
      for (const density of densities) {
        for (const collapsed of [false, true]) {
          const preferences = { appearance, material, density, collapsed };
          expect(parsePreferences(JSON.stringify({ version: 1, ...preferences }))).toEqual(preferences);
        }
      }
    }
  }
});

test("unknown fields are not retained, including prototype-shaped keys", () => {
  const preferences = parsePreferences(
    '{"version":1,"appearance":"light","extra":{"secret":true},"__proto__":{"collapsed":true}}',
  );
  expect(preferences).toEqual({ ...DEFAULT_PREFERENCES, appearance: "light" });
  expect(Object.keys(preferences).sort()).toEqual(["appearance", "collapsed", "density", "material"]);
  expect(Object.getPrototypeOf(preferences)).toBe(Object.prototype);
});

test("parser results are fresh objects and cannot change future defaults", () => {
  const preferences = parsePreferences(null);
  preferences.appearance = "dark";
  expect(parsePreferences(null)).toEqual(DEFAULT_PREFERENCES);
  expect(DEFAULT_PREFERENCES.appearance).toBe("system");
});

test("material resolution covers every platform and accessibility combination", () => {
  const materials: Material[] = ["auto", "glass", "solid"];
  const platforms: PresentationPlatform[] = ["macos", "windows", "linux", "unknown"];
  for (const material of materials) {
    for (const platform of platforms) {
      for (const reducedTransparency of [false, true]) {
        for (const forcedColors of [false, true]) {
          const expected = platform === "macos" && material !== "solid"
            && !reducedTransparency && !forcedColors ? "glass" : "solid";
          expect(resolveMaterial(material, platform, reducedTransparency, forcedColors)).toBe(expected);
        }
      }
    }
  }
});
