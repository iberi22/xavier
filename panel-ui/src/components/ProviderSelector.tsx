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
  const id = React.useId();
  const labelId = `provider-selector-label-${id}`;
  const buttonId = `provider-selector-button-${id}`;
  const listboxId = `provider-selector-listbox-${id}`;
  const [isOpen, setIsOpen] = React.useState(false);
  const active =
    providers.find((p) => p.name === activeProvider) || providers[0];

  return (
    <div className="relative w-full max-w-sm">
      <label
        id={labelId}
        htmlFor={buttonId}
        className="text-[10px] uppercase text-white/50 tracking-widest mb-2 block"
      >
        Primary Provider
      </label>
      <button
        id={buttonId}
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-labelledby={labelId}
        aria-controls={isOpen ? listboxId : undefined}
        className="w-full flex items-center justify-between bg-[#050505]/80 border border-white/10 hover:border-[#39ff14]/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]/50 p-4 rounded-xl transition-all group"
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
            id={listboxId}
            role="listbox"
            aria-labelledby={labelId}
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 10 }}
            className="absolute z-50 w-full mt-2 bg-[#0a0a0a] border border-white/10 rounded-xl shadow-2xl overflow-hidden backdrop-blur-xl"
          >
            {providers.map((p) => (
              <button
                key={p.name}
                type="button"
                role="option"
                aria-selected={activeProvider === p.name}
                onClick={() => {
                  onSwitch(p.name);
                  setIsOpen(false);
                }}
                className="w-full flex items-center justify-between p-4 hover:bg-white/5 focus-visible:outline-none focus-visible:bg-white/10 transition-colors group"
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
