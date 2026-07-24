import { Check, ChevronDown, Zap } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React from "react";

interface Provider {
  name: string;
  status: "running" | "degraded" | "error";
  configured: boolean;
}

interface ProviderSelectorProps {
  providers: Provider[];
  activeProvider: string;
  onSwitch: (name: string) => void;
}

export function ProviderSelector({
  providers,
  activeProvider,
  onSwitch,
}: ProviderSelectorProps) {
  const [isOpen, setIsOpen] = React.useState(false);
  const active =
    providers.find((p) => p.name === activeProvider) || providers[0];

  return (
    <div className="relative w-full max-w-sm">
      <label className="text-[10px] uppercase text-white/50 tracking-widest mb-2 block" id="provider-selector-label">
        Primary Provider
      </label>
      <button
        onClick={() => setIsOpen(!isOpen)}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-labelledby="provider-selector-label"
        aria-controls="provider-listbox"
        className="w-full flex items-center justify-between bg-[#050505]/80 border border-white/10 hover:border-[#39ff14]/50 p-4 rounded-xl transition-all group"
      >
        <div className="flex items-center gap-3">
          <StatusIndicator status={active?.status || "error"} />
          <span className="text-sm font-medium capitalize">
            {active?.name || "None"}
          </span>
        </div>
        <ChevronDown
          className={`w-4 h-4 text-white/30 transition-transform ${isOpen ? "rotate-180" : ""}`}
        />
      </button>

      <AnimatePresence>
        {isOpen && (
          <motion.div
            id="provider-listbox"
            role="listbox"
            aria-labelledby="provider-selector-label"
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 10 }}
            className="absolute z-50 w-full mt-2 bg-[#0a0a0a] border border-white/10 rounded-xl shadow-2xl overflow-hidden backdrop-blur-xl"
          >
            {providers.map((p) => (
              <button
                key={p.name}
                role="option"
                aria-selected={activeProvider === p.name}
                onClick={() => {
                  onSwitch(p.name);
                  setIsOpen(false);
                }}
                className="w-full flex items-center justify-between p-4 hover:bg-white/5 transition-colors group"
              >
                <div className="flex items-center gap-3">
                  <StatusIndicator status={p.status} />
                  <span
                    className={`text-sm capitalize ${activeProvider === p.name ? "text-[#39ff14]" : "text-white/70"}`}
                  >
                    {p.name}
                  </span>
                </div>
                {activeProvider === p.name && (
                  <Check className="w-4 h-4 text-[#39ff14]" />
                )}
              </button>
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function StatusIndicator({
  status,
}: {
  status: "running" | "degraded" | "error";
}) {
  const colors = {
    running: "bg-[#39ff14]",
    degraded: "bg-yellow-400",
    error: "bg-red-500",
  };

  return (
    <div className="relative">
      <div className={`w-2 h-2 rounded-full ${colors[status]}`} />
      {status === "running" && (
        <div className="absolute inset-0 w-2 h-2 rounded-full bg-[#39ff14] animate-ping opacity-75" />
      )}
    </div>
  );
}
