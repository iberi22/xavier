export type ThemeMode = "studio-dark" | "studio-bone" | "cyberpunk" | "system";

/**
 * Legacy Theme alias for backwards compatibility.
 */
export type Theme = ThemeMode;

export type ThemeVariantMode = "dark" | "light";

export type ThemeFontFamily = "inter" | "jet-brains-mono" | "system";

export type ThemeIconBorders = "rounded" | "sharp" | "pill";

export type ThemeAttenuation = "low" | "medium" | "high";

export interface ThemePreferences {
  fontFamily?: ThemeFontFamily;
  iconBorders?: ThemeIconBorders;
  attenuation?: ThemeAttenuation;
}

export interface ThemeConfig {
  id: ThemeMode;
  name: string;
  variant: ThemeVariantMode;
  backgroundColor: string;
  foregroundColor: string;
  accentColor: string;
  preferences?: ThemePreferences;
}

export interface ThemeStorageOptions {
  storageKey?: string;
  defaultTheme?: ThemeMode;
}
