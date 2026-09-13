import {
  Check,
  Laptop,
  Moon,
  Sun,
} from "lucide-react";
import React, { useState } from "react";
import { useTheme } from "../../lib/theme/theme-provider";
import type { ThemeMode } from "../../lib/theme/types";

export interface AppearancePageProps {
  onClose?: () => void;
}

export default function AppearancePage({ onClose }: AppearancePageProps) {
  const { theme, setTheme, settings, updateSettings } = useTheme();

  // Chat Settings State
  const [verboseChat, setVerboseChat] = useState(true);
  const [conversationWidth, setConversationWidth] = useState<"Default" | "Narrow" | "Wide">("Default");

  // Presets State
  const [lightPreset, setLightPreset] = useState("Default Light");
  const [darkPreset, setDarkPreset] = useState("Default Dark");

  return (
    <div className="w-full h-full overflow-y-auto px-8 py-6 text-foreground space-y-7 select-none font-sans">
      {/* Title & Subtitle */}
      <div>
        <h1 className="text-xl font-medium tracking-tight text-white/95">Appearance</h1>
        <p className="text-xs text-white/50 mt-1">
          Configure the agent's visual theme and display preferences.
        </p>
      </div>

      {/* Section: Chat Settings */}
      <div className="space-y-2">
        <h2 className="text-xs font-semibold text-white/70">Chat Settings</h2>

        <div className="rounded-xl border border-white/[0.07] bg-[#16171a]/50 divide-y divide-white/[0.05]">
          {/* Verbose Agent Chat */}
          <div className="flex items-center justify-between px-4 py-3">
            <div className="space-y-0.5">
              <span className="text-xs font-medium text-white/90">Verbose Agent Chat</span>
              <p className="text-[11px] text-white/45 leading-relaxed">
                Display and preserve intermediate thinking steps.
              </p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={verboseChat}
              aria-label="Verbose Agent Chat"
              onClick={() => setVerboseChat(!verboseChat)}
              className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full transition-colors duration-200 ease-in-out focus:outline-none ${
                verboseChat ? "bg-blue-600" : "bg-white/15"
              }`}
            >
              <span
                className={`pointer-events-none inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow-md transition duration-200 ease-in-out mt-[3px] ${
                  verboseChat ? "translate-x-4 ml-[3px]" : "translate-x-1"
                }`}
              />
            </button>
          </div>

          {/* Conversation Width */}
          <div className="flex items-center justify-between px-4 py-3">
            <div className="space-y-0.5">
              <span className="text-xs font-medium text-white/90">Conversation Width</span>
              <p className="text-[11px] text-white/45 leading-relaxed">
                Configure the maximum width of the conversation panel.
              </p>
            </div>
            <div className="inline-flex rounded-lg bg-[#111215] p-0.5 border border-white/[0.08]">
              {(["Default", "Narrow", "Wide"] as const).map((width) => (
                <button
                  key={width}
                  type="button"
                  onClick={() => setConversationWidth(width)}
                  className={`px-3 py-1 text-xs font-medium rounded-md transition-all duration-150 ${
                    conversationWidth === width
                      ? "bg-[#25272c] text-white shadow-sm font-semibold border border-white/10"
                      : "text-white/50 hover:text-white hover:bg-white/[0.04]"
                  }`}
                >
                  {width}
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* Section: Appearance (Mode Selector) */}
      <div className="space-y-2">
        <h2 className="text-xs font-semibold text-white/70">Appearance</h2>

        <div className="rounded-xl border border-white/[0.07] bg-[#16171a]/50 px-4 py-3 flex items-center justify-between">
          <div className="space-y-0.5">
            <span className="text-xs font-medium text-white/90">Appearance</span>
            <p className="text-[11px] text-white/45 leading-relaxed">
              Select light, dark, or inherit system settings.
            </p>
          </div>

          {/* 3-icon button group: [Monitor/System, Sun/Light, Moon/Dark] */}
          <div className="inline-flex items-center rounded-lg bg-[#111215] p-0.5 border border-white/[0.08]">
            <button
              type="button"
              aria-label="Inherit system theme"
              title="System"
              onClick={() => setTheme("system")}
              className={`p-1.5 rounded-md transition-all duration-150 ${
                theme === "system"
                  ? "bg-[#25272c] text-white shadow-sm border border-white/10"
                  : "text-white/40 hover:text-white/80 hover:bg-white/[0.04]"
              }`}
            >
              <Laptop className="w-3.5 h-3.5" />
            </button>

            <button
              type="button"
              aria-label="Light theme"
              title="Light"
              onClick={() => setTheme("studio-bone")}
              className={`p-1.5 rounded-md transition-all duration-150 ${
                theme === "studio-bone" || theme === "light"
                  ? "bg-[#25272c] text-white shadow-sm border border-white/10"
                  : "text-white/40 hover:text-white/80 hover:bg-white/[0.04]"
              }`}
            >
              <Sun className="w-3.5 h-3.5" />
            </button>

            <button
              type="button"
              aria-label="Dark theme"
              title="Dark"
              onClick={() => setTheme("studio-dark")}
              className={`p-1.5 rounded-md transition-all duration-150 ${
                theme === "studio-dark" || theme === "dark"
                  ? "bg-[#25272c] text-white shadow-sm border border-white/10"
                  : "text-white/40 hover:text-white/80 hover:bg-white/[0.04]"
              }`}
            >
              <Moon className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>

      {/* Section: Light Theme */}
      <div className="space-y-2">
        <h2 className="text-xs font-semibold text-white/70">Light Theme</h2>

        <div className="rounded-xl border border-white/[0.07] bg-[#16171a]/50 divide-y divide-white/[0.05]">
          {/* Preset Dropdown */}
          <div className="flex items-center justify-between px-4 py-2.5">
            <span className="text-xs font-medium text-white/90">Preset</span>
            <div className="relative">
              <select
                aria-label="Light Theme Preset"
                value={lightPreset}
                onChange={(e) => {
                  setLightPreset(e.target.value);
                  setTheme("studio-bone");
                }}
                className="bg-[#1e2025] hover:bg-[#25272d] text-white/80 text-xs px-3 py-1 pr-6 rounded-md border border-white/10 outline-none cursor-pointer appearance-none transition-colors"
              >
                <option value="Default Light" className="bg-[#1a1b20]">Default Light</option>
                <option value="studio-bone" className="bg-[#1a1b20]">Default Light (studio-bone)</option>
              </select>
              <span className="absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none text-white/40 text-[10px]">
                ▼
              </span>
            </div>
          </div>

          {/* Background Swatch */}
          <div className="flex items-center justify-between px-4 py-2.5">
            <span className="text-xs font-medium text-white/90">Background</span>
            <div className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[#1e2025] border border-white/10">
              <span
                className="w-3 h-3 rounded-[3px] border border-black/10 shrink-0"
                style={{ backgroundColor: "#EEEEEE" }}
              />
              <span className="font-mono text-[11px] text-white/70">#EEEEEE</span>
            </div>
          </div>

          {/* Foreground Swatch */}
          <div className="flex items-center justify-between px-4 py-2.5">
            <span className="text-xs font-medium text-white/90">Foreground</span>
            <div className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[#1e2025] border border-white/20 shrink-0">
              <span
                className="w-3 h-3 rounded-[3px] border border-white/20 shrink-0"
                style={{ backgroundColor: "#101010" }}
              />
              <span className="font-mono text-[11px] text-white/70">#101010</span>
            </div>
          </div>

          {/* Accent Swatch */}
          <div className="flex items-center justify-between px-4 py-2.5">
            <span className="text-xs font-medium text-white/90">Accent</span>
            <div className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[#1e2025] border border-white/10">
              <span
                className="w-3 h-3 rounded-[3px] border border-white/20 shrink-0"
                style={{ backgroundColor: "#007ACC" }}
              />
              <span className="font-mono text-[11px] text-white/70">#007ACC</span>
            </div>
          </div>
        </div>
      </div>

      {/* Section: Dark Theme */}
      <div className="space-y-2">
        <h2 className="text-xs font-semibold text-white/70">Dark Theme</h2>

        <div className="rounded-xl border border-white/[0.07] bg-[#16171a]/50 divide-y divide-white/[0.05]">
          {/* Preset Dropdown */}
          <div className="flex items-center justify-between px-4 py-2.5">
            <span className="text-xs font-medium text-white/90">Preset</span>
            <div className="relative">
              <select
                aria-label="Dark Theme Preset"
                value={darkPreset}
                onChange={(e) => {
                  setDarkPreset(e.target.value);
                  if (e.target.value === "cyberpunk") {
                    setTheme("cyberpunk");
                  } else {
                    setTheme("studio-dark");
                  }
                }}
                className="bg-[#1e2025] hover:bg-[#25272d] text-white/80 text-xs px-3 py-1 pr-6 rounded-md border border-white/10 outline-none cursor-pointer appearance-none transition-colors"
              >
                <option value="Default Dark" className="bg-[#1a1b20]">Default Dark</option>
                <option value="studio-dark" className="bg-[#1a1b20]">Default Dark (studio-dark)</option>
                <option value="cyberpunk" className="bg-[#1a1b20]">Xavier Cyberpunk</option>
              </select>
              <span className="absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none text-white/40 text-[10px]">
                ▼
              </span>
            </div>
          </div>

          {/* Background Swatch */}
          <div className="flex items-center justify-between px-4 py-2.5">
            <span className="text-xs font-medium text-white/90">Background</span>
            <div className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[#1e2025] border border-white/10">
              <span
                className="w-3 h-3 rounded-[3px] border border-white/20 shrink-0"
                style={{ backgroundColor: "#101010" }}
              />
              <span className="font-mono text-[11px] text-white/70">#101010</span>
            </div>
          </div>

          {/* Foreground Swatch */}
          <div className="flex items-center justify-between px-4 py-2.5">
            <span className="text-xs font-medium text-white/90">Foreground</span>
            <div className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[#1e2025] border border-white/10">
              <span
                className="w-3 h-3 rounded-[3px] border border-white/20 shrink-0"
                style={{ backgroundColor: "#CCCCCC" }}
              />
              <span className="font-mono text-[11px] text-white/70">#CCCCCC</span>
            </div>
          </div>

          {/* Accent Swatch */}
          <div className="flex items-center justify-between px-4 py-2.5">
            <span className="text-xs font-medium text-white/90">Accent</span>
            <div className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[#1e2025] border border-white/10">
              <span
                className="w-3 h-3 rounded-[3px] border border-white/20 shrink-0"
                style={{ backgroundColor: "#007ACC" }}
              />
              <span className="font-mono text-[11px] text-white/70">#007ACC</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
