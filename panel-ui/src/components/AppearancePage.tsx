import { Check, Contrast, Layout, Moon, Monitor, Palette, Sparkles, Sun } from "lucide-react";
import { motion } from "motion/react";
import React, { useState } from "react";
import { useTheme, type Theme } from "../lib/theme/theme-provider";

interface ThemePreset {
  id: string;
  name: string;
  description: string;
  theme: Theme;
  accent: string;
  previewBg: string;
  previewBorder: string;
}

const PRESETS: ThemePreset[] = [
  {
    id: "ola-studio-dark",
    name: "Ola Dark Studio",
    description: "Deep neutral canvas with studio borderless card elevation",
    theme: "dark",
    accent: "#39ff14",
    previewBg: "bg-[#141518]",
    previewBorder: "border-white/[0.08]",
  },
  {
    id: "ola-studio-light",
    name: "Ola Light Studio",
    description: "Clean high-contrast light workspace for studio environments",
    theme: "light",
    accent: "#2563eb",
    previewBg: "bg-slate-100",
    previewBorder: "border-slate-300",
  },
  {
    id: "neon-cyber",
    name: "Neon Cyber",
    description: "High vibrancy neon glow with deep black contrast",
    theme: "dark",
    accent: "#00f0ff",
    previewBg: "bg-[#050508]",
    previewBorder: "border-cyan-500/30",
  },
  {
    id: "minimal-monokai",
    name: "Minimal Monokai",
    description: "Subtle dark slate palette optimized for long coding sessions",
    theme: "dark",
    accent: "#f92672",
    previewBg: "bg-[#1e1e24]",
    previewBorder: "border-pink-500/20",
  },
];

