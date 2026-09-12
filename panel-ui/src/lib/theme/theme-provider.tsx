import React, { createContext, useContext, useEffect, useState } from "react";
import { DEFAULT_THEME_MODE, getResolvedVariant, getThemeBackgroundColor } from "./tokens";
import type { Theme, ThemeMode, ThemeVariantMode } from "./types";

export type { Theme, ThemeMode, ThemeVariantMode };

export interface ThemeProviderProps {
  children: React.ReactNode;
  defaultTheme?: ThemeMode;
  storageKey?: string;
}

export interface ThemeProviderState {
  theme: ThemeMode;
  resolvedTheme: ThemeVariantMode;
  setTheme: (theme: ThemeMode) => void;
}

const initialState: ThemeProviderState = {
  theme: DEFAULT_THEME_MODE,
  resolvedTheme: "dark",
  setTheme: () => null,
};

const ThemeProviderContext = createContext<ThemeProviderState>(initialState);

export function ThemeProvider({
  children,
  defaultTheme = "studio-dark",
  storageKey = "vite-ui-theme",
  ...props
}: ThemeProviderProps) {
  const [theme, setThemeState] = useState<ThemeMode>(
    () => (localStorage.getItem(storageKey) as ThemeMode) || defaultTheme,
  );

  const [systemPrefersDark, setSystemPrefersDark] = useState<boolean>(() => {
    if (typeof window === "undefined" || !window.matchMedia) return true;
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  });

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = (e: MediaQueryListEvent) => {
      setSystemPrefersDark(e.matches);
    };
    mediaQuery.addEventListener("change", handler);
    return () => mediaQuery.removeEventListener("change", handler);
  }, []);

  const resolvedTheme: ThemeVariantMode = getResolvedVariant(theme, systemPrefersDark);

  useEffect(() => {
    const root = window.document.documentElement;

    root.classList.remove("light", "dark");
    root.classList.add(resolvedTheme);

    root.setAttribute("data-theme", theme);

    const bgColor = getThemeBackgroundColor(theme, systemPrefersDark);
    let metaThemeColor = document.querySelector('meta[name="theme-color"]');
    if (!metaThemeColor) {
      metaThemeColor = document.createElement("meta");
      metaThemeColor.setAttribute("name", "theme-color");
      document.head.appendChild(metaThemeColor);
    }
    metaThemeColor.setAttribute("content", bgColor);
  }, [theme, resolvedTheme, systemPrefersDark]);

  const value = {
    theme,
    resolvedTheme,
    setTheme: (newTheme: ThemeMode) => {
      localStorage.setItem(storageKey, newTheme);
      setThemeState(newTheme);
    },
  };

  return (
    <ThemeProviderContext.Provider {...props} value={value}>
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
