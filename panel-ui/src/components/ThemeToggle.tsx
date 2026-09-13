import { Moon, Sun } from "lucide-react";
import { useTheme } from "../lib/theme/theme-provider";

export function ThemeToggle() {
  const { resolvedTheme, setTheme } = useTheme();
  const isDark = resolvedTheme !== "studio-bone";

  return (
    <button
      type="button"
      onClick={() => setTheme(isDark ? "studio-bone" : "studio-dark")}
      aria-label="Toggle theme"
      aria-pressed={isDark}
      title={isDark ? "Switch to light theme (Hueso Blanco)" : "Switch to dark theme (Studio Dark)"}
      className="p-1.5 rounded-full bg-slate-200 dark:bg-[#191a1e] text-slate-800 dark:text-white/80 hover:bg-slate-300 dark:hover:bg-white/10 border border-slate-300 dark:border-white/10 shadow-sm transition-colors flex items-center justify-center focus-visible:ring-2 focus-visible:ring-blue-500/50"
    >
      {isDark ? (
        <Sun className="w-4 h-4 text-amber-400" aria-hidden="true" />
      ) : (
        <Moon className="w-4 h-4 text-slate-700" aria-hidden="true" />
      )}
    </button>
  );
}

export default ThemeToggle;
