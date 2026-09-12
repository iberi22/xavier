import type { ThemeConfig, ThemeMode, ThemeVariantMode } from "./types";

export const THEME_STUDIO_DARK: ThemeConfig = {
  id: "studio-dark",
  name: "Studio Dark",
  variant: "dark",
  backgroundColor: "#0d0e10",
  foregroundColor: "#e1e4e8",
  accentColor: "#39ff14",
  preferences: {
    fontFamily: "inter",
    iconBorders: "rounded",
    attenuation: "medium",
  },
};

export const THEME_STUDIO_BONE: ThemeConfig = {
  id: "studio-bone",
  name: "Studio Bone",
  variant: "light",
  backgroundColor: "#f7f6f2",
  foregroundColor: "#1c1e21",
  accentColor: "#0066cc",
  preferences: {
    fontFamily: "inter",
    iconBorders: "rounded",
    attenuation: "low",
  },
};

export const THEME_CYBERPUNK: ThemeConfig = {
  id: "cyberpunk",
  name: "Cyberpunk",
  variant: "dark",
  backgroundColor: "#050505",
  foregroundColor: "#f0f0f0",
  accentColor: "#ff007f",
  preferences: {
    fontFamily: "jet-brains-mono",
    iconBorders: "sharp",
    attenuation: "high",
  },
};

export const THEME_CONFIGS: Record<Exclude<ThemeMode, "system">, ThemeConfig> = {
  "studio-dark": THEME_STUDIO_DARK,
  "studio-bone": THEME_STUDIO_BONE,
  cyberpunk: THEME_CYBERPUNK,
};

export const DEFAULT_THEME_MODE: ThemeMode = "studio-dark";

export function getResolvedVariant(
  mode: ThemeMode,
  systemPrefersDark = true,
): ThemeVariantMode {
  if (mode === "system") {
    return systemPrefersDark ? "dark" : "light";
  }
  return THEME_CONFIGS[mode]?.variant ?? "dark";
}

export function getThemeBackgroundColor(
  mode: ThemeMode,
  systemPrefersDark = true,
): string {
  if (mode === "system") {
    return systemPrefersDark
      ? THEME_STUDIO_DARK.backgroundColor
      : THEME_STUDIO_BONE.backgroundColor;
  }
  return THEME_CONFIGS[mode]?.backgroundColor ?? THEME_STUDIO_DARK.backgroundColor;
}