export function AppearancePage() {
  const { theme, setTheme } = useTheme();
  const [selectedPreset, setSelectedPreset] = useState("ola-studio-dark");
  const [glassmorphism, setGlassmorphism] = useState(true);
  const [compactMode, setCompactMode] = useState(false);
  const [reducedMotion, setReducedMotion] = useState(false);
  const [accentColor, setAccentColor] = useState("#39ff14");

  const accents = [
    { name: "Lime Neon", value: "#39ff14" },
    { name: "Electric Cyan", value: "#00f0ff" },
    { name: "Vibrant Violet", value: "#a855f7" },
    { name: "Monokai Pink", value: "#f92672" },
    { name: "Studio Blue", value: "#3b82f6" },
  ];

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8 }}
      transition={{ duration: 0.25 }}
      className="p-8 max-w-4xl space-y-8 overflow-y-auto h-full text-white/90"
    >
      <div>
        <h2 className="text-2xl font-light tracking-tight text-white flex items-center gap-2.5">
          <Palette className="w-6 h-6 text-[#39ff14]" />
          Appearance & Studio Theme
        </h2>
        <p className="text-sm text-white/40 mt-1">
          Customize Xavier Studio's visual identity, color schemes, and interface density.
        </p>
      </div>

      {/* Mode Switcher */}
      <div className="space-y-3">
        <label className="text-xs font-semibold uppercase tracking-wider text-white/50 block">
          Theme Mode
        </label>
        <div className="grid grid-cols-3 gap-3">
          <button
            type="button"
            onClick={() => setTheme("dark")}
            className={`flex items-center gap-3 p-4 rounded-xl border transition-all duration-200 text-left ${
              theme === "dark"
                ? "bg-[#26272b] border-white/20 text-white shadow-md"
                : "bg-white/[0.02] border-white/[0.06] text-white/60 hover:bg-white/[0.05] hover:text-white"
            }`}
          >
            <Moon className="w-5 h-5 text-indigo-400" />
            <div>
              <div className="text-sm font-medium">Dark Mode</div>
              <div className="text-xs text-white/40">Studio dark canvas</div>
            </div>
          </button>

          <button
            type="button"
            onClick={() => setTheme("light")}
            className={`flex items-center gap-3 p-4 rounded-xl border transition-all duration-200 text-left ${
              theme === "light"
                ? "bg-[#26272b] border-white/20 text-white shadow-md"
                : "bg-white/[0.02] border-white/[0.06] text-white/60 hover:bg-white/[0.05] hover:text-white"
            }`}
          >
            <Sun className="w-5 h-5 text-amber-400" />
            <div>
              <div className="text-sm font-medium">Light Mode</div>
              <div className="text-xs text-white/40">High clarity canvas</div>
            </div>
          </button>

          <button
            type="button"
            onClick={() => setTheme("system")}
            className={`flex items-center gap-3 p-4 rounded-xl border transition-all duration-200 text-left ${
              theme === "system"
                ? "bg-[#26272b] border-white/20 text-white shadow-md"
                : "bg-white/[0.02] border-white/[0.06] text-white/60 hover:bg-white/[0.05] hover:text-white"
            }`}
          >
            <Monitor className="w-5 h-5 text-emerald-400" />
            <div>
              <div className="text-sm font-medium">System Preference</div>
              <div className="text-xs text-white/40">Sync with OS settings</div>
            </div>
          </button>
        </div>
      </div>

      {/* Studio Presets */}
      <div className="space-y-3">
        <label className="text-xs font-semibold uppercase tracking-wider text-white/50 block">
          Ola Studio Presets
        </label>
        <div className="grid grid-cols-2 gap-4">
          {PRESETS.map((preset) => (
            <button
              key={preset.id}
              type="button"
              onClick={() => {
                setSelectedPreset(preset.id);
                setTheme(preset.theme);
                setAccentColor(preset.accent);
              }}
              className={`p-4 rounded-2xl border text-left transition-all duration-200 relative overflow-hidden ${
                selectedPreset === preset.id
                  ? "border-[#39ff14]/60 bg-white/[0.04] shadow-[0_0_20px_rgba(57,255,20,0.1)]"
                  : "border-white/[0.06] bg-white/[0.02] hover:border-white/20 hover:bg-white/[0.04]"
              }`}
            >
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <span
                    className="w-3 h-3 rounded-full"
                    style={{ backgroundColor: preset.accent }}
                  />
                  <span className="text-sm font-medium text-white">{preset.name}</span>
                </div>
                {selectedPreset === preset.id && (
                  <Check className="w-4 h-4 text-[#39ff14]" />
                )}
              </div>
              <p className="text-xs text-white/40 leading-relaxed">{preset.description}</p>
              <div
                className={`mt-3 h-10 rounded-lg p-2 border flex items-center gap-2 ${preset.previewBg} ${preset.previewBorder}`}
              >
                <div
                  className="w-4 h-4 rounded-md"
                  style={{ backgroundColor: preset.accent }}
                />
                <div className="h-2 w-16 bg-white/20 rounded-full" />
                <div className="h-2 w-8 bg-white/10 rounded-full ml-auto" />
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* Accent Colors */}
      <div className="space-y-3">
        <label className="text-xs font-semibold uppercase tracking-wider text-white/50 block">
          Accent Highlight Color
        </label>
        <div className="flex items-center gap-3 bg-white/[0.02] border border-white/[0.06] p-4 rounded-xl">
          {accents.map((acc) => (
            <button
              key={acc.value}
              type="button"
              title={acc.name}
              onClick={() => setAccentColor(acc.value)}
              className={`w-9 h-9 rounded-full flex items-center justify-center transition-transform ${
                accentColor === acc.value ? "scale-110 ring-2 ring-white/80" : "opacity-80 hover:opacity-100"
              }`}
              style={{ backgroundColor: acc.value }}
            >
              {accentColor === acc.value && <Check className="w-4 h-4 text-black" />}
            </button>
          ))}
          <span className="text-xs text-white/40 ml-auto font-mono">{accentColor}</span>
        </div>
      </div>

      {/* Visual Toggles */}
      <div className="space-y-3">
        <label className="text-xs font-semibold uppercase tracking-wider text-white/50 block">
          Interface & Density Controls
        </label>
        <div className="bg-white/[0.02] border border-white/[0.06] rounded-xl divide-y divide-white/[0.06]">
          <div className="flex items-center justify-between p-4">
            <div className="flex items-center gap-3">
              <Sparkles className="w-5 h-5 text-cyan-400" />
              <div>
                <div className="text-sm font-medium">Glassmorphism Backdrop Blur</div>
                <div className="text-xs text-white/40">
                  Enable subtle frosted glass reflections on studio cards
                </div>
              </div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={glassmorphism}
              onClick={() => setGlassmorphism(!glassmorphism)}
              className={`relative w-11 h-6 rounded-full transition-colors duration-200 ${
                glassmorphism ? "bg-[#39ff14]" : "bg-white/10"
              }`}
            >
              <div
                className={`w-4 h-4 rounded-full bg-black absolute top-1 transition-transform duration-200 ${
                  glassmorphism ? "translate-x-6" : "translate-x-1"
                }`}
              />
            </button>
          </div>

          <div className="flex items-center justify-between p-4">
            <div className="flex items-center gap-3">
              <Layout className="w-5 h-5 text-indigo-400" />
              <div>
                <div className="text-sm font-medium">Compact Interface Density</div>
                <div className="text-xs text-white/40">
                  Reduce padding and tab spacing for higher data density
                </div>
              </div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={compactMode}
              onClick={() => setCompactMode(!compactMode)}
              className={`relative w-11 h-6 rounded-full transition-colors duration-200 ${
                compactMode ? "bg-[#39ff14]" : "bg-white/10"
              }`}
            >
              <div
                className={`w-4 h-4 rounded-full bg-black absolute top-1 transition-transform duration-200 ${
                  compactMode ? "translate-x-6" : "translate-x-1"
                }`}
              />
            </button>
          </div>

          <div className="flex items-center justify-between p-4">
            <div className="flex items-center gap-3">
              <Contrast className="w-5 h-5 text-amber-400" />
              <div>
                <div className="text-sm font-medium">Reduced Motion</div>
                <div className="text-xs text-white/40">
                  Disable spring animations and tab transition effects
                </div>
              </div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={reducedMotion}
              onClick={() => setReducedMotion(!reducedMotion)}
              className={`relative w-11 h-6 rounded-full transition-colors duration-200 ${
                reducedMotion ? "bg-[#39ff14]" : "bg-white/10"
              }`}
            >
              <div
                className={`w-4 h-4 rounded-full bg-black absolute top-1 transition-transform duration-200 ${
                  reducedMotion ? "translate-x-6" : "translate-x-1"
                }`}
              />
            </button>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

export default AppearancePage;
