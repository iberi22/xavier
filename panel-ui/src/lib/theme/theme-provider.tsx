import React, { createContext, useContext, useEffect, useState } from "react";

export type Theme =
  | "dark"
  | "light"
  | "system"
  | "studio-dark"
  | "studio-bone"
  | "cyberpunk"
  | string;

export interface ThemeProviderProps {
  children: React.ReactNode;
  defaultTheme?: Theme;
  storageKey?: string;
}

export interface ThemeProviderState {
  theme: Theme;
  setTheme: (theme: Theme) => void;
}

const ThemeProviderContext = createContext<ThemeProviderState | undefined>(undefined);

export function ThemeProvider({
  children,
  defaultTheme = "studio-dark",
  storageKey = "vite-ui-theme",
  ...props
}: ThemeProviderProps) {
  const [theme, setThemeState] = useState<Theme>(
    () => (localStorage.getItem(storageKey) as Theme) || defaultTheme,
  );

  useEffect(() => {
    const root = window.document.documentElement;

    root.classList.remove(
      "light",
      "dark",
      "studio-dark",
      "studio-bone",
      "cyberpunk",
      "neon-legacy-tokens",
    );
    root.removeAttribute("data-theme");
    root.removeAttribute("data-neon-tokens");

    root.setAttribute("data-theme", theme);

    if (theme === "system") {
      const systemTheme = window.matchMedia("(prefers-color-scheme: dark)")
        .matches
        ? "dark"
        : "light";

      root.classList.add(systemTheme);
      return;
    }

    if (theme === "studio-bone" || theme === "light") {
      root.classList.add("light", "studio-bone");
    } else if (theme === "cyberpunk") {
      root.classList.add("dark", "cyberpunk", "neon-legacy-tokens");
      root.setAttribute("data-neon-tokens", "true");
    } else if (theme === "studio-dark" || theme === "dark") {
      root.classList.add("dark", "studio-dark");
    } else {
      root.classList.add(theme);
    }
  }, [theme]);

  const value = {
    theme,
    setTheme: (newTheme: Theme) => {
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
