import { Command, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect } from "react";

export interface ShortcutItem {
  keyCombination: string[];
  description: string;
  category?: string;
}

const DEFAULT_SHORTCUTS: ShortcutItem[] = [
  {
    keyCombination: ["⌘/Ctrl", "K"],
    description: "Command input focus",
    category: "Navigation",
  },
  {
    keyCombination: ["⌘/Ctrl", ","],
    description: "Open Settings",
    category: "System",
  },
  {
    keyCombination: ["⌘/Ctrl", "Shift", "L"],
    description: "Toggle Theme (Light/Dark)",
    category: "Appearance",
  },
  {
    keyCombination: ["Escape"],
    description: "Close Modals",
    category: "General",
  },
];

interface ShortcutsModalProps {
  isOpen: boolean;
  onClose: () => void;
  shortcuts?: ShortcutItem[];
}

export const ShortcutsModal: React.FC<ShortcutsModalProps> = ({
  isOpen,
  onClose,
  shortcuts = DEFAULT_SHORTCUTS,
}) => {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isOpen) {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <AnimatePresence>
      <div
        className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-md"
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcuts-modal-title"
      >
        {/* Backdrop click dismiss */}
        <div
          className="absolute inset-0"
          onClick={onClose}
          aria-hidden="true"
        />

        <motion.div
          initial={{ opacity: 0, scale: 0.95, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, y: 10 }}
          className="relative z-10 w-full max-w-lg overflow-hidden rounded-2xl bg-[#0f1013] border border-white/10 shadow-2xl p-6 text-white"
        >
          {/* Header */}
          <div className="flex items-center justify-between pb-4 mb-4 border-b border-white/10">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-xl bg-[#39ff14]/10 text-[#39ff14] border border-[#39ff14]/20">
                <Command className="w-5 h-5" />
              </div>
              <div>
                <h2
                  id="shortcuts-modal-title"
                  className="text-lg font-semibold tracking-tight text-white"
                >
                  Keyboard Shortcuts
                </h2>
                <p className="text-xs text-white/50">
                  Quick reference cheat-sheet for system actions
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="p-2 rounded-lg text-white/40 hover:text-white hover:bg-white/10 transition-colors"
              aria-label="Close keyboard shortcuts modal"
            >
              <X className="w-5 h-5" />
            </button>
          </div>

          {/* List of shortcuts */}
          <div className="space-y-3 max-h-[60vh] overflow-y-auto pr-1">
            {shortcuts.map((shortcut, index) => (
              <div
                key={index}
                className="flex items-center justify-between p-3 rounded-xl bg-white/[0.03] border border-white/5 hover:border-white/10 transition-colors"
              >
                <span className="text-sm text-white/80 font-medium">
                  {shortcut.description}
                </span>
                <div className="flex items-center gap-1.5">
                  {shortcut.keyCombination.map((key, kIdx) => (
                    <kbd
                      key={kIdx}
                      className="px-2.5 py-1 text-xs font-mono font-semibold text-white/90 bg-white/10 border border-white/20 rounded-md shadow-inner"
                    >
                      {key}
                    </kbd>
                  ))}
                </div>
              </div>
            ))}
          </div>

          {/* Footer note */}
          <div className="mt-5 pt-3 border-t border-white/5 flex justify-between items-center text-xs text-white/40">
            <span>Press <kbd className="px-1.5 py-0.5 text-[10px] font-mono bg-white/10 border border-white/15 rounded">Esc</kbd> anytime to dismiss</span>
            <span className="text-[#39ff14]/70 font-mono">Xavier IDE</span>
          </div>
        </motion.div>
      </div>
    </AnimatePresence>
  );
};

export default ShortcutsModal;
