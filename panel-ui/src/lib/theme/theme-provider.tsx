import React, { createContext, useContext, useEffect, useState } from "react";
import type {
  ResolvedTheme,
  ThemeMode,
  ThemeProviderProps,
  ThemeProviderState,
  ThemeSettings,
} from "./types";

export * from "./types";

const defaultSettings: ThemeSettings = {
  mode: "studio-dark",
  useSystemFont: true,
  borderedIcons: true,
  enableAttenuation: true,
};

const initialState: ThemeProviderState = {
  theme: "studio-dark",
  resolvedTheme: "studio-dark",
  setTheme: () => null,
  settings: defaultSettings,
  updateSettings: () => null,
};

const ThemeProviderContext = createContext<ThemeProviderState>(initialState);

export function ThemeProvider({
  children,
  defaultTheme = "studio-dark",
  storageKey = "vite-ui-theme",
}: ThemeProviderProps) {
  const [theme, setThemeState] = useState<ThemeMode>(() => {
    return (localStorage.getItem(storageKey) as ThemeMode) || defaultTheme;
  });

  const [settings, setSettings] = useState<ThemeSettings>(() => {
    const saved = localStorage.getItem(`${storageKey}-settings`);
    if (saved) {
      try {
        return { ...defaultSettings, ...JSON.parse(saved) };
      } catch {
        // fallback
      }
    }
    return { ...defaultSettings, mode: theme };
  });

  const resolveTheme = (t: ThemeMode): ResolvedTheme => {
    if (t === "studio-bone" || t === "light") {
      return "studio-bone";
    }
    if (t === "cyberpunk") {
      return "cyberpunk";
    }
    if (t === "system") {
      const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)")?.matches;
      return prefersDark ? "studio-dark" : "studio-bone";
    }
    return "studio-dark";
  };

  const resolvedTheme = resolveTheme(theme);

  useEffect(() => {
    const root = window.document.documentElement;

    root.classList.remove("light", "dark", "studio-dark", "studio-bone", "cyberpunk");

    if (resolvedTheme === "studio-bone") {
      root.classList.add("light", "studio-bone");
    } else if (resolvedTheme === "cyberpunk") {
      root.classList.add("dark", "cyberpunk");
    } else {
      root.classList.add("dark", "studio-dark");
    }

    root.setAttribute("data-theme", resolvedTheme);

    // Ergonomics classes
    if (settings.useSystemFont) {
      root.classList.add("font-system");
    } else {
      root.classList.remove("font-system");
    }

    if (settings.borderedIcons) {
      root.classList.add("icons-bordered");
    } else {
      root.classList.remove("icons-bordered");
    }

    if (settings.enableAttenuation) {
      root.classList.add("attenuation-enabled");
    } else {
      root.classList.remove("attenuation-enabled");
    }
  }, [theme, resolvedTheme, settings]);

  const setTheme = (newTheme: ThemeMode) => {
    localStorage.setItem(storageKey, newTheme);
    setThemeState(newTheme);
    setSettings((prev) => {
      const next = { ...prev, mode: newTheme };
      localStorage.setItem(`${storageKey}-settings`, JSON.stringify(next));
      return next;
    });
  };

  const updateSettings = (partial: Partial<ThemeSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...partial };
      if (partial.mode && partial.mode !== theme) {
        setThemeState(partial.mode);
        localStorage.setItem(storageKey, partial.mode);
      }
      localStorage.setItem(`${storageKey}-settings`, JSON.stringify(next));
      return next;
    });
  };

  const value: ThemeProviderState = {
    theme,
    resolvedTheme,
    setTheme,
    settings,
    updateSettings,
  };

  return (
    <ThemeProviderContext.Provider value={value}>
      {children}
    </ThemeProviderContext.Provider>
  );
}

export const useTheme = () => {
  const context = useContext(ThemeProviderContext);

  if (context === undefined) {
    throw new Error("useTheme must be used within a ThemeProvider");
  }

  return context;
};
