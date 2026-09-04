import { Moon, Sun } from "lucide-react";
import { useTheme } from "../lib/theme/theme-provider";

export function ThemeToggle() {
  const { theme, setTheme } = useTheme();

  return (
    <button
      type="button"
      onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
      aria-label="Toggle theme"
      title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
      className="p-1.5 rounded-full bg-slate-200 dark:bg-[#0a0a0a]/80 text-slate-800 dark:text-white/80 hover:bg-slate-300 dark:hover:bg-white/10 border border-slate-300 dark:border-white/10 shadow-sm transition-colors flex items-center justify-center focus-visible:ring-2 focus-visible:ring-[#39ff14]/50"
    >
      {theme === "dark" ? (
        <Sun className="w-4 h-4 text-amber-400" aria-hidden="true" />
      ) : (
        <Moon className="w-4 h-4 text-slate-700" aria-hidden="true" />
      )}
    </button>
  );
}

export default ThemeToggle;
