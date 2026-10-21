export type Appearance = "system" | "light" | "dark";
export type Material = "auto" | "glass" | "solid";
export type Density = "comfortable" | "compact";
export type PresentationPlatform = "macos" | "windows" | "linux" | "unknown";
export type ResolvedMaterial = "glass" | "solid";

export interface PresentationPreferences {
  appearance: Appearance;
  material: Material;
  density: Density;
  collapsed: boolean;
}

export const DEFAULT_PREFERENCES: Readonly<PresentationPreferences> = Object.freeze({
  appearance: "system",
  material: "auto",
  density: "comfortable",
  collapsed: false,
});

/** Parse untrusted localStorage JSON; version 1 also accepts legacy fields only. */
export function parsePreferences(stored: unknown): PresentationPreferences {
  try {
    if (typeof stored !== "string") return { ...DEFAULT_PREFERENCES };
    const value: unknown = JSON.parse(stored);
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      return { ...DEFAULT_PREFERENCES };
    }
    const fields = value as Record<string, unknown>;
    if (fields.version !== 1) return { ...DEFAULT_PREFERENCES };
    return {
      appearance:
        fields.appearance === "light" || fields.appearance === "dark"
          ? fields.appearance
          : "system",
      material:
        fields.material === "glass" || fields.material === "solid"
          ? fields.material
          : "auto",
      density: fields.density === "compact" ? "compact" : "comfortable",
      collapsed: fields.collapsed === true,
    };
  } catch {
    return { ...DEFAULT_PREFERENCES };
  }
}

/** Glass is a CSS material treatment, not Apple's native Liquid Glass. */
export function resolveMaterial(
  preference: Material,
  platform: PresentationPlatform,
  reducedTransparency: boolean,
  forcedColors: boolean,
): ResolvedMaterial {
  if (reducedTransparency || forcedColors || platform !== "macos") return "solid";
  return preference === "auto" || preference === "glass" ? "glass" : "solid";
}
