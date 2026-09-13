export type ThemeMode =
  | "studio-dark"
  | "studio-bone"
  | "cyberpunk"
  | "system"
  | "dark"
  | "light";

export type ResolvedTheme = "studio-dark" | "studio-bone" | "cyberpunk";

export interface ThemeSettings {
  mode: ThemeMode;
  useSystemFont: boolean;
  borderedIcons: boolean;
  enableAttenuation: boolean;
}

export interface ThemeProviderProps {
  children: React.ReactNode;
  defaultTheme?: ThemeMode;
  storageKey?: string;
}

export interface ThemeProviderState {
  theme: ThemeMode;
  resolvedTheme: ResolvedTheme;
  setTheme: (theme: ThemeMode) => void;
  settings: ThemeSettings;
  updateSettings: (partial: Partial<ThemeSettings>) => void;
}
