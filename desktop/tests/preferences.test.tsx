import { expect, test } from "bun:test";
import { readPreferences, writePreferences } from "../src/hooks/usePreferences";
import { DEFAULT_PREFERENCES } from "../src/presentation/preferences";

function memoryStorage() {
  const items = new Map<string, string>();
  return {
    getItem: (k: string) => items.get(k) ?? null,
    setItem: (k: string, v: string) => void items.set(k, v),
  };
}

const throwing = {
  getItem: (): string | null => {
    throw new Error("SecurityError: storage disabled");
  },
  setItem: (): void => {
    throw new Error("QuotaExceededError");
  },
};

test("persists appearance/density across a reload", () => {
  const storage = memoryStorage();
  writePreferences(() => storage, {
    ...DEFAULT_PREFERENCES,
    appearance: "dark",
    density: "compact",
  });
  const restored = readPreferences(() => storage);
  expect(restored.appearance).toBe("dark");
  expect(restored.density).toBe("compact");
});

test("survives a storage that throws", () => {
  expect(readPreferences(() => throwing)).toEqual({ ...DEFAULT_PREFERENCES });
  expect(() =>
    writePreferences(() => throwing, { ...DEFAULT_PREFERENCES, appearance: "light" }),
  ).not.toThrow();
});

test("survives a storage whose very access throws", () => {
  const denied = () => {
    throw new Error("SecurityError: localStorage is not available");
  };
  expect(readPreferences(denied)).toEqual({ ...DEFAULT_PREFERENCES });
  expect(() => writePreferences(denied, { ...DEFAULT_PREFERENCES })).not.toThrow();
});
