import type { ResolvedTheme } from "./types";

export interface ThemeColors {
  background: string;
  surface: string;
  surfaceCard: string;
  surfaceHover: string;
  surfaceActive: string;
  foreground: string;
  foregroundMuted: string;
  borderSubtle: string;
  borderFocus: string;
  accent: string;
  accentForeground: string;
}

export const THEME_TOKENS: Record<ResolvedTheme, ThemeColors> = {
  "studio-dark": {
    background: "#111215",
    surface: "#191a1e",
    surfaceCard: "#212328",
    surfaceHover: "rgba(255, 255, 255, 0.05)",
    surfaceActive: "rgba(255, 255, 255, 0.09)",
    foreground: "#e5e7eb",
    foregroundMuted: "#8c909b",
    borderSubtle: "rgba(255, 255, 255, 0.08)",
    borderFocus: "#3b82f6",
    accent: "#3b82f6",
    accentForeground: "#ffffff",
  },
  "studio-bone": {
    background: "#f7f6f2",
    surface: "#ebe8e1",
    surfaceCard: "#ffffff",
    surfaceHover: "rgba(0, 0, 0, 0.03)",
    surfaceActive: "rgba(0, 0, 0, 0.06)",
    foreground: "#1c1c1f",
    foregroundMuted: "#73716c",
    borderSubtle: "rgba(0, 0, 0, 0.08)",
    borderFocus: "#2563eb",
    accent: "#2563eb",
    accentForeground: "#ffffff",
  },
  cyberpunk: {
    background: "#050505",
    surface: "#0a0a0a",
    surfaceCard: "#141414",
    surfaceHover: "rgba(57, 255, 20, 0.08)",
    surfaceActive: "rgba(57, 255, 20, 0.15)",
    foreground: "#ffffff",
    foregroundMuted: "#888888",
    borderSubtle: "rgba(57, 255, 20, 0.2)",
    borderFocus: "#39ff14",
    accent: "#39ff14",
    accentForeground: "#000000",
  },
};

export const DEFAULT_THEME: ResolvedTheme = "studio-dark";
