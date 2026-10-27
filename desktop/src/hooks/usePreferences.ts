import { useCallback, useEffect, useState } from "react";
import {
  DEFAULT_PREFERENCES, parsePreferences, resolveMaterial,
  type Appearance, type Material, type Density, type PresentationPreferences,
} from "../presentation/preferences";
export type { Appearance } from "../presentation/preferences";
const KEY = "abbey.presentation.v1";
type PreferenceStorage = Pick<Storage, "getItem" | "setItem">;
/** Read stored preferences; any storage failure yields the defaults. */
export function readPreferences(storage: () => PreferenceStorage): PresentationPreferences {
  try {
    return parsePreferences(storage().getItem(KEY));
  } catch {
    /* Storage can be unavailable in a hardened webview. */
  }
  return { ...DEFAULT_PREFERENCES };
}
/** Persist preferences; a storage failure leaves them usable for this session. */
export function writePreferences(
  storage: () => PreferenceStorage,
  preferences: PresentationPreferences,
): void {
  try {
    storage().setItem(KEY, JSON.stringify({ version: 1, ...preferences }));
  } catch {
    /* Preferences remain usable for this session. */
  }
}
const browserStorage = () => localStorage;
export function usePreferences() {
  const [preferences, setPreferences] = useState(() => readPreferences(browserStorage));
  useEffect(() => {
    const transparency = matchMedia("(prefers-reduced-transparency: reduce)");
    const contrast = matchMedia("(forced-colors: active)");
    const increasedContrast = matchMedia("(prefers-contrast: more)");
    // A presentation hint only, never a platform/permission authority check.
    const platform = /Mac/.test(navigator.platform) ? "macos" : "unknown";
    const update = () => {
      const supported = CSS.supports("backdrop-filter", "blur(1px)") ||
        CSS.supports("-webkit-backdrop-filter", "blur(1px)");
      document.documentElement.dataset.material = resolveMaterial(
        preferences.material, platform,
        !supported || transparency.media === "not all" || transparency.matches,
        contrast.matches || increasedContrast.matches,
      );
    };
    const queries = [transparency, contrast, increasedContrast];
    update();
    queries.forEach((query) => query.addEventListener("change", update));
    return () => queries.forEach((query) => query.removeEventListener("change", update));
  }, [preferences.material]);
  useEffect(() => {
    document.documentElement.dataset.appearance = preferences.appearance;
    document.documentElement.dataset.density = preferences.density;
    writePreferences(browserStorage, preferences);
  }, [preferences]);
  const setAppearance = useCallback(
    (appearance: Appearance) => setPreferences((p) => ({ ...p, appearance })),
    [],
  );
  const toggleSidebar = useCallback(
    () => setPreferences((p) => ({ ...p, collapsed: !p.collapsed })),
    [],
  );
  const setMaterial = useCallback(
    (material: Material) => setPreferences((p) => ({ ...p, material })), [],
  );
  const setDensity = useCallback(
    (density: Density) => setPreferences((p) => ({ ...p, density })), [],
  );
  const resetPresentation = useCallback(
    () => setPreferences({ ...DEFAULT_PREFERENCES }), [],
  );
  return { ...preferences, setAppearance, setMaterial, setDensity, toggleSidebar, resetPresentation };
}
